use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::SymbolId;
use arandu_parser::ast_pool::ExprId;

use super::types::{self, ArType, TypeId, TypeInterner};

#[derive(Debug, Clone, PartialEq)]
pub enum EnumPayloadShape {
    Unit,
    /// Payload element types as interned ids (no owned `ArType` trees).
    Tuple(Vec<TypeId>),
}

/// Shared metadata maps use `Arc` so `merge_from` (item body typeck fold) is O(1)
/// per entry instead of deep-cloning every field map / generic param list.
#[derive(Debug, Default, Clone)]
pub struct TypeInfo {
    pub type_interner: TypeInterner,
    pub expr_types: Vec<Option<TypeId>>,
    pub decl_types: FxHashMap<SymbolId, TypeId>,
    /// Struct field name → interned field type.
    pub struct_fields: FxHashMap<SymbolId, Arc<FxHashMap<String, TypeId>>>,
    pub struct_field_symbols: FxHashMap<SymbolId, Arc<FxHashMap<String, SymbolId>>>,
    pub struct_field_indices: FxHashMap<SymbolId, Arc<FxHashMap<String, usize>>>,
    pub enum_variants: FxHashMap<SymbolId, (SymbolId, EnumPayloadShape)>,
    /// Pre-computed discriminant tag for each enum variant symbol.
    pub enum_variant_tags: FxHashMap<SymbolId, usize>,
    /// Ordered type-parameter symbols for generic decls (func, struct, …).
    pub generic_params: FxHashMap<SymbolId, Arc<Vec<SymbolId>>>,
    /// Type-parameter symbol → interface symbols required (`T: Display`).
    pub param_constraints: FxHashMap<SymbolId, Arc<Vec<SymbolId>>>,
    /// Interface symbol → method signatures (nominal, Go-style structural check).
    pub(crate) interfaces: FxHashMap<SymbolId, types::InterfaceInfo>,
}

impl TypeInfo {
    #[must_use]
    pub fn new() -> Self {
        Self::with_interner(TypeInterner::new())
    }

    #[must_use]
    pub fn with_interner(type_interner: TypeInterner) -> Self {
        Self {
            type_interner,
            expr_types: Vec::new(),
            decl_types: FxHashMap::default(),
            struct_fields: FxHashMap::default(),
            struct_field_symbols: FxHashMap::default(),
            struct_field_indices: FxHashMap::default(),
            enum_variants: FxHashMap::default(),
            enum_variant_tags: FxHashMap::default(),
            generic_params: FxHashMap::default(),
            param_constraints: FxHashMap::default(),
            interfaces: FxHashMap::default(),
        }
    }

    pub fn record_enum_variant_tag(&mut self, variant: SymbolId, tag: usize) {
        self.enum_variant_tags.insert(variant, tag);
    }
}

pub(crate) fn translate_type(ty: &ArType, from: &TypeInterner, to: &mut TypeInterner) -> ArType {
    match ty {
        ArType::Primitive(p) => ArType::Primitive(*p),
        ArType::Named(id, args) => {
            let new_args = args
                .iter()
                .map(|&arg_id| {
                    let resolved = from.resolve(arg_id);
                    let translated = translate_type(&resolved, from, to);
                    to.intern(translated)
                })
                .collect();
            ArType::Named(*id, new_args)
        }
        ArType::Func(params, ret) => {
            let new_params = params
                .iter()
                .map(|&param_id| {
                    let resolved = from.resolve(param_id);
                    let translated = translate_type(&resolved, from, to);
                    to.intern(translated)
                })
                .collect();
            let resolved_ret = from.resolve(*ret);
            let translated_ret = translate_type(&resolved_ret, from, to);
            let new_ret = to.intern(translated_ret);
            ArType::Func(new_params, new_ret)
        }
        ArType::Nullable(inner) => {
            let resolved = from.resolve(*inner);
            let translated = translate_type(&resolved, from, to);
            let new_inner = to.intern(translated);
            ArType::Nullable(new_inner)
        }
        ArType::Slice(inner) => {
            let resolved = from.resolve(*inner);
            let translated = translate_type(&resolved, from, to);
            let new_inner = to.intern(translated);
            ArType::Slice(new_inner)
        }
        ArType::Array(n, inner) => {
            let resolved = from.resolve(*inner);
            let translated = translate_type(&resolved, from, to);
            let new_inner = to.intern(translated);
            ArType::Array(*n, new_inner)
        }
        ArType::Ptr(inner) => {
            let resolved = from.resolve(*inner);
            let translated = translate_type(&resolved, from, to);
            let new_inner = to.intern(translated);
            ArType::Ptr(new_inner)
        }
        ArType::Ref(inner) => {
            let resolved = from.resolve(*inner);
            let translated = translate_type(&resolved, from, to);
            let new_inner = to.intern(translated);
            ArType::Ref(new_inner)
        }
        ArType::RefMut(inner) => {
            let resolved = from.resolve(*inner);
            let translated = translate_type(&resolved, from, to);
            let new_inner = to.intern(translated);
            ArType::RefMut(new_inner)
        }
        ArType::Tuple(items) => {
            let new_items = items
                .iter()
                .map(|&item_id| {
                    let resolved = from.resolve(item_id);
                    let translated = translate_type(&resolved, from, to);
                    to.intern(translated)
                })
                .collect();
            ArType::Tuple(new_items)
        }
        ArType::Result(ok, err) => {
            let resolved_ok = from.resolve(*ok);
            let translated_ok = translate_type(&resolved_ok, from, to);
            let new_ok = to.intern(translated_ok);

            let resolved_err = from.resolve(*err);
            let translated_err = translate_type(&resolved_err, from, to);
            let new_err = to.intern(translated_err);

            ArType::Result(new_ok, new_err)
        }
        ArType::Option(inner) => {
            let resolved = from.resolve(*inner);
            let translated = translate_type(&resolved, from, to);
            let new_inner = to.intern(translated);
            ArType::Option(new_inner)
        }
        ArType::Coroutine(inner) => {
            let resolved = from.resolve(*inner);
            let translated = translate_type(&resolved, from, to);
            let new_inner = to.intern(translated);
            ArType::Coroutine(new_inner)
        }
        ArType::Range(inner) => {
            let resolved = from.resolve(*inner);
            let translated = translate_type(&resolved, from, to);
            let new_inner = to.intern(translated);
            ArType::Range(new_inner)
        }
        ArType::Err => ArType::Err,
        ArType::Void => ArType::Void,
        ArType::Error => ArType::Error,
        ArType::IntLiteral => ArType::IntLiteral,
        ArType::FloatLiteral => ArType::FloatLiteral,
    }
}

impl TypeInfo {
    pub fn merge_from(&mut self, other: &TypeInfo) {
        for (&symbol, &other_type_id) in &other.decl_types {
            let other_type = other.type_interner.resolve(other_type_id);
            let translated =
                translate_type(&other_type, &other.type_interner, &mut self.type_interner);
            let id = self.type_interner.intern(translated);
            self.record_decl_type(symbol, id);
        }
        for (symbol, fields) in &other.struct_fields {
            let mut translated_fields = FxHashMap::default();
            for (name, &tid) in fields.iter() {
                let ty = other.type_interner.resolve(tid);
                let translated = translate_type(&ty, &other.type_interner, &mut self.type_interner);
                translated_fields.insert(name.clone(), self.type_interner.intern(translated));
            }
            self.struct_fields
                .insert(*symbol, Arc::new(translated_fields));
        }
        for (symbol, field_symbols) in &other.struct_field_symbols {
            self.struct_field_symbols
                .insert(*symbol, Arc::clone(field_symbols));
        }
        for (symbol, field_indices) in &other.struct_field_indices {
            self.struct_field_indices
                .insert(*symbol, Arc::clone(field_indices));
        }
        for (symbol, (enum_id, shape)) in &other.enum_variants {
            let translated_shape = match shape {
                EnumPayloadShape::Unit => EnumPayloadShape::Unit,
                EnumPayloadShape::Tuple(tids) => {
                    let mut new_tids = Vec::with_capacity(tids.len());
                    for &tid in tids {
                        let ty = other.type_interner.resolve(tid);
                        let translated =
                            translate_type(&ty, &other.type_interner, &mut self.type_interner);
                        new_tids.push(self.type_interner.intern(translated));
                    }
                    EnumPayloadShape::Tuple(new_tids)
                }
            };
            self.enum_variants
                .insert(*symbol, (*enum_id, translated_shape));
        }
        for (&symbol, &tag) in &other.enum_variant_tags {
            self.enum_variant_tags.insert(symbol, tag);
        }
        for (symbol, params) in &other.generic_params {
            self.generic_params.insert(*symbol, Arc::clone(params));
        }
        for (symbol, constraints) in &other.param_constraints {
            self.param_constraints
                .insert(*symbol, Arc::clone(constraints));
        }
        for (symbol, interface_info) in &other.interfaces {
            let mut translated_methods = Vec::new();
            for (name, tid) in &interface_info.methods {
                let ty = other.type_interner.resolve(*tid);
                let translated = translate_type(&ty, &other.type_interner, &mut self.type_interner);
                translated_methods.push((name.clone(), self.type_interner.intern(translated)));
            }
            self.interfaces.insert(
                *symbol,
                types::InterfaceInfo {
                    methods: translated_methods,
                },
            );
        }
        // Expr types (body typeck shards): re-intern TypeIds into `self`.
        if other.expr_types.len() > self.expr_types.len() {
            self.expr_types.resize(other.expr_types.len(), None);
        }
        for (idx, slot) in other.expr_types.iter().enumerate() {
            let Some(other_id) = slot else {
                continue;
            };
            if self.expr_types[idx].is_some() {
                continue; // keep first writer (signatures / earlier merge)
            }
            let other_ty = other.type_interner.resolve(*other_id);
            let translated =
                translate_type(&other_ty, &other.type_interner, &mut self.type_interner);
            let id = self.type_interner.intern(translated);
            self.expr_types[idx] = Some(id);
        }
    }

    pub fn record_expr_type(&mut self, expr: ExprId, id: TypeId) {
        let idx = expr.as_usize();
        if self.expr_types.len() <= idx {
            self.expr_types.resize(idx + 1, None);
        }
        self.expr_types[idx] = Some(id);
    }

    pub fn record_decl_type(&mut self, symbol: SymbolId, id: TypeId) {
        self.decl_types.insert(symbol, id);
    }

    #[must_use]
    pub fn expr_type(&self, expr: ExprId) -> Option<ArType> {
        self.expr_types
            .get(expr.as_usize())
            .and_then(|id| id.as_ref().map(|id| self.type_interner.resolve(*id)))
    }

    #[must_use]
    pub fn expr_type_id(&self, expr: ExprId) -> Option<TypeId> {
        self.expr_types.get(expr.as_usize()).copied().flatten()
    }

    #[must_use]
    pub fn decl_type(&self, symbol: SymbolId) -> Option<ArType> {
        self.decl_types
            .get(&symbol)
            .map(|id| self.type_interner.resolve(*id))
    }

    #[must_use]
    pub fn decl_type_id(&self, symbol: SymbolId) -> Option<TypeId> {
        self.decl_types.get(&symbol).copied()
    }

    #[must_use]
    pub fn resolve_type_id(&self, id: TypeId) -> ArType {
        self.type_interner.resolve(id)
    }
}

impl arandu_middle::layout::StructLayoutProvider for TypeInfo {
    fn get_struct_fields(
        &self,
        struct_id: SymbolId,
    ) -> Option<&rustc_hash::FxHashMap<String, TypeId>> {
        self.struct_fields.get(&struct_id).map(|a| a.as_ref())
    }

    fn get_struct_field_indices(
        &self,
        struct_id: SymbolId,
    ) -> Option<&rustc_hash::FxHashMap<String, usize>> {
        self.struct_field_indices
            .get(&struct_id)
            .map(|a| a.as_ref())
    }

    fn get_generic_params(&self, struct_id: SymbolId) -> Option<&[SymbolId]> {
        self.generic_params.get(&struct_id).map(|v| v.as_slice())
    }

    fn get_enum_variants(
        &self,
        enum_id: SymbolId,
    ) -> Option<Vec<arandu_middle::layout::EnumPayloadShape>> {
        let mut variant_list: Vec<(usize, &EnumPayloadShape)> = self
            .enum_variants
            .iter()
            .filter(|(_var_symbol, (parent_enum_id, _shape))| *parent_enum_id == enum_id)
            .map(|(var_symbol, (_parent, shape))| {
                let tag = self.enum_variant_tags.get(var_symbol).copied().unwrap_or(0);
                (tag, shape)
            })
            .collect();

        if variant_list.is_empty() {
            return None;
        }

        variant_list.sort_by_key(|(tag, _shape)| *tag);
        variant_list.dedup_by_key(|(tag, _shape)| *tag);

        let mut mapped_variants = Vec::new();
        for (_tag, shape) in variant_list {
            let payload_ty = match shape {
                EnumPayloadShape::Unit => None,
                EnumPayloadShape::Tuple(tids) => {
                    if tids.is_empty() {
                        None
                    } else if tids.len() == 1 {
                        Some(tids[0])
                    } else {
                        // Multi-payload: layout uses the interned Tuple type if present.
                        self.type_interner.lookup(&ArType::Tuple(tids.clone()))
                    }
                }
            };
            mapped_variants.push(arandu_middle::layout::EnumPayloadShape { payload_ty });
        }

        Some(mapped_variants)
    }
}
