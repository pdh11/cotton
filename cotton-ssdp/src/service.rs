use crate::engine::{Callback, Engine};
use crate::udp::TargetedReceive;
use crate::udp;
use crate::{Advertisement, Notification};

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
            if let Notification::Alive {
                ref notification_type,
                ref unique_service_name,
                ref location,
            } = r {
                if !m.contains_key(unique_service_name) {
                    m.insert(unique_service_name.clone(), r.clone());
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

/// The type of [`udp::setup_socket`]
type SocketFn = fn(u16) -> Result<std::net::UdpSocket, std::io::Error>;

/// The type of [`mio::Registry::register`]
type RegisterFn = fn(
    &mio::Registry,
    &mut mio::net::UdpSocket,
    mio::Token,
) -> std::io::Result<()>;

impl Service {
    fn new_inner(
        registry: &mio::Registry,
        tokens: (mio::Token, mio::Token),
        socket: SocketFn,
        register: RegisterFn,
        interfaces: Vec<cotton_netif::NetworkEvent>,
    ) -> Result<Self, std::io::Error> {
        let mut multicast_socket = mio::net::UdpSocket::from_std(socket(1900u16)?);
        let mut search_socket = mio::net::UdpSocket::from_std(socket(0u16)?); // ephemeral port
        let mut engine = Engine::<SyncCallback>::new();

        for netif in interfaces {
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
            udp::setup_socket,
            |r, s, t| r.register(s, t, mio::Interest::READABLE),
            cotton_netif::get_interfaces()?.collect()
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

    fn my_err() -> std::io::Error {
        std::io::Error::from(std::io::ErrorKind::Other)
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
            cotton_netif::get_interfaces().unwrap().collect(),
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
                    Ok(std::net::UdpSocket::bind("127.0.0.1:0").unwrap())
                }
            },
            bogus_register,
            cotton_netif::get_interfaces().unwrap().collect(),
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
            udp::setup_socket,
            |r, s, t| r.register(s, t, mio::Interest::READABLE),
            Vec::default(),
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
            udp::setup_socket,
            bogus_register,
            cotton_netif::get_interfaces().unwrap().collect(),
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
            udp::setup_socket,
            |_, _, t| {
                if t == SSDP_TOKEN1 {
                    Ok(())
                } else {
                    Err(my_err())
                }
            },
            cotton_netif::get_interfaces().unwrap().collect(),
        );

        assert!(e.is_err());
    }
}
