use arandu_parser::{
    Attribute, ConstDecl, EnumDecl, EnumPayload, EnumVariant, FieldDecl, FuncDecl, FuncName,
    FuncSignature, GenericParam, InterfaceDecl, Param, StructDecl, TopLevelDecl, TypeAliasDecl,
    TypeName, WhereItem,
};

use crate::{ScopeId, SymbolKind};

use super::Resolver;

impl<'a> Resolver<'a> {
    pub(crate) fn resolve_top_level(&mut self, scope: ScopeId, decl: &TopLevelDecl) {
        match decl {
            TopLevelDecl::Const(decl) => self.resolve_const(scope, decl),
            TopLevelDecl::TypeAlias(decl) => self.resolve_type_alias(scope, decl),
            TopLevelDecl::Func(decl) => self.resolve_func(scope, decl),
            TopLevelDecl::Struct(decl) => self.resolve_struct(scope, decl),
            TopLevelDecl::Enum(decl) => self.resolve_enum(scope, decl),
            TopLevelDecl::Interface(decl) => self.resolve_interface(scope, decl),
            TopLevelDecl::Extern(decl) => {
                self.resolve_attrs(scope, &decl.attrs);
                for member in &decl.members {
                    self.resolve_signature(scope, member);
                }
            }
            TopLevelDecl::Error(_) => {}
        }
    }

    pub(crate) fn resolve_const(&mut self, scope: ScopeId, decl: &ConstDecl) {
        self.resolve_attrs(scope, &decl.attrs);
        if let Some(ty) = &decl.ty {
            self.resolve_type_expr(scope, *ty);
        }
        self.resolve_expr(scope, decl.value);
    }

    pub(crate) fn resolve_type_alias(&mut self, scope: ScopeId, decl: &TypeAliasDecl) {
        self.resolve_attrs(scope, &decl.attrs);
        let alias_scope = self.symbols.new_scope(scope);
        self.define_generics(alias_scope, &decl.generic_params);
        self.resolve_type_expr(alias_scope, decl.ty);
    }

    pub(crate) fn resolve_func(&mut self, scope: ScopeId, decl: &FuncDecl) {
        self.resolve_attrs(scope, &decl.attrs);
        let func_scope = self.symbols.new_scope(scope);
        self.define_generics(func_scope, &decl.generic_params);
        if let FuncName::Method { receiver, .. } = &decl.name {
            self.resolve_type_name(func_scope, receiver);
        }
        for where_item in &decl.where_clause {
            self.resolve_where_item(func_scope, where_item);
        }
        for param in &decl.params {
            self.resolve_param(func_scope, param);
        }
        if let Some(result) = &decl.result {
            self.resolve_result_type(func_scope, result);
        }
        self.resolve_block_in_scope(func_scope, self.pool, &decl.body);
    }

    pub(crate) fn resolve_struct(&mut self, scope: ScopeId, decl: &StructDecl) {
        self.resolve_attrs(scope, &decl.attrs);
        let struct_scope = self.symbols.new_scope(scope);
        self.define_generics(struct_scope, &decl.generic_params);
        for where_item in &decl.where_clause {
            self.resolve_where_item(struct_scope, where_item);
        }
        let mut seen_fields = smallvec::SmallVec::<[(&str, arandu_lexer::Span); 8]>::new();
        for field in &decl.fields {
            if let Some((_, prev_span)) = seen_fields.iter().find(|(name, _)| *name == field.name) {
                self.diagnostics.push(
                    crate::Diagnostic::error(
                        crate::DiagCode::T030DuplicateFieldDecl,
                        format!(
                            "field '{}' is already declared in struct '{}'",
                            field.name, decl.name
                        ),
                        field.span,
                    )
                    .with_label(*prev_span, "first declaration here")
                    .with_label(field.span, "duplicate field"),
                );
            } else {
                seen_fields.push((&field.name, field.span));
                self.resolve_field(struct_scope, field);
            }
        }
    }

    pub(crate) fn resolve_enum(&mut self, scope: ScopeId, decl: &EnumDecl) {
        self.resolve_attrs(scope, &decl.attrs);
        let enum_scope = self.symbols.new_scope(scope);
        self.define_generics(enum_scope, &decl.generic_params);
        for where_item in &decl.where_clause {
            self.resolve_where_item(enum_scope, where_item);
        }
        for variant in &decl.variants {
            self.resolve_enum_variant(enum_scope, variant);
        }
    }

    pub(crate) fn resolve_interface(&mut self, scope: ScopeId, decl: &InterfaceDecl) {
        self.resolve_attrs(scope, &decl.attrs);
        let interface_scope = self.symbols.new_scope(scope);
        self.define_generics(interface_scope, &decl.generic_params);
        for where_item in &decl.where_clause {
            self.resolve_where_item(interface_scope, where_item);
        }
        for member in &decl.members {
            self.resolve_signature(interface_scope, member);
        }
    }

    pub(crate) fn resolve_signature(&mut self, scope: ScopeId, signature: &FuncSignature) {
        self.resolve_attrs(scope, &signature.attrs);
        let sig_scope = self.symbols.new_scope(scope);
        self.define_generics(sig_scope, &signature.generic_params);
        for where_item in &signature.where_clause {
            self.resolve_where_item(sig_scope, where_item);
        }
        for param in &signature.params {
            self.resolve_param(sig_scope, param);
        }
        if let Some(result) = &signature.result {
            self.resolve_result_type(sig_scope, result);
        }
    }

    pub(crate) fn define_generics(&mut self, scope: ScopeId, generics: &[GenericParam]) {
        for generic in generics {
            self.define(scope, &generic.name, SymbolKind::TypeParam, generic.span);
            for constraint in &generic.constraints {
                self.resolve_type_name(scope, constraint);
            }
        }
    }

    pub(crate) fn resolve_where_item(&mut self, scope: ScopeId, item: &WhereItem) {
        self.resolve_type_name(
            scope,
            &TypeName {
                span: item.span,
                path: vec![item.name.clone()].into(),
            },
        );
        for constraint in &item.constraints {
            self.resolve_type_name(scope, constraint);
        }
    }

    pub(crate) fn resolve_param(&mut self, scope: ScopeId, param: &Param) {
        self.resolve_attrs(scope, &param.attrs);
        self.resolve_type_expr(scope, param.ty);
        self.define(scope, &param.name, SymbolKind::Param, param.span);
        let is_mut = param.ownership == Some(arandu_parser::Ownership::Mut);
        if let Some(symbol_id) = self
            .resolved
            .definitions
            .get(&param.span.into())
            .copied()
            .filter(|_| is_mut)
        {
            self.resolved.mutable_symbols.insert(symbol_id);
        }
    }

    pub(crate) fn resolve_field(&mut self, scope: ScopeId, field: &FieldDecl) {
        self.resolve_attrs(scope, &field.attrs);
        self.resolve_type_expr(scope, field.ty);
        self.define(scope, &field.name, SymbolKind::Field, field.span);
    }

    pub(crate) fn resolve_enum_variant(&mut self, scope: ScopeId, variant: &EnumVariant) {
        self.resolve_attrs(scope, &variant.attrs);
        self.define(scope, &variant.name, SymbolKind::EnumVariant, variant.span);
        match &variant.payload {
            Some(EnumPayload::Tuple { types, .. }) => {
                for ty in self.pool.type_expr_list(*types) {
                    self.resolve_type_expr(scope, *ty);
                }
            }
            Some(EnumPayload::Struct { fields, .. }) => {
                let variant_scope = self.symbols.new_scope(scope);
                for field in fields {
                    self.resolve_field(variant_scope, field);
                }
            }
            None => {}
        }
    }

    pub(crate) fn resolve_attrs(&mut self, scope: ScopeId, attrs: &[Attribute]) {
        for attr in attrs {
            for arg in &attr.args {
                self.resolve_expr(scope, *arg);
            }
        }
    }
}
