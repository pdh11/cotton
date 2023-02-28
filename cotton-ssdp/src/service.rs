use crate::engine::{Callback, Engine};
use crate::udp::TargetedReceive;
use crate::{Advertisement, Notification};
use nix::sys::socket::setsockopt;
use nix::sys::socket::sockopt::Ipv4PacketInfo;
use std::os::unix::io::AsRawFd;

type NewSocketFn = fn() -> std::io::Result<socket2::Socket>;
type SockoptFn = fn(&socket2::Socket, bool) -> std::io::Result<()>;
type RawSockoptFn =
    fn(&socket2::Socket, bool) -> Result<(), nix::errno::Errno>;
type BindFn =
    fn(&socket2::Socket, std::net::SocketAddrV4) -> std::io::Result<()>;

fn new_socket_inner(
    port: u16,
    new_socket: NewSocketFn,
    nonblocking: SockoptFn,
    reuse_address: SockoptFn,
    bind: BindFn,
    ipv4_packetinfo: RawSockoptFn,
) -> std::io::Result<mio::net::UdpSocket> {
    let socket = new_socket()?;
    nonblocking(&socket, true)?;
    reuse_address(&socket, true)?;
    bind(
        &socket,
        std::net::SocketAddrV4::new(std::net::Ipv4Addr::UNSPECIFIED, port),
    )?;
    ipv4_packetinfo(&socket, true)?;
    Ok(mio::net::UdpSocket::from_std(socket.into()))
}

fn new_socket(port: u16) -> Result<mio::net::UdpSocket, std::io::Error> {
    new_socket_inner(
        port,
        || {
            socket2::Socket::new(
                socket2::Domain::IPV4,
                socket2::Type::DGRAM,
                None,
            )
        },
        socket2::Socket::set_nonblocking,
        socket2::Socket::set_reuse_address,
        |s, a| s.bind(&socket2::SockAddr::from(a)),
        |s, b| setsockopt(s.as_raw_fd(), Ipv4PacketInfo, &b),
    )
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

// The type of new_socket
type SocketFn = fn(u16) -> Result<mio::net::UdpSocket, std::io::Error>;

// The type of registry::register
type RegisterFn = fn(
    &mio::Registry,
    &mut mio::net::UdpSocket,
    mio::Token,
) -> std::io::Result<()>;

impl Service {
    fn new_inner<FN, ITER>(
        registry: &mio::Registry,
        tokens: (mio::Token, mio::Token),
        socket: SocketFn,
        register: RegisterFn,
        get_interfaces: FN,
    ) -> Result<Self, std::io::Error>
    where
        ITER: Iterator<Item = cotton_netif::NetworkEvent>,
        FN: Fn() -> std::io::Result<ITER>,
    {
        let mut multicast_socket = socket(1900u16)?;
        let mut search_socket = socket(0u16)?; // ephemeral port
        let mut engine = Engine::<SyncCallback>::new();

        for netif in get_interfaces()? {
            // Ignore errors -- some interfaces are returned on which
            // join_multicast failes (lxcbr0)
            _ = engine.on_interface_event(
                netif,
                &multicast_socket,
                &search_socket,
            );
        }

        register(registry, &mut multicast_socket, tokens.0)?;
        register(registry, &mut search_socket, tokens.1)?;

        Ok(Self {
            engine,
            multicast_socket,
            search_socket,
        })
    }

    pub fn new(
        registry: &mio::Registry,
        tokens: (mio::Token, mio::Token),
    ) -> Result<Self, std::io::Error> {
        Self::new_inner(
            registry,
            tokens,
            new_socket,
            |r, s, t| r.register(s, t, mio::Interest::READABLE),
            cotton_netif::get_interfaces,
        )
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

#[cfg(test)]
mod tests {
    use super::*;

    fn my_err() -> std::io::Error {
        std::io::Error::from(std::io::ErrorKind::Other)
    }

    fn bogus_new_socket() -> std::io::Result<socket2::Socket> {
        Err(my_err())
    }
    fn bogus_setsockopt(_: &socket2::Socket, b: bool) -> std::io::Result<()> {
        assert!(b);
        Err(my_err())
    }
    fn bogus_raw_setsockopt(
        _: &socket2::Socket,
        _: bool,
    ) -> Result<(), nix::errno::Errno> {
        Err(nix::errno::Errno::ENOTTY)
    }
    fn bogus_bind(
        _: &socket2::Socket,
        _: std::net::SocketAddrV4,
    ) -> std::io::Result<()> {
        Err(my_err())
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn new_socket_passes_on_creation_error() {
        let e = new_socket_inner(
            0u16,
            bogus_new_socket,
            bogus_setsockopt,
            bogus_setsockopt,
            bogus_bind,
            bogus_raw_setsockopt,
        );

        assert!(e.is_err());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn new_socket_passes_on_nonblocking_error() {
        let e = new_socket_inner(
            0u16,
            || {
                socket2::Socket::new(
                    socket2::Domain::IPV4,
                    socket2::Type::DGRAM,
                    None,
                )
            },
            bogus_setsockopt,
            bogus_setsockopt,
            bogus_bind,
            bogus_raw_setsockopt,
        );

        assert!(e.is_err());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn new_socket_passes_on_reuseaddr_error() {
        let e = new_socket_inner(
            0u16,
            || {
                socket2::Socket::new(
                    socket2::Domain::IPV4,
                    socket2::Type::DGRAM,
                    None,
                )
            },
            |s, b| s.set_nonblocking(b),
            bogus_setsockopt,
            bogus_bind,
            bogus_raw_setsockopt,
        );

        assert!(e.is_err());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn new_socket_passes_on_bind_error() {
        let e = new_socket_inner(
            0u16,
            || {
                socket2::Socket::new(
                    socket2::Domain::IPV4,
                    socket2::Type::DGRAM,
                    None,
                )
            },
            |s, b| s.set_nonblocking(b),
            |s, b| s.set_reuse_address(b),
            bogus_bind,
            bogus_raw_setsockopt,
        );

        assert!(e.is_err());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn new_socket_passes_on_pktinfo_error() {
        let e = new_socket_inner(
            0u16,
            || {
                socket2::Socket::new(
                    socket2::Domain::IPV4,
                    socket2::Type::DGRAM,
                    None,
                )
            },
            |s, b| s.set_nonblocking(b),
            |s, b| s.set_reuse_address(b),
            |s, a| s.bind(&socket2::SockAddr::from(a)),
            bogus_raw_setsockopt,
        );

        assert!(e.is_err());
    }

    fn bogus_register(
        _: &mio::Registry,
        _: &mut mio::net::UdpSocket,
        _: mio::Token,
    ) -> std::io::Result<()> {
        Err(my_err())
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn instantiate() {
        const SSDP_TOKEN1: mio::Token = mio::Token(37);
        const SSDP_TOKEN2: mio::Token = mio::Token(94);
        let poll = mio::Poll::new().unwrap();

        let _ =
            Service::new(poll.registry(), (SSDP_TOKEN1, SSDP_TOKEN2)).unwrap();
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn service_passes_on_socket_failure() {
        const SSDP_TOKEN1: mio::Token = mio::Token(37);
        const SSDP_TOKEN2: mio::Token = mio::Token(94);
        let poll = mio::Poll::new().unwrap();

        let e = Service::new_inner(
            poll.registry(),
            (SSDP_TOKEN1, SSDP_TOKEN2),
            |_| Err(std::io::Error::new(std::io::ErrorKind::Other, "TEST")),
            bogus_register,
            cotton_netif::get_interfaces,
        );

        assert!(e.is_err());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn service_passes_on_second_socket_failure() {
        const SSDP_TOKEN1: mio::Token = mio::Token(37);
        const SSDP_TOKEN2: mio::Token = mio::Token(94);
        let poll = mio::Poll::new().unwrap();

        let e = Service::new_inner(
            poll.registry(),
            (SSDP_TOKEN1, SSDP_TOKEN2),
            |p| {
                if p != 0 {
                    Ok(mio::net::UdpSocket::bind(
                        "127.0.0.1:0".parse().unwrap(),
                    )
                    .unwrap())
                } else {
                    Err(std::io::Error::new(std::io::ErrorKind::Other, "TEST"))
                }
            },
            bogus_register,
            cotton_netif::get_interfaces,
        );

        assert!(e.is_err());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn service_passes_on_get_interfaces_failure() {
        const SSDP_TOKEN1: mio::Token = mio::Token(37);
        const SSDP_TOKEN2: mio::Token = mio::Token(94);
        let poll = mio::Poll::new().unwrap();

        let e = Service::new_inner(
            poll.registry(),
            (SSDP_TOKEN1, SSDP_TOKEN2),
            new_socket,
            bogus_register,
            || {
                if false {
                    cotton_netif::get_interfaces()
                } else {
                    Err(my_err())
                }
            },
        );

        assert!(e.is_err());
    }
}
