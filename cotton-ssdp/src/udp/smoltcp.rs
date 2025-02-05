use super::{Error, Syscall};
use smoltcp::iface::Interface;
use smoltcp::wire;

/// Wrap a smoltcp `Interface` so it can be used by cotton-ssdp
pub struct WrappedInterface<'a>(core::cell::RefCell<&'a mut Interface>);

impl<'a> WrappedInterface<'a> {
    /// Create a new `WrappedInterface`
    ///
    /// The interface and device are mutably borrowed, so the
    /// `WrappedInterface` should be short-lived.
    pub fn new(iface: &'a mut Interface) -> Self {
        Self(core::cell::RefCell::new(iface))
    }
}

impl super::Multicast for WrappedInterface<'_> {
    fn join_multicast_group(
        &self,
        multicast_address: &core::net::IpAddr,
        _interface: cotton_netif::InterfaceIndex,
    ) -> Result<(), Error> {
        self.0
            .borrow_mut()
            .join_multicast_group(*multicast_address)
            .map(|_| ())
            .map_err(|e| Error::SmoltcpMulticast(Syscall::JoinMulticast, e))
    }

    fn leave_multicast_group(
        &self,
        _multicast_address: &core::net::IpAddr,
        _interface: cotton_netif::InterfaceIndex,
    ) -> Result<(), Error> {
        Err(Error::NotImplemented)
    }
}

/// Wrap a smoltcp socket so it can be used by cotton-ssdp
pub struct WrappedSocket<'a, 'b>(
    core::cell::RefCell<&'a mut smoltcp::socket::udp::Socket<'b>>,
);

impl<'a, 'b> WrappedSocket<'a, 'b> {
    /// Create a new `WrappedSocket`
    ///
    /// The socket is mutably borrowed, so the `WrappedSocket` should be
    /// short-lived.
    pub fn new(socket: &'a mut smoltcp::socket::udp::Socket<'b>) -> Self {
        Self(core::cell::RefCell::new(socket))
    }
}

impl super::TargetedSend for WrappedSocket<'_, '_> {
    fn send_with<F>(
        &self,
        size: usize,
        to: &core::net::SocketAddr,
        _from: &core::net::IpAddr,
        f: F,
    ) -> Result<(), Error>
    where
        F: FnOnce(&mut [u8]) -> usize,
    {
        let ep: wire::IpEndpoint = (*to).into();

        self.0
            .borrow_mut()
            .send_with(size, ep, f)
            .map_err(Error::SmoltcpUdpSend)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "std")]
    use super::super::Multicast;
    #[cfg(feature = "std")]
    use super::super::TargetedSend;
    use super::*;

    #[test]
    #[cfg(feature = "std")]
    fn join_multicast_succeeds() {
        let mut device =
            smoltcp::phy::Loopback::new(smoltcp::phy::Medium::Ethernet);
        let mac_address = [0, 1, 2, 3, 4, 5];
        let config = smoltcp::iface::Config::new(
            smoltcp::wire::EthernetAddress::from_bytes(&mac_address[..])
                .into(),
        );
        let mut iface = smoltcp::iface::Interface::new(
            config,
            &mut device,
            smoltcp::time::Instant::ZERO,
        );
        let wi = WrappedInterface::new(&mut iface);

        let rc = wi.join_multicast_group(
            &core::net::IpAddr::V4(core::net::Ipv4Addr::new(
                239, 255, 255, 250,
            )),
            cotton_netif::InterfaceIndex(
                core::num::NonZeroU32::new(1).unwrap(),
            ),
        );
        assert!(rc.is_ok());
    }

    #[test]
    #[cfg(feature = "std")]
    fn join_multicast_fails() {
        let mut device =
            smoltcp::phy::Loopback::new(smoltcp::phy::Medium::Ethernet);
        let mac_address = [0, 1, 2, 3, 4, 5];
        let config = smoltcp::iface::Config::new(
            smoltcp::wire::EthernetAddress::from_bytes(&mac_address[..])
                .into(),
        );
        let mut iface = smoltcp::iface::Interface::new(
            config,
            &mut device,
            smoltcp::time::Instant::ZERO,
        );
        let wi = WrappedInterface::new(&mut iface);

        // 4 multicast groups per iface supported by default; so let's add 5
        for i in 0..4 {
            let rc = wi.join_multicast_group(
                &core::net::IpAddr::V4(core::net::Ipv4Addr::new(
                    239,
                    255,
                    255,
                    250 + i,
                )),
                cotton_netif::InterfaceIndex(
                    core::num::NonZeroU32::new(1).unwrap(),
                ),
            );
            assert!(rc.is_ok());
        }
        let rc = wi.join_multicast_group(
            &core::net::IpAddr::V4(core::net::Ipv4Addr::new(
                239, 255, 255, 254,
            )),
            cotton_netif::InterfaceIndex(
                core::num::NonZeroU32::new(1).unwrap(),
            ),
        );

        assert!(rc.is_err());
    }

    #[test]
    #[cfg(feature = "std")]
    fn leave_multicast_fails() {
        let mut device =
            smoltcp::phy::Loopback::new(smoltcp::phy::Medium::Ethernet);
        let mac_address = [0, 1, 2, 3, 4, 5];
        let config = smoltcp::iface::Config::new(
            smoltcp::wire::EthernetAddress::from_bytes(&mac_address[..])
                .into(),
        );
        let mut iface = smoltcp::iface::Interface::new(
            config,
            &mut device,
            smoltcp::time::Instant::ZERO,
        );
        iface.update_ip_addrs(|a| {
            a.push(smoltcp::wire::IpCidr::Ipv4(smoltcp::wire::Ipv4Cidr::new(
                smoltcp::wire::Ipv4Address::new(10, 0, 0, 1),
                8,
            )))
            .unwrap();
        });
        let wi = WrappedInterface::new(&mut iface);

        let rc = wi.leave_multicast_group(
            &core::net::IpAddr::V4(core::net::Ipv4Addr::new(
                239, 255, 255, 250,
            )),
            cotton_netif::InterfaceIndex(
                core::num::NonZeroU32::new(1).unwrap(),
            ),
        );
        assert!(rc.is_err());
    }

    #[cfg(feature = "std")]
    fn sender(buf: &mut [u8]) -> usize {
        buf[0] = 0;
        1
    }

    #[test]
    #[cfg(feature = "std")]
    fn send_succeeds() {
        let mut device =
            smoltcp::phy::Loopback::new(smoltcp::phy::Medium::Ethernet);
        let mac_address = [0, 1, 2, 3, 4, 5];
        let config = smoltcp::iface::Config::new(
            smoltcp::wire::EthernetAddress::from_bytes(&mac_address[..])
                .into(),
        );
        let mut iface = smoltcp::iface::Interface::new(
            config,
            &mut device,
            smoltcp::time::Instant::ZERO,
        );
        iface.update_ip_addrs(|a| {
            a.push(smoltcp::wire::IpCidr::Ipv4(smoltcp::wire::Ipv4Cidr::new(
                smoltcp::wire::Ipv4Address::new(10, 0, 0, 1),
                8,
            )))
            .unwrap();
        });
        let mut sockets = smoltcp::iface::SocketSet::new(vec![]);

        let udp_rx_buffer = smoltcp::socket::udp::PacketBuffer::new(
            vec![smoltcp::socket::udp::PacketMetadata::EMPTY; 4],
            vec![0; 1024],
        );
        let udp_tx_buffer = smoltcp::socket::udp::PacketBuffer::new(
            vec![smoltcp::socket::udp::PacketMetadata::EMPTY],
            vec![0; 1024],
        );
        let udp_socket =
            smoltcp::socket::udp::Socket::new(udp_rx_buffer, udp_tx_buffer);
        let udp_handle = sockets.add(udp_socket);

        let mut udp_socket =
            sockets.get_mut::<smoltcp::socket::udp::Socket>(udp_handle);
        _ = udp_socket.bind(1900);
        let ws = WrappedSocket::new(&mut udp_socket);

        let rc = ws.send_with(
            20,
            &core::net::SocketAddr::V4(core::net::SocketAddrV4::new(
                core::net::Ipv4Addr::new(10, 0, 0, 2),
                12345,
            )),
            &core::net::IpAddr::V4(core::net::Ipv4Addr::UNSPECIFIED),
            sender,
        );

        assert!(rc.is_ok());
    }

    #[test]
    #[cfg(feature = "std")]
    fn send_fails() {
        let mut device =
            smoltcp::phy::Loopback::new(smoltcp::phy::Medium::Ethernet);
        let mac_address = [0, 1, 2, 3, 4, 5];
        let config = smoltcp::iface::Config::new(
            smoltcp::wire::EthernetAddress::from_bytes(&mac_address[..])
                .into(),
        );
        let mut iface = smoltcp::iface::Interface::new(
            config,
            &mut device,
            smoltcp::time::Instant::ZERO,
        );
        iface.update_ip_addrs(|a| {
            a.push(smoltcp::wire::IpCidr::Ipv4(smoltcp::wire::Ipv4Cidr::new(
                smoltcp::wire::Ipv4Address::new(10, 0, 0, 1),
                8,
            )))
            .unwrap();
        });
        let mut sockets = smoltcp::iface::SocketSet::new(vec![]);

        let udp_rx_buffer = smoltcp::socket::udp::PacketBuffer::new(
            vec![smoltcp::socket::udp::PacketMetadata::EMPTY; 4],
            vec![0; 1024],
        );
        let udp_tx_buffer = smoltcp::socket::udp::PacketBuffer::new(
            vec![smoltcp::socket::udp::PacketMetadata::EMPTY],
            vec![0; 1024],
        );
        let udp_socket =
            smoltcp::socket::udp::Socket::new(udp_rx_buffer, udp_tx_buffer);
        let udp_handle = sockets.add(udp_socket);

        let mut udp_socket =
            sockets.get_mut::<smoltcp::socket::udp::Socket>(udp_handle);
        //
        // No bound local port => send_with returns Unaddressable
        //_ = udp_socket.bind(1900);
        //
        let ws = WrappedSocket::new(&mut udp_socket);

        let rc = ws.send_with(
            20,
            &core::net::SocketAddr::V4(core::net::SocketAddrV4::new(
                core::net::Ipv4Addr::new(10, 0, 0, 2),
                12345,
            )),
            &core::net::IpAddr::V4(core::net::Ipv4Addr::UNSPECIFIED),
            sender,
        );

        assert!(rc.is_err());
    }
}
