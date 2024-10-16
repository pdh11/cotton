#[cfg(feature = "std")]
use std::fmt::{Debug, Error, Formatter};

const MAX_DEVICES: u8 = 32;
const MAX_PORTS: u8 = 16;
const MAX_HUBS: u8 = 16;

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
    pub fn new() -> Self {
        Self { parent: [0u8; 32] }
    }

    pub fn is_present(&self, device: u8) -> bool {
        self.parent.get(device as usize).is_some_and(|x| *x > 0)
    }

    pub fn device_connect(
        &mut self,
        parent_hub: u8,
        parent_port: u8,
        is_hub: bool,
    ) -> Option<u8> {
        if parent_hub >= MAX_HUBS || parent_port >= MAX_PORTS {
            return None;
        }

        // @TODO: check for already present (possible if USB power glitches)

        if is_hub {
            for i in 1..MAX_HUBS {
                if !self.is_present(i) {
                    self.parent[i as usize] = (parent_port << 4) + parent_hub;
                    return Some(i);
                }
            }
        } else {
            for i in (1..MAX_DEVICES).rev() {
                if !self.is_present(i) {
                    self.parent[i as usize] = (parent_port << 4) + parent_hub;
                    return Some(i);
                }
            }
        }
        None
    }

    pub fn device_disconnect(
        &mut self,
        parent_hub: u8,
        parent_port: u8,
    ) -> u32 {
        if parent_hub >= MAX_HUBS || parent_port >= MAX_PORTS {
            return 0;
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
        bitset
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    extern crate alloc;

    #[test]
    fn create() {
        let bus = Topology::new();
        let e = format!("{:?}", bus);
        assert_eq!(e, "0");
    }

    #[test]
    fn one_device() {
        let mut bus = Topology::new();
        let d = bus.device_connect(0, 1, false);
        assert_eq!(d, Some(31));
        assert!(bus.is_present(31));
        assert!(!bus.is_present(30));
        let e = format!("{:?}", bus);
        assert_eq!(e, "0:(31)");
    }

    #[test]
    fn one_hub() {
        let mut bus = Topology::new();
        let d = bus.device_connect(0, 1, true);
        assert_eq!(d, Some(1));
        assert!(bus.is_present(1));
        assert!(!bus.is_present(31));
        let e = format!("{:?}", bus);
        assert_eq!(e, "0:(1)");
    }

    #[test]
    fn child_device() {
        let mut bus = Topology::new();
        let d = bus.device_connect(0, 1, true).unwrap();
        assert_eq!(d, 1);
        let dd = bus.device_connect(1, 2, false).unwrap();
        assert_eq!(dd, 31);
        assert!(bus.is_present(1));
        assert!(!bus.is_present(30));
        let e = format!("{:?}", bus);
        assert_eq!(e, "0:(1:(31))");
    }

    #[test]
    fn one_device_disconnect() {
        let mut bus = Topology::new();
        let d = bus.device_connect(0, 1, false);
        assert_eq!(d, Some(31));
        assert!(bus.is_present(31));
        assert!(!bus.is_present(30));
        let m = bus.device_disconnect(0, 1);
        assert_eq!(m, 0x8000_0000);
        let e = format!("{:?}", bus);
        assert_eq!(e, "0");
    }

    #[test]
    fn child_device_disconnect() {
        let mut bus = Topology::new();
        let d = bus.device_connect(0, 1, true).unwrap();
        assert_eq!(d, 1);
        let dd = bus.device_connect(1, 2, false).unwrap();
        assert_eq!(dd, 31);
        assert!(bus.is_present(1));

        // the child device disappears but the hub is still there
        let m = bus.device_disconnect(1, 2);
        assert_eq!(m, 0x8000_0000);
        let e = format!("{:?}", bus);
        assert_eq!(e, "0:(1)");
    }

    #[test]
    fn child_device_root_disconnect() {
        let mut bus = Topology::new();
        let d = bus.device_connect(0, 1, true).unwrap();
        assert_eq!(d, 1);
        let dd = bus.device_connect(1, 2, false).unwrap();
        assert_eq!(dd, 31);

        // the hub disappears, so its child device does too
        let m = bus.device_disconnect(0, 1);
        assert_eq!(m, 0x8000_0002);
        let e = format!("{:?}", bus);
        assert_eq!(e, "0");
    }

    #[test]
    fn too_many_hubs() {
        let mut bus = Topology::new();
        let mut hubs = 0;

        loop {
            let d = bus.device_connect(0, hubs + 1, true);
            if d.is_none() {
                break;
            }
            hubs = hubs + 1;
        }
        assert_eq!(hubs, 15);
        assert_eq!(
            format!("{:?}", bus),
            "0:(1 2 3 4 5 6 7 8 9 10 11 12 13 14 15)"
        );
    }

    #[test]
    fn too_many_devices() {
        let mut bus = Topology::new();
        let mut devices = 0;
        bus.device_connect(0, 15, true);

        loop {
            let d = bus.device_connect(devices & 1, devices / 2, false);
            if d.is_none() {
                break;
            }
            devices = devices + 1;
        }
        assert_eq!(devices, 31);
        assert_eq!(format!("{:?}", bus), "0:(1:(3 5 7 9 11 13 15 17 19 21 23 25 27 29 31) 2 4 6 8 10 12 14 16 18 20 22 24 26 28 30)"

        );
    }

    #[test]
    fn ludicrous_input_rejected() {
        let mut bus = Topology::new();

        assert!(bus.device_connect(100, 100, true).is_none());
        assert_eq!(bus.device_disconnect(100, 100), 0);
    }
}
