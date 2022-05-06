//! Enumerating network interfaces and their IP addresses
//!
//! The netif crate encapsulates the obtaining of the host's network
//! interfaces and IP addresses.
//!
//! @todo If "eth0:1" is an alias, getifaddrs reports two interfaces,
//!  but netlink reports one but with two addresses.
//!
//! @todo getifaddrs doesn't directly provide InterfaceIndex
//!
//! @todo IPv6
//!

use bitflags::bitflags;
use std::net::IpAddr;

/** Kernel network interface index (1-based)
 */
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct InterfaceIndex(pub u32);

bitflags! {
    pub struct Flags: u32 {
        const NONE = 0;
        const UP = 0x1;
        const BROADCAST = 0x2;
        const LOOPBACK = 0x4;
        const POINTTOPOINT = 0x8; // not preserving Posix misspelling
        const RUNNING = 0x40;
        const PROMISCUOUS = 0x100;
        const MULTICAST = 0x1000;
    }
}

#[derive(Debug, Clone)]
pub enum NetworkEvent {
    NewLink(InterfaceIndex, String, Flags),
    DelLink(InterfaceIndex),
    NewAddr(InterfaceIndex, IpAddr, u8),
    DelAddr(InterfaceIndex),
}

#[cfg(target_os = "linux")]
pub mod dynamic; // Uses netlink, which is Linux-specific

#[cfg(target_os = "linux")]
pub use dynamic::network_interfaces_dynamic;

#[cfg(unix)]
pub mod getifaddrs;

#[cfg(unix)]
pub use getifaddrs::network_interfaces_static;
