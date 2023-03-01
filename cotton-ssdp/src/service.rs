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

/** High-level reactor-style SSDP service using mio.

Use a `Service` to discover network resources using SSDP, or to advertise
network resources which your program provides. Or both.

The implementation integrates with the [`mio`] crate, which provides a
"reactor-style" I/O API suited for running several I/O operations in a
single thread. (SSDP, being relatively low-bandwidth and non-urgent,
is unlikely to require any more high-performance I/O solution.)

The implementation requires _two_ UDP sockets: one bound to the
well-known SSDP port number (1900) which subscribes to the multicast
group, and a second bound to a random port for sending unicast
searches and receiving unicast replies. (It would be possible to get
by with a single socket if cotton-ssdp knew it was the _only_ SSDP
implementation running on that IP address -- but if there might be
other implementations running, it needs its own search socket in order
not to steal other applications' packets.)

For that reason, _two_ MIO tokens are required; these should be passed
to [`Service::new`], which takes care of registering them with the MIO
poller. Likewise, the main polling loop can indicate readiness on
either token at any time, and the corresponding "`*_ready`" method on
`Service` -- [`Service::multicast_ready`] or [`Service::search_ready`]
-- should be called in response. All this can be seen in [the
ssdp-search-mio
example](https://github.com/pdh11/cotton/blob/main/cotton-ssdp/examples/ssdp-search-mio.rs),
from which the example code below is adapted.

# Example subscriber

This code starts a search for _all_ SSDP resources on the local
network, from all network interfaces, and stores unique ones in a
`HashMap`. The map will be populated as the MIO polling loop runs.

```rust
# use cotton_ssdp::*;
# use std::collections::HashMap;
# use std::cell::RefCell;
# const SSDP_TOKEN1: mio::Token = mio::Token(0);
# const SSDP_TOKEN2: mio::Token = mio::Token(1);
# let mut poll = mio::Poll::new().unwrap();
# #[cfg(not(miri))]
# let mut ssdp = Service::new(poll.registry(), (SSDP_TOKEN1, SSDP_TOKEN2)).unwrap();
# #[cfg(not(miri))]
    let map = RefCell::new(HashMap::new());
# #[cfg(not(miri))]
    ssdp.subscribe(
        "ssdp:all",
        Box::new(move |r| {
            let mut m = map.borrow_mut();
            if let NotificationSubtype::AliveLocation(_) =
                &r.notification_subtype
            {
                if !m.contains_key(&r.unique_service_name) {
                    m.insert(r.unique_service_name.clone(), r.clone());
                }
            }
        }),
    );
```

# Example advertiser

This code sets up an advertisement for a (fictitious) resource,
ostensibly available over HTTP on port 3333. The actual advertisements
will be sent (and any incoming searches replied to) as the MIO polling
loop runs.

(The [UPnP Device
Architecture](https://openconnectivity.org/developer/specifications/upnp-resources/upnp/archive-of-previously-published-upnp-device-architectures/)
specifies exactly what to advertise in the case of a _UPnP_ implementation;
this simpler example is not in itself compliant with that document.)

```rust
# use cotton_ssdp::*;
# const SSDP_TOKEN1: mio::Token = mio::Token(0);
# const SSDP_TOKEN2: mio::Token = mio::Token(1);
# let mut poll = mio::Poll::new().unwrap();
# #[cfg(not(miri))]
# let mut ssdp = Service::new(poll.registry(), (SSDP_TOKEN1, SSDP_TOKEN2)).unwrap();
    let uuid = uuid::Uuid::new_v4();
# #[cfg(not(miri))]
    ssdp.advertise(
        uuid.to_string(),
        cotton_ssdp::Advertisement {
            notification_type: "test".to_string(),
            location: url::Url::parse("http://127.0.0.1:3333/test").unwrap(),
        },
    );
```

Notice that the URL in the `location` field uses the localhost IP
address. The `Service` itself takes care of rewriting that, on a
per-network-interface basis, to the IP address on which each SSDP
peer will be able to reach the host where the `Service` is
running. For instance, if your Ethernet IP address is 192.168.1.3,
and your wifi IP address is 10.0.4.7, anyone listening to SSDP on
Ethernet will see `http://192.168.1.3:3333/test` and anyone listening on
wifi will see `http://10.0.4.7:3333/test`. (For how this is done, see the use
of [`url::Url::set_ip_host`] in [`Engine::on_data`].)

# The polling loop

The actual MIO polling loop, mentioned above, should be written in the
standard way common to all MIO applications. For instance, it might look like
this:

```rust
# use cotton_ssdp::*;
# use std::collections::HashMap;
# use std::cell::RefCell;
# const SSDP_TOKEN1: mio::Token = mio::Token(0);
# const SSDP_TOKEN2: mio::Token = mio::Token(1);
# let mut poll = mio::Poll::new().unwrap();
# let mut events = mio::Events::with_capacity(128);
# #[cfg(not(miri))]
# let mut ssdp = Service::new(poll.registry(), (SSDP_TOKEN1, SSDP_TOKEN2)).unwrap();
# #[cfg(not(miri))]
    loop {
        poll.poll(&mut events, None).unwrap();

        for event in &events {
            match event.token() {
                SSDP_TOKEN1 => ssdp.multicast_ready(event),
                SSDP_TOKEN2 => ssdp.search_ready(event),
                // ... other tokens as required by the application ...
                _ => (),
            }
        }
#       break;
    }
```

*/
pub struct Service {
    engine: Engine<SyncCallback>,
    multicast_socket: mio::net::UdpSocket,
    search_socket: mio::net::UdpSocket,
}

/// The type of [`new_socket`]
type SocketFn = fn(u16) -> Result<mio::net::UdpSocket, std::io::Error>;

/// The type of [`mio::Registry::register`]
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

    /// Create a new `Service`, including its two UDP sockets
    ///
    /// And registers the sockets with the [`mio::Registry`]
    ///
    /// # Errors
    ///
    /// Can return a `std::io::Error` if any of the underlying socket
    /// calls fail.
    ///
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

    pub fn multicast_ready(&mut self, _event: &mio::event::Event) {
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

    pub fn search_ready(&mut self, _event: &mio::event::Event) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

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
            socket2::Socket::set_nonblocking,
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
            socket2::Socket::set_nonblocking,
            socket2::Socket::set_reuse_address,
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
            socket2::Socket::set_nonblocking,
            socket2::Socket::set_reuse_address,
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
                if p == 0 {
                    Err(std::io::Error::new(std::io::ErrorKind::Other, "TEST"))
                } else {
                    Ok(mio::net::UdpSocket::bind(
                        "127.0.0.1:0".parse().unwrap(),
                    )
                    .unwrap())
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
                Err::<
                    std::iter::Empty<cotton_netif::NetworkEvent>,
                    std::io::Error,
                >(my_err())
            },
        );

        assert!(e.is_err());
    }


    #[test]
    #[cfg_attr(miri, ignore)]
    fn service_ok_with_no_netifs() {
        const SSDP_TOKEN1: mio::Token = mio::Token(37);
        const SSDP_TOKEN2: mio::Token = mio::Token(94);
        let poll = mio::Poll::new().unwrap();

        let e = Service::new_inner(
            poll.registry(),
            (SSDP_TOKEN1, SSDP_TOKEN2),
            new_socket,
            |r, s, t| r.register(s, t, mio::Interest::READABLE),
            || Ok(std::iter::empty::<cotton_netif::NetworkEvent>())
        );

        assert!(e.is_ok());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn service_passes_on_register_failure() {
        const SSDP_TOKEN1: mio::Token = mio::Token(37);
        const SSDP_TOKEN2: mio::Token = mio::Token(94);
        let poll = mio::Poll::new().unwrap();

        let e = Service::new_inner(
            poll.registry(),
            (SSDP_TOKEN1, SSDP_TOKEN2),
            new_socket,
            bogus_register,
            cotton_netif::get_interfaces,
        );

        assert!(e.is_err());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn service_passes_on_second_register_failure() {
        const SSDP_TOKEN1: mio::Token = mio::Token(37);
        const SSDP_TOKEN2: mio::Token = mio::Token(94);
        let poll = mio::Poll::new().unwrap();

        let e = Service::new_inner(
            poll.registry(),
            (SSDP_TOKEN1, SSDP_TOKEN2),
            new_socket,
            |_, _, t| {
                if t == SSDP_TOKEN1 {
                    Ok(())
                } else {
                    Err(my_err())
                }
            },
            cotton_netif::get_interfaces,
        );

        assert!(e.is_err());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn services_can_communicate() {
        const SSDP_TOKEN1: mio::Token = mio::Token(1);
        const SSDP_TOKEN2: mio::Token = mio::Token(2);
        const SSDP_TOKEN3: mio::Token = mio::Token(3);
        const SSDP_TOKEN4: mio::Token = mio::Token(4);
        let mut poll = mio::Poll::new().unwrap();
        let mut ssdp1 =
            Service::new(poll.registry(), (SSDP_TOKEN1, SSDP_TOKEN2)).unwrap();
        let mut ssdp2 =
            Service::new(poll.registry(), (SSDP_TOKEN3, SSDP_TOKEN4)).unwrap();

        ssdp1.advertise(
            "uuid:999",
            Advertisement {
                notification_type: "upnp::Directory:3".to_string(),
                location: url::Url::parse("http://127.0.0.1/description.xml")
                    .unwrap(),
            },
        );

        let seen = Rc::new(RefCell::new(Vec::new()));
        let seen2 = seen.clone();

        ssdp2.subscribe(
            "upnp::Directory:3",
            Box::new(move |r| {
                seen2.borrow_mut().push(r.clone());
            }),
        );

        let mut events = mio::Events::with_capacity(1024);
        while !seen.borrow().iter().any(|r| {
            r.notification_type == "upnp::Directory:3"
                && r.unique_service_name == "uuid:999"
        }) {
            poll.poll(&mut events, Some(std::time::Duration::from_secs(5)))
                .unwrap();
            assert!(!events.is_empty()); // timeout

            for event in &events {
                // We could tell, from event.token, which socket is
                // readable. But as this is a test, for coverage
                // purposes we always check everything.
                ssdp1.multicast_ready(event);
                ssdp1.search_ready(event);
                ssdp2.multicast_ready(event);
                ssdp2.search_ready(event);
            }
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn services_can_communicate_unicast() {
        const SSDP_TOKEN1: mio::Token = mio::Token(1);
        const SSDP_TOKEN2: mio::Token = mio::Token(2);
        const SSDP_TOKEN3: mio::Token = mio::Token(3);
        const SSDP_TOKEN4: mio::Token = mio::Token(4);

        let mut poll = mio::Poll::new().unwrap();
        let mut ssdp1 =
            Service::new(poll.registry(), (SSDP_TOKEN1, SSDP_TOKEN2)).unwrap();

        ssdp1.advertise(
            "uuid:999",
            Advertisement {
                notification_type: "upnp::Directory:3".to_string(),
                location: url::Url::parse("http://127.0.0.1/description.xml")
                    .unwrap(),
            },
        );

        // Get initial NOTIFY out of the way
        let mut events = mio::Events::with_capacity(1024);
        loop {
            poll.poll(
                &mut events,
                Some(std::time::Duration::from_millis(100)),
            )
            .unwrap();
            if events.is_empty() {
                break;
            }

            // We could tell, from event.token, which socket is readable. But
            // as this is a test, for coverage purposes we always check
            // everything.
            for event in &events {
                ssdp1.multicast_ready(event);
                ssdp1.search_ready(event);
            }
        }

        let mut ssdp2 =
            Service::new(poll.registry(), (SSDP_TOKEN3, SSDP_TOKEN4)).unwrap();
        let seen = Rc::new(RefCell::new(Vec::new()));
        let seen2 = seen.clone();

        // ssdp1's initial NOTIFY has already happened, so the only way we'll
        // find it here is if searching (with unicast reply) also works.
        ssdp2.subscribe(
            "upnp::Directory:3",
            Box::new(move |r| {
                seen2.borrow_mut().push(r.clone());
            }),
        );

        while !seen.borrow().iter().any(|r| {
            r.notification_type == "upnp::Directory:3"
                && r.unique_service_name == "uuid:999"
        }) {
            poll.poll(&mut events, Some(std::time::Duration::from_secs(5)))
                .unwrap();
            assert!(!events.is_empty()); // timeout

            for event in &events {
                ssdp1.multicast_ready(event);
                ssdp1.search_ready(event);
                ssdp2.multicast_ready(event);
                ssdp2.search_ready(event);
            }
        }
    }
}
