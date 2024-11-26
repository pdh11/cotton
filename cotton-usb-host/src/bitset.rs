#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Clone, Copy, PartialEq, Eq, Default)]
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
#[path = "tests/bitset.rs"]
mod tests;
