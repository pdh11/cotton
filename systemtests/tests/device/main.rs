#[cfg(feature = "arm")]
mod device_test;

#[cfg(feature = "arm")]
mod ssdp_test;

#[cfg(feature = "stm32f746-nucleo")]
mod stm32f746_nucleo;

#[cfg(feature = "rp2040-w5500")]
mod rp2040_w5500;

#[cfg(feature = "rp2350-w6100")]
mod rp2350_w6100;
