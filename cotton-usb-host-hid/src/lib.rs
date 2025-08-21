#![cfg_attr(not(feature = "std"), no_std)]
mod debug;
pub mod hid;
pub use hid::{Hid, IdentifyHid};
