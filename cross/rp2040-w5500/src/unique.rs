use core::hash::Hasher;

pub trait UniqueId {
    fn id(&self, salt: &[u8]) -> u64;
}

pub struct UniqueFlashId {
    id: [u64; 2],
}

impl UniqueFlashId {
    /// Construct a UniqueFlashId
    ///
    /// # Safety
    ///
    /// Must be run on RP2040 as it calls a RP2040-specific function in
    /// rp2040-flash.
    pub unsafe fn new() -> Self {
        let mut unique_bytes = [0u8; 16];
        rp2040_flash::flash::flash_unique_id(&mut unique_bytes, true);

        defmt::println!("Unique id {}", unique_bytes[0..8]);
        Self {
            id: [
                u64::from_le_bytes(unique_bytes[0..8].try_into().unwrap()),
                u64::from_le_bytes(unique_bytes[8..16].try_into().unwrap()),
            ],
        }
    }
}

impl UniqueId for UniqueFlashId {
    fn id(&self, salt: &[u8]) -> u64 {
        let mut h =
            siphasher::sip::SipHasher::new_with_keys(self.id[0], self.id[1]);
        h.write(salt);
        h.finish()
    }
}

pub fn mac_address<U: UniqueId>(unique: &U, salt: &[u8]) -> [u8; 6] {
    let mut mac_address = [0u8; 6];
    let r = unique.id(salt).to_ne_bytes();
    mac_address.copy_from_slice(&r[0..6]);
    mac_address[0] &= 0xFE; // clear multicast bit
    mac_address[0] |= 2; // set local bit
    mac_address
}
