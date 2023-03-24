use crate::engine::{Callback, Engine};
use crate::udp::TargetedReceive;
use crate::{Advertisement, Notification};
use cotton_netif::get_interfaces_async;
use futures::Stream;
use futures_util::StreamExt;
use std::error::Error;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::udp::setup_socket;

struct AsyncCallback {
    channel: mpsc::Sender<Notification>,
}

impl Callback for AsyncCallback {
    fn on_notification(&self, n: &Notification) {
        let _ = self.channel.try_send(n.clone());
    }
}

struct Inner {
    engine: Mutex<Engine<AsyncCallback>>,
    multicast_socket: tokio::net::UdpSocket,
    search_socket: tokio::net::UdpSocket,
}

impl Inner {
    fn new(
        engine: Engine<AsyncCallback>,
    ) -> Result<Inner, std::io::Error> {
        let multicast_socket = setup_socket(1900u16)?;
        let search_socket = setup_socket(0u16)?;

        // @todo IPv6 https://stackoverflow.com/questions/3062205/setting-the-source-ip-for-a-udp-socket
        Ok(Inner {
            engine: Mutex::new(engine),
            multicast_socket: tokio::net::UdpSocket::from_std(multicast_socket)?,
            search_socket: tokio::net::UdpSocket::from_std(search_socket)?,
        })
    }
}

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
    pub async fn new() -> Result<Self, Box<dyn Error>> {
        let mut s = get_interfaces_async()?;
        let inner = Arc::new(Inner::new(Engine::new())?);
        let inner2 = inner.clone();

        tokio::spawn(async move {
            loop {
                println!("select");

                tokio::select! {
                    e = s.next() => if let Some(Ok(event)) = e {
                        inner.engine.lock().unwrap().on_interface_event(
                            event,
                            &inner.multicast_socket,
                            &inner.search_socket,
                        )
                            .unwrap_or_else(
                                |err| println!("SSDP error {err}"));
                    },
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
