use cotton_unique::UniqueId;
use smoltcp::{socket::dhcpv4, time::Instant, wire::IpCidr};

/// A helper container for a TCP/IP stack and some of its metadata
pub struct Stack {
    /// The underlying Smoltcp implementation
    pub interface: smoltcp::iface::Interface,
    /// Persistent socket data for active sockets
    pub socket_set: smoltcp::iface::SocketSet<'static>,
    dhcp_handle: smoltcp::iface::SocketHandle,
}

impl Stack {
    /// Construct a new TCP Stack abstraction
    ///
    /// From an interface, a MAC address, and some storage for the
    /// socket metadata.
    pub fn new<D: smoltcp::phy::Device>(
        device: &mut D,
        unique: &UniqueId,
        mac_address: &[u8; 6],
        sockets: &'static mut [smoltcp::iface::SocketStorage<'static>],
        now: smoltcp::time::Instant,
    ) -> Stack {
        let mut config = smoltcp::iface::Config::new(
            smoltcp::wire::EthernetAddress::from_bytes(mac_address).into(),
        );
        config.random_seed = unique.id(b"smoltcp-config-random");
        let interface = smoltcp::iface::Interface::new(config, device, now);
        let mut socket_set = smoltcp::iface::SocketSet::new(sockets);

        let mut dhcp_socket = smoltcp::socket::dhcpv4::Socket::new();
        let mut retry_config = smoltcp::socket::dhcpv4::RetryConfig::default();
        retry_config.discover_timeout = smoltcp::time::Duration::from_secs(2);
        retry_config.initial_request_timeout =
            smoltcp::time::Duration::from_millis(500);
        retry_config.request_retries = 10;
        dhcp_socket.set_retry_config(retry_config);
        let dhcp_handle = socket_set.add(dhcp_socket);

        Stack {
            interface,
            socket_set,
            dhcp_handle,
        }
    }

    /// Poll the interface for new packets, then the DHCP socket
    pub fn poll<D: smoltcp::phy::Device>(
        &mut self,
        now: smoltcp::time::Instant,
        device: &mut D,
    ) -> Option<smoltcp::time::Duration> {
        while self.interface.poll(now, device, &mut self.socket_set) {
            self.poll_dhcp();
        }
        self.interface.poll_delay(now, &self.socket_set)
    }

    /// Poll the DHCP socket for any updates
    ///
    /// Smoltcp's `dhcpv4::Socket` takes care of retrying/rebinding
    fn poll_dhcp(&mut self) {
        let socket =
            self.socket_set.get_mut::<dhcpv4::Socket>(self.dhcp_handle);
        let event = socket.poll();
        match event {
            None => {}
            Some(dhcpv4::Event::Configured(config)) => {
                defmt::println!("DHCP config acquired!");
                defmt::println!("IP address:      {}", config.address);

                self.interface.update_ip_addrs(|addrs| {
                    addrs.clear();
                    addrs.push(IpCidr::Ipv4(config.address)).unwrap();
                });

                if let Some(router) = config.router {
                    self.interface
                        .routes_mut()
                        .add_default_ipv4_route(router)
                        .unwrap();
                } else {
                    self.interface.routes_mut().remove_default_ipv4_route();
                }
            }
            Some(dhcpv4::Event::Deconfigured) => {
                defmt::println!("DHCP lost config!");
                self.interface.update_ip_addrs(|addrs| {
                    addrs.clear();
                });
            }
        }
    }
}

/// Encapsulating the SSDP retransmit process
///
/// The idea is, every 15 minutes or so, send a few repeated salvos of
/// notification messages. The interval between salvos is randomised to
/// help avoid network congestion.
///
pub struct RefreshTimer {
    next_salvo: Instant,
    phase: u8,
}

impl RefreshTimer {
    /// Create a new [`RefreshTimer`]
    ///
    #[must_use]
    pub fn new(now: Instant) -> Self {
        Self {
            next_salvo: now,
            phase: 1u8,
        }
    }

    /// Obtain the desired delay before the next refresh is needed
    #[must_use]
    pub fn next_refresh(&self, now: Instant) -> smoltcp::time::Duration {
        if now > self.next_salvo {
            smoltcp::time::Duration::ZERO
        } else {
            self.next_salvo - now
        }
    }

    /// Update the refresh timer
    ///
    /// The desired timeout duration can be obtained from
    /// [`RefreshTimer::next_refresh`].
    ///
    pub fn update_refresh(&mut self, now: Instant) {
        if self.next_salvo > now {
            return;
        }
        let random_offset = (now.micros() % 6) as u64; // not really random
        let period_sec = if self.phase == 0 { 800 } else { 1 } + random_offset;
        self.next_salvo += smoltcp::time::Duration::from_secs(period_sec);
        self.phase = (self.phase + 1) % 4;
    }

    /// Reset the refresh timer (e.g. if network has gone away and come back)
    pub fn reset(&mut self, now: Instant) {
        *self = Self::new(now)
    }
}
