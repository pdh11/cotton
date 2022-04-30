use cotton_netif::*;
use cotton_ssdp::*;
use futures::Stream;
use futures_util::StreamExt;
use futures_util::TryFutureExt;
use libc;
use nix::cmsg_space;
use nix::sys::socket::setsockopt;
use nix::sys::socket::sockopt::Ipv4PacketInfo;
use nix::sys::socket::ControlMessage;
use nix::sys::socket::ControlMessageOwned;
use nix::sys::socket::InetAddr;
use nix::sys::socket::MsgFlags;
use nix::sys::socket::SockAddr;
use nix::sys::uio::IoVec;
use slotmap::SlotMap;
use std::collections::HashMap;
use std::error::Error;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
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
    up: bool,
    listening: bool,
}

fn receive(fd: RawFd, buffer: &mut [u8]) -> Result<(usize, IpAddr, SocketAddr), std::io::Error> {
    let mut cmsgspace = cmsg_space!(libc::in_pktinfo);
    let iov = [IoVec::from_mut_slice(buffer)];
    let r = nix::sys::socket::recvmsg(fd, &iov, Some(&mut cmsgspace), MsgFlags::empty())?;
    //println!("recvmsg ok");
    let pi = match r.cmsgs().next() {
        Some(ControlMessageOwned::Ipv4PacketInfo(pi)) => pi,
        _ => return Err(std::io::ErrorKind::InvalidData.into()),
    };
    let rxon = nix::sys::socket::Ipv4Addr(pi.ipi_spec_dst).to_std();
    let wasto = nix::sys::socket::Ipv4Addr(pi.ipi_addr).to_std();
    let wasfrom = match r.address {
        Some(SockAddr::Inet(a)) => a.to_std(),
        _ => return Err(std::io::ErrorKind::InvalidData.into()),
    };
    println!(
        "PI ix {} sd {:} ad {:} addr {:?}",
        pi.ipi_ifindex, rxon, wasto, wasfrom
    );
    Ok((r.bytes, rxon.into(), wasfrom.into()))
}

fn send_from(
    fd: RawFd,
    buffer: &[u8],
    to: SocketAddr,
    ix: NetworkInterface,
) -> Result<(), std::io::Error> {
    let iov = [IoVec::from_slice(buffer)];
    let pi = libc::in_pktinfo {
        ipi_ifindex: ix.value() as i32,
        ipi_addr: libc::in_addr { s_addr: 0 },
        ipi_spec_dst: libc::in_addr { s_addr: 0 },
    };

    let cmsg = [ControlMessage::Ipv4PacketInfo(&pi)];
    let dest = SockAddr::Inet(InetAddr::from_std(&to));
    let r = nix::sys::socket::sendmsg(fd, &iov, &cmsg, MsgFlags::empty(), Some(&dest));
    if let Err(e) = r {
        //println!("sendmsg {:?}", e);
        return Err(e.into());
    }
    //println!("sendmsg OK");
    Ok(())
}

fn parse(packet: &str) -> Result<Message, std::io::Error> {
    let mut iter = packet.lines();

    let prefix = iter
        .next()
        .ok_or(std::io::Error::from(std::io::ErrorKind::UnexpectedEof))?;

    //println!("  pfx {:?}", prefix);
    let mut map = HashMap::new();
    while let Some(line) = iter.next() {
        //println!("    line {:?}", line);
        if let Some((key, value)) = line.split_once(":") {
            map.insert(key.to_ascii_uppercase(), value.trim());
        }
    }
    match prefix {
        "NOTIFY * HTTP/1.1" => {
            if let Some(&nts) = map.get("NTS") {
                match nts {
                    "ssdp:alive" => {
                        if let (Some(nt), Some(usn), Some(loc)) =
                            (map.get("NT"), map.get("USN"), map.get("LOCATION"))
                        {
                            return Ok(Message::NotifyAlive(Alive {
                                notification_type: nt.to_string(),
                                unique_service_name: usn.to_string(),
                                location: loc.to_string(),
                            }));
                        }
                    }
                    "ssdp:byebye" => {
                        if let (Some(nt), Some(usn)) = (map.get("NT"), map.get("USN")) {
                            return Ok(Message::NotifyByeBye(ByeBye {
                                notification_type: nt.to_string(),
                                unique_service_name: usn.to_string(),
                            }));
                        }
                    }
                    _ => {}
                }
            }
        }
        "HTTP/1.1 200 OK" => {
            if let (Some(st), Some(usn), Some(loc)) =
                (map.get("ST"), map.get("USN"), map.get("LOCATION"))
            {
                return Ok(Message::Response(Response {
                    search_target: st.to_string(),
                    unique_service_name: usn.to_string(),
                    location: loc.to_string(),
                }));
            }
        }
        "M-SEARCH * HTTP/1.1" => {
            if let (Some(st), Some(mx)) = (map.get("ST"), map.get("MX")) {
                if let Ok(mxn) = mx.parse::<u8>() {
                    return Ok(Message::Search(Search {
                        search_target: st.to_string(),
                        maximum_wait_sec: mxn,
                    }));
                }
            }
        }
        _ => {}
    }
    Err(std::io::ErrorKind::InvalidData.into())
}

struct ActiveSearch {
    notification_type: String,
    channel: mpsc::Sender<Response>,
}

slotmap::new_key_type! { struct ActiveSearchKey; }

struct Inner {
    active_searches: SlotMap<ActiveSearchKey, ActiveSearch>,
}

impl Inner {
    fn subscribe(&mut self, notification_type: String) -> impl Stream<Item = Response> {
        let (snd, rcv) = mpsc::channel(100);
        let s = ActiveSearch {
            notification_type,
            channel: snd,
        };
        self.active_searches.insert(s); // @todo notify searchers (another mpsc?)
        ReceiverStream::new(rcv)
    }

    fn broadcast(&mut self, response: Response) {
        self.active_searches.retain(|_, s| {
            // @todo cleverer matching
            if s.notification_type == "ssdp:all" || s.notification_type == response.search_target {
                match s.channel.try_send(response.clone()) {
                    Ok(_) => true,
                    Err(mpsc::error::TrySendError::Full(_)) => true,
                    _ => false,
                }
            } else {
                true
            }
        });
    }
}

struct Task {
    inner: Arc<Mutex<Inner>>,
    multicast_socket: tokio::net::UdpSocket,
    search_socket: tokio::net::UdpSocket,
    interfaces: HashMap<NetworkInterface, Interface>,
}

impl Task {
    async fn new(inner: Arc<Mutex<Inner>>) -> Result<Task, std::io::Error> {
        let multicast_socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 1900u16)).await?;
        setsockopt(multicast_socket.as_raw_fd(), Ipv4PacketInfo, &true)?;

        let search_socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0u16)).await?;
        setsockopt(search_socket.as_raw_fd(), Ipv4PacketInfo, &true)?;

        Ok(Task {
            inner,
            multicast_socket,
            search_socket,
            interfaces: HashMap::new(),
        })
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
            //println!("RX from {} to {}", wasfrom, wasto);
            if let Ok(s) = std::str::from_utf8(&buf[0..n]) {
                if let Ok(m) = parse(s) {
                    println!("  {:?}", m);
                    match m {
                        Message::NotifyAlive(a) =>
                            self.inner.lock().unwrap().broadcast(Response {
                                search_target: a.notification_type,
                                unique_service_name: a.unique_service_name,
                                location: a.location
                            }),
                        _ => (),
                    };
                } else {
                    println!("  BAD {}", s);
                }
            } else {
                println!("  not UTF-8");
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
            println!("RX from {} to {}", wasfrom, wasto);
            if let Ok(s) = std::str::from_utf8(&buf[0..n]) {
                if let Ok(m) = parse(s) {
                    println!("  {:?}", m);
                    if let Message::Response(r) = m {
                        self.inner.lock().unwrap().broadcast(r);
                    }
                } else {
                    println!("  BAD {}", s);
                }
            } else {
                println!("  not UTF-8");
            }
        }
    }

    /** Process changes (from cotton_netif) to the list of IP interfaces
     */
    fn process_interface_event(&mut self, e: NetworkEvent) -> Result<(), std::io::Error> {
        let search_all = b"M-SEARCH * HTTP/1.1\r
HOST: 239.255.255.250:1900\r
MAN: \"ssdp:discover\"\r
MX: 5\r
ST: ssdp:all\r
\r\n";

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
                                self.multicast_socket
                                    .join_multicast_v4("239.255.255.250".parse().unwrap(), ipv4)?;
                            }
                            self.search_socket
                                .try_io(tokio::io::Interest::WRITABLE, || {
                                    send_from(
                                        self.search_socket.as_raw_fd(),
                                        search_all,
                                        "239.255.255.250:1900".parse().unwrap(),
                                        ix,
                                    )
                                })?;
                            println!("New socket on {}", name);
                            v.listening = true;
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
            NetworkEvent::NewAddr(ix, name, addr, prefix) => {
                let settings = IPSettings {
                    addr,
                    _prefix: prefix,
                };
                if let Some(ref mut v) = self.interfaces.get_mut(&ix) {
                    if v.up && !v.listening {
                        if let IpAddr::V4(ipv4) = settings.addr {
                            self.multicast_socket
                                .join_multicast_v4("239.255.255.250".parse().unwrap(), ipv4)?;
                        }
                        self.search_socket
                            .try_io(tokio::io::Interest::WRITABLE, || {
                                send_from(
                                    self.search_socket.as_raw_fd(),
                                    search_all,
                                    "239.255.255.250:1900".parse().unwrap(),
                                    ix,
                                )
                            })?;
                        println!("New socket on {}", name);
                        v.listening = true;
                    }
                    v.ip = Some(settings);
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
    // adverts: ? url::Url::set_ip_host
}

/** An SSDP service
 *
 * Handles incoming and outgoing searches.
 */
impl Service {
    pub async fn new() -> Result<Self, Box<dyn Error>> {
        let inner = Arc::new(Mutex::new(Inner {
            active_searches: SlotMap::with_key(),
        }));

        let (mut s, mut task) = tokio::try_join!(
            network_interfaces_dynamic(),
            Task::new(inner.clone()).map_err(|e| Box::new(e))
        )?;

        tokio::spawn(async move {
            loop {
                //println!("select");

                tokio::select! {
                    e = s.next() => if let Some(event) = e {
                        task.process_interface_event(event)
                            .unwrap_or_else(|err| println!("SSDP error {}", err))
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
    pub fn subscribe<A>(&mut self, notification_type: A) -> impl Stream<Item = Response>
    where
        A: Into<String>,
    {
        self.inner
            .lock()
            .unwrap()
            .subscribe(notification_type.into())
    }

    /* @todo advertise() */
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut s = Service::new().await?;

    let mut map = HashMap::new();

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
