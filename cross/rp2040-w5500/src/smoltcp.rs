use cotton_unique::UniqueId;
use smoltcp::{socket::dhcpv4, wire::IpCidr};

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
