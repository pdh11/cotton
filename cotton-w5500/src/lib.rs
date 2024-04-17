//! Helpers for using the W5500 SPI Ethernet controller
#![cfg_attr(not(feature = "std"), no_std)]
#![warn(missing_docs)]
#![warn(rustdoc::missing_crate_level_docs)]

/// Using W5500 with smoltcp
#[cfg(feature = "smoltcp")]
pub mod smoltcp;
