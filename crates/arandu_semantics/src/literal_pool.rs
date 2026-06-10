//! Interned literal storage for AMIR constants (C2).

use fxhash::FxHashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LiteralId(pub u32);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AmirLiteralEntry {
    Int(String),
    Float(String),
    Str(String),
    Char(String),
}

#[derive(Debug, Default, Clone)]
pub struct AmirLiteralPool {
    pub entries: Vec<AmirLiteralEntry>,
    pub index: FxHashMap<AmirLiteralEntry, LiteralId>,
}

impl AmirLiteralPool {
    pub fn intern(&mut self, entry: AmirLiteralEntry) -> LiteralId {
        if let Some(&id) = self.index.get(&entry) {
            return id;
        }
        let id = LiteralId(self.entries.len() as u32);
        self.index.insert(entry.clone(), id);
        self.entries.push(entry);
        id
    }

    #[must_use]
    pub fn get(&self, id: LiteralId) -> &AmirLiteralEntry {
        &self.entries[id.0 as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_literal_deduplication() {
        let mut pool = AmirLiteralPool::default();
        let lit1 = pool.intern(AmirLiteralEntry::Int("42".to_string()));
        let lit2 = pool.intern(AmirLiteralEntry::Int("42".to_string()));
        let lit3 = pool.intern(AmirLiteralEntry::Int("100".to_string()));

        assert_eq!(lit1, lit2);
        assert_ne!(lit1, lit3);
        assert_eq!(pool.entries.len(), 2);
    }
}
