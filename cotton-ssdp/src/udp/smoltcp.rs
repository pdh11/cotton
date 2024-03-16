use super::{Error, Syscall};
use smoltcp::iface::Interface;
use smoltcp::phy::Device;
use smoltcp::wire;

/// A newtype to assist in converting between std and smoltcp IPv4 addresses
///
/// `std::net::Ipv4Addr` <=> `smoltcp::wire::Ipv4Address`
///
/// Because we don't own either `std::Ipv4Addr` or
/// `smoltcp::wire::Ipv4Address`, we can't directly write the `From`
/// or `Into`. But we can invent an intermediate type that can be
/// `From`'d or `Into`'d to either, so the conversions are a two-step
/// process, but client code still doesn't have to get its hands
/// dirty:
///
/// ```rust
/// use smoltcp::wire;
/// use cotton_ssdp::udp::smoltcp::GenericIpv4Address;
/// let s = wire::Ipv4Address::new(192, 168, 0, 110);
/// let g: no_std_net::Ipv4Addr = GenericIpv4Address::from(s).into();
/// assert_eq!(g, no_std_net::Ipv4Addr::new(192, 168, 0, 110));
/// ```
///
/// Hopefully once IP-addresses-in-core lands
/// <https://github.com/rust-lang/rust/pull/104265>, smoltcp will be able
/// to use the std (core) types, and all these tiresome conversions
/// will go away.
///
/// See also [`GenericIpAddress`] for `IpAddr` <=> `IpAddress`, and [`GenericSocketAddr`] for `SocketAddr` <=> `IpEndpoint`.
///
pub struct GenericIpv4Address(no_std_net::Ipv4Addr);

impl From<wire::Ipv4Address> for GenericIpv4Address {
    fn from(ip: wire::Ipv4Address) -> Self {
        Self(no_std_net::Ipv4Addr::from(ip.0))
    }
}

impl From<GenericIpv4Address> for wire::Ipv4Address {
    fn from(ip: GenericIpv4Address) -> Self {
        Self(ip.0.octets())
    }
}

impl From<no_std_net::Ipv4Addr> for GenericIpv4Address {
    fn from(ip: no_std_net::Ipv4Addr) -> Self {
        Self(ip)
    }
}

impl From<GenericIpv4Address> for no_std_net::Ipv4Addr {
    fn from(ip: GenericIpv4Address) -> Self {
        ip.0
    }
}

/// A newtype to assist in converting between std and smoltcp IP addresses
///
/// `std::net::IpAddr` <=> `smoltcp::wire::IpAddress`
///
/// Because we don't own either `std::IpAddr` or
/// `smoltcp::wire::IpAddress`, we can't directly write the `From`
/// or `Into`. But we can invent an intermediate type that can be
/// `From`'d or `Into`'d to either, so the conversions are a two-step
/// process, but client code still doesn't have to get its hands
/// dirty:
///
/// ```rust
/// use smoltcp::wire;
/// use cotton_ssdp::udp::smoltcp::GenericIpAddress;
/// let s = wire::IpAddress::v4(169, 254, 11, 11);
/// let g: no_std_net::IpAddr = GenericIpAddress::from(s).into();
/// assert_eq!(
///     g,
///     no_std_net::IpAddr::V4(no_std_net::Ipv4Addr::new(169, 254, 11, 11))
/// );
/// ```
///
/// Hopefully once IP-addresses-in-core lands
/// <https://github.com/rust-lang/rust/pull/104265>, smoltcp will be able
/// to use the std (core) types, and all these tiresome conversions
/// will go away.
///
/// See also [`GenericIpv4Address`] for `Ipv4Addr` <=> `Ipv4Address`, and [`GenericSocketAddr`] for `SocketAddr` <=> `IpEndpoint`.
///
pub struct GenericIpAddress(no_std_net::IpAddr);

impl From<wire::IpAddress> for GenericIpAddress {
    fn from(ip: wire::IpAddress) -> Self {
        // smoltcp may or may not have been compiled with IPv6 support, and
        // we can't tell
        #[allow(unreachable_patterns)]
        match ip {
            wire::IpAddress::Ipv4(v4) => {
                Self(no_std_net::IpAddr::V4(no_std_net::Ipv4Addr::from(v4.0)))
            }
            _ => {
                Self(no_std_net::IpAddr::V4(no_std_net::Ipv4Addr::UNSPECIFIED))
            }
        }
    }
}

impl From<GenericIpAddress> for wire::IpAddress {
    fn from(ip: GenericIpAddress) -> Self {
        match ip.0 {
            no_std_net::IpAddr::V4(v4) => {
                Self::Ipv4(wire::Ipv4Address(v4.octets()))
            }
            no_std_net::IpAddr::V6(_) => {
                Self::Ipv4(wire::Ipv4Address::UNSPECIFIED)
            }
        }
    }
}

impl From<no_std_net::IpAddr> for GenericIpAddress {
    fn from(ip: no_std_net::IpAddr) -> Self {
        Self(ip)
    }
}

impl From<GenericIpAddress> for no_std_net::IpAddr {
    fn from(ip: GenericIpAddress) -> Self {
        ip.0
    }
}

/// A newtype to assist in converting between std and smoltcp IP/port pairs
///
/// `std::net::SocketAddr` <=> `smoltcp::wire::IpEndpoint`
///
/// Because we don't own either `std::SocketAddr` or
/// `smoltcp::wire::IpEndpoint`, we can't directly write the `From` or
/// `Into`. But we can invent an intermediate type that can be
/// `From`'d or `Into`'d to either, so the conversions are a two-step
/// process, but client code still doesn't have to get its hands
/// dirty:
///
/// ```rust
/// use smoltcp::wire;
/// use cotton_ssdp::udp::smoltcp::GenericSocketAddr;
/// let s = wire::IpEndpoint::new(wire::IpAddress::v4(169, 254, 11, 11), 8080);
/// let g: no_std_net::SocketAddr = GenericSocketAddr::from(s).into();
/// assert_eq!(
///     g,
///     no_std_net::SocketAddr::V4(no_std_net::SocketAddrV4::new(
///         no_std_net::Ipv4Addr::new(169, 254, 11, 11),
///         8080
///     ))
/// );
/// ```
///
/// Hopefully once IP-addresses-in-core lands
/// <https://github.com/rust-lang/rust/pull/104265>, smoltcp will be able
/// to use the std (core) types, and all these tiresome conversions
/// will go away.
///
/// See also [`GenericIpv4Address`] for `Ipv4Addr` <=> `Ipv4Address`, and [`GenericIpAddress`] for `IpAddr` <=> `IpAddress`.
///
pub struct GenericSocketAddr(no_std_net::SocketAddr);

impl From<wire::IpEndpoint> for GenericSocketAddr {
    fn from(ep: wire::IpEndpoint) -> Self {
        // smoltcp may or may not have been compiled with IPv6 support, and
        // we can't tell
        #[allow(unreachable_patterns)]
        match ep.addr {
            wire::IpAddress::Ipv4(v4) => Self(no_std_net::SocketAddr::V4(
                no_std_net::SocketAddrV4::new(
                    GenericIpv4Address::from(v4).into(),
                    ep.port,
                ),
            )),
            _ => Self(no_std_net::SocketAddr::V4(
                no_std_net::SocketAddrV4::new(
                    no_std_net::Ipv4Addr::UNSPECIFIED,
                    ep.port,
                ),
            )),
        }
    }
}

impl From<GenericSocketAddr> for wire::IpEndpoint {
    fn from(sa: GenericSocketAddr) -> Self {
        match sa.0 {
            no_std_net::SocketAddr::V4(v4) => Self::new(
                wire::IpAddress::Ipv4(GenericIpv4Address(*v4.ip()).into()),
                v4.port(),
            ),
            no_std_net::SocketAddr::V6(_) => Self::new(
                wire::IpAddress::Ipv4(wire::Ipv4Address::UNSPECIFIED),
                0,
            ),
        }
    }
}

impl From<no_std_net::SocketAddr> for GenericSocketAddr {
    fn from(ip: no_std_net::SocketAddr) -> Self {
        Self(ip)
    }
}

impl From<GenericSocketAddr> for no_std_net::SocketAddr {
    fn from(ip: GenericSocketAddr) -> Self {
        ip.0
    }
}

/// Wrap a smoltcp `Interface` so it can be used by cotton-ssdp
pub struct WrappedInterface<'a, D: Device>(
    core::cell::RefCell<&'a mut Interface>,
    core::cell::RefCell<&'a mut D>,
    smoltcp::time::Instant,
);

impl<'a, D: Device> WrappedInterface<'a, D> {
    /// Create a new `WrappedInterface`
    ///
    /// The interface and device are mutably borrowed, so the
    /// `WrappedInterface` should be short-lived.
    pub fn new(
        iface: &'a mut Interface,
        device: &'a mut D,
        now: smoltcp::time::Instant,
    ) -> Self {
        Self(
            core::cell::RefCell::new(iface),
            core::cell::RefCell::new(device),
            now,
        )
    }
}

impl<'a, D: Device> super::Multicast for WrappedInterface<'a, D> {
    fn join_multicast_group(
        &self,
        multicast_address: &no_std_net::IpAddr,
        _interface: cotton_netif::InterfaceIndex,
    ) -> Result<(), Error> {
        self.0
            .borrow_mut()
            .join_multicast_group::<D, wire::IpAddress>(
                &mut self.1.borrow_mut(),
                GenericIpAddress::from(*multicast_address).into(),
                self.2,
            )
            .map(|_| ())
            .map_err(|e| Error::SmoltcpMulticast(Syscall::JoinMulticast, e))
    }

    fn leave_multicast_group(
        &self,
        _multicast_address: &no_std_net::IpAddr,
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

impl<'a, 'b> super::TargetedSend for WrappedSocket<'a, 'b> {
    fn send_with<F>(
        &self,
        size: usize,
        to: &no_std_net::SocketAddr,
        _from: &no_std_net::IpAddr,
        f: F,
    ) -> Result<(), Error>
    where
        F: FnOnce(&mut [u8]) -> usize,
    {
        let ep: wire::IpEndpoint = GenericSocketAddr::from(*to).into();
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
    fn ipv4_smoltcp_to_std() {
        let s = wire::Ipv4Address::new(192, 168, 0, 110);
        let g: no_std_net::Ipv4Addr = GenericIpv4Address::from(s).into();
        assert_eq!(g, no_std_net::Ipv4Addr::new(192, 168, 0, 110));
    }

    #[test]
    fn ipv4_std_to_smoltcp() {
        let s = no_std_net::Ipv4Addr::new(192, 168, 0, 110);
        let g: wire::Ipv4Address = GenericIpv4Address::from(s).into();
        assert_eq!(g, wire::Ipv4Address::new(192, 168, 0, 110));
    }

    #[test]
    fn ip_smoltcp_to_std() {
        let s = wire::IpAddress::v4(169, 254, 11, 11);
        let g: no_std_net::IpAddr = GenericIpAddress::from(s).into();
        assert_eq!(
            g,
            no_std_net::IpAddr::V4(no_std_net::Ipv4Addr::new(
                169, 254, 11, 11
            ))
        );
    }

    #[test]
    fn ip_std_to_smoltcp() {
        let s = no_std_net::IpAddr::V4(no_std_net::Ipv4Addr::new(
            169, 254, 11, 11,
        ));
        let g: wire::IpAddress = GenericIpAddress::from(s).into();
        assert_eq!(g, wire::IpAddress::v4(169, 254, 11, 11));
    }

    #[test]
    fn ip_std_to_smoltcp_copes_with_ipv6() {
        let s = no_std_net::IpAddr::V6(no_std_net::Ipv6Addr::LOCALHOST);
        let g: wire::IpAddress = GenericIpAddress::from(s).into();
        assert_eq!(g, wire::IpAddress::v4(0, 0, 0, 0)); // unspecified
    }

    #[test]
    fn socketaddr_smoltcp_to_std() {
        let s =
            wire::IpEndpoint::new(wire::IpAddress::v4(169, 254, 11, 11), 8080);
        let g: no_std_net::SocketAddr = GenericSocketAddr::from(s).into();
        assert_eq!(
            g,
            no_std_net::SocketAddr::V4(no_std_net::SocketAddrV4::new(
                no_std_net::Ipv4Addr::new(169, 254, 11, 11),
                8080
            ))
        );
    }

    #[test]
    fn socketaddr_std_to_smoltcp() {
        let s = no_std_net::SocketAddr::V4(no_std_net::SocketAddrV4::new(
            no_std_net::Ipv4Addr::new(169, 254, 11, 11),
            8080,
        ));
        let g: wire::IpEndpoint = GenericSocketAddr::from(s).into();
        assert_eq!(
            g,
            wire::IpEndpoint::new(wire::IpAddress::v4(169, 254, 11, 11), 8080)
        );
    }

    #[test]
    fn socketaddr_std_to_smoltcp_copes_with_ipv6() {
        let s = no_std_net::SocketAddr::V6(no_std_net::SocketAddrV6::new(
            no_std_net::Ipv6Addr::LOCALHOST,
            8080,
            0,
            1,
        ));
        let g: wire::IpEndpoint = GenericSocketAddr::from(s).into();
        assert_eq!(
            g,
            wire::IpEndpoint::new(wire::IpAddress::v4(0, 0, 0, 0), 0)
        );
    }

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
        let wi = WrappedInterface::new(
            &mut iface,
            &mut device,
            smoltcp::time::Instant::ZERO,
        );

        let rc = wi.join_multicast_group(
            &no_std_net::IpAddr::V4(no_std_net::Ipv4Addr::new(
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
        let wi = WrappedInterface::new(
            &mut iface,
            &mut device,
            smoltcp::time::Instant::ZERO,
        );

        // 4 multicast groups per iface supported by default; so let's add 5
        for i in 0..4 {
            let rc = wi.join_multicast_group(
                &no_std_net::IpAddr::V4(no_std_net::Ipv4Addr::new(
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
            &no_std_net::IpAddr::V4(no_std_net::Ipv4Addr::new(
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
        let wi = WrappedInterface::new(
            &mut iface,
            &mut device,
            smoltcp::time::Instant::ZERO,
        );

        let rc = wi.leave_multicast_group(
            &no_std_net::IpAddr::V4(no_std_net::Ipv4Addr::new(
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
            &no_std_net::SocketAddr::V4(no_std_net::SocketAddrV4::new(
                no_std_net::Ipv4Addr::new(10, 0, 0, 2),
                12345,
            )),
            &no_std_net::IpAddr::V4(no_std_net::Ipv4Addr::UNSPECIFIED),
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
            &no_std_net::SocketAddr::V4(no_std_net::SocketAddrV4::new(
                no_std_net::Ipv4Addr::new(10, 0, 0, 2),
                12345,
            )),
            &no_std_net::IpAddr::V4(no_std_net::Ipv4Addr::UNSPECIFIED),
            sender,
        );

        assert!(rc.is_err());
    }
}
