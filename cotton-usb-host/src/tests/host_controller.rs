use super::*;

#[test]
fn packet_default() {
    let p = InterruptPacket::default();
    assert_eq!(p.size, 0);
}

#[test]
fn packet_new() {
    let p = InterruptPacket::new();
    assert_eq!(p.size, 0);
}

#[test]
fn packet_deref() {
    let mut p = InterruptPacket::new();
    p.size = 10;
    p.data[9] = 1;
    assert_eq!(p.len(), 10);
    assert_eq!((&p)[9], 1);
}

fn add_one(b: &mut [u8]) {
    b[0] += 1;
}

#[test]
fn dataphase_accessors() {
    let mut b = [1u8; 1];
    let mut d1 = DataPhase::In(&mut b);
    assert!(d1.is_in());
    assert!(!d1.is_out());
    assert!(!d1.is_none());
    d1.in_with(add_one);
    assert_eq!(b[0], 2);
    let mut d1 = DataPhase::Out(&b);
    assert!(!d1.is_in());
    assert!(d1.is_out());
    assert!(!d1.is_none());
    d1.in_with(add_one);
    assert_eq!(b[0], 2); // not IN, nothing added
    let mut d1 = DataPhase::None;
    assert!(!d1.is_in());
    assert!(!d1.is_out());
    assert!(d1.is_none());
    d1.in_with(add_one);
    assert_eq!(b[0], 2); // not IN, nothing added
}
