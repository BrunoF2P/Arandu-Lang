use arandu_parser::{
    CatchHandler, Expr, FieldInit, FieldPattern, LambdaBody, LambdaParam, MatchArm, MatchArmBody,
    Pattern,
};

use crate::{DiagCode, Diagnostic, ScopeId, SymbolKind};

use super::Resolver;

impl Resolver {
    pub(crate) fn resolve_expr(&mut self, scope: ScopeId, expr: &Expr) {
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
                let base = type_name
                    .path
                    .last()
                    .map_or("", std::string::String::as_str);
                if matches!(
                    (base, member.as_str()),
                    ("Result", "Ok" | "Err") | ("Option", "Some")
                ) {
                    return;
                }
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
                let then_scope = self.resolve_condition(scope, condition);
                self.resolve_block_child(then_scope, then_block);
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
            | Expr::Nil { .. }
            | Expr::Error(_) => {}
        }
    }

    pub(crate) fn resolve_field_init(&mut self, scope: ScopeId, field: &FieldInit) {
        self.resolve_expr(scope, &field.value);
    }

    pub(crate) fn resolve_lambda(
        &mut self,
        parent: ScopeId,
        params: &[LambdaParam],
        body: &LambdaBody,
    ) {
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

    pub(crate) fn resolve_catch_handler(&mut self, parent: ScopeId, handler: &CatchHandler) {
        match handler {
            CatchHandler::Expr { expr, .. } => self.resolve_expr(parent, expr),
            CatchHandler::Block { span, error, block } => {
                let scope = self.symbols.new_scope(parent);
                self.define(scope, error, SymbolKind::Local, *span);
                self.resolve_block_in_scope(scope, block);
            }
        }
    }

    pub(crate) fn resolve_match_arm(&mut self, parent: ScopeId, arm: &MatchArm) {
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

    pub(crate) fn resolve_pattern(&mut self, scope: ScopeId, pattern: &Pattern) {
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
            Pattern::TypeTuple { payload, .. } => {
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

    pub(crate) fn resolve_field_pattern(&mut self, scope: ScopeId, field: &FieldPattern) {
        if let Some(pattern) = &field.pattern {
            self.resolve_pattern(scope, pattern);
        } else {
            self.define(scope, &field.name, SymbolKind::Local, field.span);
        }
    }
}
