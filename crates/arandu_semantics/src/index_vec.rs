use std::marker::PhantomData;
use std::ops::{Index, IndexMut};

pub trait IdIndex: Copy + PartialEq + Eq {
    fn to_usize(self) -> usize;
    fn from_usize(value: usize) -> Self;
}

impl IdIndex for usize {
    #[inline]
    fn to_usize(self) -> usize {
        self
    }
    #[inline]
    fn from_usize(value: usize) -> Self {
        value
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IndexVec<I: IdIndex, T> {
    pub raw: Vec<T>,
    _marker: PhantomData<I>,
}

impl<I: IdIndex, T> IndexVec<I, T> {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            raw: Vec::new(),
            _marker: PhantomData,
        }
    }

    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            raw: Vec::with_capacity(capacity),
            _marker: PhantomData,
        }
    }

    pub fn push(&mut self, val: T) -> I {
        let idx = self.raw.len();
        self.raw.push(val);
        I::from_usize(idx)
    }

    pub fn push_many<Iter>(&mut self, values: Iter) -> std::ops::Range<I>
    where
        Iter: IntoIterator<Item = T>,
    {
        let start = self.raw.len();
        self.raw.extend(values);
        I::from_usize(start)..I::from_usize(self.raw.len())
    }

    pub fn get(&self, index: I) -> Option<&T> {
        self.raw.get(index.to_usize())
    }

    pub fn get_mut(&mut self, index: I) -> Option<&mut T> {
        self.raw.get_mut(index.to_usize())
    }

    pub fn len(&self) -> usize {
        self.raw.len()
    }

    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.raw.iter()
    }

    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, T> {
        self.raw.iter_mut()
    }

    pub fn ids(&self) -> impl Iterator<Item = I> + '_ {
        (0..self.raw.len()).map(I::from_usize)
    }

    pub fn as_slice(&self) -> &[T] {
        &self.raw
    }

    pub fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.raw
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::newtype_index;

    newtype_index!(TestId);

    #[test]
    fn ids_iterate_over_dense_indices() {
        let mut vec = IndexVec::<TestId, i32>::with_capacity(2);
        assert_eq!(TestId::from_usize(7).as_usize(), 7);
        assert_eq!(vec.push(10), TestId(0));
        assert_eq!(vec.push(20), TestId(1));
        assert_eq!(vec.ids().collect::<Vec<_>>(), vec![TestId(0), TestId(1)]);
        assert_eq!(vec.as_slice(), &[10, 20]);
    }

    #[test]
    fn push_many_returns_typed_range() {
        let mut vec = IndexVec::<TestId, i32>::new();
        vec.push(1);
        let range = vec.push_many([2, 3, 4]);
        assert_eq!(range, TestId(1)..TestId(4));
        assert_eq!(vec.as_slice(), &[1, 2, 3, 4]);
    }
}

impl<I: IdIndex, T> Index<I> for IndexVec<I, T> {
    type Output = T;
    fn index(&self, index: I) -> &Self::Output {
        &self.raw[index.to_usize()]
    }
}

impl<I: IdIndex, T> IndexMut<I> for IndexVec<I, T> {
    fn index_mut(&mut self, index: I) -> &mut Self::Output {
        &mut self.raw[index.to_usize()]
    }
}

impl<I: IdIndex, T> Default for IndexVec<I, T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<I: IdIndex, T> From<Vec<T>> for IndexVec<I, T> {
    fn from(raw: Vec<T>) -> Self {
        Self {
            raw,
            _marker: PhantomData,
        }
    }
}

#[macro_export]
macro_rules! newtype_index {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub u32);

        impl $name {
            #[must_use]
            #[inline]
            pub const fn as_usize(self) -> usize {
                self.0 as usize
            }

            #[must_use]
            #[inline]
            pub const fn from_usize(v: usize) -> Self {
                Self(v as u32)
            }
        }

        impl $crate::index_vec::IdIndex for $name {
            #[inline]
            fn to_usize(self) -> usize {
                self.0 as usize
            }

            #[inline]
            fn from_usize(value: usize) -> Self {
                Self(value as u32)
            }
        }
    };
}
