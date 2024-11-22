#![cfg_attr(not(feature = "std"), no_std)]
mod debug;
pub mod mass_storage;
pub use mass_storage::{IdentifyMassStorage, MassStorage};
