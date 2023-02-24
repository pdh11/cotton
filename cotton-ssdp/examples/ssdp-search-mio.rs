use cotton_ssdp::engine::{Callback, Engine};
use cotton_ssdp::udp::TargetedReceive;
use cotton_ssdp::*;
use nix::sys::socket::setsockopt;
use nix::sys::socket::sockopt::Ipv4PacketInfo;
use std::cell::RefCell;
use std::collections::HashMap;
use std::error::Error;
use std::net::UdpSocket;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::os::unix::io::AsRawFd;

struct SyncCallback {
    callback: Box<dyn Fn(&Notification)>,
}

impl Callback for SyncCallback {
    fn on_notification(&self, r: &Notification) {
        (self.callback)(r);
    }
}

struct Service {
    engine: Engine<SyncCallback>,
    pub multicast_socket: mio::net::UdpSocket,
    pub search_socket: mio::net::UdpSocket,
}

impl Service {
    fn new() -> Result<Self, Box<dyn Error>> {
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
        let multicast_socket: std::net::UdpSocket = multicast_socket.into();
        let multicast_socket = mio::net::UdpSocket::from_std(multicast_socket);

        let search_socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0u16))?;
        setsockopt(search_socket.as_raw_fd(), Ipv4PacketInfo, &true)?;
        let search_socket = mio::net::UdpSocket::from_std(search_socket);

        let mut engine = Engine::<SyncCallback>::new();

        for netif in cotton_netif::get_interfaces()? {
            engine.on_interface_event(
                netif,
                &multicast_socket,
                &search_socket,
            )?;
        }

        Ok(Self {
            engine,
            multicast_socket,
            search_socket,
        })
    }

    pub fn subscribe<A>(
        &mut self,
        notification_type: A,
        callback: Box<dyn Fn(&Notification)>,
    ) where
        A: Into<String>,
    {
        self.engine.subscribe(
            notification_type.into(),
            SyncCallback { callback },
            &self.search_socket,
        );
    }

    pub fn advertise<USN>(
        &mut self,
        unique_service_name: USN,
        advertisement: Advertisement,
    ) where
        USN: Into<String>,
    {
        self.engine.advertise(
            unique_service_name.into(),
            advertisement,
            &self.search_socket,
        );
    }

    pub fn multicast_ready(&mut self, event: &mio::event::Event) {
        if event.is_readable() {
            let mut buf = [0u8; 1500];
            if let Ok((n, wasto, wasfrom)) =
                self.multicast_socket.receive_to(&mut buf)
            {
                self.engine.on_data(
                    &buf[0..n],
                    &self.search_socket,
                    wasto,
                    wasfrom,
                );
            }
        }
    }

    pub fn search_ready(&mut self, event: &mio::event::Event) {
        if event.is_readable() {
            let mut buf = [0u8; 1500];
            if let Ok((n, wasto, wasfrom)) =
                self.search_socket.receive_to(&mut buf)
            {
                self.engine.on_data(
                    &buf[0..n],
                    &self.search_socket,
                    wasto,
                    wasfrom,
                );
            }
        }
    }
}

const SSDP_TOKEN1: mio::Token = mio::Token(0);
const SSDP_TOKEN2: mio::Token = mio::Token(1);

fn main() -> Result<(), Box<dyn Error>> {
    println!(
        "ssdp-search-mio from {} {}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    );

    let mut poll = mio::Poll::new()?;
    let mut events = mio::Events::with_capacity(128);

    let mut ssdp = Service::new()?;

    poll.registry().register(
        &mut ssdp.multicast_socket,
        SSDP_TOKEN1,
        mio::Interest::READABLE,
    )?;
    poll.registry().register(
        &mut ssdp.search_socket,
        SSDP_TOKEN2,
        mio::Interest::READABLE,
    )?;

    let map = RefCell::new(HashMap::new());

    let uuid = uuid::Uuid::new_v4();

    ssdp.advertise(
        uuid.to_string(),
        cotton_ssdp::Advertisement {
            notification_type: "test".to_string(),
            location: url::Url::parse("http://127.0.0.1/test").unwrap(),
        },
    );

    ssdp.subscribe(
        "ssdp:all",
        Box::new(move |r| {
            println!("GOT {:?}", r);
            let mut m = map.borrow_mut();
            if let NotificationSubtype::AliveLocation(loc) =
                &r.notification_subtype
            {
                if !m.contains_key(&r.unique_service_name) {
                    println!("+ {}", r.notification_type);
                    println!("  {} at {}", r.unique_service_name, loc);
                    m.insert(r.unique_service_name.clone(), r.clone());
                }
            }
        }),
    );

    loop {
        poll.poll(&mut events, None).unwrap();

        for event in &events {
            match event.token() {
                SSDP_TOKEN1 => ssdp.multicast_ready(event),
                SSDP_TOKEN2 => ssdp.search_ready(event),
                _ => (),
            }
        }
    }
}
