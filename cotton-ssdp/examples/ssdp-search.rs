use cotton_netif::*;
use cotton_ssdp::ssdp;
use cotton_ssdp::*;
use futures::Stream;
use futures_util::StreamExt;
use nix::cmsg_space;
use nix::sys::socket::setsockopt;
use nix::sys::socket::sockopt::Ipv4PacketInfo;
use nix::sys::socket::ControlMessage;
use nix::sys::socket::ControlMessageOwned;
use nix::sys::socket::MsgFlags;
use nix::sys::socket::SockaddrStorage;
use slotmap::SlotMap;
use std::collections::HashMap;
use std::error::Error;
use std::io::IoSlice;
use std::io::IoSliceMut;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::os::unix::io::AsRawFd;
use std::os::unix::prelude::RawFd;
use std::sync::{Arc, Mutex};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

#[derive(Debug)]
pub struct IPSettings {
    addr: IpAddr,
    _prefix: u8,
}

#[derive(Debug)]
pub struct Interface {
    ip: Option<IPSettings>,
    /// @todo Multiple ip addresses per interface
    up: bool,
    listening: bool,
}

fn receive(
    fd: RawFd,
    buffer: &mut [u8],
) -> Result<(usize, IpAddr, SocketAddr), std::io::Error> {
    let mut cmsgspace = cmsg_space!(libc::in_pktinfo);
    let mut iov = [IoSliceMut::new(buffer)];
    let r = nix::sys::socket::recvmsg::<SockaddrStorage>(
        fd,
        &mut iov,
        Some(&mut cmsgspace),
        MsgFlags::empty(),
    )?;
    //println!("recvmsg ok");
    let pi = match r.cmsgs().next() {
        Some(ControlMessageOwned::Ipv4PacketInfo(pi)) => pi,
        _ => {
            println!("receive: no pktinfo");
            return Err(std::io::ErrorKind::InvalidData.into());
        }
    };
    let rxon = Ipv4Addr::from(u32::from_be(pi.ipi_spec_dst.s_addr));
    let _wasto = Ipv4Addr::from(u32::from_be(pi.ipi_addr.s_addr));
    let wasfrom = {
        if let Some(ss) = r.address {
            if let Some(sin) = ss.as_sockaddr_in() {
                SocketAddrV4::new(Ipv4Addr::from(sin.ip()), sin.port())
            } else {
                println!("receive: wasfrom not ipv4");
                return Err(std::io::ErrorKind::InvalidData.into());
            }
        } else {
            println!("receive: wasfrom no address");
            return Err(std::io::ErrorKind::InvalidData.into());
        }
    };
    //    println!(
    //        "PI ix {} sd {:} ad {:} addr {:?}",
    //        pi.ipi_ifindex, rxon, wasto, wasfrom
    //    );
    Ok((r.bytes, IpAddr::V4(rxon), SocketAddr::V4(wasfrom)))
}

/** Send a UDP datagram from a specific interface
 *
 * Works even if two interfaces share the same IP range (169.254/16, for
 * instance) so long as they have different addresses.
 *
 * For how this works see https://man7.org/linux/man-pages/man7/ip.7.html
 *
 * This facility probably only works on Linux.
 */
fn send_from(
    fd: RawFd,
    buffer: &[u8],
    to: SocketAddr,
    ix: InterfaceIndex,
) -> Result<(), std::io::Error> {
    let iov = [IoSlice::new(buffer)];
    let pi = libc::in_pktinfo {
        ipi_ifindex: ix.0 as i32,
        ipi_addr: libc::in_addr { s_addr: 0 },
        ipi_spec_dst: libc::in_addr { s_addr: 0 },
    };

    let cmsg = ControlMessage::Ipv4PacketInfo(&pi);
    let dest = match to {
        SocketAddr::V4(ipv4) => SockaddrStorage::from(ipv4),
        SocketAddr::V6(ipv6) => SockaddrStorage::from(ipv6),
    };
    let r = nix::sys::socket::sendmsg(
        fd,
        &iov,
        &[cmsg],
        MsgFlags::empty(),
        Some(&dest),
    );
    if let Err(e) = r {
        println!("sendmsg {:?}", e);
        return Err(e.into());
    }
    println!("sendmsg to {:?} OK", to);
    Ok(())
}

fn target_match(search: &str, candidate: &str) -> bool {
    if search == "ssdp:all" {
        return true;
    }
    if search == candidate {
        return true;
    }
    // UPnP DA 1.0 s1.2.3
    if let Some((sbase, sversion)) = search.rsplit_once(':') {
        if let Some((cbase, cversion)) = candidate.rsplit_once(':') {
            if sbase == cbase {
                if let Ok(sversion) = sversion.parse::<usize>() {
                    if let Ok(cversion) = cversion.parse::<usize>() {
                        return cversion <= sversion;
                    }
                }
            }
        }
    }
    false
}

struct ActiveSearch {
    notification_type: String,
    channel: mpsc::Sender<Response>,
}

slotmap::new_key_type! { struct ActiveSearchKey; }

struct Inner {
    active_searches: SlotMap<ActiveSearchKey, ActiveSearch>,
    advertisements: HashMap<String, Advertisement>,
}

impl Inner {
    fn subscribe(
        &mut self,
        notification_type: String,
    ) -> impl Stream<Item = Response> {
        let (snd, rcv) = mpsc::channel(100);
        let s = ActiveSearch {
            notification_type,
            channel: snd,
        };
        self.active_searches.insert(s); // @todo notify searchers (another mpsc?)
        ReceiverStream::new(rcv)
    }

    fn broadcast(&mut self, response: &Response) {
        self.active_searches.retain(|_, s| {
            if target_match(&s.notification_type, &response.search_target) {
                matches!(
                    s.channel.try_send(response.clone()),
                    Ok(_) | Err(mpsc::error::TrySendError::Full(_))
                )
            } else {
                true
            }
        });
    }

    fn advertise(
        &mut self,
        unique_service_name: String,
        advertisement: Advertisement,
    ) {
        self.advertisements
            .insert(unique_service_name, advertisement);
    }
}

struct Task {
    inner: Arc<Mutex<Inner>>,
    multicast_socket: tokio::net::UdpSocket,
    search_socket: tokio::net::UdpSocket,
    interfaces: HashMap<InterfaceIndex, Interface>,
}

impl Task {
    async fn new(inner: Arc<Mutex<Inner>>) -> Result<Task, std::io::Error> {
        let multicast_socket = socket2::Socket::new(
            socket2::Domain::IPV4,
            socket2::Type::DGRAM,
            None,
        )?;
        multicast_socket.set_nonblocking(true)?;
        multicast_socket.set_reuse_address(true)?;
        let multicast_addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 1900u16);
        multicast_socket.bind(&socket2::SockAddr::from(multicast_addr))?;
        setsockopt(multicast_socket.as_raw_fd(), Ipv4PacketInfo, &true)?;
        let multicast_socket = UdpSocket::from_std(multicast_socket.into())?;

        let search_socket =
            UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0u16)).await?;
        setsockopt(search_socket.as_raw_fd(), Ipv4PacketInfo, &true)?;

        Ok(Task {
            inner,
            multicast_socket,
            search_socket,
            interfaces: HashMap::new(),
        })
    }

    fn interface_for(&self, addr: IpAddr) -> Option<InterfaceIndex> {
        for (k, v) in &self.interfaces {
            if let Some(settings) = &v.ip {
                if settings.addr == addr {
                    return Some(*k);
                }
            }
        }
        None
    }

    /** Process data arriving on the multicast socket
     *
     * This will be a mixture of notifications and search requests.
     */
    fn process_multicast(&mut self) {
        let mut buf = [0u8; 1500];
        if let Ok((n, wasto, wasfrom)) = self
            .multicast_socket
            .try_io(tokio::io::Interest::READABLE, || {
                receive(self.multicast_socket.as_raw_fd(), &mut buf)
            })
        {
            println!("MC RX from {} to {}", wasfrom, wasto);
            if let Ok(m) = ssdp::parse(&buf[0..n]) {
                println!("  {:?}", m);
                match m {
                    Message::NotifyAlive(a) => {
                        self.inner.lock().unwrap().broadcast(&Response {
                            search_target: a.notification_type,
                            unique_service_name: a.unique_service_name,
                            location: a.location,
                        })
                    }
                    Message::Search(s) => {
                        if let Some(ix) = self.interface_for(wasto) {
                            println!("Got search for {}", s.search_target);
                            let inner = self.inner.lock().unwrap();
                            for (key, value) in &inner.advertisements {
                                if target_match(
                                    &s.search_target,
                                    &value.notification_type,
                                ) {
                                    println!(
                                        "  {} matches, replying",
                                        value.notification_type
                                    );
                                    let mut url = value.location.clone();
                                    let _ = url.set_ip_host(wasto);

                                    let message = format!(
                                        "HTTP/1.1 200 OK\r
CACHE-CONTROL: max-age=1800\r
ST: {}\r
USN: {}\r
LOCATION: {}\r
SERVER: none/0.0 UPnP/1.0 cotton/0.1\r
\r\n",
                                        value.notification_type, key, url
                                    );
                                    let _ = self.search_socket.try_io(
                                        tokio::io::Interest::WRITABLE,
                                        || {
                                            send_from(
                                                self.search_socket.as_raw_fd(),
                                                message.as_bytes(),
                                                wasfrom,
                                                ix,
                                            )
                                        },
                                    );
                                }
                            }
                        }
                    }
                    _ => (),
                };
            }
        }
    }

    /** Process data arriving on the search socket
     *
     * This should only be response packets.
     */
    fn process_search(&mut self) {
        let mut buf = [0u8; 1500];
        if let Ok((n, wasto, wasfrom)) = self
            .search_socket
            .try_io(tokio::io::Interest::READABLE, || {
                receive(self.search_socket.as_raw_fd(), &mut buf)
            })
        {
            println!("UC RX from {} to {}", wasfrom, wasto);
            if let Ok(m) = ssdp::parse(&buf[0..n]) {
                println!("  {:?}", m);
                if let Message::Response(r) = m {
                    self.inner.lock().unwrap().broadcast(&r);
                }
            }
        }
    }

    /** Process changes (from cotton_netif) to the list of IP interfaces
     */
    fn process_interface_event(
        &mut self,
        e: NetworkEvent,
    ) -> Result<(), std::io::Error> {
        let search_all = b"M-SEARCH * HTTP/1.1\r
HOST: 239.255.255.250:1900\r
MAN: \"ssdp:discover\"\r
MX: 5\r
ST: ssdp:all\r
\r\n";

        println!("if event {:?}", e);
        match e {
            NetworkEvent::NewLink(ix, name, flags) => {
                let up = flags.contains(
                    cotton_netif::Flags::RUNNING
                        | cotton_netif::Flags::UP
                        | cotton_netif::Flags::MULTICAST,
                );
                if let Some(ref mut v) = self.interfaces.get_mut(&ix) {
                    v.up = up;
                    if v.up && !v.listening {
                        if let Some(ref ip) = v.ip {
                            if let IpAddr::V4(ipv4) = ip.addr {
                                self.multicast_socket.join_multicast_v4(
                                    "239.255.255.250".parse().unwrap(),
                                    ipv4,
                                )?;
                                println!("Searching on {:?}", ip);
                                self.search_socket.try_io(
                                    tokio::io::Interest::WRITABLE,
                                    || {
                                        send_from(
                                            self.search_socket.as_raw_fd(),
                                            search_all,
                                            "239.255.255.250:1900"
                                                .parse()
                                                .unwrap(),
                                            ix,
                                        )
                                    },
                                )?;
                                println!("New socket on {}", name);
                                v.listening = true;
                            }
                        }
                    }
                } else {
                    self.interfaces.insert(
                        ix,
                        Interface {
                            ip: None,
                            up,
                            listening: false,
                        },
                    );
                }
            }
            NetworkEvent::NewAddr(ix, addr, prefix) => {
                let settings = IPSettings {
                    addr,
                    _prefix: prefix,
                };
                if let Some(ref mut v) = self.interfaces.get_mut(&ix) {
                    if v.up && !v.listening {
                        if let IpAddr::V4(ipv4) = settings.addr {
                            self.multicast_socket
                                .join_multicast_v4(
                                    "239.255.255.250".parse().unwrap(),
                                    ipv4,
                                )
                                .map_err(|e| {
                                    println!("jmg failed {:?}", e);
                                    e
                                })?;
                            println!("Searching on {:?}", settings.addr);
                            self.search_socket.try_io(
                                tokio::io::Interest::WRITABLE,
                                || {
                                    send_from(
                                        self.search_socket.as_raw_fd(),
                                        search_all,
                                        "239.255.255.250:1900"
                                            .parse()
                                            .unwrap(),
                                        ix,
                                    )
                                },
                            )?;
                            println!("New socket on {:?}", settings.addr);
                            v.listening = true;
                        }
                        v.ip = Some(settings);
                    }
                } else {
                    self.interfaces.insert(
                        ix,
                        Interface {
                            ip: Some(settings),
                            up: false,
                            listening: false,
                        },
                    );
                }
            }
            _ => {} // @todo network-gone events
        }
        Ok(())
    }
}

pub struct Service {
    inner: Arc<Mutex<Inner>>,
}

/** An SSDP service
 *
 * Handles incoming and outgoing searches.
 */
impl Service {
    pub async fn new() -> Result<Self, Box<dyn Error>> {
        let inner = Arc::new(Mutex::new(Inner {
            active_searches: SlotMap::with_key(),
            advertisements: HashMap::default(),
        }));

        let (mut s, mut task) = tokio::try_join!(
            get_interfaces_async(),
            Task::new(inner.clone())
        )?;

        tokio::spawn(async move {
            loop {
                println!("select");

                tokio::select! {
                    e = s.next() => if let Some(Ok(event)) = e {
                        task.process_interface_event(event)
                            .unwrap_or_else(
                                |err| println!("SSDP error {}", err))
                    },
                    _ = task.multicast_socket.readable() => task.process_multicast(),
                    _ = task.search_socket.readable() => task.process_search(),
                };
            }
        });

        Ok(Service { inner })
    }

    /* @todo Subscriber wants ByeByes as well as Alives!
     */
    pub fn subscribe<A>(
        &mut self,
        notification_type: A,
    ) -> impl Stream<Item = Response>
    where
        A: Into<String>,
    {
        self.inner
            .lock()
            .unwrap()
            .subscribe(notification_type.into())
    }

    pub fn advertise<USN>(
        &mut self,
        unique_service_name: USN,
        advertisement: Advertisement,
    ) where
        USN: Into<String>,
    {
        self.inner
            .lock()
            .unwrap()
            .advertise(unique_service_name.into(), advertisement);
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    println!(
        "ssdp-search from {} {}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    );

    let mut s = Service::new().await?;

    let mut map = HashMap::new();

    let uuid = uuid::Uuid::new_v4();

    s.advertise(
        uuid.to_string(),
        Advertisement {
            notification_type: "test".to_string(),
            location: url::Url::parse("http://127.0.0.1/test").unwrap(),
        },
    );

    while let Some(r) = s.subscribe("ssdp:all").next().await {
        //println!("GOT {:?}", r);
        if !map.contains_key(&r.unique_service_name) {
            println!("+ {}", r.search_target);
            println!("  {} at {}", r.unique_service_name, r.location);
            map.insert(r.unique_service_name.clone(), r);
        }
    }

    Ok(())
}
