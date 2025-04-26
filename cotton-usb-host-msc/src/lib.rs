#![cfg_attr(not(feature = "std"), no_std)]
#![allow(clippy::uninlined_format_args)]

mod debug;
pub mod mass_storage;
pub use mass_storage::{IdentifyMassStorage, MassStorage};
