/// A compact representation of a set of integers, 0-31 inclusive
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct BitSet(
    /// A bitfield, with a 1 in bit N signifying that N is present in the BitSet
    pub u32,
);

impl BitSet {
    /// Create a new, empty BitSet
    pub const fn new() -> Self {
        Self(0)
    }

    /// An iterator over the integers currently in the set
    ///
    /// Note this is not a "live" representation: a snapshot of set membership
    /// is taken when you call iter().
    pub fn iter(&self) -> impl Iterator<Item = u8> {
        BitIterator::new(self.0)
    }

    /// Add n to the set
    pub fn set(&mut self, n: u8) {
        assert!(n < 32);
        self.0 |= 1 << n;
    }

    /// Remove n from the set, if present
    pub fn clear(&mut self, n: u8) {
        assert!(n < 32);
        self.0 &= !(1 << n);
    }

    /// Add to the set the smallest integer not already present
    ///
    /// And return it. Or if the set is "full" (integers 0-31 are all
    /// present), return None.
    pub fn set_any(&mut self) -> Option<u8> {
        let next = self.0.trailing_ones() as u8;
        if next >= 32 {
            None
        } else {
            self.set(next);
            Some(next)
        }
    }

    /// Is n present in the set?
    pub fn contains(&self, n: u8) -> bool {
        assert!(n < 32);
        (self.0 & (1 << n)) != 0
    }
}

struct BitIterator(u32);

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
