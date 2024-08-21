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
//!  - [x] Turn async into a (cargo) Feature
//!

#![cfg_attr(not(feature = "std"), no_std)]
#![warn(missing_docs)]
#![warn(rustdoc::missing_crate_level_docs)]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]
#![cfg_attr(docsrs, feature(doc_cfg_hide))]
#![cfg_attr(docsrs, doc(cfg_hide(doc)))]

// NB "docsrs" here really means "nightly", but that isn't an available cfg

extern crate alloc;

/** Events passed to interface observers
 */
pub mod network_event;
pub use network_event::{Flags, InterfaceIndex, NetworkEvent};

/** Dynamic listing using Linux's netlink socket
 */
#[cfg(all(target_os = "linux", feature = "async"))]
pub mod linux_netlink;

#[cfg(all(target_os = "linux", feature = "async"))]
#[doc(inline)]
pub use linux_netlink::get_interfaces_async;

/** Static listing using Linux/glibc's getifaddrs(3)
 */
#[cfg(all(feature = "sync", not(target_os = "none")))]
pub mod getifaddrs;

#[cfg(all(feature = "sync", not(target_os = "none")))]
#[doc(inline)]
pub use getifaddrs::get_interfaces;

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "std")]
    use std::collections::HashMap;

    fn make_index(i: u32) -> InterfaceIndex {
        InterfaceIndex(core::num::NonZeroU32::new(i).unwrap())
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_index_debug() {
        let ix = make_index(3);
        let s = format!("{ix:?}");
        assert_eq!(s, "InterfaceIndex(3)".to_string());
    }

    #[test]
    #[allow(clippy::clone_on_copy)]
    fn test_index_clone() {
        let ix = make_index(4);
        let ix2 = ix.clone();
        let ix3 = ix;
        assert_eq!(ix, ix2);
        assert_eq!(ix, ix3);
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_index_hash() {
        let mut h = HashMap::new();
        h.insert(make_index(1), "eth0");
        h.insert(make_index(2), "eth1");

        assert_eq!(h.get(&make_index(1)), Some(&"eth0"));
    }

    #[test]
    fn test_index_partialeq() {
        assert!(make_index(1).eq(&make_index(1)));
        assert!(make_index(2).ne(&make_index(3)));
    }

    #[test]
    fn test_index_partialord() {
        assert!(make_index(1).lt(&make_index(2)));
        assert!(make_index(3).ge(&make_index(2)));
    }

    #[test]
    fn test_index_ord() {
        assert_eq!(
            make_index(1).cmp(&make_index(2)),
            core::cmp::Ordering::Less
        );
        assert_eq!(
            make_index(3).cmp(&make_index(2)),
            core::cmp::Ordering::Greater
        );
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_event_debug() {
        let e = NetworkEvent::DelLink(make_index(7));
        let s = format!("{e:?}");
        assert_eq!(s, "DelLink(InterfaceIndex(7))");
    }

    #[test]
    fn test_event_partialeq() {
        assert!(NetworkEvent::DelLink(make_index(1))
            .eq(&NetworkEvent::DelLink(make_index(1))));
        assert!(NetworkEvent::DelLink(make_index(2))
            .ne(&NetworkEvent::DelLink(make_index(3))));
    }

    #[test]
    fn test_event_clone() {
        let e = NetworkEvent::DelLink(make_index(1));
        let e2 = e.clone();
        assert_eq!(e, e2);
    }

    #[test]
    fn test_flags_default() {
        let f = Flags::default();
        assert_eq!(f, Flags::empty());
    }

    #[test]
    #[allow(clippy::clone_on_copy)]
    fn test_flags_clone() {
        let f = Flags::POINTTOPOINT;
        let g = f.clone();
        assert_eq!(g, Flags::POINTTOPOINT);
    }

    #[test]
    fn test_flags_copy() {
        let f = Flags::POINTTOPOINT;
        let g = f;
        assert_eq!(g, Flags::POINTTOPOINT);
    }

    #[test]
    fn test_flags_partialeq() {
        assert!(Flags::POINTTOPOINT.ne(&Flags::empty()));
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_flags_debug() {
        let s = format!("{:?}", Flags::MULTICAST);
        assert_eq!(s, "Flags(4096)");
    }
}
