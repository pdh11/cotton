#![cfg_attr(not(feature = "std"), no_std)]

mod debug;
pub mod midi;
pub use midi::{IdentifyMidi, Midi, UsbMidiEventPacket};
