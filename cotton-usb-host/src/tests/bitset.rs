use super::*;
extern crate alloc;

#[test]
fn set() {
    let mut bs = BitSet::default();
    bs.set(4);
    assert_eq!(bs.0, 1 << 4);
}

#[test]
fn clear() {
    let mut bs = BitSet(0xFFFF);
    bs.clear(7);
    assert_eq!(bs.0, 0xFF7F);
}

#[test]
fn iter() {
    let mut bs = BitSet::new();
    bs.0 = 0x80008001;
    let mut iter = bs.iter();
    assert_eq!(iter.next(), Some(0));
    assert_eq!(iter.next(), Some(15));
    assert_eq!(iter.next(), Some(31));
    assert_eq!(iter.next(), None);
}

#[test]
fn contains() {
    let mut bs = BitSet::new();
    bs.0 = 0x80008001;
    assert!(bs.contains(0));
    assert!(bs.contains(31));
    assert!(!bs.contains(1));
    assert!(!bs.contains(30));
}

#[test]
fn set_any() {
    let mut bs = BitSet::new();
    let n = bs.set_any();
    assert_eq!(n, Some(0));
}

#[test]
fn set_any_final() {
    let mut bs = BitSet::new();
    bs.0 = 0x7FFF_FFFF;
    let n = bs.set_any();
    assert_eq!(n, Some(31));
}

#[test]
fn set_any_fail() {
    let mut bs = BitSet::new();
    bs.0 = 0xFFFF_FFFF;
    let n = bs.set_any();
    assert_eq!(n, None);
}
