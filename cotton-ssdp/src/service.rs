use crate::engine::{Callback, Engine};
use crate::udp::TargetedReceive;
use crate::{Advertisement, Notification};
use mockall::automock;
use nix::sys::socket::setsockopt;
use nix::sys::socket::sockopt::Ipv4PacketInfo;
use std::error::Error;
use std::os::unix::io::AsRawFd;

#[automock]
trait Socket {
    fn new(
        dom: socket2::Domain,
        ty: socket2::Type,
        flags: Option<socket2::Protocol>,
    ) -> Result<Self, std::io::Error>
    where
        Self: Sized;
    fn set_nonblocking(&self, nb: bool) -> std::io::Result<()>;
    fn set_reuse_address(&self, nb: bool) -> std::io::Result<()>;
    fn set_ipv4_packetinfo(&self, nb: bool) -> std::io::Result<()>;
    fn bind(&self, addr: &socket2::SockAddr) -> std::io::Result<()>;
    fn to_mio(self) -> mio::net::UdpSocket;
}

impl Socket for socket2::Socket {
    fn new(
        dom: socket2::Domain,
        ty: socket2::Type,
        flags: Option<socket2::Protocol>,
    ) -> Result<Self, std::io::Error> {
        Self::new(dom, ty, flags)
    }

    fn set_nonblocking(&self, nb: bool) -> std::io::Result<()> {
        self.set_nonblocking(nb)
    }

    fn set_reuse_address(&self, nb: bool) -> std::io::Result<()> {
        self.set_reuse_address(nb)
    }

    fn set_ipv4_packetinfo(&self, nb: bool) -> std::io::Result<()> {
        Ok(setsockopt(self.as_raw_fd(), Ipv4PacketInfo, &nb)?)
    }

    fn bind(&self, addr: &socket2::SockAddr) -> std::io::Result<()> {
        self.bind(addr)
    }

    fn to_mio(self) -> mio::net::UdpSocket {
        mio::net::UdpSocket::from_std(self.into())
    }
}

fn new_socket<SCK: Socket>(
    port: u16,
) -> Result<mio::net::UdpSocket, Box<dyn Error>> {
    let socket = SCK::new(socket2::Domain::IPV4, socket2::Type::DGRAM, None)?;

    socket.set_nonblocking(true)?;
    socket.set_reuse_address(true)?;
    let addr =
        std::net::SocketAddrV4::new(std::net::Ipv4Addr::UNSPECIFIED, port);
    socket.bind(&socket2::SockAddr::from(addr))?;
    socket.set_ipv4_packetinfo(true)?;
    Ok(socket.to_mio())
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
type SocketFn = fn(u16) -> Result<mio::net::UdpSocket, Box<dyn Error>>;

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
    ) -> Result<Self, Box<dyn Error>>
    where
        ITER: Iterator<Item = cotton_netif::NetworkEvent>,
        FN: Fn() -> std::io::Result<ITER>,
    {
        let mut multicast_socket = socket(1900u16)?;
        let mut search_socket = socket(0u16)?; // ephemeral port
        let mut engine = Engine::<SyncCallback>::new();

        for netif in get_interfaces()? {
            engine.on_interface_event(
                netif,
                &multicast_socket,
                &search_socket,
            )?;
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
    ) -> Result<Self, Box<dyn Error>> {
        Self::new_inner(
            registry,
            tokens,
            new_socket::<socket2::Socket>,
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
    use mockall::predicate;
    use serial_test::serial;
    use std::sync::Arc;

    #[test]
    #[serial]
    #[cfg_attr(miri, ignore)]
    fn new_socket_sets_up_socket() {
        let ctx = MockSocket::new_context();
        ctx.expect()
            .withf(|x, y, z| {
                x == &socket2::Domain::IPV4
                    && y == &socket2::Type::DGRAM
                    && z.is_none()
            })
            .returning(|_x, _y, _z| {
                let mut mock = MockSocket::default();

                let real_socket = Arc::new(
                    std::net::UdpSocket::bind("127.0.0.1:0").unwrap(),
                );
                mock.expect_set_nonblocking()
                    .with(predicate::eq(true))
                    .return_once(|_| Ok(()));
                mock.expect_set_reuse_address()
                    .with(predicate::eq(true))
                    .return_once(|_| Ok(()));
                mock.expect_bind()
                    .withf(|addr| {
                        let v4 = addr.as_socket_ipv4().unwrap();
                        v4.ip() == &std::net::Ipv4Addr::UNSPECIFIED
                            && v4.port() == 9100
                    })
                    .return_once(|_| Ok(()));
                mock.expect_set_ipv4_packetinfo()
                    .with(predicate::eq(true))
                    .return_once(|_| Ok(()));
                mock.expect_to_mio().return_once(move || {
                    mio::net::UdpSocket::from_std(
                        Arc::try_unwrap(real_socket).unwrap(),
                    )
                });
                Ok(mock)
            });

        let s = new_socket::<MockSocket>(9100);
        assert!(s.is_ok());
    }

    #[test]
    #[serial]
    fn new_socket_passes_on_creation_error() {
        let ctx = MockSocket::new_context();
        ctx.expect().returning(|_x, _y, _z| {
            Err(std::io::Error::new(std::io::ErrorKind::Other, "TEST"))
        });

        let s = new_socket::<MockSocket>(9100);
        assert!(s.is_err());
    }

    #[test]
    #[serial]
    fn new_socket_passes_on_nonblocking_error() {
        let ctx = MockSocket::new_context();
        ctx.expect().returning(|_x, _y, _z| {
            let mut mock = MockSocket::default();

            mock.expect_set_nonblocking().return_once(|_| {
                Err(std::io::Error::new(std::io::ErrorKind::Other, "TEST"))
            });

            Ok(mock)
        });

        let s = new_socket::<MockSocket>(9100);
        assert!(s.is_err());
    }

    #[test]
    #[serial]
    fn new_socket_passes_on_reuseaddr_error() {
        let ctx = MockSocket::new_context();
        ctx.expect().returning(|_x, _y, _z| {
            let mut mock = MockSocket::default();

            mock.expect_set_nonblocking()
                .with(predicate::eq(true))
                .return_once(|_| Ok(()));
            mock.expect_set_reuse_address().return_once(|_| {
                Err(std::io::Error::new(std::io::ErrorKind::Other, "TEST"))
            });

            Ok(mock)
        });

        let s = new_socket::<MockSocket>(9100);
        assert!(s.is_err());
    }

    #[test]
    #[serial]
    fn new_socket_passes_on_bind_error() {
        let ctx = MockSocket::new_context();
        ctx.expect().returning(|_x, _y, _z| {
            let mut mock = MockSocket::default();

            mock.expect_set_nonblocking()
                .with(predicate::eq(true))
                .return_once(|_| Ok(()));
            mock.expect_set_reuse_address()
                .with(predicate::eq(true))
                .return_once(|_| Ok(()));
            mock.expect_bind().return_once(|_| {
                Err(std::io::Error::new(std::io::ErrorKind::Other, "TEST"))
            });

            Ok(mock)
        });

        let s = new_socket::<MockSocket>(9100);
        assert!(s.is_err());
    }

    #[test]
    #[serial]
    #[cfg_attr(miri, ignore)]
    fn new_socket_passes_on_setsockopt_error() {
        let ctx = MockSocket::new_context();
        ctx.expect().returning(|_x, _y, _z| {
            let mut mock = MockSocket::default();

            mock.expect_set_nonblocking()
                .with(predicate::eq(true))
                .return_once(|_| Ok(()));
            mock.expect_set_reuse_address()
                .with(predicate::eq(true))
                .return_once(|_| Ok(()));
            mock.expect_bind()
                .withf(|addr| {
                    let v4 = addr.as_socket_ipv4().unwrap();
                    v4.ip() == &std::net::Ipv4Addr::UNSPECIFIED
                        && v4.port() == 9100
                })
                .return_once(|_| Ok(()));
            mock.expect_set_ipv4_packetinfo().return_once(|_| {
                Err(std::io::Error::new(std::io::ErrorKind::Other, "TEST"))
            });

            Ok(mock)
        });

        let s = new_socket::<MockSocket>(9100);
        assert!(s.is_err());
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
    #[serial]
    #[cfg_attr(miri, ignore)]
    fn service_passes_on_socket_failure() {
        const SSDP_TOKEN1: mio::Token = mio::Token(37);
        const SSDP_TOKEN2: mio::Token = mio::Token(94);
        let poll = mio::Poll::new().unwrap();

        let e = Service::new_inner(
            poll.registry(), (SSDP_TOKEN1, SSDP_TOKEN2),
            |_| Err(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "TEST"))),
            |r, s, t| r.register(s, t, mio::Interest::READABLE),
            cotton_netif::get_interfaces,
        );

        assert!(e.is_err());
    }

    #[test]
    #[serial]
    #[cfg_attr(miri, ignore)]
    fn service_passes_on_second_socket_failure() {
        const SSDP_TOKEN1: mio::Token = mio::Token(37);
        const SSDP_TOKEN2: mio::Token = mio::Token(94);
        let poll = mio::Poll::new().unwrap();

        let e = Service::new_inner(
            poll.registry(), (SSDP_TOKEN1, SSDP_TOKEN2),
            |p| if p != 0 {
                Ok(mio::net::UdpSocket::bind("127.0.0.1:0".parse().unwrap()).unwrap())
                } else {
                    Err(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "TEST")))
                },
            |r, s, t| r.register(s, t, mio::Interest::READABLE),
            cotton_netif::get_interfaces,
        );

        assert!(e.is_err());
    }
}
