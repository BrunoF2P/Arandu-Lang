use fxhash::FxHashMap;

use arandu_lexer::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SymbolId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScopeId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    pub name: String,
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
    scopes: Vec<Scope>,
    symbols: Vec<Symbol>,
    module_members: FxHashMap<String, FxHashMap<String, SymbolId>>,
    pub(crate) associated_members: FxHashMap<String, FxHashMap<String, SymbolId>>,
}

impl Default for SymbolTable {
    fn default() -> Self {
        Self::new()
    }
}

impl SymbolTable {
    #[must_use]
    pub fn new() -> Self {
        Self {
            scopes: vec![Scope {
                parent: None,
                symbols: Vec::new(),
            }],
            symbols: Vec::new(),
            module_members: FxHashMap::default(),
            associated_members: FxHashMap::default(),
        }
    }

    pub fn merge_from(&mut self, other: SymbolTable) {
        let self_symbols_len = self.symbols.len() as u32;
        let self_scopes_len = self.scopes.len() as u32;

        let map_scope = |old_scope: ScopeId| -> ScopeId {
            if old_scope.0 == 0 {
                ScopeId(0)
            } else {
                ScopeId(old_scope.0 + self_scopes_len - 1)
            }
        };

        let map_symbol = |old_symbol: SymbolId| -> SymbolId {
            SymbolId(old_symbol.0 + self_symbols_len)
        };

        // 1. Merge other scopes (except global scope at index 0)
        for old_symbol_id in &other.scopes[0].symbols {
            self.scopes[0].symbols.push(map_symbol(*old_symbol_id));
        }

        for i in 1..other.scopes.len() {
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


    #[must_use]
    pub fn global_scope(&self) -> ScopeId {
        ScopeId(0)
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

        let id = SymbolId(u32::try_from(self.symbols.len()).expect("symbol count overflow"));
        self.symbols.push(Symbol {
            id,
            name: name.to_string(),
            kind,
            span,
            scope,
        });
        self.scope_mut(scope).symbols.push(id);
        Ok(id)
    }

    #[must_use]
    pub fn get(&self, id: SymbolId) -> &Symbol {
        &self.symbols[id.0 as usize]
    }

    pub fn iter(&self) -> impl Iterator<Item = &Symbol> {
        self.symbols.iter()
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
            .entry(module.to_string())
            .or_default()
            .insert(member.to_string(), id);
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
        let id = self.define(
            self.global_scope(),
            &format!("{ty}.{member}"),
            SymbolKind::AssociatedFunc,
            span,
        )?;
        self.associated_members
            .entry(ty.to_string())
            .or_default()
            .insert(member.to_string(), id);
        Ok(id)
    }

    #[must_use]
    pub fn lookup_associated_member(&self, ty: &str, member: &str) -> Option<SymbolId> {
        self.associated_members
            .get(ty)
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

    fn find_in_scope(&self, scope: ScopeId, name: &str) -> Option<SymbolId> {
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
