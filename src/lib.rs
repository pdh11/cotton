use bitflags::bitflags;
use std::net::IpAddr;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct NetworkInterface(u32);

impl NetworkInterface {
    pub fn value(&self) -> u32 { return self.0; }
}

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
    NewLink(NetworkInterface, String, Flags),
    DelLink(NetworkInterface),
    NewAddr(NetworkInterface, String, IpAddr, u8),
    DelAddr(NetworkInterface),
}

#[cfg(target_os = "linux")]
pub mod dynamic; // Uses netlink, which is Linux-specific

pub use dynamic::network_interfaces_dynamic;
