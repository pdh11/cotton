use cotton_netif::*;
use cotton_ssdp::ssdp;
use cotton_ssdp::udp::TargetedReceive;
use cotton_ssdp::udp::TargetedSend;
use cotton_ssdp::*;
use futures::Stream;
use futures_util::StreamExt;
use nix::sys::socket::setsockopt;
use nix::sys::socket::sockopt::Ipv4PacketInfo;
use slotmap::SlotMap;
use std::collections::HashMap;
use std::error::Error;
use std::net::{IpAddr, Ipv4Addr, SocketAddrV4};
use std::os::unix::io::AsRawFd;
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

trait Callback {
    fn on_response(&self, response: &Response) -> Result<(), ()>;
}

struct ActiveSearch<CB: Callback> {
    notification_type: String,
    callback: CB,
}

slotmap::new_key_type! { struct ActiveSearchKey; }

struct Inner<CB: Callback> {
    active_searches: SlotMap<ActiveSearchKey, ActiveSearch<CB>>,
    advertisements: HashMap<String, Advertisement>,
}

impl<CB: Callback> Inner<CB> {
    fn subscribe(&mut self, notification_type: String, callback: CB) {
        let s = ActiveSearch {
            notification_type,
            callback,
        };
        self.active_searches.insert(s); // @todo notify searchers (another mpsc?)
    }

    fn broadcast(&mut self, response: &Response) {
        self.active_searches.retain(|_, s| {
            if target_match(&s.notification_type, &response.search_target) {
                s.callback.on_response(response).is_ok()
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
        // @todo send notifies
    }
}

struct Task<CB: Callback> {
    inner: Arc<Mutex<Inner<CB>>>,
    multicast_socket: tokio::net::UdpSocket,
    search_socket: tokio::net::UdpSocket,
    interfaces: HashMap<InterfaceIndex, Interface>,
}

impl<CB: Callback> Task<CB> {
    async fn new(
        inner: Arc<Mutex<Inner<CB>>>,
    ) -> Result<Task<CB>, std::io::Error> {
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
        if let Ok((n, wasto, wasfrom)) =
            self.multicast_socket.receive_to(&mut buf)
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
                        if let Some(_) = self.interface_for(wasto) {
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
                                    let _ = self.search_socket.send_from(
                                        message.as_bytes(),
                                        wasfrom,
                                        wasto,
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
        if let Ok((n, wasto, wasfrom)) =
            self.search_socket.receive_to(&mut buf)
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
                                self.search_socket.send_from(
                                    search_all,
                                    "239.255.255.250:1900".parse().unwrap(),
                                    ip.addr,
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
                            self.search_socket.send_from(
                                search_all,
                                "239.255.255.250:1900".parse().unwrap(),
                                settings.addr,
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

struct AsyncCallback {
    channel: mpsc::Sender<Response>,
}

impl Callback for AsyncCallback {
    fn on_response(&self, response: &Response) -> Result<(), ()> {
        if matches!(
            self.channel.try_send(response.clone()),
            Ok(_) | Err(mpsc::error::TrySendError::Full(_))
        ) {
            Ok(())
        } else {
            Err(())
        }
    }
}

pub struct AsyncService {
    inner: Arc<Mutex<Inner<AsyncCallback>>>,
}

/** Asynchronous SSDP service
 *
 * Handles incoming and outgoing searches using async, await, and the
 * Tokio crate.
 */
impl AsyncService {
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

        Ok(AsyncService { inner })
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
        let (snd, rcv) = mpsc::channel(100);
        self.inner.lock().unwrap().subscribe(
            notification_type.into(),
            AsyncCallback { channel: snd },
        );
        ReceiverStream::new(rcv)
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

    let mut s = AsyncService::new().await?;

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
