//! Implementing statistically-unique per-device IDs based on chip IDs
//!
//! The cotton-unique crate encapsulates the creation of per-device unique
//! identifiers -- for things such as Ethernet MAC addresses, or UPnP UUIDs.
//!
//! Most microcontrollers (e.g. STM32, RA6M5) have a unique per-unit
//! identifier built-in; RP2040 does not, but on that platform it's intended
//! to use the unique identifier in the associated SPI flash chip instead.
//!
//! But it's not a good idea to just use the raw chip ID as the MAC
//! address, for several reasons: it's the wrong size, it's quite
//! predictable (it's not 96 random bits per chip, it typically
//! encodes the chip batch number and die position on the wafer, so
//! two different STM32s might have IDs that differ only in one or two
//! bits, meaning we can't just pick any 46 bits from the 96 in case
//! we accidentally pick seldom-changing ones) — and, worst of all, if
//! anyone were to use the same ID for anything else later, they might
//! be surprised if it were very closely correlated with the device's
//! MAC address.
//!
//! So the thing to do, is to hash the unique ID along with a key, or
//! salt, which indicates what we're using it for. The result is thus
//! deterministic and consistent on any one device for a particular
//! salt, but varies from one device to another (and from one salt to
//! another).
//!
//! For instance, the cotton-ssdp device tests obtain a MAC address by
//! hashing the STM32 unique ID with the salt string "stm32-eth", and
//! UPnP UUIDs by hashing the *same* ID with a *different* salt.
//!
//! This does not *guarantee* uniqueness, but if the has function is
//! doing its job, the odds of a collision involve a factor of 2^-64 --
//! or in other words are highly unlikely.
#![no_std]
#![warn(missing_docs)]
#![warn(rustdoc::missing_crate_level_docs)]
use core::hash::Hasher;

/// An object from qhich unique identifers can be obtained
pub struct UniqueId {
    id: [u64; 2],
}

impl UniqueId {
    /// Create a new UniqueId object
    ///
    /// The `unique_bytes` can be a raw unique chip ID, as they are hashed
    /// and salted before any client code sees them.
    pub fn new(unique_bytes: &[u8; 16]) -> Self {
        Self {
            id: [
                u64::from_le_bytes(unique_bytes[0..8].try_into().unwrap()),
                u64::from_le_bytes(unique_bytes[8..16].try_into().unwrap()),
            ],
        }
    }

    /// Return a (statistically) unique identifier for a specific purpose
    ///
    /// The `salt` string should concisely express the purpose for which the
    /// identifier is needed; i.e., identifiers for different purposes must
    /// have different salts.
    pub fn id(&self, salt: &[u8]) -> u64 {
        let mut h =
            siphasher::sip::SipHasher::new_with_keys(self.id[0], self.id[1]);
        h.write(salt);
        h.finish()
    }


    /// Return a (statistically) unique identifier for a specific purpose
    ///
    /// This is very similar to `id` but takes two `salt` values, a string
    /// and a u32. This is intended to be helpful when creating identifiers
    /// larger than u64; see the implementation of `uuid()` for an example.
    pub fn id2(&self, salt: &[u8], salt2: u32) -> u64 {
        let mut h =
            siphasher::sip::SipHasher::new_with_keys(self.id[0], self.id[1]);
        h.write(salt);
        h.write_u32(salt2);
        h.finish()
    }
}

/// Return a statistically-unique but consistent MAC address
///
/// The recommendation is that the `salt` string encodes the network
/// address somehow (so that multi-homed hosts get different MAC
/// addresses on different interfaces); for instance b"stm32-eth" or
/// b"w5500-spi0".
pub fn mac_address(unique: &UniqueId, salt: &[u8]) -> [u8; 6] {
    let mut mac_address = [0u8; 6];
    let r = unique.id(salt).to_ne_bytes();
    mac_address.copy_from_slice(&r[0..6]);
    mac_address[0] &= 0xFE; // clear multicast bit
    mac_address[0] |= 2; // set local bit
    mac_address
}

/// Return a statistically-unique but consistent UUID
///
/// The recommendation is that the `salt` string encodes the purpose of
/// the UUID somehow.
pub fn uuid(unique: &UniqueId, salt: &[u8]) -> u128 {
    // uuid crate isn't no_std :(
    let mut u1 = unique.id2(salt, 0);
    let mut u2 = unique.id2(salt, 1);
    // Variant 1
    u2 |= 0x8000_0000_0000_0000_u64;
    u2 &= !0x4000_0000_0000_0000_u64;
    // Version 5
    u1 &= !0xF000;
    u1 |= 0x5000;

    ((u1 as u128) << 64) | (u2 as u128)
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate alloc;

    #[test]
    fn test_id() {
        let raw_id = [0u8; 16];
        let unique = UniqueId::new(&raw_id);
        let id = unique.id(b"test-vector");
        // There's nothing particularly magic about this value, the point is
        // (a) it's not zero, and (b) it never changes from run to run.
        assert_eq!(11391256791731596036u64, id);
    }

    #[test]
    fn test_id2() {
        let raw_id = [0u8; 16];
        let unique = UniqueId::new(&raw_id);
        let id = unique.id2(b"test-vector", 37);
        assert_eq!(17344812425781864766u64, id);
    }

    #[test]
    fn test_saltiness() {
        let raw_id = [0u8; 16];
        let unique = UniqueId::new(&raw_id);
        let id1 = unique.id(b"eth0");
        let id2 = unique.id(b"eth1");
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_saltiness2() {
        let raw_id = [0u8; 16];
        let unique = UniqueId::new(&raw_id);
        let id1 = unique.id2(b"need-longer-id", 0);
        let id2 = unique.id2(b"need-longer-id", 1);
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_mac() {
        let raw_id = [0u8; 16];
        let unique = UniqueId::new(&raw_id);
        let mac = mac_address(&unique, b"eth0");
        assert_eq!(0x62, mac[0]);
        assert_eq!(0x67, mac[1]);
        assert_eq!(0x0B, mac[2]);
        assert_eq!(0xE3, mac[3]);
        assert_eq!(0xD9, mac[4]);
        assert_eq!(0xBD, mac[5]);
    }

    #[test]
    fn test_uuid() {
        let raw_id = [0u8; 16];
        let unique = UniqueId::new(&raw_id);
        let uuid = alloc::format!("{:032x}",
                                  uuid(&unique, b"upnp-media-renderer:0"));
        assert_eq!("2505b7b1dfa35c2d8f029e3409457472", uuid);
    }
}
