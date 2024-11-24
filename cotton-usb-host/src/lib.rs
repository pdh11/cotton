#![doc = include_str!("../README.md")]
#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]
#![cfg_attr(docsrs, feature(doc_cfg_hide))]
#![cfg_attr(docsrs, doc(cfg_hide(doc)))]

pub mod async_pool;
pub mod bitset;
mod debug;
pub mod device;
pub mod host;
pub mod host_controller;
pub mod interrupt;
pub mod topology;
pub mod usb_bus;
pub mod wire;

#[cfg(feature = "std")]
pub mod mocks;
