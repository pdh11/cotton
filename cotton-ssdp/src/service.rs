use crate::engine::{Callback, Engine};
use crate::udp::TargetedReceive;
use crate::{Advertisement, Notification};
use mockall::mock;
use nix::sys::socket::setsockopt;
use nix::sys::socket::sockopt::Ipv4PacketInfo;
use std::error::Error;
use std::os::unix::io::AsRawFd;

mock! {
    pub Socket {
        fn new(dom: socket2::Domain, ty: socket2::Type, flags: Option<u32>) ->
            Result<Self, std::io::Error>;
        fn set_nonblocking(&self, nb: bool) -> Result<(),std::io::Error>;
        fn set_reuse_address(&self, nb: bool) -> Result<(),std::io::Error>;
        fn bind(&self, addr: &socket2::SockAddr) -> Result<(),std::io::Error>;
        fn as_raw_fd(&self) -> std::os::unix::io::RawFd;
    }
    impl Into<std::net::UdpSocket> for Socket {
        fn into(self) -> std::net::UdpSocket;
    }
}

#[cfg(not(test))]
use socket2::Socket;

#[cfg(test)]
type Socket = MockSocket;

fn new_socket(port: u16) -> Result<mio::net::UdpSocket, Box<dyn Error>> {
    let socket =
        Socket::new(socket2::Domain::IPV4, socket2::Type::DGRAM, None)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use mockall::predicate;
    use std::sync::Arc;
    use serial_test::serial;

    #[test]
    #[serial]
    fn new_socket_sets_up_socket() {
        let ctx = MockSocket::new_context();
        ctx.expect().returning(|_x, _y, _z| {
            let mut mock = MockSocket::default();

            let real_socket =
                Arc::new(std::net::UdpSocket::bind("127.0.0.1:0").unwrap());
            let fd = real_socket.as_raw_fd();

            mock.expect_set_nonblocking()
                .with(predicate::eq(true))
                .return_once(|_| Ok(()));
            mock.expect_set_reuse_address()
                .with(predicate::eq(true))
                .return_once(|_| Ok(()));
            mock.expect_bind()
                .withf (|addr| {
                    let v4 = addr.as_socket_ipv4().unwrap();
                    v4.ip() == &std::net::Ipv4Addr::UNSPECIFIED
                        && v4.port() == 9100
                })
                .return_once(|_| Ok(()));
            mock.expect_as_raw_fd().return_once(move || fd);
            mock.expect_into()
                .return_once(move || Arc::try_unwrap(real_socket).unwrap());
            Ok(mock)
        });

        let s = new_socket(9100);
        assert!(s.is_ok());
    }

    #[test]
    #[serial]
    fn new_socket_passes_on_creation_error() {
        let ctx = MockSocket::new_context();
        ctx.expect().returning(|_x, _y, _z| {
            Err(std::io::Error::new(std::io::ErrorKind::Other, "TEST"))
        });

        let s = new_socket(9100);
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

        let s = new_socket(9100);
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

        let s = new_socket(9100);
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

        let s = new_socket(9100);
        assert!(s.is_err());
    }

    #[test]
    #[serial]
    fn new_socket_passes_on_setsockopt_error() {
        let ctx = MockSocket::new_context();
        ctx.expect().returning(|_x, _y, _z| {
            let mut mock = MockSocket::default();

            let real_socket =
                Arc::new(std::net::UdpSocket::bind("127.0.0.1:0").unwrap());

            mock.expect_set_nonblocking()
                .with(predicate::eq(true))
                .return_once(|_| Ok(()));
            mock.expect_set_reuse_address()
                .with(predicate::eq(true))
                .return_once(|_| Ok(()));
            mock.expect_bind()
                .withf (|addr| {
                    let v4 = addr.as_socket_ipv4().unwrap();
                    v4.ip() == &std::net::Ipv4Addr::UNSPECIFIED
                        && v4.port() == 9100
                })
                .return_once(|_| Ok(()));

            // This assumes that Cargo's fd 0 is not an IP socket -- but that
            // seems like a reasonable assumption.
            mock.expect_as_raw_fd().return_once(|| 0);
            Ok(mock)
        });

        let s = new_socket(9100);
        assert!(s.is_err());
    }
}
