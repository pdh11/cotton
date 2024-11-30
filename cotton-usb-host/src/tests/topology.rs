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
    assert_eq!(m.0, 0x8000_0000);
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
    assert_eq!(m.0, 0x8000_0000);
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
    assert_eq!(m.0, 0x8000_0002);
    let e = format!("{:?}", bus);
    assert_eq!(e, "0");
}

#[test]
fn repeated_connect() {
    let mut bus = Topology::new();
    let d = bus.device_connect(0, 1, true).unwrap();
    assert_eq!(d, 1);
    let d = bus.device_connect(0, 1, true).unwrap();
    assert_eq!(d, 1);
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
        hubs += 1;
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
    bus.device_connect(0, 14, true);
    bus.device_connect(0, 13, true);

    loop {
        let d = bus.device_connect(devices & 3, (devices / 4) + 1, false);
        if d.is_none() {
            break;
        }
        devices += 1;
    }
    assert_eq!(devices, 28); // plus the three hubs, is 31
    assert_eq!(format!("{:?}", bus), "0:(1:(6 10 14 18 22 26 30) 2:(5 9 13 17 21 25 29) 3:(4 8 12 16 20 24 28) 7 11 15 19 23 27 31)"
        );
}

#[test]
fn ludicrous_input_rejected() {
    let mut bus = Topology::new();

    assert!(bus.device_connect(100, 100, true).is_none());
    assert_eq!(bus.device_disconnect(100, 100).0, 0);
}
