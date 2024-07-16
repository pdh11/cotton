#![no_std]
pub mod setup;
pub mod smoltcp;

pub unsafe fn unique_flash_id() -> cotton_unique::UniqueId {
    let mut unique_bytes = [0u8; 16];
    rp2040_flash::flash::flash_unique_id(&mut unique_bytes, true);
    cotton_unique::UniqueId::new(&unique_bytes)
}
