//! A Wiznet W5500 driver for smoltcp
//!
//! This crate includes an implementation of `smoltcp::phy::Device`
//! which uses the [W5500](https://crates.io/crates/w5500) crate to
//! target [smoltcp](https://crates.io/crates/smoltcp) to the Wiznet
//! W5500 SPI-to-Ethernet chip, as found on the
//! [W5500-EVB-Pico](https://thepihut.com/products/wiznet-w5100s-evb-pico-rp2040-board-with-ethernet)
//! board (and in many other places). The W5500 is operated in
//! "MACRAW" (raw packet) mode, which allows more flexible networking
//! (via smoltcp) than is possible using the W5500's onboard TCP/UDP
//! mode -- for instance, it enables IPv6 support, which would
//! otherwise require the somewhat rarer W6100 chip.
//!
//! Although cotton-w5500 works well with cotton-unique, it is
//! relatively stand-alone: it does not depend on cotton-unique nor on
//! any other part of the Cotton project.
#![cfg_attr(not(feature = "std"), no_std)]
#![warn(missing_docs)]
#![warn(rustdoc::missing_crate_level_docs)]

/// Using W5500 with smoltcp
#[cfg(feature = "smoltcp")]
pub mod smoltcp;
