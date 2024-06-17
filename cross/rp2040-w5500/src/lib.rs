#![no_std]
pub mod smoltcp;

/// Construct a UniqueId for RP2040 from the SPI flash unique ID
///
/// The RP2040 itself does not contain a unique chip identifier.
/// But RP2040-based designs typically incorporate a SPI flash
/// chip which *does* contain a unique chip identifier, which is
/// what is used here.
///
/// Note that not all SPI flash chips have this feature. The
/// Winbond parts commonly seen on RP2040 devboards
/// (JEDEC=0xEF7015) support an 8-byte unique ID;
/// https://forums.raspberrypi.com/viewtopic.php?t=331949 suggests
/// that LCSC (Zetta) parts have a 16-byte unique ID (which is
/// *not* unique in just its first 8 bytes), JEDEC=0xBA6015.
/// Macronix and Spansion parts do not have a unique ID.
///
/// # Safety
///
/// Must be run on RP2040 as it calls a RP2040-specific function in
/// rp2040-flash. Also, no other flash access can be happening
/// concurrently (e.g. in other threads); it is recommended to call
/// this once during early startup and then pass the result around as
/// needed.
pub unsafe fn unique_flash_id() -> cotton_unique::UniqueId {
    let mut unique_bytes = [0u8; 16];
    rp2040_flash::flash::flash_unique_id(&mut unique_bytes, true);
    cotton_unique::UniqueId::new(&unique_bytes)
}
