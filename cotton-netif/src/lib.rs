//! Enumerating network interfaces and their IP addresses
//!
//! The netif crate encapsulates the obtaining of the host's network
//! interfaces and IP addresses. It supports both static/synchronous
//! listing (i.e., a snapshot of the current list of network
//! interfaces) using [`get_interfaces`] and dynamic/asynchronous
//! listing (i.e., getting events as network interfaces and addresses
//! come and go) using [`get_interfaces_async`].
//!
//! At present this crate *only works on Linux* (and maybe BSD) but
//! the structure is such that adding compatibility with other
//! platforms in future, shouldn't require changes to any client code.
//!
//! Todo:
//!  - [x] IPv6 in `linux_netlink`
//!  - [x] Better test coverage
//!  - [x] Does `DelAddr` need to include the address? *yes*
//!  - [x] Can `get_interfaces_async` itself not be async?
//!  - [ ] Can we use just one netlink socket, perhaps with lower-level neli?
//!  - [ ] Turn async into a (cargo) Feature
//!

#![cfg_attr(target_os = "none", no_std)]
#![warn(missing_docs)]
#![warn(rustdoc::missing_crate_level_docs)]

extern crate alloc;

use bitflags::bitflags;

/** Kernel network interface index (1-based)
 */
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct InterfaceIndex(pub u32);

bitflags! {
    /// Flags describing a network interface's features and state
    ///
    /// Corresponds to Linux's SIOCGIFFLAGS
    #[derive(Default)]
    pub struct Flags: u32 {
        #[doc = "Interface is enabled"]
        const UP = 0x1;

        #[doc = "Interface is broadcast-capable"]
        const BROADCAST = 0x2;

        #[doc = "Interface is loopback-only"]
        const LOOPBACK = 0x4;

        #[doc = "Interface is point-to-point (e.g. PPP)"]
        const POINTTOPOINT = 0x8; // not preserving Posix misspelling

        #[doc = "Interface is operational"]
        const RUNNING = 0x40;

        #[doc = "Interface is multicast-capable"]
        const MULTICAST = 0x1000;
    }
}

#[cfg(target_os="none")]
use smoltcp::wire::IpAddress;
#[cfg(not(target_os="none"))]
use std::net::IpAddr as IpAddress;

/** Event when a new interface or address is detected, or when one disappears
 */
#[derive(Debug, Clone, PartialEq)]
pub enum NetworkEvent {
    /** A new network interface is detected. */
    NewLink(InterfaceIndex, alloc::string::String, Flags),

    /** A previously-seen interface has gone away (e.g. USB unplug). */
    DelLink(InterfaceIndex),

    /** An interface has a new address; note that each interface can have several addresses.
     */
    NewAddr(InterfaceIndex, IpAddress, u8),

    /** A previously-active address has been deactivated. */
    DelAddr(InterfaceIndex, IpAddress, u8),
}

/** Dynamic listing using Linux's netlink socket
 */
#[cfg(target_os = "linux")]
pub mod linux_netlink;

#[cfg(target_os = "linux")]
#[doc(inline)]
pub use linux_netlink::get_interfaces_async;

/** Static listing using Linux/glibc's getifaddrs(3)
 */
#[cfg(not(target_os = "none"))]
pub mod getifaddrs;

#[cfg(not(target_os = "none"))]
#[doc(inline)]
pub use getifaddrs::get_interfaces;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_index_debug() {
        let ix = InterfaceIndex(3);
        let s = format!("{:?}", ix);
        assert_eq!(s, "InterfaceIndex(3)".to_string());
    }

    #[test]
    #[allow(clippy::clone_on_copy)]
    fn test_index_clone() {
        let ix = InterfaceIndex(4);
        let ix2 = ix.clone();
        let ix3 = ix;
        assert_eq!(ix, ix2);
        assert_eq!(ix, ix3);
    }

    #[test]
    fn test_index_hash() {
        let mut h = HashMap::new();
        h.insert(InterfaceIndex(1), "eth0");
        h.insert(InterfaceIndex(2), "eth1");

        assert_eq!(h.get(&InterfaceIndex(1)), Some(&"eth0"));
    }

    #[test]
    fn test_index_partialeq() {
        assert!(InterfaceIndex(1).eq(&InterfaceIndex(1)));
        assert!(InterfaceIndex(2).ne(&InterfaceIndex(3)));
    }

    #[test]
    fn test_event_debug() {
        let e = NetworkEvent::DelLink(InterfaceIndex(7));
        let s = format!("{:?}", e);
        assert_eq!(s, "DelLink(InterfaceIndex(7))");
    }

    #[test]
    fn test_event_partialeq() {
        assert!(NetworkEvent::DelLink(InterfaceIndex(1))
            .eq(&NetworkEvent::DelLink(InterfaceIndex(1))));
        assert!(NetworkEvent::DelLink(InterfaceIndex(2))
            .ne(&NetworkEvent::DelLink(InterfaceIndex(3))));
    }

    #[test]
    fn test_event_clone() {
        let e = NetworkEvent::DelLink(InterfaceIndex(1));
        let e2 = e.clone();
        assert_eq!(e, e2);
    }
}
