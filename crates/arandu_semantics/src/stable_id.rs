use std::hash::{Hash, Hasher};
use std::marker::PhantomData;

/// A type-safe generational ID consisting of an `index` and a `generation`.
/// Used in LSP and incremental query compilation to detect stale references.
pub struct GenerationalId<T> {
    index: u32,
    generation: u32,
    _marker: PhantomData<T>,
}

impl<T> GenerationalId<T> {
    /// Creates a new `GenerationalId` with given index and generation.
    #[must_use]
    pub const fn new(index: u32, generation: u32) -> Self {
        Self {
            index,
            generation,
            _marker: PhantomData,
        }
    }

    /// Returns the raw index of this ID.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.index
    }

    /// Returns the generation number of this ID.
    #[must_use]
    pub const fn generation(self) -> u32 {
        self.generation
    }
}

impl<T> Clone for GenerationalId<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for GenerationalId<T> {}

impl<T> std::fmt::Debug for GenerationalId<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "GenerationalId({}g{})", self.index, self.generation)
    }
}

impl<T> PartialEq for GenerationalId<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.generation == other.generation
    }
}

impl<T> Eq for GenerationalId<T> {}

impl<T> PartialOrd for GenerationalId<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for GenerationalId<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.index
            .cmp(&other.index)
            .then_with(|| self.generation.cmp(&other.generation))
    }
}

impl<T> Hash for GenerationalId<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.index.hash(state);
        self.generation.hash(state);
    }
}

#[derive(Debug, Clone)]
struct Slot<T> {
    value: Option<T>,
    generation: u32,
    next_free: Option<u32>,
}

/// A generational SlotMap implementation providing O(1) insertion, retrieval, and deletion.
/// When slots are freed, their generation is incremented, rendering old `GenerationalId` keys invalid.
#[derive(Debug, Clone)]
pub struct SlotMap<T> {
    slots: Vec<Slot<T>>,
    free_head: Option<u32>,
    len: usize,
}

impl<T> SlotMap<T> {
    /// Creates an empty `SlotMap`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slots: Vec::new(),
            free_head: None,
            len: 0,
        }
    }

    /// Inserts a value into the SlotMap, returning its unique type-safe `GenerationalId`.
    pub fn insert(&mut self, value: T) -> GenerationalId<T> {
        if let Some(index) = self.free_head {
            let slot = &mut self.slots[index as usize];
            let next_free = slot.next_free;
            slot.value = Some(value);
            slot.generation += 1;
            slot.next_free = None;
            self.free_head = next_free;
            self.len += 1;
            GenerationalId::new(index, slot.generation)
        } else {
            let index = self.slots.len() as u32;
            let slot = Slot {
                value: Some(value),
                generation: 1,
                next_free: None,
            };
            self.slots.push(slot);
            self.len += 1;
            GenerationalId::new(index, 1)
        }
    }

    /// Retrieves a reference to the value associated with the key.
    /// Returns `None` if the key is invalid or has been replaced.
    pub fn get(&self, key: GenerationalId<T>) -> Option<&T> {
        let slot = self.slots.get(key.index() as usize)?;
        if slot.generation == key.generation() {
            slot.value.as_ref()
        } else {
            None
        }
    }

    /// Retrieves a mutable reference to the value associated with the key.
    /// Returns `None` if the key is invalid or has been replaced.
    pub fn get_mut(&mut self, key: GenerationalId<T>) -> Option<&mut T> {
        let slot = self.slots.get_mut(key.index() as usize)?;
        if slot.generation == key.generation() {
            slot.value.as_mut()
        } else {
            None
        }
    }

    /// Removes the value associated with the key, returning it.
    /// Returns `None` if the key was invalid.
    pub fn remove(&mut self, key: GenerationalId<T>) -> Option<T> {
        let index = key.index() as usize;
        let slot = self.slots.get_mut(index)?;
        if slot.generation == key.generation() && slot.value.is_some() {
            let val = slot.value.take();
            slot.next_free = self.free_head;
            self.free_head = Some(key.index());
            self.len -= 1;
            val
        } else {
            None
        }
    }

    /// Returns `true` if the SlotMap contains a valid entry for the key.
    #[must_use]
    pub fn contains_key(&self, key: GenerationalId<T>) -> bool {
        self.get(key).is_some()
    }

    /// Returns the number of occupied slots in the SlotMap.
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if the SlotMap contains no elements.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Clears all elements from the SlotMap.
    pub fn clear(&mut self) {
        self.slots.clear();
        self.free_head = None;
        self.len = 0;
    }
}

impl<T> Default for SlotMap<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slotmap_insert_get_remove() {
        let mut map = SlotMap::<String>::new();
        let k1 = map.insert("hello".to_string());
        let k2 = map.insert("world".to_string());

        assert_eq!(map.len(), 2);
        assert_eq!(map.get(k1), Some(&"hello".to_string()));
        assert_eq!(map.get(k2), Some(&"world".to_string()));

        // Remove k1
        let val = map.remove(k1);
        assert_eq!(val, Some("hello".to_string()));
        assert_eq!(map.len(), 1);

        // Accessing k1 now should return None
        assert_eq!(map.get(k1), None);

        // Re-inserting should re-use slot but change generation
        let k3 = map.insert("rust".to_string());
        assert_eq!(k3.index(), k1.index());
        assert_ne!(k3.generation(), k1.generation());
        assert_eq!(map.get(k3), Some(&"rust".to_string()));
        assert_eq!(map.get(k1), None); // Old key remains invalid
    }
}
