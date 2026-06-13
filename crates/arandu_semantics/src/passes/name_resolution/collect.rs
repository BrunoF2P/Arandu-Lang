use arandu_parser::{FuncName, ImportDecl, TopLevelDecl};

use crate::{DiagCode, Diagnostic, ScopeId, SymbolKind};

use super::Resolver;
use super::util::is_type_case;

impl<'a> Resolver<'a> {
    pub(crate) fn collect_import(&mut self, scope: ScopeId, import: &ImportDecl) {
        match import {
            ImportDecl::Module { span, path } => {
                if let Some(root) = path.first() {
                    self.define(scope, root, SymbolKind::Module, *span);
                } else {
                    self.diagnostics.push(Diagnostic::error(
                        DiagCode::M001UnresolvedImport,
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

    pub(crate) fn collect_top_level(&mut self, scope: ScopeId, decl: &TopLevelDecl) {
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
                                        "associated function '{receiver}.{name}' is already declared"
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
                    if let Ok(symbol) = self.symbols.define_associated_member(
                        &decl.name,
                        &variant.name,
                        variant.span,
                    ) {
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
            TopLevelDecl::Error(_) => {}
        }
    }
}
