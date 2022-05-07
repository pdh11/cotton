use super::*;
use nix::ifaddrs;
use nix::net::if_::InterfaceFlags;
use std::collections::hash_map::Entry;
use std::collections::HashMap;

use std::net::{IpAddr, Ipv4Addr};

/** Obtain the current list of network interfaces

The supplied function will be called with a sequence of [NetworkEvent]
objects, each describing a network interface (as
[NetworkEvent::NewLink]) or an address on that interface (as
[NetworkEvent::NewAddr]). An interface may have several addresses,
both IPv4 and IPv6. In all cases, the [NetworkEvent::NewLink] event
describing an interface, will be generated before that interface's
[NetworkEvent::NewAddr] event or events.

As the list is a snapshot of the current state, no [NetworkEvent::DelLink]
or [NetworkEvent::DelAddr] events will be generated.

For a simple listing of the returned information, just use println:

```rust
# use cotton_netif::*;
get_interfaces(|e| println!("{:?}", e))?;
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
get_interfaces(|e| match e {
    NetworkEvent::NewLink(_i, name, flags) => {
        if flags.contains(Flags::RUNNING | Flags::UP | Flags::MULTICAST) {
            println!("New multicast-capable interface: {}", name);
        }
    },
    _ => {},
})?;
# Ok::<(), std::io::Error>(())
```

 */
pub fn get_interfaces<FN>(mut callback: FN) -> Result<(), std::io::Error>
where
    FN: FnMut(NetworkEvent),
{
    let mut map = InterfaceMap::new();
    for ifaddr in ifaddrs::getifaddrs()? {
        let (new_link, new_addr) = map.check(ifaddr);
        if let Some(msg) = new_link {
            callback(msg);
        }
        if let Some(msg) = new_addr {
            callback(msg);
        }
    }
    Ok(())
}

/** Manage the mapping between interface names and indexes
 */
struct InterfaceMap {
    index_map: HashMap<String, u32>,
    next_index: u32,
}

impl InterfaceMap {
    fn new() -> InterfaceMap {
        InterfaceMap {
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

    #[test]
    fn new_ipv4() {
        let mut map = InterfaceMap::new();

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

        let (link, addr) = map.check(ifaddr);

        assert!(link.is_some());
        assert!(addr.is_some());

        assert_eq!(
            link.unwrap(),
            NetworkEvent::NewLink(
                InterfaceIndex(1),
                "eth0".to_string(),
                Flags::UP
            )
        );
        assert_eq!(
            addr.unwrap(),
            NetworkEvent::NewAddr(
                InterfaceIndex(1),
                Ipv4Addr::new(192, 168, 100, 1).into(),
                24
            )
        );
    }

    #[test]
    fn ipv4_alias() {
        let mut map = InterfaceMap::new();

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

        map.check(ifaddr);

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

        let (link, addr) = map.check(ifaddr2);

        assert!(link.is_none());
        assert!(addr.is_some());

        assert_eq!(
            addr.unwrap(),
            NetworkEvent::NewAddr(
                InterfaceIndex(1),
                Ipv4Addr::new(169, 254, 99, 99).into(),
                16
            )
        );
    }

    #[test]
    fn ipv4_twoif() {
        let mut map = InterfaceMap::new();

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

        map.check(ifaddr);

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

        let (link, addr) = map.check(ifaddr2);

        assert!(link.is_some());
        assert!(addr.is_some());

        assert_eq!(
            link.unwrap(),
            NetworkEvent::NewLink(
                InterfaceIndex(2),
                "eth1".to_string(),
                Flags::UP | Flags::RUNNING
            )
        );
        assert_eq!(
            addr.unwrap(),
            NetworkEvent::NewAddr(
                InterfaceIndex(2),
                Ipv4Addr::new(169, 254, 99, 99).into(),
                16
            )
        );
    }

    #[test]
    fn ipv4_ipv6() {
        let mut map = InterfaceMap::new();

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

        map.check(ifaddr);

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

        let (link, addr) = map.check(ifaddr2);

        assert!(link.is_none());
        assert!(addr.is_some());

        assert_eq!(
            addr.unwrap(),
            NetworkEvent::NewAddr(
                InterfaceIndex(1),
                Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1).into(),
                32
            )
        );
    }
}
