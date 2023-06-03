//! Device-side binaries for the Cotton project, targetting the
//! STM32F746-Nucleo development board. These serve as example code
//! for using Cotton crates such as cotton-ssdp, and also act as the
//! device-side component of Cotton's system-tests.
//!
//! Includes:
//! - [stm32f746_nucleo_hello](../stm32f746_nucleo_hello/index.html):
//!   Minimal "Hello, World!" application. If this test fails, it's unlikely
//!   that any other tests will pass.
//!
//! - [stm32f746_nucleo_dhcp_rtic](../stm32f746_nucleo_dhcp_rtic/index.html):
//!   Near-minimal networking example: using STM32 Nucleo Ethernet with
//!   Smoltcp and RTIC (1.0) in order to obtain a DHCP address. If this test
//!   fails, it's unlikely that any other network-related tests will pass.
//!
//! - [stm32f746_nucleo_ssdp_rtic](../stm32f746_nucleo_ssdp_rtic/index.html):
//!   Uses STM32 Nucleo Ethernet with Smoltcp, RTIC, and cotton-ssdp in order
//!   to advertise a (test-only) SSDP resource on the local network.
//!
#![no_std]
#![no_main]
#![warn(missing_docs)]
#![warn(rustdoc::missing_crate_level_docs)]

/// Common code and helper functions used across different STM32F746 tests
pub mod common;
