use cotton_unique::UniqueId;

/// Construct a UniqueId from the SPI flash unique ID
///
/// # Safety
///
/// Must be run on RP2040 as it calls a RP2040-specific function in
/// rp2040-flash. Also, no other flash access can be happening
/// concurrently (e.g. in other threads); it is recommended to call
/// this once during early startup and then pass the result around as
/// needed.
pub unsafe fn unique_flash_id() -> UniqueId {
    let mut unique_bytes = [0u8; 16];
    cortex_m::interrupt::free(|_cs| {
        rp2040_flash::flash::flash_unique_id(&mut unique_bytes, true)
    });
    UniqueId::new(&unique_bytes)
}
