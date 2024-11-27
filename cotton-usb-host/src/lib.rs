#![doc = include_str!("../README.md")]
//#![warn(missing_docs)]
#![warn(rustdoc::missing_crate_level_docs)]
#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]
#![cfg_attr(docsrs, feature(doc_cfg_hide))]
#![cfg_attr(docsrs, doc(cfg_hide(doc)))]

/// Encapsulates waiting for any one of N resources to become available
pub mod async_pool;

/// A compact representation of a set of 32 booleans
pub mod bitset;
mod debug;

/// Example device-drivers for USB devices
pub mod device;

/// Example host-controller drivers
pub mod host;

/// Abstraction over host-controller drivers
pub mod host_controller;

/// Implementing Stream in terms of an interrupt IN endpoint
pub mod interrupt;

/// Encapsulating the layout of a USB bus
pub mod topology;

/// Main encapsulation of a USB bus and all its devices
pub mod usb_bus;

/// Data representations straight from the USB standards
pub mod wire;

/// A mock host-controller driver, for writing unit tests
#[cfg(feature = "std")]
pub mod mocks;
