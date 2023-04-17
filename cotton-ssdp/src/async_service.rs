use crate::engine::{Callback, Engine};
use crate::udp;
use crate::udp::TargetedReceive;
use crate::{Advertisement, Notification};
use futures::Stream;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

struct AsyncCallback {
    channel: mpsc::Sender<Notification>,
}

impl Callback for AsyncCallback {
    fn on_notification(&self, n: &Notification) {
        let _ = self.channel.try_send(n.clone());
    }
}

/// The type of [`udp::std::setup_socket`]
type SetupSocketFn = fn(u16) -> Result<std::net::UdpSocket, std::io::Error>;

/// The type of [`tokio::net::UdpSocket::from_std`]
type FromStdFn =
    fn(std::net::UdpSocket) -> Result<tokio::net::UdpSocket, std::io::Error>;

struct Inner {
    engine: Mutex<Engine<AsyncCallback>>,
    multicast_socket: tokio::net::UdpSocket,
    search_socket: tokio::net::UdpSocket,
}

impl Inner {
    fn new(engine: Engine<AsyncCallback>) -> Result<Inner, std::io::Error> {
        Self::new_inner(
            engine,
            udp::std::setup_socket,
            tokio::net::UdpSocket::from_std,
        )
    }

    fn new_inner(
        engine: Engine<AsyncCallback>,
        setup_socket: SetupSocketFn,
        from_std: FromStdFn,
    ) -> Result<Inner, std::io::Error> {
        let multicast_socket = setup_socket(1900u16)?;
        let search_socket = setup_socket(0u16)?;

        // @todo IPv6 https://stackoverflow.com/questions/3062205/setting-the-source-ip-for-a-udp-socket
        Ok(Inner {
            engine: Mutex::new(engine),
            multicast_socket: from_std(multicast_socket)?,
            search_socket: from_std(search_socket)?,
        })
    }
}

/// The type of [`Inner::new`]
type InnerNewFn = fn(Engine<AsyncCallback>) -> Result<Inner, std::io::Error>;

/** High-level asynchronous SSDP service using tokio.
 *
 * Handles incoming and outgoing searches using `async`, `await`, and the
 * Tokio crate.
 */
pub struct AsyncService {
    inner: Arc<Inner>,
}

impl AsyncService {
    /// Create a new `AsyncService`, including its two UDP sockets
    ///
    /// # Errors
    ///
    /// Can return a `std::io::Error` if any of the underlying socket
    /// calls fail.
    ///
    /// # Panics
    ///
    /// Will panic if the internal mutex cannot be locked; that would indicate
    /// a bug in cotton-ssdp.
    ///
    pub fn new() -> Result<Self, std::io::Error> {
        Self::new_inner(Inner::new)
    }

    fn new_inner(create: InnerNewFn) -> Result<Self, std::io::Error> {
        let inner = Arc::new(create(Engine::new())?);
        let inner2 = inner.clone();

        tokio::spawn(async move {
            loop {
                println!("select");

                tokio::select! {
                    _ = inner.multicast_socket.readable() => {
                        let mut buf = [0u8; 1500];
                        if let Ok((n, wasto, wasfrom)) =
                            inner.multicast_socket.receive_to(&mut buf) {
                            inner.engine.lock().unwrap().on_data(
                                &buf[0..n],
                                &inner.search_socket,
                                wasto,
                                wasfrom,
                            );
                        }
                    },
                    _ = inner.search_socket.readable() => {
                        let mut buf = [0u8; 1500];
                        if let Ok((n, wasto, wasfrom)) =
                            inner.search_socket.receive_to(&mut buf)
                        {
                            inner.engine.lock().unwrap().on_data(
                                &buf[0..n],
                                &inner.search_socket,
                                wasto,
                                wasfrom,
                            );
                        }
                    },
                    _ = tokio::time::sleep(
                        inner.engine.lock().unwrap().next_wakeup()
                    ) => {
                        inner.engine.lock().unwrap().wakeup(
                            &inner.search_socket);
                    },
                };
            }
        });

        Ok(AsyncService { inner: inner2 })
    }

    /// Notify the `AsyncService` of a network interface change
    ///
    /// Network interface changes can be obtained from
    /// `cotton_netif::get_interfaces` or
    /// `cotton_netif::get_interfaces_async`, filtering if desired --
    /// or, created manually.
    ///
    /// Note that `AsyncService` will do nothing if it has no network
    /// interfaces to work with.
    ///
    /// # Panics
    ///
    /// Will panic if the internal mutex cannot be locked; that would indicate
    /// a bug in cotton-ssdp.
    ///
    pub fn on_network_event(&self, event: &cotton_netif::NetworkEvent) {
        _ = self.inner.engine.lock().unwrap().on_network_event(
            event,
            &self.inner.multicast_socket,
            &self.inner.search_socket,
        );
    }

    /// Subscribe to SSDP notifications for a resource type.
    ///
    /// # Panics
    ///
    /// Will panic if the internal mutex cannot be locked; that would indicate
    /// a bug in cotton-ssdp.
    ///
    pub fn subscribe<A>(
        &mut self,
        notification_type: A,
    ) -> impl Stream<Item = Notification>
    where
        A: Into<String>,
    {
        let (snd, rcv) = mpsc::channel(100);
        self.inner.engine.lock().unwrap().subscribe(
            notification_type.into(),
            AsyncCallback { channel: snd },
            &self.inner.search_socket,
        );
        ReceiverStream::new(rcv)
    }

    /// Announce a new resource
    ///
    /// And start responding to any searches matching it.
    ///
    /// # Panics
    ///
    /// Will panic if the internal mutex cannot be locked; that would indicate
    /// a bug in cotton-ssdp.
    ///
    pub fn advertise<USN>(
        &mut self,
        unique_service_name: USN,
        advertisement: Advertisement,
    ) where
        USN: Into<String>,
    {
        self.inner.engine.lock().unwrap().advertise(
            unique_service_name.into(),
            advertisement,
            &self.inner.search_socket,
        );
    }

    /// Announce the disappearance of a resource
    ///
    /// And stop responding to searches.
    ///
    /// # Panics
    ///
    /// Will panic if the internal mutex cannot be locked; that would indicate
    /// a bug in cotton-ssdp.
    ///
    pub fn deadvertise(&mut self, unique_service_name: &str) {
        self.inner
            .engine
            .lock()
            .unwrap()
            .deadvertise(unique_service_name, &self.inner.search_socket);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn my_err() -> std::io::Error {
        std::io::Error::from(std::io::ErrorKind::Other)
    }

    fn bogus_fromstd(
        _: std::net::UdpSocket,
    ) -> Result<tokio::net::UdpSocket, std::io::Error> {
        Err(my_err())
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn service_passes_on_socket_failure() {
        let engine = Engine::<AsyncCallback>::new();
        let e = Inner::new_inner(engine, |_| Err(my_err()), bogus_fromstd);

        assert!(e.is_err());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn service_passes_on_second_socket_failure() {
        let engine = Engine::<AsyncCallback>::new();
        let e = Inner::new_inner(
            engine,
            |p| {
                if p == 0 {
                    Err(my_err())
                } else {
                    Ok(std::net::UdpSocket::bind("127.0.0.1:0").unwrap())
                }
            },
            bogus_fromstd,
        );

        assert!(e.is_err());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn service_passes_on_fromstd_failure() {
        let engine = Engine::<AsyncCallback>::new();
        let e = Inner::new_inner(
            engine,
            crate::udp::std::setup_socket,
            bogus_fromstd,
        );

        assert!(e.is_err());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn service_passes_on_second_fromstd_failure() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let engine = Engine::<AsyncCallback>::new();
                let e = Inner::new_inner(
                    engine,
                    crate::udp::std::setup_socket,
                    |s| {
                        if s.local_addr().unwrap().port() == 1900u16 {
                            tokio::net::UdpSocket::from_std(s)
                        } else {
                            Err(my_err())
                        }
                    },
                );

                assert!(e.is_err());
            });
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn service_passes_on_inner_failure() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let e = AsyncService::new_inner(|_| Err(my_err()));
                assert!(e.is_err());
            });
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn service_succeeds() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let e = AsyncService::new();
                assert!(e.is_ok());
            });
    }
}
