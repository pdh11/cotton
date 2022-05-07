//! Enumerating network interfaces and their IP addresses
//!
//! The netif crate encapsulates the obtaining of the host's network
//! interfaces and IP addresses. It supports both static/synchronous
//! listing (i.e., a snapshot of the current list of network
//! interfaces) using [get_interfaces] and dynamic/asynchronous
//! listing (i.e., getting events as network interfaces and addresses
//! come and go) using [get_interfaces_async].
//!
//! At present this crate *only works on Linux* (and maybe BSD) but
//! the structure is such that adding compatibility with other
//! platforms in future, shouldn't require changes to any client code.
//!
//! Todo:
//!  - [x] IPv6 in linux_netlink
//!  - [ ] Better test coverage
//!  - [ ] Turn async into a (cargo) Feature
//!

#![warn(missing_docs)]
#![warn(rustdoc::missing_crate_level_docs)]

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

/** Event when a new interface or address is detected, or when one disappears
 */
#[derive(Debug, Clone)]
pub enum NetworkEvent {
    /** A new network interface is detected. */
    NewLink(InterfaceIndex, String, Flags),

    /** A previously-seen interface has gone away (e.g. USB unplug). */
    DelLink(InterfaceIndex),

    /** An interface has a new address; note that each interface can have several addresses.
     */
    NewAddr(InterfaceIndex, std::net::IpAddr, u8),

    /** A previously-active address has been deactivated. */
    DelAddr(InterfaceIndex),
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
#[cfg(unix)]
pub mod getifaddrs;

#[cfg(unix)]
#[doc(inline)]
pub use getifaddrs::get_interfaces;
