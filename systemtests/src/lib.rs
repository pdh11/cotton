#[cfg(feature = "arm")]
mod device_test;

#[cfg(feature = "arm")]
pub use device_test::{device_test, DeviceTest};
