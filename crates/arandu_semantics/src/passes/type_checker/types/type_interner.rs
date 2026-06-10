//! Type Interning — Canonical Type Identity via `TypeId`
//!
//! This module provides a `TypeInterner` that maps every `ArType` to a unique
//! `TypeId(u32)`. Interning serves several purposes:
//!
//! 1. **O(1) type equality** — comparing `TypeId`s is a single integer compare
//!    instead of a recursive structural walk.
//! 2. **Deduplication** — identical recursive types are stored only once.
//! 3. **Cache-friendliness** — downstream passes (monomorphization, codegen)
//!    can use `TypeId` as a dense index into `IndexVec`-backed tables.
//!
//! ## Design
//!
//! The interner uses a two-level scheme:
//! - A `HashMap<ArType, TypeId>` for dedup lookup.
//! - A `Vec<ArType>` (indexed by `TypeId`) for id→type resolution.
//!
//! Interning is additive and append-only; types are never removed.

use crate::SymbolTable;
use crate::newtype_index;
use crate::passes::type_checker::types::ArType;
use fxhash::FxHashMap;

newtype_index!(TypeId);

/// A global interner that assigns a unique `TypeId` to every structural `ArType`.
#[derive(Debug, Clone)]
pub struct TypeInterner {
    /// Forward map: ArType → TypeId  (deduplication).
    map: FxHashMap<ArType, TypeId>,
    /// Reverse map: TypeId → ArType  (resolution).
    types: Vec<ArType>,
}

impl TypeInterner {
    #[must_use]
    pub fn new() -> Self {
        Self {
            map: FxHashMap::default(),
            types: Vec::new(),
        }
    }

    /// Intern a type, returning its canonical `TypeId`.
    /// If the type has been interned before, returns the same id.
    pub fn intern(&mut self, ty: ArType) -> TypeId {
        if let Some(&id) = self.map.get(&ty) {
            return id;
        }
        let id = TypeId::from_usize(self.types.len());
        self.map.insert(ty.clone(), id);
        self.types.push(ty);
        id
    }

    /// Resolve a `TypeId` back to its `ArType`.
    ///
    /// # Panics
    /// Panics if `id` was not produced by this interner.
    #[must_use]
    pub fn resolve(&self, id: TypeId) -> &ArType {
        &self.types[id.as_usize()]
    }

    /// Try to resolve a `TypeId`, returning `None` if out of range.
    #[must_use]
    pub fn try_resolve(&self, id: TypeId) -> Option<&ArType> {
        self.types.get(id.as_usize())
    }

    /// Look up a type without interning it. Returns `None` if the type
    /// has never been interned.
    #[must_use]
    pub fn lookup(&self, ty: &ArType) -> Option<TypeId> {
        self.map.get(ty).copied()
    }

    /// Number of unique types interned so far.
    #[must_use]
    pub fn len(&self) -> usize {
        self.types.len()
    }

    /// Returns `true` if no types have been interned.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.types.is_empty()
    }

    /// Display a `TypeId` using the symbol table for named types.
    #[must_use]
    pub fn display(&self, id: TypeId, symbols: &SymbolTable) -> String {
        self.resolve(id).display(symbols)
    }
}

impl Default for TypeInterner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::passes::type_checker::types::Primitive;

    #[test]
    fn test_intern_returns_same_id_for_same_type() {
        let mut interner = TypeInterner::new();
        let id1 = interner.intern(ArType::Primitive(Primitive::Int));
        let id2 = interner.intern(ArType::Primitive(Primitive::Int));
        assert_eq!(id1, id2);
        assert_eq!(interner.len(), 1);
    }

    #[test]
    fn test_intern_returns_different_id_for_different_types() {
        let mut interner = TypeInterner::new();
        let id1 = interner.intern(ArType::Primitive(Primitive::Int));
        let id2 = interner.intern(ArType::Primitive(Primitive::Bool));
        assert_ne!(id1, id2);
        assert_eq!(interner.len(), 2);
    }

    #[test]
    fn test_resolve_roundtrip() {
        let mut interner = TypeInterner::new();
        let ty = ArType::Nullable(Box::new(ArType::Primitive(Primitive::Str)));
        let id = interner.intern(ty.clone());
        assert_eq!(*interner.resolve(id), ty);
    }

    #[test]
    fn test_lookup_returns_none_for_unknown_type() {
        let interner = TypeInterner::new();
        assert_eq!(interner.lookup(&ArType::Void), None);
    }

    #[test]
    fn test_lookup_returns_id_after_intern() {
        let mut interner = TypeInterner::new();
        let id = interner.intern(ArType::Void);
        assert_eq!(interner.lookup(&ArType::Void), Some(id));
    }

    #[test]
    fn test_complex_recursive_type_dedup() {
        let mut interner = TypeInterner::new();
        let result_ty = ArType::Result(
            Box::new(ArType::Primitive(Primitive::Int)),
            Box::new(ArType::Err),
        );
        let id1 = interner.intern(result_ty.clone());
        let id2 = interner.intern(result_ty);
        assert_eq!(id1, id2);
        assert_eq!(interner.len(), 1);
    }

    #[test]
    fn test_func_type_interning() {
        let mut interner = TypeInterner::new();
        let func_ty = ArType::Func(
            vec![
                ArType::Primitive(Primitive::Int),
                ArType::Primitive(Primitive::Str),
            ],
            Box::new(ArType::Primitive(Primitive::Bool)),
        );
        let id = interner.intern(func_ty.clone());
        assert_eq!(*interner.resolve(id), func_ty);
    }

    #[test]
    fn test_all_primitives_get_unique_ids() {
        let mut interner = TypeInterner::new();
        let prims = [
            Primitive::Int,
            Primitive::Uint,
            Primitive::Float,
            Primitive::Bool,
            Primitive::Str,
            Primitive::Char,
            Primitive::Byte,
            Primitive::Any,
        ];
        let ids: Vec<TypeId> = prims
            .iter()
            .map(|&p| interner.intern(ArType::Primitive(p)))
            .collect();
        // All IDs should be unique
        for (i, a) in ids.iter().enumerate() {
            for (j, b) in ids.iter().enumerate() {
                if i != j {
                    assert_ne!(
                        a, b,
                        "primitives {:?} and {:?} got same TypeId",
                        prims[i], prims[j]
                    );
                }
            }
        }
        assert_eq!(interner.len(), prims.len());
    }
}
