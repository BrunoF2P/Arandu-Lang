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

#[derive(Debug, Clone)]
struct DenseSlot {
    index_or_next_free: u32,
    generation: u32,
}

/// A generational DenseSlotMap implementation providing O(1) insertion, retrieval, and deletion.
/// Values are stored in a contiguous vector (`values`), guaranteeing excellent cache locality for linear iterations.
#[derive(Debug, Clone)]
pub struct DenseSlotMap<T> {
    slots: Vec<DenseSlot>,
    dense_to_slot: Vec<u32>,
    values: Vec<T>,
    free_head: Option<u32>,
}

impl<T> DenseSlotMap<T> {
    /// Creates an empty `DenseSlotMap`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slots: Vec::new(),
            dense_to_slot: Vec::new(),
            values: Vec::new(),
            free_head: None,
        }
    }

    /// Inserts a value into the DenseSlotMap, returning its unique type-safe `GenerationalId`.
    pub fn insert(&mut self, value: T) -> GenerationalId<T> {
        let dense_idx = self.values.len() as u32;
        if let Some(slot_idx) = self.free_head {
            let slot = &mut self.slots[slot_idx as usize];
            let next_free = if slot.index_or_next_free == u32::MAX {
                None
            } else {
                Some(slot.index_or_next_free)
            };
            slot.index_or_next_free = dense_idx;
            slot.generation += 1;
            self.free_head = next_free;

            self.dense_to_slot.push(slot_idx);
            self.values.push(value);
            GenerationalId::new(slot_idx, slot.generation)
        } else {
            let slot_idx = self.slots.len() as u32;
            let slot = DenseSlot {
                index_or_next_free: dense_idx,
                generation: 1,
            };
            self.slots.push(slot);
            self.dense_to_slot.push(slot_idx);
            self.values.push(value);
            GenerationalId::new(slot_idx, 1)
        }
    }

    /// Retrieves a reference to the value associated with the key.
    /// Returns `None` if the key is invalid or has been replaced.
    pub fn get(&self, key: GenerationalId<T>) -> Option<&T> {
        let slot = self.slots.get(key.index() as usize)?;
        if slot.generation == key.generation() {
            let dense_idx = slot.index_or_next_free as usize;
            self.values.get(dense_idx)
        } else {
            None
        }
    }

    /// Retrieves a mutable reference to the value associated with the key.
    /// Returns `None` if the key is invalid or has been replaced.
    pub fn get_mut(&mut self, key: GenerationalId<T>) -> Option<&mut T> {
        let slot = self.slots.get_mut(key.index() as usize)?;
        if slot.generation == key.generation() {
            let dense_idx = slot.index_or_next_free as usize;
            self.values.get_mut(dense_idx)
        } else {
            None
        }
    }

    /// Removes the value associated with the key, returning it.
    /// Returns `None` if the key was invalid.
    pub fn remove(&mut self, key: GenerationalId<T>) -> Option<T> {
        let slot = self.slots.get(key.index() as usize)?;
        if slot.generation != key.generation() {
            return None;
        }

        let dense_idx = slot.index_or_next_free as usize;
        let last_dense_idx = self.values.len() - 1;

        // Guard: only update dense_to_slot if the swapped element is not the removed one itself
        if dense_idx != last_dense_idx {
            let last_slot_idx = self.dense_to_slot[last_dense_idx];
            self.dense_to_slot[dense_idx] = last_slot_idx;
            self.slots[last_slot_idx as usize].index_or_next_free = dense_idx as u32;
        }

        self.dense_to_slot.pop();
        let value = self.values.swap_remove(dense_idx);

        // Put the slot back in the free list and bump its generation
        let free_slot = &mut self.slots[key.index() as usize];
        free_slot.generation += 1;
        free_slot.index_or_next_free = self.free_head.unwrap_or(u32::MAX);
        self.free_head = Some(key.index());

        Some(value)
    }

    /// Returns `true` if the DenseSlotMap contains a valid entry for the key.
    #[must_use]
    pub fn contains_key(&self, key: GenerationalId<T>) -> bool {
        self.get(key).is_some()
    }

    /// Returns the number of occupied slots in the DenseSlotMap.
    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Returns `true` if the DenseSlotMap contains no elements.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Clears all elements from the DenseSlotMap.
    pub fn clear(&mut self) {
        self.slots.clear();
        self.dense_to_slot.clear();
        self.values.clear();
        self.free_head = None;
    }

    /// Returns a slice of the contiguous raw values inside the slot map.
    #[must_use]
    pub fn values(&self) -> &[T] {
        &self.values
    }

    /// Returns a mutable slice of the contiguous raw values inside the slot map.
    #[must_use]
    pub fn values_mut(&mut self) -> &mut [T] {
        &mut self.values
    }

    /// Iterates over all active keys and values in the slot map.
    /// Note: The iteration order matches the memory layout (insertion order with holes
    /// filled by swap-removes), not necessarily the original sequence of ID creation.
    pub fn iter(&self) -> impl Iterator<Item = (GenerationalId<T>, &T)> {
        self.values.iter().enumerate().map(|(dense_idx, value)| {
            let slot_idx = self.dense_to_slot[dense_idx];
            let generation = self.slots[slot_idx as usize].generation;
            let id = GenerationalId::new(slot_idx, generation);
            (id, value)
        })
    }
}

impl<T> Default for DenseSlotMap<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// A deterministic, session-independent identifier for global symbols (like functions, types, and scopes).
use std::sync::Arc;

/// Calculated by hashing the symbol's fully qualified path (e.g. `prelude::io::println`) to guarantee stability,
/// while keeping the path string to resolve collisions and guarantee identity.
#[derive(Debug, Clone, Ord, PartialOrd)]
pub struct StableHandle {
    pub hash: u64,
    pub path: Arc<str>,
}

impl PartialEq for StableHandle {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash && self.path == other.path
    }
}

impl Eq for StableHandle {}

impl std::hash::Hash for StableHandle {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.hash.hash(state);
        self.path.hash(state);
    }
}

impl StableHandle {
    /// Creates a stable handle by hashing the given qualified path.
    #[must_use]
    pub fn from_path(path: &str) -> Self {
        use std::hash::Hasher;
        let mut hasher = rustc_hash::FxHasher::default();
        hasher.write(path.as_bytes());
        let hash = hasher.finish();
        Self {
            hash,
            path: Arc::from(path),
        }
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

    #[test]
    fn test_dense_slotmap_basic() {
        let mut map = DenseSlotMap::<String>::new();
        let k1 = map.insert("hello".to_string());
        let k2 = map.insert("world".to_string());

        assert_eq!(map.len(), 2);
        assert_eq!(map.get(k1), Some(&"hello".to_string()));
        assert_eq!(map.get(k2), Some(&"world".to_string()));

        // Swap remove the first item
        let val = map.remove(k1);
        assert_eq!(val, Some("hello".to_string()));
        assert_eq!(map.len(), 1);
        assert_eq!(map.get(k1), None);
        assert_eq!(map.get(k2), Some(&"world".to_string())); // Still valid!

        // Values slice should be dense and contiguous
        assert_eq!(map.values(), &["world".to_string()]);
    }

    #[test]
    fn stale_key_returns_none_after_remove() {
        let mut map = DenseSlotMap::new();
        let key = map.insert(42u32);
        map.remove(key);
        assert!(map.get(key).is_none()); // antiga geração, deve falhar

        let key2 = map.insert(99u32); // reutiliza o slot, geração+1
        assert!(map.get(key).is_none()); // key antigo ainda inválido
        assert_eq!(map.get(key2), Some(&99u32));
    }

    #[test]
    fn test_dense_slotmap_iter() {
        let mut map = DenseSlotMap::new();
        let k1 = map.insert("a".to_string());
        let k2 = map.insert("b".to_string());
        let k3 = map.insert("c".to_string());

        let pairs: Vec<_> = map.iter().collect();
        assert_eq!(pairs.len(), 3);
        assert_eq!(pairs[0], (k1, &"a".to_string()));
        assert_eq!(pairs[1], (k2, &"b".to_string()));
        assert_eq!(pairs[2], (k3, &"c".to_string()));

        // Remove the middle one
        map.remove(k2);

        let pairs2: Vec<_> = map.iter().collect();
        assert_eq!(pairs2.len(), 2);
        // "c" should be swapped into index 1 (where "b" was)
        assert_eq!(pairs2[0], (k1, &"a".to_string()));
        assert_eq!(pairs2[1], (k3, &"c".to_string()));
    }

    #[test]
    fn test_stable_handle_determinism() {
        let h1 = StableHandle::from_path("foo::bar");
        let h2 = StableHandle::from_path("foo::bar");
        let h3 = StableHandle::from_path("foo::baz");

        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
    }

    #[test]
    fn test_stable_handle_collision_safety() {
        let h1 = StableHandle {
            hash: 12345,
            path: std::sync::Arc::from("path::one"),
        };
        let h2 = StableHandle {
            hash: 12345,
            path: std::sync::Arc::from("path::two"),
        };
        assert_ne!(h1, h2);
    }
}
