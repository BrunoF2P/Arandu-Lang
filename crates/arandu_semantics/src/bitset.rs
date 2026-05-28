use crate::index_vec::IdIndex;
use std::marker::PhantomData;

/// A high-performance, dense bitset designed for compiler analyses (liveness, definite initialization, DCE, SSA).
/// Operates on 64-bit words for maximum CPU throughput and cache locality.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BitSet<I: IdIndex = usize> {
    words: Vec<u64>,
    _marker: PhantomData<I>,
}

impl<I: IdIndex> BitSet<I> {
    /// Creates a new, empty `BitSet`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            words: Vec::new(),
            _marker: PhantomData,
        }
    }

    /// Creates a new `BitSet` pre-allocated to hold elements up to `capacity`.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        let num_words = capacity.div_ceil(64);
        Self {
            words: vec![0; num_words],
            _marker: PhantomData,
        }
    }

    /// Inserts `id` into the bitset. Returns `true` if the element was not already present.
    pub fn insert(&mut self, id: I) -> bool {
        let bit = id.to_usize();
        let word_idx = bit / 64;
        let bit_idx = bit % 64;
        if word_idx >= self.words.len() {
            self.words.resize(word_idx + 1, 0);
        }
        let mask = 1u64 << bit_idx;
        let old = self.words[word_idx];
        self.words[word_idx] |= mask;
        (old & mask) == 0
    }

    /// Removes `id` from the bitset. Returns `true` if the element was present.
    pub fn remove(&mut self, id: I) -> bool {
        let bit = id.to_usize();
        let word_idx = bit / 64;
        let bit_idx = bit % 64;
        if word_idx >= self.words.len() {
            return false;
        }
        let mask = 1u64 << bit_idx;
        let old = self.words[word_idx];
        self.words[word_idx] &= !mask;
        (old & mask) != 0
    }

    /// Checks if `id` is present in the bitset.
    #[must_use]
    pub fn contains(&self, id: I) -> bool {
        let bit = id.to_usize();
        let word_idx = bit / 64;
        let bit_idx = bit % 64;
        if word_idx >= self.words.len() {
            return false;
        }
        (self.words[word_idx] & (1u64 << bit_idx)) != 0
    }

    /// Clears all elements from the bitset, keeping the allocated capacity.
    pub fn clear(&mut self) {
        self.words.fill(0);
    }

    /// Returns `true` if the bitset contains no elements.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.words.iter().all(|&w| w == 0)
    }

    /// Returns the number of set bits (elements) in the bitset.
    #[must_use]
    pub fn len(&self) -> usize {
        self.words.iter().map(|&w| w.count_ones() as usize).sum()
    }

    /// Computes the union in-place (`self |= other`).
    /// Returns `true` if `self` was changed.
    pub fn union_with(&mut self, other: &Self) -> bool {
        let mut changed = false;
        if other.words.len() > self.words.len() {
            self.words.resize(other.words.len(), 0);
        }
        for (w_self, &w_other) in self.words.iter_mut().zip(other.words.iter()) {
            let old = *w_self;
            let new = old | w_other;
            if old != new {
                *w_self = new;
                changed = true;
            }
        }
        changed
    }

    /// Computes the intersection in-place (`self &= other`).
    /// Returns `true` if `self` was changed.
    pub fn intersect_with(&mut self, other: &Self) -> bool {
        let mut changed = false;
        let len_self = self.words.len();
        let len_other = other.words.len();
        let min_len = usize::min(len_self, len_other);
        for i in 0..min_len {
            let old = self.words[i];
            let new = old & other.words[i];
            if old != new {
                self.words[i] = new;
                changed = true;
            }
        }
        for i in min_len..len_self {
            if self.words[i] != 0 {
                self.words[i] = 0;
                changed = true;
            }
        }
        changed
    }

    /// Computes the difference in-place (`self &= !other`).
    /// Returns `true` if `self` was changed.
    pub fn difference_with(&mut self, other: &Self) -> bool {
        let mut changed = false;
        let len_self = self.words.len();
        let len_other = other.words.len();
        let min_len = usize::min(len_self, len_other);
        for i in 0..min_len {
            let old = self.words[i];
            let new = old & !other.words[i];
            if old != new {
                self.words[i] = new;
                changed = true;
            }
        }
        changed
    }

    /// Returns an iterator over all elements present in the bitset.
    pub fn iter(&self) -> BitSetIter<'_, I> {
        BitSetIter {
            set: self,
            word_idx: 0,
            bit_idx: 0,
        }
    }
}

pub struct BitSetIter<'a, I: IdIndex> {
    set: &'a BitSet<I>,
    word_idx: usize,
    bit_idx: usize,
}

impl<'a, I: IdIndex> Iterator for BitSetIter<'a, I> {
    type Item = I;

    fn next(&mut self) -> Option<Self::Item> {
        while self.word_idx < self.set.words.len() {
            let word = self.set.words[self.word_idx];
            if word == 0 {
                self.word_idx += 1;
                self.bit_idx = 0;
                continue;
            }
            let remaining_word = word >> self.bit_idx;
            if remaining_word == 0 {
                self.word_idx += 1;
                self.bit_idx = 0;
                continue;
            }
            let tz = remaining_word.trailing_zeros() as usize;
            self.bit_idx += tz;
            let val = self.word_idx * 64 + self.bit_idx;
            self.bit_idx += 1;
            if self.bit_idx >= 64 {
                self.word_idx += 1;
                self.bit_idx = 0;
            }
            return Some(I::from_usize(val));
        }
        None
    }
}

impl<I: IdIndex> Default for BitSet<I> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_contains_remove() {
        let mut set = BitSet::<usize>::new();
        assert!(set.is_empty());
        assert_eq!(set.len(), 0);

        assert!(set.insert(10));
        assert!(set.insert(100));
        assert!(!set.insert(10));

        assert_eq!(set.len(), 2);
        assert!(!set.is_empty());

        assert!(set.contains(10));
        assert!(set.contains(100));
        assert!(!set.contains(50));

        assert!(set.remove(10));
        assert!(!set.contains(10));
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn test_union_intersection_difference() {
        let mut a = BitSet::<usize>::new();
        a.insert(1);
        a.insert(2);

        let mut b = BitSet::<usize>::new();
        b.insert(2);
        b.insert(3);

        let mut u = a.clone();
        assert!(u.union_with(&b));
        assert!(u.contains(1));
        assert!(u.contains(2));
        assert!(u.contains(3));

        let mut i = a.clone();
        assert!(i.intersect_with(&b));
        assert!(!i.contains(1));
        assert!(i.contains(2));
        assert!(!i.contains(3));

        let mut d = a.clone();
        assert!(d.difference_with(&b));
        assert!(d.contains(1));
        assert!(!d.contains(2));
    }

    #[test]
    fn test_iter() {
        let mut set = BitSet::<usize>::new();
        set.insert(5);
        set.insert(63);
        set.insert(64);
        set.insert(129);

        let items: Vec<usize> = set.iter().collect();
        assert_eq!(items, vec![5, 63, 64, 129]);
    }
}
