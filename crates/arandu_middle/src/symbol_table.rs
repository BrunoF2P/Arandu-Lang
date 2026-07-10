#[cfg(test)]
mod tests {
    use super::*;
    const S: Span = Span::new(0, 0, 0);

    #[test]
    fn new_table_has_global_scope() {
        let table = SymbolTable::new(0);
        assert_eq!(table.global_scope(), ScopeId(0));
    }

    #[test]
    fn define_and_get_symbol() {
        let mut table = SymbolTable::new(0);
        let id = table
            .define(ScopeId(0), "foo", SymbolKind::Func, S)
            .unwrap();
        assert_eq!(id, SymbolId::new(0, 0));
        assert_eq!(table.get(id).name, "foo");
        assert_eq!(table.get(id).kind, SymbolKind::Func);
    }

    #[test]
    fn define_duplicate_in_same_scope_fails() {
        let mut table = SymbolTable::new(0);
        table
            .define(ScopeId(0), "dup", SymbolKind::Const, S)
            .unwrap();
        let result = table.define(ScopeId(0), "dup", SymbolKind::Const, S);
        assert!(result.is_err());
    }

    #[test]
    fn lookup_any_walks_scope_chain() {
        let mut table = SymbolTable::new(0);
        let outer = ScopeId(0);
        let inner = table.new_scope(outer);
        table.define(outer, "a", SymbolKind::Const, S).unwrap();
        assert!(table.lookup_any(inner, "a").is_some());
    }

    #[test]
    fn lookup_any_not_found() {
        let table = SymbolTable::new(0);
        assert!(table.lookup_any(ScopeId(0), "nonexistent").is_none());
    }

    #[test]
    fn lookup_value_skips_type_symbols() {
        let mut table = SymbolTable::new(0);
        table
            .define(ScopeId(0), "MyType", SymbolKind::Struct, S)
            .unwrap();
        assert!(table.lookup_value(ScopeId(0), "MyType").is_none());
        assert!(table.lookup_type(ScopeId(0), "MyType").is_some());
    }

    #[test]
    fn new_scope_increases_scope_count() {
        let mut table = SymbolTable::new(0);
        assert_eq!(table.scopes.len(), 1);
        let child = table.new_scope(ScopeId(0));
        assert_eq!(child, ScopeId(1));
        assert_eq!(table.scopes.len(), 2);
    }

    #[test]
    fn module_members_basic() {
        let mut table = SymbolTable::new(0);
        let id = table.define_module_member("mod1", "foo", S).unwrap();
        assert_eq!(table.lookup_module_member("mod1", "foo"), Some(id));
        assert_eq!(table.lookup_module_member("mod1", "bar"), None);
    }

    #[test]
    fn associated_members_basic() {
        let mut table = SymbolTable::new(0);
        let id = table
            .define_associated_member("MyStruct", "method", S)
            .unwrap();
        assert_eq!(
            table.lookup_associated_member("MyStruct", "method"),
            Some(id)
        );
    }

    #[test]
    fn value_candidates_filters_type() {
        let mut table = SymbolTable::new(0);
        table
            .define(ScopeId(0), "fn1", SymbolKind::Func, S)
            .unwrap();
        table
            .define(ScopeId(0), "St", SymbolKind::Struct, S)
            .unwrap();
        let vals = table.value_candidates(ScopeId(0));
        assert_eq!(vals.len(), 1);
        assert_eq!(vals[0].name, "fn1");
    }

    #[test]
    fn type_candidates_filters_value() {
        let mut table = SymbolTable::new(0);
        table
            .define(ScopeId(0), "fn1", SymbolKind::Func, S)
            .unwrap();
        table
            .define(ScopeId(0), "St", SymbolKind::Struct, S)
            .unwrap();
        let types = table.type_candidates(ScopeId(0));
        assert_eq!(types.len(), 1);
        assert_eq!(types[0].name, "St");
    }

    #[test]
    fn merge_from_basic() {
        let mut table1 = SymbolTable::new(0);
        let mut table2 = SymbolTable::new(0);
        let _id = table2
            .define(ScopeId(0), "other", SymbolKind::Const, S)
            .unwrap();
        let len_before = table1.symbols.len();
        table1.merge_from(table2);
        assert_eq!(table1.symbols.len(), len_before + 1);
        assert!(table1.lookup_any(ScopeId(0), "other").is_some());
    }

    #[test]
    fn iter_includes_all_symbols() {
        let mut table = SymbolTable::new(0);
        table.define(ScopeId(0), "a", SymbolKind::Func, S).unwrap();
        table.define(ScopeId(0), "b", SymbolKind::Const, S).unwrap();
        let names: Vec<&str> = table.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"a"));
        assert!(names.contains(&"b"));
    }

    #[test]
    fn kind_classification() {
        assert!(SymbolKind::Func.is_value());
        assert!(!SymbolKind::Func.is_type());
        assert!(SymbolKind::Struct.is_type());
        assert!(!SymbolKind::Struct.is_value());
        assert!(SymbolKind::Local.is_value());
        assert!(SymbolKind::TypeParam.is_type());
    }
}

use rustc_hash::FxHashMap;
use smol_str::SmolStr;

use arandu_lexer::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LocalSymbolId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SymbolId {
    pub file_id: crate::db::FileId,
    pub local_id: LocalSymbolId,
}

impl SymbolId {
    pub const DUMMY: Self = Self {
        file_id: 0,
        local_id: LocalSymbolId(u32::MAX),
    };

    pub fn new(file_id: crate::db::FileId, local_id: u32) -> Self {
        Self {
            file_id,
            local_id: LocalSymbolId(local_id),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScopeId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    Module,
    ImportValue,
    ImportType,
    Func,
    Const,
    TypeAlias,
    Struct,
    Enum,
    Interface,
    ExternFunc,
    Param,
    Local,
    Field,
    EnumVariant,
    TypeParam,
    NamespaceMember,
    AssociatedFunc,
}

impl SymbolKind {
    #[must_use]
    pub fn is_value(self) -> bool {
        matches!(
            self,
            SymbolKind::ImportValue
                | SymbolKind::Func
                | SymbolKind::Const
                | SymbolKind::ExternFunc
                | SymbolKind::Param
                | SymbolKind::Local
                | SymbolKind::EnumVariant
        )
    }

    #[must_use]
    pub fn is_type(self) -> bool {
        matches!(
            self,
            SymbolKind::ImportType
                | SymbolKind::TypeAlias
                | SymbolKind::Struct
                | SymbolKind::Enum
                | SymbolKind::Interface
                | SymbolKind::TypeParam
        )
    }
}

#[derive(Debug, Clone)]
pub struct Symbol {
    pub id: SymbolId,
    pub name: SmolStr,
    pub kind: SymbolKind,
    pub span: Span,
    pub scope: ScopeId,
}

#[derive(Debug, Clone)]
pub struct Scope {
    pub parent: Option<ScopeId>,
    symbols: Vec<SymbolId>,
}

#[derive(Debug, Clone)]
pub struct SymbolTable {
    pub file_id: crate::db::FileId,
    scopes: Vec<Scope>,
    symbols: Vec<Symbol>,
    pub imported_symbols: FxHashMap<SymbolId, Symbol>,
    pub module_members: FxHashMap<SmolStr, FxHashMap<SmolStr, SymbolId>>,
    pub associated_members: FxHashMap<SmolStr, FxHashMap<SmolStr, SymbolId>>,
    global_scope_id: ScopeId,
    pub builtin_alloc: Option<SymbolId>,
    pub builtin_free: Option<SymbolId>,
}

impl Default for SymbolTable {
    fn default() -> Self {
        Self::new(0)
    }
}

impl SymbolTable {
    #[must_use]
    pub fn new(file_id: crate::db::FileId) -> Self {
        Self {
            file_id,
            scopes: vec![Scope {
                parent: None,
                symbols: Vec::new(),
            }],
            symbols: Vec::new(),
            imported_symbols: FxHashMap::default(),
            module_members: FxHashMap::default(),
            associated_members: FxHashMap::default(),
            global_scope_id: ScopeId(0),
            builtin_alloc: None,
            builtin_free: None,
        }
    }

    #[tracing::instrument(level = "trace", target = "arandu_middle", skip(self, other))]
    pub fn merge_from(&mut self, other: SymbolTable) {
        let self_symbols_len = self.symbols.len() as u32;
        let self_scopes_len = self.scopes.len() as u32;

        let other_global = other.global_scope();
        let self_global = self.global_scope();

        let map_scope = |old_scope: ScopeId| -> ScopeId {
            if old_scope == other_global {
                self_global
            } else {
                ScopeId(old_scope.0 + self_scopes_len - 1)
            }
        };

        let map_symbol = |old_symbol: SymbolId| -> SymbolId {
            SymbolId {
                file_id: self.file_id,
                local_id: LocalSymbolId(old_symbol.local_id.0 + self_symbols_len),
            }
        };

        // 1. Merge other's global scope symbols into self's global scope symbols
        for old_symbol_id in &other.scopes[other_global.0 as usize].symbols {
            self.scopes[self_global.0 as usize]
                .symbols
                .push(map_symbol(*old_symbol_id));
        }

        for i in 0..other.scopes.len() {
            if i == other_global.0 as usize {
                continue;
            }
            let old_scope = &other.scopes[i];
            let new_parent = old_scope.parent.map(map_scope);
            let new_symbols = old_scope.symbols.iter().map(|id| map_symbol(*id)).collect();
            self.scopes.push(Scope {
                parent: new_parent,
                symbols: new_symbols,
            });
        }

        // 2. Merge other symbols
        for old_symbol in other.symbols {
            let new_id = map_symbol(old_symbol.id);
            let new_scope = map_scope(old_symbol.scope);
            self.symbols.push(Symbol {
                id: new_id,
                name: old_symbol.name,
                kind: old_symbol.kind,
                span: old_symbol.span,
                scope: new_scope,
            });
        }

        // 3. Merge other module members
        for (module, members) in other.module_members {
            let entry = self.module_members.entry(module).or_default();
            for (member, old_symbol_id) in members {
                entry.insert(member, map_symbol(old_symbol_id));
            }
        }

        // 4. Merge other associated members
        for (ty, members) in other.associated_members {
            let entry = self.associated_members.entry(ty).or_default();
            for (member, old_symbol_id) in members {
                entry.insert(member, map_symbol(old_symbol_id));
            }
        }
    }

    /// Extend `self` with the symbols from `other` that have index >= `base_count`.
    ///
    /// Used after typechecking a stdlib file whose resolver was given
    /// `self.clone()` as the starting symbol table and then added new symbols
    /// (e.g. TypeParams). The new symbols preserve the same `SymbolId`s they
    /// were assigned in `other`, so all `type_info` references remain valid.
    ///
    /// Note: the new symbols are only added to `self.symbols` for `get(id)`
    /// lookup. They are NOT added to any scope because they belong to specific
    /// function/enum scopes in the stdlib files and polluting the global scope
    /// would cause `N003RedefinedName` errors for user code with same-named
    /// type parameters.
    pub fn merge_from_extending(&mut self, other: &SymbolTable, base_count: usize) {
        for symbol in other.symbols.iter().skip(base_count) {
            // Sanity: the ID must match the current length.
            assert_eq!(
                symbol.id.local_id.0 as usize,
                self.symbols.len(),
                "symbol ID mismatch during extend: expected {} got {}",
                self.symbols.len(),
                symbol.id.local_id.0
            );
            // Only add to the symbols vector for get(id) access.
            // Do NOT add to any scope to avoid polluting name lookup.
            self.symbols.push(symbol.clone());
        }
    }

    pub fn setup_prelude_scope(&mut self) {
        if self.global_scope_id == ScopeId(0) {
            let new_global = self.new_scope(ScopeId(0));
            self.global_scope_id = new_global;
        }
    }

    #[must_use]
    pub fn global_scope(&self) -> ScopeId {
        self.global_scope_id
    }

    pub fn new_scope(&mut self, parent: ScopeId) -> ScopeId {
        let id = ScopeId(u32::try_from(self.scopes.len()).expect("scope count overflow"));
        self.scopes.push(Scope {
            parent: Some(parent),
            symbols: Vec::new(),
        });
        id
    }

    /// Defines a new symbol in the specified scope.
    ///
    /// # Errors
    ///
    /// Returns `Err(existing_symbol_id)` if a symbol with the same name already exists in the given scope.
    pub fn define(
        &mut self,
        scope: ScopeId,
        name: &str,
        kind: SymbolKind,
        span: Span,
    ) -> Result<SymbolId, SymbolId> {
        if let Some(existing) = self.find_in_scope(scope, name) {
            return Err(existing);
        }

        let id = SymbolId {
            file_id: self.file_id,
            local_id: LocalSymbolId(
                u32::try_from(self.symbols.len()).expect("symbol count overflow"),
            ),
        };
        self.symbols.push(Symbol {
            id,
            name: name.into(),
            kind,
            span,
            scope,
        });
        self.scope_mut(scope).symbols.push(id);
        Ok(id)
    }

    /// Inserts an imported symbol into the global scope.
    ///
    /// # Errors
    /// Returns `Err(existing)` if a different symbol with the same name already exists in the global scope.
    pub fn insert_imported(&mut self, symbol: Symbol) -> Result<(), SymbolId> {
        if let Some(existing) = self.find_in_scope(self.global_scope_id, &symbol.name) {
            if existing == symbol.id {
                return Ok(()); // exact same symbol already imported
            }
            return Err(existing);
        }

        let id = symbol.id;
        self.imported_symbols.insert(id, symbol);
        self.scope_mut(self.global_scope_id).symbols.push(id);
        Ok(())
    }

    /// Registers a symbol from another file so it can be looked up by ID,
    /// but DOES NOT put it into the global scope.
    pub fn register_imported_symbol(&mut self, symbol: Symbol) {
        self.imported_symbols.insert(symbol.id, symbol);
    }

    #[must_use]
    pub fn get(&self, id: SymbolId) -> &Symbol {
        self.try_get(id)
            .expect("symbol id not found in this SymbolTable")
    }

    /// Safe lookup: local ids out of range or missing imports return `None`.
    #[must_use]
    pub fn try_get(&self, id: SymbolId) -> Option<&Symbol> {
        if id.file_id == self.file_id {
            self.symbols.get(id.local_id.0 as usize)
        } else {
            self.imported_symbols.get(&id)
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Symbol> {
        self.symbols.iter().chain(self.imported_symbols.values())
    }

    #[must_use]
    pub fn lookup_value(&self, scope: ScopeId, name: &str) -> Option<SymbolId> {
        self.lookup_with(scope, name, SymbolKind::is_value)
    }

    #[must_use]
    pub fn lookup_type(&self, scope: ScopeId, name: &str) -> Option<SymbolId> {
        self.lookup_with(scope, name, SymbolKind::is_type)
    }

    #[must_use]
    pub fn lookup_module(&self, scope: ScopeId, name: &str) -> Option<SymbolId> {
        self.lookup_with(scope, name, |kind| kind == SymbolKind::Module)
    }

    #[must_use]
    pub fn lookup_any(&self, scope: ScopeId, name: &str) -> Option<SymbolId> {
        let mut current = Some(scope);
        while let Some(scope_id) = current {
            if let Some(symbol) = self.find_in_scope(scope_id, name) {
                return Some(symbol);
            }
            current = self.scope(scope_id).parent;
        }
        None
    }

    #[must_use]
    pub fn value_candidates(&self, scope: ScopeId) -> Vec<&Symbol> {
        self.candidates(scope, SymbolKind::is_value)
    }

    #[must_use]
    pub fn type_candidates(&self, scope: ScopeId) -> Vec<&Symbol> {
        self.candidates(scope, SymbolKind::is_type)
    }

    /// Defines a member in the specified module.
    ///
    /// # Errors
    ///
    /// Returns `Err(existing_symbol_id)` if a member with the same name already exists in the module.
    pub fn define_module_member(
        &mut self,
        module: &str,
        member: &str,
        span: Span,
    ) -> Result<SymbolId, SymbolId> {
        let id = self.define(
            self.global_scope(),
            &format!("{module}.{member}"),
            SymbolKind::NamespaceMember,
            span,
        )?;
        self.module_members
            .entry(module.into())
            .or_default()
            .insert(member.into(), id);
        Ok(id)
    }

    #[must_use]
    pub fn lookup_module_member(&self, module: &str, member: &str) -> Option<SymbolId> {
        self.module_members
            .get(module)
            .and_then(|m| m.get(member))
            .copied()
    }

    /// Defines an associated member (e.g. method) on a type.
    ///
    /// # Errors
    ///
    /// Returns `Err(existing_symbol_id)` if the member is already defined on the type.
    pub fn define_associated_member(
        &mut self,
        ty: &str,
        member: &str,
        span: Span,
    ) -> Result<SymbolId, SymbolId> {
        let base_ty = ty.split('.').next_back().unwrap_or(ty);
        let id = self.define(
            self.global_scope(),
            &format!("{base_ty}.{member}"),
            SymbolKind::AssociatedFunc,
            span,
        )?;
        self.associated_members
            .entry(base_ty.into())
            .or_default()
            .insert(member.into(), id);
        Ok(id)
    }

    #[must_use]
    pub fn lookup_associated_member(&self, ty: &str, member: &str) -> Option<SymbolId> {
        let base_ty = ty.split('.').next_back().unwrap_or(ty);
        self.associated_members
            .get(base_ty)
            .and_then(|m| m.get(member))
            .copied()
    }

    fn lookup_with(
        &self,
        scope: ScopeId,
        name: &str,
        pred: fn(SymbolKind) -> bool,
    ) -> Option<SymbolId> {
        let mut current = Some(scope);
        while let Some(scope_id) = current {
            for symbol_id in self.scope(scope_id).symbols.iter().rev() {
                let symbol = self.get(*symbol_id);
                if symbol.name == name && pred(symbol.kind) {
                    return Some(*symbol_id);
                }
            }
            current = self.scope(scope_id).parent;
        }
        None
    }

    fn candidates(&self, scope: ScopeId, pred: fn(SymbolKind) -> bool) -> Vec<&Symbol> {
        let mut out = Vec::new();
        let mut current = Some(scope);
        while let Some(scope_id) = current {
            for symbol_id in &self.scope(scope_id).symbols {
                let symbol = self.get(*symbol_id);
                if pred(symbol.kind) {
                    out.push(symbol);
                }
            }
            current = self.scope(scope_id).parent;
        }
        out
    }

    #[must_use]
    pub fn find_in_scope(&self, scope: ScopeId, name: &str) -> Option<SymbolId> {
        self.scope(scope)
            .symbols
            .iter()
            .copied()
            .find(|symbol_id| self.get(*symbol_id).name == name)
    }

    fn scope(&self, id: ScopeId) -> &Scope {
        &self.scopes[id.0 as usize]
    }

    fn scope_mut(&mut self, id: ScopeId) -> &mut Scope {
        &mut self.scopes[id.0 as usize]
    }
}
