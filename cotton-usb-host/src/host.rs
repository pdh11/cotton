/// HostController implementation for Raspberry Pi Pico / RP2040
#[cfg(feature = "rp2040")]
pub mod rp2040;

/// HostController implementation for Raspberry Pi Pico 2 / RP235x
#[cfg(feature = "rp235x")]
pub mod rp235x;
