use arandu_parser::{Program, TopLevelDecl, FuncName};

use crate::{ResolutionResult, ResolvedNames, SymbolTable, SymbolKind};

mod collect;
mod decls;
mod expr;
mod program;
mod stmt;
mod symbols;
mod types;
mod util;

#[must_use]
pub fn create_symbol_table_with_prelude() -> SymbolTable {
    let mut table = SymbolTable::new();
    let span = arandu_lexer::Span::new(0, 0, 0);
    for (module, members) in [
        ("io", ["println", "create", "remove"].as_slice()),
        ("err", ["new"].as_slice()),
    ] {
        for member in members {
            let _ = table.define_module_member(module, member, span);
        }
    }
    table
}

#[must_use]
pub fn resolve(program: &Program) -> ResolutionResult {
    Resolver::new(&program.pool).resolve_program(program)
}


#[must_use]
pub fn collect_symbols(program: &Program) -> (SymbolTable, ResolvedNames, crate::DocCommentMap, Vec<crate::Diagnostic>) {
    let mut resolver = Resolver {
        symbols: SymbolTable::new(),
        resolved: ResolvedNames::default(),
        docs: crate::DocCommentMap::default(),
        diagnostics: Vec::new(),
        pool: &program.pool,
    };

    for doc in &program.docs {
        resolver.docs
            .entry(crate::NodeKey::from(doc.target_span))
            .or_default()
            .push(doc.text.clone());
    }

    let global = resolver.symbols.global_scope();
    if let Some(module) = &program.module
        && let Some(root) = module.path.first()
    {
        resolver.define(global, root, SymbolKind::Module, module.span);
    }

    for import in &program.imports {
        resolver.collect_import(global, import);
    }

    for decl_id in &program.decls {
        let decl = program.pool.decl(*decl_id);
        resolver.collect_top_level(global, decl);
    }

    if let Some(module) = &program.module {
        let module_name = module.path.join(".");
        for decl_id in &program.decls {
            let decl = program.pool.decl(*decl_id);
            match decl {
                TopLevelDecl::Const(d) => {
                    let _ = resolver.symbols.define_module_member(&module_name, &d.name, d.span);
                }
                TopLevelDecl::TypeAlias(d) => {
                    let _ = resolver.symbols.define_module_member(&module_name, &d.name, d.span);
                }
                TopLevelDecl::Func(d) => {
                    if let FuncName::Free { span, name } = &d.name {
                        let _ = resolver.symbols.define_module_member(&module_name, name, *span);
                    }
                }
                TopLevelDecl::Struct(d) => {
                    let _ = resolver.symbols.define_module_member(&module_name, &d.name, d.span);
                }
                TopLevelDecl::Enum(d) => {
                    let _ = resolver.symbols.define_module_member(&module_name, &d.name, d.span);
                }
                TopLevelDecl::Interface(d) => {
                    let _ = resolver.symbols.define_module_member(&module_name, &d.name, d.span);
                }
                TopLevelDecl::Extern(d) => {
                    for member in &d.members {
                        let _ = resolver.symbols.define_module_member(&module_name, &member.name, member.span);
                    }
                }
                TopLevelDecl::Error(_) => {}
            }
        }
    }

    (resolver.symbols, resolver.resolved, resolver.docs, resolver.diagnostics)
}



#[must_use]
pub fn resolve_with_symbols(
    global_symbols: SymbolTable,
    resolved: ResolvedNames,
    docs: crate::DocCommentMap,
    diagnostics: Vec<crate::Diagnostic>,
    program: &Program,
) -> ResolutionResult {
    let mut resolver = Resolver {
        symbols: global_symbols,
        resolved,
        docs,
        diagnostics,
        pool: &program.pool,
    };


    let global = resolver.symbols.global_scope();
    for decl_id in &program.decls {
        let decl = program.pool.decl(*decl_id);
        resolver.resolve_top_level(global, decl);
    }

    ResolutionResult {
        symbols: resolver.symbols,
        resolved: resolver.resolved,
        docs: resolver.docs,
        diagnostics: resolver.diagnostics,
    }
}


struct Resolver<'a> {
    symbols: SymbolTable,
    resolved: ResolvedNames,
    docs: crate::DocCommentMap,
    diagnostics: Vec<crate::Diagnostic>,
    pool: &'a arandu_parser::ast_pool::AstPool,
}
