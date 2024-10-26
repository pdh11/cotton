#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BitSet(pub u32);

impl BitSet {
    pub const fn new() -> Self {
        Self(0)
    }

    pub fn iter(&self) -> impl Iterator<Item = u8> {
        BitIterator::new(self.0)
    }

    pub fn set(&mut self, n: u8) {
        assert!(n < 32);
        self.0 |= 1 << n;
    }

    pub fn clear(&mut self, n: u8) {
        assert!(n < 32);
        self.0 &= !(1 << n);
    }

    pub fn set_any(&mut self) -> Option<u8> {
        let next = self.0.trailing_ones() as u8;
        if next >= 32 {
            None
        } else {
            self.set(next);
            Some(next)
        }
    }

    pub fn contains(&self, n: u8) -> bool {
        assert!(n < 32);
        (self.0 & (1 << n)) != 0
    }
}

impl Default for BitSet {
    fn default() -> Self {
        Self::new()
    }
}

pub struct BitIterator(pub u32);

impl BitIterator {
    pub const fn new(n: u32) -> Self {
        Self(n)
    }
}

impl Iterator for BitIterator {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        if self.0 == 0 {
            None
        } else {
            let n = self.0.trailing_zeros();
            self.0 &= !(1 << n);
            Some(n as u8)
        }
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    extern crate alloc;

    #[test]
    fn set() {
        let mut bs = BitSet::new();
        bs.set(4);
        assert_eq!(bs.0, 1 << 4);
    }

    #[test]
    fn clear() {
        let mut bs = BitSet::default();
        bs.0 = 0xFFFF;
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
}
