use crate::bitset::BitSet;
#[cfg(feature = "std")]
use std::fmt::{Debug, Error, Formatter};

const MAX_DEVICES: u8 = 32;
const MAX_PORTS: u8 = 16;
const MAX_HUBS: u8 = 16;

/// Representing the topology of the USB bus attached to this host controller
///
/// This includes which devices are hubs, and which devices are downstream of
/// which hubs. This information can be dumped (for logging/debugging purposes)
/// using the implemented `Debug` or `defmt::Format` traits.
///
/// The topology is represented in a compact form: for each possible
/// device (0-31, but really 1-31 as 0 isn't valid), a u8 stores its
/// parent hub in the lower 4 bits, and the port number on that hub in
/// the upper four bits.
#[derive(Default, Clone)]
pub struct Topology {
    parent: [u8; MAX_DEVICES as usize],
}

#[cfg(feature = "std")]
impl Debug for Topology {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        fn fmt_inner(
            bus: &Topology,
            i: usize,
            f: &mut Formatter<'_>,
        ) -> Result<(), Error> {
            write!(f, "{}", i).unwrap();

            let mut any = false;
            for j in 1..(MAX_DEVICES as usize) {
                let parent = bus.parent[j];
                if parent != 0 && (parent & 15) == i as u8 {
                    any = true;
                }
            }
            if any {
                write!(f, ":(").unwrap();
                any = false;
                for j in 1..(MAX_DEVICES as usize) {
                    let parent = bus.parent[j];
                    if parent != 0 && (parent & 15) == i as u8 {
                        if any {
                            write!(f, " ").unwrap();
                        }
                        fmt_inner(bus, j, f).unwrap();
                        any = true;
                    }
                }
                write!(f, ")").unwrap();
            }
            Ok(())
        }

        fmt_inner(self, 0, f)
    }
}

#[cfg(feature = "defmt")]
impl defmt::Format for Topology {
    fn format(&self, f: defmt::Formatter<'_>) {
        fn fmt_inner(bus: &Topology, i: usize, f: defmt::Formatter<'_>) {
            defmt::write!(f, "{}", i);

            let mut any = false;
            for j in 1..(MAX_DEVICES as usize) {
                let parent = bus.parent[j];
                if parent != 0 && (parent & 15) == i as u8 {
                    any = true;
                }
            }
            if any {
                defmt::write!(f, ":(");
                any = false;
                for j in 1..(MAX_DEVICES as usize) {
                    let parent = bus.parent[j];
                    if parent != 0 && (parent & 15) == i as u8 {
                        if any {
                            defmt::write!(f, " ");
                        }
                        fmt_inner(bus, j, f);
                        any = true;
                    }
                }
                defmt::write!(f, ")");
            }
        }

        fmt_inner(self, 0, f)
    }
}

impl Topology {
    /// Create a new Topology object representing an empty bus (0 devices)
    pub fn new() -> Self {
        Self { parent: [0u8; 32] }
    }

    /// Is this USB device address believed present on the bus?
    pub fn is_present(&self, device: u8) -> bool {
        self.parent.get(device as usize).is_some_and(|x| *x > 0)
    }

    /// A new USB device has been connected
    ///
    /// # Parameters
    ///  - parent_hub: USB device address of parent hub (0 if attached to root)
    ///  - parent_port: Port number (1-based) on parent hub
    ///  - is_hub: Is this device itself a hub?
    ///
    /// Returns `Some(N)` if the device is to be given USB device address `N`,
    /// or `None` if the limit of attached devices has already been reached.
    pub fn device_connect(
        &mut self,
        parent_hub: u8,
        parent_port: u8,
        is_hub: bool,
    ) -> Option<u8> {
        if parent_hub >= MAX_HUBS || parent_port >= MAX_PORTS {
            return None;
        }
        let entry = (parent_port << 4) + parent_hub;
        if let Some(i) = self.parent.iter().position(|e| *e == entry) {
            return Some(i as u8);
        }

        if is_hub {
            for i in 1..MAX_HUBS {
                if !self.is_present(i) {
                    self.parent[i as usize] = entry;
                    return Some(i);
                }
            }
        } else {
            for i in (1..MAX_DEVICES).rev() {
                if !self.is_present(i) {
                    self.parent[i as usize] = entry;
                    return Some(i);
                }
            }
        }
        None
    }

    /// A USB device has been disconnected
    ///
    /// Because the device has *already* gone, we aren't told *its* address,
    /// we're just told where in the bus it used to be attached.
    ///
    /// # Parameters
    ///  - parent_hub: USB device address of parent hub (0 if attached to root)
    ///  - parent_port: Port number (1-based) on parent hub
    ///
    /// Returns a bitmask of the device addresses which must now be
    /// considered disconnected -- not just the device referenced by
    /// `parent_hub`/`parent_port`, but anything downstream of that device
    /// if it, itself, was a hub.
    pub fn device_disconnect(
        &mut self,
        parent_hub: u8,
        parent_port: u8,
    ) -> BitSet {
        if parent_hub >= MAX_HUBS || parent_port >= MAX_PORTS {
            return BitSet::default();
        }

        let mut bitset = 0u32;

        loop {
            let old_bitset = bitset;

            for i in 0..MAX_DEVICES {
                let parent = self.parent[i as usize];
                if parent != 0 {
                    let hub = parent & 15;
                    let port = parent >> 4;
                    if hub == parent_hub && port == parent_port {
                        bitset |= 1 << i;
                        self.parent[i as usize] = 0;
                    }

                    if (bitset & 1 << hub) != 0 {
                        bitset |= 1 << i;
                        self.parent[i as usize] = 0;
                    }
                }
            }

            // continue until no further changes
            if old_bitset == bitset {
                break;
            }
        }
        BitSet(bitset)
    }
}

#[cfg(all(test, feature = "std"))]
#[path = "tests/topology.rs"]
mod tests;
