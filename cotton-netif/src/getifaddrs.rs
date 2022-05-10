use super::*;
use nix::ifaddrs;
use nix::net::if_::InterfaceFlags;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};

/** Obtain the current list of network interfaces

The returned iterator provides a sequence of [NetworkEvent]
objects, each describing a network interface (as
[NetworkEvent::NewLink]) or an address on that interface (as
[NetworkEvent::NewAddr]). An interface may have several addresses,
both IPv4 and IPv6. In all cases, the [NetworkEvent::NewLink] event
describing an interface, will be produced before that interface's
[NetworkEvent::NewAddr] event or events.

As the list is a snapshot of the current state, no [NetworkEvent::DelLink]
or [NetworkEvent::DelAddr] events will be generated.

For a simple listing of the returned information, just use println:

```rust
# use cotton_netif::*;
for e in get_interfaces()? {
    println!("{:?}", e);
}
# Ok::<(), std::io::Error>(())
```

The output of that program on an example system might look like this (notice
that interface `eno1` has three different addresses):

```text
NewLink(InterfaceIndex(1), "lo", UP | LOOPBACK | RUNNING)
NewLink(InterfaceIndex(2), "eno1", UP | BROADCAST | RUNNING | MULTICAST)
NewLink(InterfaceIndex(3), "eno2", UP | BROADCAST | RUNNING | MULTICAST)
NewLink(InterfaceIndex(4), "imp0", UP | POINTTOPOINT | MULTICAST)
NewLink(InterfaceIndex(5), "docker0", UP | BROADCAST | MULTICAST)
NewAddr(InterfaceIndex(1), 127.0.0.1, 8)
NewAddr(InterfaceIndex(2), 192.168.168.15, 24)
NewAddr(InterfaceIndex(2), 169.254.100.100, 16)
NewAddr(InterfaceIndex(4), 169.254.0.1, 24)
NewAddr(InterfaceIndex(5), 172.17.0.1, 16)
NewAddr(InterfaceIndex(1), ::1, 128)
NewAddr(InterfaceIndex(2), fe80::fac0:2a3b:d68e:80a2, 64)
```

As another example, here is how to list all available
multicast-capable interfaces:

```rust
# use cotton_netif::*;
for name in get_interfaces()?
    .filter_map(|e| match e {
        NetworkEvent::NewLink(_i, name, flags)
            if flags.contains(Flags::RUNNING | Flags::UP | Flags::MULTICAST)
                => Some(name),
        _ => None,
    }) {
    println!("New multicast-capable interface: {}", name);
};
# Ok::<(), std::io::Error>(())
```

 */
pub fn get_interfaces(
) -> Result<impl Iterator<Item = NetworkEvent>, std::io::Error> {
    get_interfaces_inner(ifaddrs::getifaddrs)
}

fn get_interfaces_inner(
    getifaddrs: fn() -> nix::Result<ifaddrs::InterfaceAddressIterator>,
) -> Result<impl Iterator<Item = NetworkEvent>, std::io::Error> {
    Ok(InterfaceMap::new(getifaddrs()?))
}

/** Manage the mapping between interface names and indexes
 */
struct InterfaceMap<ITER>
where
    ITER: Iterator<Item = ifaddrs::InterfaceAddress>,
{
    iter: ITER,
    pending: Option<NetworkEvent>,
    index_map: HashMap<String, u32>,
    next_index: u32,
}

impl<ITER> Iterator for InterfaceMap<ITER>
where
    ITER: Iterator<Item = ifaddrs::InterfaceAddress>,
{
    type Item = NetworkEvent;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pending.is_some() {
            self.pending.take()
        } else {
            loop {
                let ifaddr = self.iter.next()?;
                let (new_link, new_addr) = self.check(ifaddr);
                if new_link.is_some() {
                    self.pending = new_addr;
                    return new_link;
                }
                if new_addr.is_some() {
                    return new_addr;
                }
                // otherwise go round again
            }
        }
    }
}

impl<ITER> InterfaceMap<ITER>
where
    ITER: Iterator<Item = ifaddrs::InterfaceAddress>,
{
    fn new(iter: ITER) -> InterfaceMap<ITER> {
        InterfaceMap {
            iter,
            pending: None,
            index_map: Default::default(),
            next_index: 1,
        }
    }

    /** Process one InterfaceAddress result from getifaddrs
     *
     * One result can give rise to at most one NewLink message and one NewAddr,
     * so we return a 2-tuple of Options.
     */
    fn check(
        &mut self,
        ifaddr: ifaddrs::InterfaceAddress,
    ) -> (Option<NetworkEvent>, Option<NetworkEvent>) {
        /* Undo Linux aliasing: "eth0:1" is "eth0" really. */
        let name = match ifaddr.interface_name.split_once(':') {
            None => ifaddr.interface_name,
            Some((prefix, _alias)) => prefix.to_string(),
        };

        let (index, link_message) = match self.index_map.entry(name) {
            Entry::Occupied(e) => (*e.get(), None),
            Entry::Vacant(e) => {
                let index = self.next_index;
                self.next_index += 1;
                let name = e.key().clone();
                e.insert(index);
                (
                    index,
                    Some(NetworkEvent::NewLink(
                        InterfaceIndex(index),
                        name,
                        map_interface_flags(&ifaddr.flags),
                    )),
                )
            }
        };

        let mut addr_message = None;

        if let (Some(addr), Some(mask)) = (ifaddr.address, ifaddr.netmask) {
            if let Some(ipv4) = addr.as_sockaddr_in() {
                let ip = IpAddr::from(Ipv4Addr::from(ipv4.ip()));
                if let Some(netmask) = mask.as_sockaddr_in() {
                    addr_message = Some(NetworkEvent::NewAddr(
                        InterfaceIndex(index),
                        ip,
                        netmask.ip().leading_ones() as u8,
                    ));
                }
            } else if let Some(ipv6) = addr.as_sockaddr_in6() {
                if let Some(netmask) = mask.as_sockaddr_in6() {
                    addr_message = Some(NetworkEvent::NewAddr(
                        InterfaceIndex(index),
                        IpAddr::from(ipv6.ip()),
                        u128::from_be_bytes(netmask.as_ref().sin6_addr.s6_addr)
                            .leading_ones() as u8,
                    ));
                }
            }
        }
        (link_message, addr_message)
    }
}

fn map_interface_flags(flags: &InterfaceFlags) -> Flags {
    let mut newflags = Default::default();
    for (iff, newf) in [
        (InterfaceFlags::IFF_UP, Flags::UP),
        (InterfaceFlags::IFF_RUNNING, Flags::RUNNING),
        (InterfaceFlags::IFF_LOOPBACK, Flags::LOOPBACK),
        (InterfaceFlags::IFF_POINTOPOINT, Flags::POINTTOPOINT),
        (InterfaceFlags::IFF_BROADCAST, Flags::BROADCAST),
        (InterfaceFlags::IFF_MULTICAST, Flags::MULTICAST),
    ] {
        if flags.contains(iff) {
            newflags |= newf;
        }
    }
    newflags
}

#[cfg(test)]
mod tests {
    use super::*;
    use nix::sys::socket::SockaddrLike;
    use nix::sys::socket::SockaddrStorage;
    use nix::sys::socket::UnixAddr;
    use std::net::Ipv6Addr;
    use std::net::SocketAddrV4;
    use std::net::SocketAddrV6;

    #[test]
    fn flag_up() {
        assert_eq!(map_interface_flags(&InterfaceFlags::IFF_UP), Flags::UP);
    }

    #[test]
    fn flag_running() {
        assert_eq!(
            map_interface_flags(&InterfaceFlags::IFF_RUNNING),
            Flags::RUNNING
        );
    }

    #[test]
    fn flag_loopback() {
        assert_eq!(
            map_interface_flags(&InterfaceFlags::IFF_LOOPBACK),
            Flags::LOOPBACK
        );
    }

    #[test]
    fn flag_p2p() {
        assert_eq!(
            map_interface_flags(&InterfaceFlags::IFF_POINTOPOINT),
            Flags::POINTTOPOINT
        );
    }

    #[test]
    fn flag_broadcast() {
        assert_eq!(
            map_interface_flags(&InterfaceFlags::IFF_BROADCAST),
            Flags::BROADCAST
        );
    }

    #[test]
    fn flag_multicast() {
        assert_eq!(
            map_interface_flags(&InterfaceFlags::IFF_MULTICAST),
            Flags::MULTICAST
        );
    }

    fn test_iter(
        ifaddrs: &[ifaddrs::InterfaceAddress],
    ) -> impl Iterator<Item = ifaddrs::InterfaceAddress> {
        ifaddrs.to_vec().into_iter()
    }

    #[test]
    fn new_ipv4() {
        let addr = SocketAddrV4::new(Ipv4Addr::new(192, 168, 100, 1), 80);
        let mask = SocketAddrV4::new(Ipv4Addr::new(255, 255, 255, 0), 80);

        let ifaddr = ifaddrs::InterfaceAddress {
            interface_name: "eth0".to_string(),
            flags: InterfaceFlags::IFF_UP,
            address: Some(addr.into()),
            netmask: Some(mask.into()),
            broadcast: None,
            destination: None,
        };

        let mut map = InterfaceMap::new(test_iter(&[ifaddr]));

        let link = map.next();

        assert!(link.is_some());

        assert_eq!(
            link.unwrap(),
            NetworkEvent::NewLink(
                InterfaceIndex(1),
                "eth0".to_string(),
                Flags::UP
            )
        );

        let addr = map.next();
        assert!(addr.is_some());
        assert_eq!(
            addr.unwrap(),
            NetworkEvent::NewAddr(
                InterfaceIndex(1),
                Ipv4Addr::new(192, 168, 100, 1).into(),
                24
            )
        );

        let fin = map.next();

        assert!(fin.is_none());
    }

    #[test]
    fn new_no_address() {
        let addr = SocketAddrV4::new(Ipv4Addr::new(192, 168, 100, 1), 80);
        let mask = SocketAddrV4::new(Ipv4Addr::new(255, 255, 255, 0), 80);

        let ifaddr = ifaddrs::InterfaceAddress {
            interface_name: "eth0".to_string(),
            flags: InterfaceFlags::IFF_UP,
            address: Some(addr.into()),
            netmask: Some(mask.into()),
            broadcast: None,
            destination: None,
        };

        let addr = SocketAddrV4::new(Ipv4Addr::new(169, 254, 99, 99), 80);

        let ifaddr2 = ifaddrs::InterfaceAddress {
            interface_name: "eth0:1".to_string(),
            flags: InterfaceFlags::IFF_UP,
            address: Some(addr.into()),
            netmask: None, //<-- that won't work then
            broadcast: None,
            destination: None,
        };

        let mut map = InterfaceMap::new(test_iter(&[ifaddr, ifaddr2]));

        let link = map.next();
        assert!(link.is_some());
        assert_eq!(
            link.unwrap(),
            NetworkEvent::NewLink(
                InterfaceIndex(1),
                "eth0".to_string(),
                Flags::UP
            )
        );

        let addr = map.next();
        assert!(addr.is_some());
        assert_eq!(
            addr.unwrap(),
            NetworkEvent::NewAddr(
                InterfaceIndex(1),
                Ipv4Addr::new(192, 168, 100, 1).into(),
                24
            )
        );

        let fin = map.next(); // No second address
        assert!(fin.is_none());
    }

    #[test]
    fn new_not_ip() {
        let addr = SocketAddrV4::new(Ipv4Addr::new(192, 168, 100, 1), 80);
        let mask = SocketAddrV4::new(Ipv4Addr::new(255, 255, 255, 0), 80);

        let ifaddr = ifaddrs::InterfaceAddress {
            interface_name: "eth0".to_string(),
            flags: InterfaceFlags::IFF_UP,
            address: Some(addr.into()),
            netmask: Some(mask.into()),
            broadcast: None,
            destination: None,
        };

        let addr = unsafe {
            // Small palaver to obtain a SockaddrStorage that isn't IPv4 or
            // IPv6
            let addr = UnixAddr::new("/tmp/foo").unwrap();
            SockaddrStorage::from_raw(
                (&addr as &dyn SockaddrLike).as_ptr(),
                Some(addr.len()),
            )
            .unwrap()
        };

        let ifaddr2 = ifaddrs::InterfaceAddress {
            interface_name: "eth0:1".to_string(),
            flags: InterfaceFlags::IFF_UP,
            address: Some(addr.clone()),
            netmask: Some(addr),
            broadcast: None,
            destination: None,
        };

        let mut map = InterfaceMap::new(test_iter(&[ifaddr, ifaddr2]));

        let _ = map.next(); // link
        let _ = map.next(); // addr
        let fin = map.next();

        assert!(fin.is_none());
    }

    #[test]
    fn ipv4_alias() {
        let addr = SocketAddrV4::new(Ipv4Addr::new(192, 168, 100, 1), 80);
        let mask = SocketAddrV4::new(Ipv4Addr::new(255, 255, 255, 0), 80);

        let ifaddr = ifaddrs::InterfaceAddress {
            interface_name: "eth0".to_string(),
            flags: InterfaceFlags::IFF_UP,
            address: Some(addr.into()),
            netmask: Some(mask.into()),
            broadcast: None,
            destination: None,
        };

        let addr = SocketAddrV4::new(Ipv4Addr::new(169, 254, 99, 99), 80);
        let mask = SocketAddrV4::new(Ipv4Addr::new(255, 255, 0, 0), 80);

        let ifaddr2 = ifaddrs::InterfaceAddress {
            interface_name: "eth0:1".to_string(),
            flags: InterfaceFlags::IFF_UP,
            address: Some(addr.into()),
            netmask: Some(mask.into()),
            broadcast: None,
            destination: None,
        };

        let mut map = InterfaceMap::new(test_iter(&[ifaddr, ifaddr2]));

        let link = map.next();
        assert!(link.is_some());

        let addr = map.next();
        assert!(addr.is_some());

        let addr = map.next();
        assert!(addr.is_some());
        assert_eq!(
            addr.unwrap(),
            NetworkEvent::NewAddr(
                InterfaceIndex(1),
                Ipv4Addr::new(169, 254, 99, 99).into(),
                16
            )
        );

        let fin = map.next();
        assert!(fin.is_none());
    }

    #[test]
    fn ipv4_twoif() {
        let addr = SocketAddrV4::new(Ipv4Addr::new(192, 168, 100, 1), 80);
        let mask = SocketAddrV4::new(Ipv4Addr::new(255, 255, 255, 0), 80);

        let ifaddr = ifaddrs::InterfaceAddress {
            interface_name: "eth0".to_string(),
            flags: InterfaceFlags::IFF_UP,
            address: Some(addr.into()),
            netmask: Some(mask.into()),
            broadcast: None,
            destination: None,
        };

        let addr = SocketAddrV4::new(Ipv4Addr::new(169, 254, 99, 99), 80);
        let mask = SocketAddrV4::new(Ipv4Addr::new(255, 255, 0, 0), 80);

        let ifaddr2 = ifaddrs::InterfaceAddress {
            interface_name: "eth1".to_string(),
            flags: InterfaceFlags::IFF_UP | InterfaceFlags::IFF_RUNNING,
            address: Some(addr.into()),
            netmask: Some(mask.into()),
            broadcast: None,
            destination: None,
        };

        let mut map = InterfaceMap::new(test_iter(&[ifaddr, ifaddr2]));

        let link = map.next();
        assert!(link.is_some());

        let addr = map.next();
        assert!(addr.is_some());

        let link = map.next();
        assert!(link.is_some());
        assert_eq!(
            link.unwrap(),
            NetworkEvent::NewLink(
                InterfaceIndex(2),
                "eth1".to_string(),
                Flags::UP | Flags::RUNNING
            )
        );

        let addr = map.next();
        assert!(addr.is_some());
        assert_eq!(
            addr.unwrap(),
            NetworkEvent::NewAddr(
                InterfaceIndex(2),
                Ipv4Addr::new(169, 254, 99, 99).into(),
                16
            )
        );

        let fin = map.next();
        assert!(fin.is_none());
    }

    #[test]
    fn ipv4_ipv6() {
        let addr = SocketAddrV4::new(Ipv4Addr::new(192, 168, 100, 1), 80);
        let mask = SocketAddrV4::new(Ipv4Addr::new(255, 255, 255, 0), 80);

        let ifaddr = ifaddrs::InterfaceAddress {
            interface_name: "eth0".to_string(),
            flags: InterfaceFlags::IFF_UP,
            address: Some(addr.into()),
            netmask: Some(mask.into()),
            broadcast: None,
            destination: None,
        };

        let addr = SocketAddrV6::new(
            Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1),
            80,
            0,
            0,
        );
        let mask = SocketAddrV6::new(
            Ipv6Addr::new(0xFFFF, 0xFFFF, 0, 0, 0, 0, 0, 0),
            80,
            0,
            0,
        );

        let ifaddr2 = ifaddrs::InterfaceAddress {
            interface_name: "eth0".to_string(),
            flags: InterfaceFlags::IFF_UP,
            address: Some(addr.into()),
            netmask: Some(mask.into()),
            broadcast: None,
            destination: None,
        };

        let mut map = InterfaceMap::new(test_iter(&[ifaddr, ifaddr2]));

        let link = map.next(); // Returns IPv4

        assert!(link.is_some());

        assert_eq!(
            link.unwrap(),
            NetworkEvent::NewLink(
                InterfaceIndex(1),
                "eth0".to_string(),
                Flags::UP
            )
        );

        let addr = map.next();

        assert!(addr.is_some());

        assert_eq!(
            addr.unwrap(),
            NetworkEvent::NewAddr(
                InterfaceIndex(1),
                Ipv4Addr::new(192, 168, 100, 1).into(),
                24
            )
        );

        let addr2 = map.next(); // Returns IPv6

        assert!(addr2.is_some());

        assert_eq!(
            addr2.unwrap(),
            NetworkEvent::NewAddr(
                InterfaceIndex(1),
                Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1).into(),
                32
            )
        );
    }

    #[test]
    fn ipv4_ipv6_bad_mask() {
        let addr4 = SocketAddrV4::new(Ipv4Addr::new(192, 168, 100, 1), 80);
        let mask4 = SocketAddrV4::new(Ipv4Addr::new(255, 255, 255, 0), 80);

        let addr6 = SocketAddrV6::new(
            Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1),
            80,
            0,
            0,
        );
        let mask6 = SocketAddrV6::new(
            Ipv6Addr::new(0xFFFF, 0xFFFF, 0, 0, 0, 0, 0, 0),
            80,
            0,
            0,
        );

        let ifaddr = ifaddrs::InterfaceAddress {
            interface_name: "eth0".to_string(),
            flags: InterfaceFlags::IFF_UP,
            address: Some(addr4.into()),
            netmask: Some(mask6.into()), // note mismatch
            broadcast: None,
            destination: None,
        };

        let ifaddr2 = ifaddrs::InterfaceAddress {
            interface_name: "eth0".to_string(),
            flags: InterfaceFlags::IFF_UP,
            address: Some(addr6.into()),
            netmask: Some(mask4.into()), // note mismatch
            broadcast: None,
            destination: None,
        };

        let mut map = InterfaceMap::new(test_iter(&[ifaddr, ifaddr2]));

        let link = map.next();
        assert!(link.is_some());

        /* No valid addrs */
        let fin = map.next();
        assert!(fin.is_none());
    }

    #[test]
    fn get_interfaces_passes_through_errors() {
        let s = get_interfaces_inner(|| Err(nix::errno::Errno::ENOTTY));
        assert!(s.is_err());
    }

    #[test]
    fn zzz_instantiate() {
        assert!(get_interfaces().is_ok());
    }
}
