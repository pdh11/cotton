#![cfg_attr(not(feature = "std"), no_std)]
#![doc = include_str!("../README.md")]
#![warn(missing_docs)]
#![warn(rustdoc::missing_crate_level_docs)]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]
#![cfg_attr(docsrs, feature(doc_cfg_hide))]
#![cfg_attr(docsrs, doc(cfg_hide(doc)))]
mod debug;

/// A generic SCSI device
pub mod scsi_device;
pub use scsi_device::{PeripheralType, ScsiDevice};

/// An abstract communication channel with a SCSI device
///
/// Usually, these days, not actual SCSI hardware, but instead SCSI
/// tunnelled over something else (USB, ATAPI).
pub mod scsi_transport;
pub use scsi_transport::{Error, ScsiTransport};

/// A generic asynchronous block device with a "read/write blocks" interface
pub mod async_block_device;
pub use async_block_device::{AsyncBlockDevice, DeviceInfo};

/// Implementing AsyncBlockDevice in terms of ScsiDevice
pub mod scsi_block_device;
pub use scsi_block_device::ScsiBlockDevice;
