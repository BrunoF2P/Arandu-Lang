use super::super::TypeChecker;
use super::super::types::{ArType, Primitive};
use arandu_parser::Program;

thread_local! {
    static IS_LOADING_STDLIB: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn find_stdlib_path(relative: &str) -> Option<std::path::PathBuf> {
    let mut current = std::env::current_dir().ok()?;
    loop {
        let candidate = current.join(relative);
        if candidate.exists() {
            return Some(candidate);
        }
        if let Some(parent) = current.parent() {
            current = parent.to_path_buf();
        } else {
            break;
        }
    }
    None
}

fn load_stdlib_signatures(
    checker: &mut TypeChecker<'_>,
    program: &Program,
    current_module: Option<&str>,
) {
    let mut visited = rustc_hash::FxHashSet::default();
    let mut queue = Vec::new();
    let start_relative_path = "stdlib/core/prelude.aru";
    if let Some(path) = find_stdlib_path(start_relative_path) {
        queue.push(path);
    }
    for import in &program.imports {
        if let arandu_parser::ImportDecl::External { source, .. } = import {
            if source.starts_with("std.core.") {
                let relative = source.strip_prefix("std.core.").unwrap();
                let stdlib_rel = format!("stdlib/core/{relative}.aru");
                if let Some(p) = find_stdlib_path(&stdlib_rel) {
                    queue.push(p);
                }
            } else if source.starts_with("std.alloc.") {
                let relative = source.strip_prefix("std.alloc.").unwrap();
                let stdlib_rel = format!("stdlib/alloc/{relative}.aru");
                if let Some(p) = find_stdlib_path(&stdlib_rel) {
                    queue.push(p);
                }
            }
        }
    }

    while let Some(path) = queue.pop() {
        if !visited.insert(path.clone()) {
            continue;
        }
        if let Ok(source) = std::fs::read_to_string(&path)
            && let Ok(program) = arandu_parser::parse(&source) {
                let module_name = program.module.as_ref().map(|m| m.path.join("."));
                let is_current = module_name.as_deref() == current_module;

                if !is_current {
                    // Build a ResolvedNames that maps declaration spans to the
                    // *existing* SymbolIds already in checker.symbols (registered
                    // by load_stdlib_transitively during name resolution).
                    // This avoids the ID-mismatch that occurred when
                    // collect_symbols generated fresh IDs independent of the
                    // global table.
                    let mut resolved = arandu_resolve::ResolvedNames::default();

                    if let Some(ref mod_name) = module_name {
                        for decl_id in &program.decls {
                            let decl = program.pool.decl(*decl_id);
                            match decl {
                                arandu_parser::TopLevelDecl::Enum(d) => {
                                    if let Some(sym) =
                                        checker.symbols.lookup_module_member(mod_name, &d.name)
                                    {
                                        resolved.define(d.span, sym);
                                    }
                                }
                                arandu_parser::TopLevelDecl::Struct(d) => {
                                    if let Some(sym) =
                                        checker.symbols.lookup_module_member(mod_name, &d.name)
                                    {
                                        resolved.define(d.span, sym);
                                    }
                                }
                                arandu_parser::TopLevelDecl::Func(d) => match &d.name {
                                    arandu_parser::FuncName::Free { span, name } => {
                                        if let Some(sym) =
                                            checker.symbols.lookup_module_member(mod_name, name)
                                        {
                                            resolved.define(*span, sym);
                                        }
                                    }
                                    arandu_parser::FuncName::Method { span, receiver, name } => {
                                        let receiver_name = receiver.path.join(".");
                                        if let Some(sym) = checker
                                            .symbols
                                            .lookup_associated_member(&receiver_name, name)
                                        {
                                            resolved.define(*span, sym);
                                        }
                                    }
                                },
                                arandu_parser::TopLevelDecl::Extern(d) => {
                                    for member in &d.members {
                                        if let Some(sym) = checker
                                            .symbols
                                            .lookup_module_member(mod_name, &member.name)
                                        {
                                            resolved.define(member.span, sym);
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }

                    // Resolve using the global checker.symbols so all cross-file
                    // references (e.g. mem.ptr_offset in pointer.aru) resolve to
                    // the correct SymbolIds.
                    let res = arandu_resolve::resolve_with_symbols(
                        checker.symbols.clone(),
                        resolved,
                        arandu_resolve::DocCommentMap::default(),
                        Vec::new(),
                        &program,
                    );

                    // Type-check the stdlib file and merge type signatures.
                    let tc_result = crate::type_check(res, &program);

                    // The resolver may have created new symbols (e.g. TypeParams
                    // for generic parameters). Extend checker.symbols so that any
                    // SymbolId referenced in the merged type_info is valid.
                    let checker_sym_count = checker.symbols.iter().count();
                    let tc_sym_count = tc_result.symbols.iter().count();
                    if tc_sym_count > checker_sym_count {
                        // Collect extra symbols and merge them into checker.symbols.
                        // Since tc_result.symbols = checker.symbols + new syms at
                        // the end, we can merge the full tc_result.symbols; merge_from
                        // will re-offset new ones and ignore conflicts on existing names.
                        checker
                            .symbols
                            .merge_from_extending(&tc_result.symbols, checker_sym_count);
                    }

                    checker.type_info.merge_from(&tc_result.type_info);
                }

                // Enqueue imports
                for import in &program.imports {
                    if let arandu_parser::ImportDecl::External { source, .. } = import {
                        if source.starts_with("std.core.") {
                            let relative = source.strip_prefix("std.core.").unwrap();
                            let stdlib_rel = format!("stdlib/core/{relative}.aru");
                            if let Some(p) = find_stdlib_path(&stdlib_rel) {
                                queue.push(p);
                            }
                        } else if source.starts_with("std.alloc.") {
                            let relative = source.strip_prefix("std.alloc.").unwrap();
                            let stdlib_rel = format!("stdlib/alloc/{relative}.aru");
                            if let Some(p) = find_stdlib_path(&stdlib_rel) {
                                queue.push(p);
                            }
                        }
                    }
                }
            }
    }
}

pub(crate) fn register_prelude(checker: &mut TypeChecker<'_>, program: &Program) {
    let any_id = super::super::types::intern_type(ArType::Primitive(Primitive::Any));
    let void_id = super::super::types::intern_type(ArType::Void);
    let str_id = super::super::types::intern_type(ArType::Primitive(Primitive::Str));
    let err_literal_id = super::super::types::intern_type(ArType::Err);

    let result_any_err = super::super::types::intern_type(ArType::Result(any_id, err_literal_id));
    let result_void_err = super::super::types::intern_type(ArType::Result(void_id, err_literal_id));

    let println_ty = ArType::Func(vec![any_id], void_id);
    let create_ty = ArType::Func(vec![str_id], result_any_err);
    let remove_ty = ArType::Func(vec![str_id], result_void_err);
    let err_new_ty = ArType::Func(vec![str_id], err_literal_id);

    for (module, members_with_types) in [
        (
            "io",
            vec![
                ("println", println_ty),
                ("create", create_ty),
                ("remove", remove_ty),
            ],
        ),
        ("err", vec![("new", err_new_ty)]),
    ] {
        for (member, ty) in members_with_types {
            if let Some(symbol_id) = checker.symbols.lookup_module_member(module, member) {
                checker.record_decl_type(symbol_id, ty);
            }
        }
    }

    let is_loading = IS_LOADING_STDLIB.with(|cell| cell.replace(true));
    if !is_loading {
        let current_module = program.module.as_ref().map(|m| m.path.join("."));
        load_stdlib_signatures(checker, program, current_module.as_deref());
        IS_LOADING_STDLIB.with(|cell| cell.set(false));
    }
}
