use core::ops::{BitOr, BitOrAssign};

/** Kernel network interface index (1-based)
 */
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InterfaceIndex(pub core::num::NonZeroU32);

/// Flags describing a network interface's features and state
///
/// Corresponds to Linux's SIOCGIFFLAGS
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Flags(u32);

impl Flags {
    #[doc = "Interface is enabled"]
    pub const UP: Self = Self(0x1);

    #[doc = "Interface is broadcast-capable"]
    pub const BROADCAST: Self = Self(0x2);

    #[doc = "Interface is loopback-only"]
    pub const LOOPBACK: Self = Self(0x4);

    #[doc = "Interface is point-to-point (e.g. PPP) -- not preserving Posix misspelling"]
    pub const POINTTOPOINT: Self = Self(0x8);

    #[doc = "Interface is operational"]
    pub const RUNNING: Self = Self(0x40);

    #[doc = "Interface is multicast-capable"]
    pub const MULTICAST: Self = Self(0x1000);

    #[doc = "An empty set of flags"]
    pub fn empty() -> Self {
        Self(0)
    }

    #[doc = "Check whether a subset of flags are set"]
    pub fn contains(&self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }
}

impl BitOr for Flags {
    type Output = Self;
    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl BitOrAssign for Flags {
    fn bitor_assign(&mut self, other: Self) {
        self.0 |= other.0;
    }
}

use no_std_net::IpAddr as IpAddress;

/** Event when a new interface or address is detected, or when one disappears
 */
#[derive(Debug, Clone, PartialEq, Eq)]
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
