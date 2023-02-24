use crate::engine::{Callback, Engine};
use crate::udp::TargetedReceive;
use crate::{Advertisement, Notification};
use nix::sys::socket::setsockopt;
use nix::sys::socket::sockopt::Ipv4PacketInfo;
use std::error::Error;
use std::os::unix::io::AsRawFd;

fn new_socket(port: u16) -> Result<mio::net::UdpSocket, Box<dyn Error>> {
    let socket = socket2::Socket::new(
        socket2::Domain::IPV4,
        socket2::Type::DGRAM,
        None,
    )?;
    socket.set_nonblocking(true)?;
    socket.set_reuse_address(true)?;
    let addr =
        std::net::SocketAddrV4::new(std::net::Ipv4Addr::UNSPECIFIED, port);
    socket.bind(&socket2::SockAddr::from(addr))?;
    setsockopt(socket.as_raw_fd(), Ipv4PacketInfo, &true)?;
    let socket: std::net::UdpSocket = socket.into();
    Ok(mio::net::UdpSocket::from_std(socket))
}

struct SyncCallback {
    callback: Box<dyn Fn(&Notification)>,
}

impl Callback for SyncCallback {
    fn on_notification(&self, r: &Notification) {
        (self.callback)(r);
    }
}

pub struct Service {
    engine: Engine<SyncCallback>,
    multicast_socket: mio::net::UdpSocket,
    search_socket: mio::net::UdpSocket,
}

impl Service {
    pub fn new(
        registry: &mio::Registry,
        tokens: (mio::Token, mio::Token),
    ) -> Result<Self, Box<dyn Error>> {
        let mut multicast_socket = new_socket(1900u16)?;
        let mut search_socket = new_socket(0u16)?; // ephemeral port
        let mut engine = Engine::<SyncCallback>::new();

        for netif in cotton_netif::get_interfaces()? {
            engine.on_interface_event(
                netif,
                &multicast_socket,
                &search_socket,
            )?;
        }

        registry.register(
            &mut multicast_socket,
            tokens.0,
            mio::Interest::READABLE,
        )?;
        registry.register(
            &mut search_socket,
            tokens.1,
            mio::Interest::READABLE,
        )?;

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
