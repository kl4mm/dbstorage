pub struct BitMap<const SIZE: usize> {
    inner: [u8; SIZE],
    occupied: usize,
}

impl<const SIZE: usize> Default for BitMap<SIZE> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const SIZE: usize> BitMap<SIZE> {
    pub fn new() -> Self {
        Self {
            inner: [0; SIZE],
            occupied: 0,
        }
    }

    pub fn set(&mut self, i: usize, val: bool) {
        let index = i >> 3;
        let bit_index = 1 << (i & 7);

        if val {
            if !self.check(i) {
                self.occupied += 1;
            }

            self.inner[index] |= bit_index;
        } else {
            if self.check(i) && self.occupied > 0 {
                self.occupied -= 1;
            }

            self.inner[index] &= !bit_index;
        }
    }

    pub fn check(&self, i: usize) -> bool {
        let pos_i = i / 8;
        let pos_j = i % 8;

        let b = self.inner[pos_i];

        (1 << pos_j) & b > 0
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.inner
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.inner
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.occupied == 0
    }

    #[inline]
    pub fn is_full(&self) -> bool {
        self.occupied == SIZE * 8
    }
}

#[cfg(test)]
mod test {
    use super::BitMap;

    #[test]
    fn test_bitmap() {
        let mut bm = BitMap::<128>::new();

        bm.set(0, true);
        bm.set(1, true);
        bm.set(8, true);
        bm.set(177, true);
        bm.set(200, true);
        bm.set(512, true);

        assert!(bm.check(0));
        assert!(bm.check(1));
        assert!(bm.check(8));
        assert!(bm.check(177));
        assert!(bm.check(200));
        assert!(bm.check(512));
    }

    #[test]
    fn test_occupied() {
        let mut bm = BitMap::<1>::new();
        assert!(bm.is_empty());

        bm.set(0, true);
        assert!(!bm.is_empty());

        bm.set(1, true);
        bm.set(2, true);
        bm.set(3, true);
        bm.set(4, true);
        bm.set(5, true);
        bm.set(6, true);
        bm.set(7, true);

        assert!(bm.is_full());
    }
}
