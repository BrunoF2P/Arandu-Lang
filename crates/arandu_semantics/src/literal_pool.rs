//! Interned literal storage for AMIR constants (C2).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LiteralId(pub u32);

#[derive(Debug, Default, Clone)]
pub struct AmirLiteralPool {
    pub entries: Vec<AmirLiteralEntry>,
}

#[derive(Debug, Clone)]
pub enum AmirLiteralEntry {
    Int(String),
    Float(String),
    Str(String),
    Char(String),
}

impl AmirLiteralPool {
    pub fn intern(&mut self, entry: AmirLiteralEntry) -> LiteralId {
        let id = self.entries.len();
        self.entries.push(entry);
        LiteralId(id as u32)
    }

    pub fn get(&self, id: LiteralId) -> &AmirLiteralEntry {
        &self.entries[id.0 as usize]
    }
}
