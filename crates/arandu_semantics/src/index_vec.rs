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

    pub fn push(&mut self, val: T) -> I {
        let idx = self.raw.len();
        self.raw.push(val);
        I::from_usize(idx)
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
