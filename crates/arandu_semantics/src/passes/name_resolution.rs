use arandu_lexer::Span;
use arandu_parser::{
    Attribute, BinaryOp, BindingItem, Block, CatchHandler, Condition, ConstDecl, DeferBody,
    EnumDecl, EnumPayload, EnumVariant, Expr, FieldDecl, FieldInit, FieldPattern, ForBinding,
    ForClause, FuncDecl, FuncName, FuncSignature, GenericParam, ImportDecl, InterfaceDecl,
    LambdaBody, LambdaParam, MatchArm, MatchArmBody, Param, Pattern, Place, PlaceSuffix, Program,
    ResultType, SimpleStmt, Stmt, StructDecl, TopLevelDecl, TypeAliasDecl, TypeExpr, TypeName,
    UnaryOp, WhereItem,
};

use crate::{
    DiagCode, Diagnostic, DocCommentMap, NodeKey, ResolutionResult, ResolvedNames, ScopeId,
    SymbolKind, SymbolTable,
};

pub fn resolve(program: &Program) -> ResolutionResult {
    Resolver::new().resolve_program(program)
}

struct Resolver {
    symbols: SymbolTable,
    resolved: ResolvedNames,
    docs: DocCommentMap,
    diagnostics: Vec<Diagnostic>,
}

impl Resolver {
    fn new() -> Self {
        let mut resolver = Self {
            symbols: SymbolTable::new(),
            resolved: ResolvedNames::default(),
            docs: DocCommentMap::default(),
            diagnostics: Vec::new(),
        };
        resolver.define_prelude();
        resolver
    }

    fn resolve_program(mut self, program: &Program) -> ResolutionResult {
        for doc in &program.docs {
            self.docs
                .entry(NodeKey::from(doc.target_span))
                .or_default()
                .push(doc.text.clone());
        }

        let global = self.symbols.global_scope();
        if let Some(module) = &program.module
            && let Some(root) = module.path.first()
        {
            self.define(global, root, SymbolKind::Module, module.span);
        }

        for import in &program.imports {
            self.collect_import(global, import);
        }

        for decl in &program.decls {
            self.collect_top_level(global, decl);
        }

        for decl in &program.decls {
            self.resolve_top_level(global, decl);
        }

        ResolutionResult {
            symbols: self.symbols,
            resolved: self.resolved,
            docs: self.docs,
            diagnostics: self.diagnostics,
        }
    }

    fn define_prelude(&mut self) {
        let span = Span::new(0, 0, 0, 0, 0, 0);
        for (module, members) in [
            ("io", ["println", "create", "remove"].as_slice()),
            ("err", ["new"].as_slice()),
        ] {
            for member in members {
                let _ = self.symbols.define_module_member(module, member, span);
            }
        }
    }

    fn collect_import(&mut self, scope: ScopeId, import: &ImportDecl) {
        match import {
            ImportDecl::Module { span, path } => {
                if let Some(root) = path.first() {
                    self.define(scope, root, SymbolKind::Module, *span);
                } else {
                    self.diagnostics.push(Diagnostic::error(
                        DiagCode::N006UnresolvedImport,
                        "empty import path",
                        *span,
                    ));
                }
            }
            ImportDecl::Named { items, .. } => {
                for item in items {
                    let name = item.alias.as_ref().unwrap_or(&item.name);
                    let kind = if is_type_case(name) {
                        SymbolKind::ImportType
                    } else {
                        SymbolKind::ImportValue
                    };
                    self.define(scope, name, kind, item.span);
                }
            }
        }
    }

    fn collect_top_level(&mut self, scope: ScopeId, decl: &TopLevelDecl) {
        match decl {
            TopLevelDecl::Const(decl) => {
                self.define(scope, &decl.name, SymbolKind::Const, decl.span);
            }
            TopLevelDecl::TypeAlias(decl) => {
                self.define(scope, &decl.name, SymbolKind::TypeAlias, decl.span);
            }
            TopLevelDecl::Func(decl) => match &decl.name {
                FuncName::Free { span, name } => {
                    self.define(scope, name, SymbolKind::Func, *span);
                }
                FuncName::Method {
                    span,
                    receiver,
                    name,
                } => {
                    let receiver = receiver.path.join(".");
                    match self
                        .symbols
                        .define_associated_member(&receiver, name, *span)
                    {
                        Ok(symbol) => self.resolved.define(*span, symbol),
                        Err(previous) => {
                            let previous_symbol = self.symbols.get(previous);
                            self.diagnostics.push(
                                Diagnostic::error(
                                    DiagCode::N003RedefinedName,
                                    format!(
                                        "associated function '{}.{name}' is already declared",
                                        receiver
                                    ),
                                    *span,
                                )
                                .with_label(previous_symbol.span, "previous declaration is here"),
                            );
                        }
                    }
                }
            },
            TopLevelDecl::Struct(decl) => {
                self.define(scope, &decl.name, SymbolKind::Struct, decl.span);
            }
            TopLevelDecl::Enum(decl) => {
                self.define(scope, &decl.name, SymbolKind::Enum, decl.span);
                for variant in &decl.variants {
                    if let Ok(symbol) = self.symbols.define_associated_member(&decl.name, &variant.name, variant.span) {
                        self.resolved.define(variant.span, symbol);
                    }
                }
            }
            TopLevelDecl::Interface(decl) => {
                self.define(scope, &decl.name, SymbolKind::Interface, decl.span);
            }
            TopLevelDecl::Extern(decl) => {
                for member in &decl.members {
                    self.define(scope, &member.name, SymbolKind::ExternFunc, member.span);
                }
            }
        }
    }

    fn resolve_top_level(&mut self, scope: ScopeId, decl: &TopLevelDecl) {
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
        }
    }

    fn resolve_const(&mut self, scope: ScopeId, decl: &ConstDecl) {
        self.resolve_attrs(scope, &decl.attrs);
        if let Some(ty) = &decl.ty {
            self.resolve_type_expr(scope, ty);
        }
        self.resolve_expr(scope, &decl.value);
    }

    fn resolve_type_alias(&mut self, scope: ScopeId, decl: &TypeAliasDecl) {
        self.resolve_attrs(scope, &decl.attrs);
        let alias_scope = self.symbols.new_scope(scope);
        self.define_generics(alias_scope, &decl.generic_params);
        self.resolve_type_expr(alias_scope, &decl.ty);
    }

    fn resolve_func(&mut self, scope: ScopeId, decl: &FuncDecl) {
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
        self.resolve_block_in_scope(func_scope, &decl.body);
    }

    fn resolve_struct(&mut self, scope: ScopeId, decl: &StructDecl) {
        self.resolve_attrs(scope, &decl.attrs);
        let struct_scope = self.symbols.new_scope(scope);
        self.define_generics(struct_scope, &decl.generic_params);
        for where_item in &decl.where_clause {
            self.resolve_where_item(struct_scope, where_item);
        }
        for field in &decl.fields {
            self.resolve_field(struct_scope, field);
        }
    }

    fn resolve_enum(&mut self, scope: ScopeId, decl: &EnumDecl) {
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

    fn resolve_interface(&mut self, scope: ScopeId, decl: &InterfaceDecl) {
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

    fn resolve_signature(&mut self, scope: ScopeId, signature: &FuncSignature) {
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

    fn define_generics(&mut self, scope: ScopeId, generics: &[GenericParam]) {
        for generic in generics {
            self.define(scope, &generic.name, SymbolKind::TypeParam, generic.span);
            for constraint in &generic.constraints {
                self.resolve_type_name(scope, constraint);
            }
        }
    }

    fn resolve_where_item(&mut self, scope: ScopeId, item: &WhereItem) {
        self.resolve_type_name(
            scope,
            &TypeName {
                span: item.span,
                path: vec![item.name.clone()],
            },
        );
        for constraint in &item.constraints {
            self.resolve_type_name(scope, constraint);
        }
    }

    fn resolve_param(&mut self, scope: ScopeId, param: &Param) {
        self.resolve_attrs(scope, &param.attrs);
        self.resolve_type_expr(scope, &param.ty);
        self.define(scope, &param.name, SymbolKind::Param, param.span);
    }

    fn resolve_field(&mut self, scope: ScopeId, field: &FieldDecl) {
        self.resolve_attrs(scope, &field.attrs);
        self.resolve_type_expr(scope, &field.ty);
        self.define(scope, &field.name, SymbolKind::Field, field.span);
    }

    fn resolve_enum_variant(&mut self, scope: ScopeId, variant: &EnumVariant) {
        self.resolve_attrs(scope, &variant.attrs);
        self.define(scope, &variant.name, SymbolKind::EnumVariant, variant.span);
        match &variant.payload {
            Some(EnumPayload::Tuple { types, .. }) => {
                for ty in types {
                    self.resolve_type_expr(scope, ty);
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

    fn resolve_attrs(&mut self, scope: ScopeId, attrs: &[Attribute]) {
        for attr in attrs {
            for arg in &attr.args {
                self.resolve_expr(scope, arg);
            }
        }
    }

    fn resolve_block_child(&mut self, parent: ScopeId, block: &Block) {
        let scope = self.symbols.new_scope(parent);
        self.resolve_block_in_scope(scope, block);
    }

    fn resolve_block_in_scope(&mut self, scope: ScopeId, block: &Block) {
        for stmt in &block.statements {
            self.resolve_stmt(scope, stmt);
        }
    }

    fn resolve_stmt(&mut self, scope: ScopeId, stmt: &Stmt) {
        match stmt {
            Stmt::VarDecl {
                bindings, value, ..
            } => self.resolve_var_decl(scope, bindings, value),
            Stmt::Set { places, value, .. } => {
                for place in places {
                    self.resolve_place(scope, place);
                }
                self.resolve_expr(scope, value);
            }
            Stmt::Return { values, .. } => {
                for value in values {
                    self.resolve_expr(scope, value);
                }
            }
            Stmt::Break { .. } | Stmt::Continue { .. } => {}
            Stmt::Free { expr, .. } => self.resolve_expr(scope, expr),
            Stmt::Expr { expr, .. } => self.resolve_expr(scope, expr),
            Stmt::If {
                condition,
                then_block,
                else_block,
                ..
            } => {
                self.resolve_condition(scope, condition);
                self.resolve_block_child(scope, then_block);
                if let Some(block) = else_block {
                    self.resolve_block_child(scope, block);
                }
            }
            Stmt::For { clause, body, .. } => self.resolve_for(scope, clause, body),
            Stmt::While {
                condition, body, ..
            } => {
                self.resolve_condition(scope, condition);
                self.resolve_block_child(scope, body);
            }
            Stmt::Match { expr, .. } => self.resolve_expr(scope, expr),
            Stmt::Defer { body, .. } | Stmt::ErrDefer { body, .. } => {
                self.resolve_defer_body(scope, body);
            }
            Stmt::Unsafe { block, .. } => self.resolve_block_child(scope, block),
        }
    }

    fn resolve_var_decl(&mut self, scope: ScopeId, bindings: &[BindingItem], value: &Expr) {
        self.resolve_expr(scope, value);
        for binding in bindings {
            if let Some(ty) = &binding.ty {
                self.resolve_type_expr(scope, ty);
            }
        }
        for binding in bindings {
            self.define(scope, &binding.name, SymbolKind::Local, binding.span);
        }
    }

    fn resolve_simple_stmt(&mut self, scope: ScopeId, stmt: &SimpleStmt) {
        match stmt {
            SimpleStmt::VarDecl {
                bindings, value, ..
            } => self.resolve_var_decl(scope, bindings, value),
            SimpleStmt::Set { places, value, .. } => {
                for place in places {
                    self.resolve_place(scope, place);
                }
                self.resolve_expr(scope, value);
            }
            SimpleStmt::Expr { expr, .. } => self.resolve_expr(scope, expr),
        }
    }

    fn resolve_for(&mut self, parent: ScopeId, clause: &ForClause, body: &Block) {
        let scope = self.symbols.new_scope(parent);
        match clause {
            ForClause::In {
                bindings, iterable, ..
            } => {
                self.resolve_expr(parent, iterable);
                for binding in bindings {
                    self.define_for_binding(scope, binding);
                }
            }
            ForClause::CStyle {
                init,
                condition,
                step,
                ..
            } => {
                if let Some(init) = init {
                    self.resolve_simple_stmt(scope, init);
                }
                if let Some(condition) = condition {
                    self.resolve_expr(scope, condition);
                }
                if let Some(step) = step {
                    self.resolve_simple_stmt(scope, step);
                }
            }
        }
        self.resolve_block_in_scope(scope, body);
    }

    fn define_for_binding(&mut self, scope: ScopeId, binding: &ForBinding) {
        self.define(scope, &binding.name, SymbolKind::Local, binding.span);
    }

    fn resolve_defer_body(&mut self, scope: ScopeId, body: &DeferBody) {
        match body {
            DeferBody::Expr { expr, .. } => self.resolve_expr(scope, expr),
            DeferBody::Block { block, .. } => self.resolve_block_child(scope, block),
        }
    }

    fn resolve_condition(&mut self, scope: ScopeId, condition: &Condition) {
        match condition {
            Condition::Expr { expr, .. } => self.resolve_expr(scope, expr),
            Condition::Is { expr, pattern, .. } => {
                self.resolve_expr(scope, expr);
                let pattern_scope = self.symbols.new_scope(scope);
                self.resolve_pattern(pattern_scope, pattern);
            }
        }
    }

    fn resolve_place(&mut self, scope: ScopeId, place: &Place) {
        self.resolve_assignment_target(scope, &place.root, place.span);
        for suffix in &place.suffixes {
            if let PlaceSuffix::Index { expr, .. } = suffix {
                self.resolve_expr(scope, expr);
            }
        }
    }

    fn resolve_expr(&mut self, scope: ScopeId, expr: &Expr) {
        match expr {
            Expr::Path { span, path } => {
                if let Some(root) = path.first() {
                    if path.len() > 1 && self.resolve_namespace_member(scope, root, &path[1], *span)
                    {
                        return;
                    }
                    self.resolve_value_name(scope, root, *span);
                }
            }
            Expr::TypePath {
                type_name,
                member,
                span,
                ..
            } => {
                let type_resolved = self.resolve_type_name(scope, type_name);
                if type_resolved {
                    let ty = type_name.path.join(".");
                    if let Some(symbol) = self.symbols.lookup_associated_member(&ty, member) {
                        self.resolved.value_ref(*span, symbol);
                    } else {
                        self.diagnostics.push(Diagnostic::error(
                            DiagCode::N010UndefinedAssociatedFunction,
                            format!("associated function '{ty}.{member}' is not declared"),
                            *span,
                        ));
                    }
                }
            }
            Expr::Generic { callee, args, .. } => {
                self.resolve_expr(scope, callee);
                for arg in args {
                    self.resolve_type_expr(scope, arg);
                }
            }
            Expr::Field { span, base, field } | Expr::SafeField { span, base, field } => {
                if let Expr::Path { path, .. } = &**base
                    && let Some(root) = path.first()
                    && path.len() == 1
                    && self.resolve_namespace_member(scope, root, field, *span)
                {
                    return;
                }
                self.resolve_expr(scope, base);
            }
            Expr::Try { expr: base, .. } => {
                self.resolve_expr(scope, base);
            }
            Expr::Index { base, index, .. } | Expr::SafeIndex { base, index, .. } => {
                self.resolve_expr(scope, base);
                self.resolve_expr(scope, index);
            }
            Expr::Call {
                callee,
                args,
                trailing_block,
                ..
            } => {
                self.resolve_expr(scope, callee);
                for arg in args {
                    self.resolve_expr(scope, arg);
                }
                if let Some(block) = trailing_block {
                    self.resolve_block_child(scope, block);
                }
            }
            Expr::StructLiteral { ty, fields, .. } => {
                self.resolve_type_expr(scope, ty);
                for field in fields {
                    self.resolve_field_init(scope, field);
                }
            }
            Expr::Array { items, .. } => {
                for item in items {
                    self.resolve_expr(scope, item);
                }
            }
            Expr::Lambda { params, body, .. } => self.resolve_lambda(scope, params, body),
            Expr::Alloc { expr, .. } | Expr::Group { expr, .. } => self.resolve_expr(scope, expr),
            Expr::AsyncBlock { block, .. } | Expr::UnsafeBlock { block, .. } => {
                self.resolve_block_child(scope, block);
            }
            Expr::If {
                condition,
                then_block,
                else_block,
                ..
            } => {
                self.resolve_condition(scope, condition);
                self.resolve_block_child(scope, then_block);
                self.resolve_block_child(scope, else_block);
            }
            Expr::Match { value, arms, .. } => {
                self.resolve_expr(scope, value);
                for arm in arms {
                    self.resolve_match_arm(scope, arm);
                }
            }
            Expr::Catch { expr, handler, .. } => {
                self.resolve_expr(scope, expr);
                self.resolve_catch_handler(scope, handler);
            }
            Expr::NullCoalesce { left, right, .. } => {
                self.resolve_expr(scope, left);
                self.resolve_expr(scope, right);
            }
            Expr::Cast { expr, ty, .. } => {
                self.resolve_expr(scope, expr);
                self.resolve_type_expr(scope, ty);
            }
            Expr::Unary { op: _, expr, .. } => self.resolve_expr(scope, expr),
            Expr::Binary {
                op: _, left, right, ..
            } => {
                self.resolve_expr(scope, left);
                self.resolve_expr(scope, right);
            }
            Expr::InterpolatedString { parts, .. } => {
                for part in parts {
                    if let arandu_parser::StringPart::Expr { expr, .. } = part {
                        self.resolve_expr(scope, expr);
                    }
                }
            }
            Expr::Int { .. }
            | Expr::Float { .. }
            | Expr::Bool { .. }
            | Expr::Char { .. }
            | Expr::Nil { .. } => {}
        }
    }

    fn resolve_field_init(&mut self, scope: ScopeId, field: &FieldInit) {
        self.resolve_expr(scope, &field.value);
    }

    fn resolve_lambda(&mut self, parent: ScopeId, params: &[LambdaParam], body: &LambdaBody) {
        let scope = self.symbols.new_scope(parent);
        for param in params {
            if let Some(ty) = &param.ty {
                self.resolve_type_expr(scope, ty);
            }
            self.define(scope, &param.name, SymbolKind::Param, param.span);
        }
        match body {
            LambdaBody::Expr { expr, .. } => self.resolve_expr(scope, expr),
            LambdaBody::Block { block, .. } => self.resolve_block_in_scope(scope, block),
        }
    }

    fn resolve_catch_handler(&mut self, parent: ScopeId, handler: &CatchHandler) {
        match handler {
            CatchHandler::Expr { expr, .. } => self.resolve_expr(parent, expr),
            CatchHandler::Block { span, error, block } => {
                let scope = self.symbols.new_scope(parent);
                self.define(scope, error, SymbolKind::Local, *span);
                self.resolve_block_in_scope(scope, block);
            }
        }
    }

    fn resolve_match_arm(&mut self, parent: ScopeId, arm: &MatchArm) {
        let scope = self.symbols.new_scope(parent);
        self.resolve_pattern(scope, &arm.pattern);
        if let Some(guard) = &arm.guard {
            self.resolve_expr(scope, guard);
        }
        match &arm.body {
            MatchArmBody::Expr { expr, .. } => self.resolve_expr(scope, expr),
            MatchArmBody::Block { block, .. } => self.resolve_block_in_scope(scope, block),
        }
    }

    fn resolve_pattern(&mut self, scope: ScopeId, pattern: &Pattern) {
        match pattern {
            Pattern::Wildcard { .. } => {}
            Pattern::Bind { span, name } => {
                self.define(scope, name, SymbolKind::Local, *span);
            }
            Pattern::Literal { expr, .. } => self.resolve_expr(scope, expr),
            Pattern::Enum {
                type_name, payload, ..
            } => {
                self.resolve_type_name(scope, type_name);
                for item in payload {
                    self.resolve_pattern(scope, item);
                }
            }
            Pattern::TypeTuple {
                span,
                name,
                payload,
            } => {
                self.resolve_type_name(
                    scope,
                    &TypeName {
                        span: *span,
                        path: vec![name.clone()],
                    },
                );
                for item in payload {
                    self.resolve_pattern(scope, item);
                }
            }
            Pattern::Struct {
                type_name, fields, ..
            } => {
                self.resolve_type_name(scope, type_name);
                for field in fields {
                    self.resolve_field_pattern(scope, field);
                }
            }
            Pattern::Tuple { items, .. } => {
                for item in items {
                    self.resolve_pattern(scope, item);
                }
            }
            Pattern::Range { start, end, .. } => {
                self.resolve_expr(scope, start);
                self.resolve_expr(scope, end);
            }
        }
    }

    fn resolve_field_pattern(&mut self, scope: ScopeId, field: &FieldPattern) {
        if let Some(pattern) = &field.pattern {
            self.resolve_pattern(scope, pattern);
        } else {
            self.define(scope, &field.name, SymbolKind::Local, field.span);
        }
    }

    fn resolve_result_type(&mut self, scope: ScopeId, result: &ResultType) {
        match result {
            ResultType::Single { ty, .. } => self.resolve_type_expr(scope, ty),
            ResultType::Multi { types, .. } => {
                for ty in types {
                    self.resolve_type_expr(scope, ty);
                }
            }
        }
    }

    fn resolve_type_expr(&mut self, scope: ScopeId, ty: &TypeExpr) {
        match ty {
            TypeExpr::Primitive { .. } => {}
            TypeExpr::Named { name, args, .. } => {
                self.resolve_type_name(scope, name);
                for arg in args {
                    self.resolve_type_expr(scope, arg);
                }
            }
            TypeExpr::Nullable { inner, .. }
            | TypeExpr::Pointer { inner, .. }
            | TypeExpr::Slice { inner, .. }
            | TypeExpr::Group { inner, .. } => self.resolve_type_expr(scope, inner),
            TypeExpr::Array { elem, .. } => self.resolve_type_expr(scope, elem),
            TypeExpr::Func { params, result, .. } => {
                for param in params {
                    self.resolve_type_expr(scope, param);
                }
                if let Some(result) = result {
                    self.resolve_result_type(scope, result);
                }
            }
        }
    }

    fn resolve_type_name(&mut self, scope: ScopeId, name: &TypeName) -> bool {
        let Some(root) = name.path.first() else {
            return false;
        };
        if name.path.len() > 1
            && self.symbols.lookup_module(scope, root).is_some()
        {
            let member = &name.path[1];
                if let Some(symbol) = self.symbols.lookup_module_member(root, member) {
                    self.resolved.type_ref(name.span, symbol);
                    return true;
                } else {
                    self.diagnostics.push(Diagnostic::error(
                        DiagCode::N009UndefinedNamespaceMember,
                        format!("namespace member '{root}.{member}' is not declared"),
                        name.span,
                    ));
                    return false;
                }
        }
        if let Some(symbol) = self.symbols.lookup_type(scope, root) {
            self.resolved.type_ref(name.span, symbol);
            return true;
        }
        if let Some(symbol) = self.symbols.lookup_any(scope, root)
            && self.symbols.get(symbol).kind.is_value()
        {
            self.diagnostics.push(Diagnostic::error(
                DiagCode::N005ValueUsedAsType,
                format!("value '{root}' cannot be used as a type"),
                name.span,
            ));
            return false;
        }
        let mut diagnostic = Diagnostic::error(
            DiagCode::N002UndefinedType,
            format!("type '{root}' is not declared"),
            name.span,
        );
        if let Some(suggestion) = self.suggest_type(scope, root) {
            diagnostic = diagnostic.with_hint(format!("did you mean '{suggestion}'?"));
        }
        self.diagnostics.push(diagnostic);
        false
    }

    fn resolve_value_name(&mut self, scope: ScopeId, name: &str, span: Span) {
        if let Some(symbol) = self.symbols.lookup_value(scope, name) {
            self.resolved.value_ref(span, symbol);
            return;
        }
        if self.symbols.lookup_module(scope, name).is_some() {
            self.diagnostics.push(Diagnostic::error(
                DiagCode::N008NamespaceUsedAsValue,
                format!("namespace '{name}' cannot be used as a value"),
                span,
            ));
            return;
        }
        if let Some(symbol) = self.symbols.lookup_any(scope, name)
            && self.symbols.get(symbol).kind.is_type()
        {
            self.diagnostics.push(Diagnostic::error(
                DiagCode::N004TypeUsedAsValue,
                format!("type '{name}' cannot be used as a value"),
                span,
            ));
            return;
        }
        let mut diagnostic = Diagnostic::error(
            DiagCode::N001UndefinedValue,
            format!("value '{name}' is not declared"),
            span,
        );
        if let Some(suggestion) = self.suggest_value(scope, name) {
            diagnostic = diagnostic.with_hint(format!("did you mean '{suggestion}'?"));
        }
        self.diagnostics.push(diagnostic);
    }

    fn resolve_assignment_target(&mut self, scope: ScopeId, name: &str, span: Span) {
        if let Some(symbol) = self.symbols.lookup_value(scope, name) {
            self.resolved.value_ref(span, symbol);
            return;
        }
        let diagnostic = Diagnostic::error(
            DiagCode::N007UndefinedAssignmentTarget,
            format!("assignment target '{name}' is not declared"),
            span,
        )
        .with_hint(format!(
            "declare it first with `{name} = ...`, then mutate with `set {name} = ...`"
        ));
        self.diagnostics.push(diagnostic);
    }

    fn resolve_namespace_member(
        &mut self,
        scope: ScopeId,
        namespace: &str,
        member: &str,
        span: Span,
    ) -> bool {
        if self.symbols.lookup_module(scope, namespace).is_none() {
            return false;
        }
        if let Some(symbol) = self.symbols.lookup_module_member(namespace, member) {
            self.resolved.value_ref(span, symbol);
        } else {
            self.diagnostics.push(Diagnostic::error(
                DiagCode::N009UndefinedNamespaceMember,
                format!("namespace member '{namespace}.{member}' is not declared"),
                span,
            ));
        }
        true
    }

    fn define(&mut self, scope: ScopeId, name: &str, kind: SymbolKind, span: Span) {
        match self.symbols.define(scope, name, kind, span) {
            Ok(symbol) => self.resolved.define(span, symbol),
            Err(previous) => {
                let previous_symbol = self.symbols.get(previous);
                self.diagnostics.push(
                    Diagnostic::error(
                        DiagCode::N003RedefinedName,
                        format!("name '{name}' is already declared in this scope"),
                        span,
                    )
                    .with_label(previous_symbol.span, "previous declaration is here"),
                );
            }
        }
    }

    fn suggest_value(&self, scope: ScopeId, name: &str) -> Option<String> {
        self.suggest_from(name, self.symbols.value_candidates(scope))
    }

    fn suggest_type(&self, scope: ScopeId, name: &str) -> Option<String> {
        self.suggest_from(name, self.symbols.type_candidates(scope))
    }

    fn suggest_from<'a>(
        &self,
        name: &str,
        candidates: impl IntoIterator<Item = &'a crate::Symbol>,
    ) -> Option<String> {
        candidates
            .into_iter()
            .min_by_key(|symbol| levenshtein(name, &symbol.name))
            .map(|symbol| symbol.name.clone())
    }
}

fn is_type_case(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}

fn levenshtein(left: &str, right: &str) -> usize {
    let right_len = right.chars().count();
    let mut previous: Vec<usize> = (0..=right_len).collect();
    let mut current = vec![0; right_len + 1];

    for (i, left_char) in left.chars().enumerate() {
        current[0] = i + 1;
        for (j, right_char) in right.chars().enumerate() {
            let cost = usize::from(left_char != right_char);
            current[j + 1] = (previous[j + 1] + 1)
                .min(current[j] + 1)
                .min(previous[j] + cost);
        }
        std::mem::swap(&mut previous, &mut current);
    }

    previous[right_len]
}

#[allow(dead_code)]
fn _keep_ops_exhaustive(_: UnaryOp, _: BinaryOp) {}
