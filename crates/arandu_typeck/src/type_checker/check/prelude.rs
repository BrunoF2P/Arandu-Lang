use super::super::types::{ArType, Primitive};
use super::super::{SessionMode, TypeChecker};
use arandu_middle::StdlibPathCache;
use arandu_middle::parse_cache::ParseCache;
use arandu_parser::Program;

fn load_stdlib_signatures(
    checker: &mut TypeChecker<'_>,
    program: &Program,
    current_module: Option<&str>,
    cache: &mut ParseCache,
    stdlib_cache: &mut StdlibPathCache,
) {
    let mut visited = rustc_hash::FxHashSet::default();
    let mut queue = Vec::new();
    let start_relative_path = "stdlib/core/prelude.aru";
    if let Some(path) = stdlib_cache.get_or_resolve(start_relative_path) {
        queue.push(path);
    }
    for import in &program.imports {
        if let arandu_parser::ImportDecl::External { source, .. } = import {
            if source.starts_with("std.core.") {
                let relative = source.strip_prefix("std.core.").unwrap();
                let stdlib_rel = format!("stdlib/core/{relative}.aru");
                if let Some(p) = stdlib_cache.get_or_resolve(&stdlib_rel) {
                    queue.push(p);
                }
            } else if source.starts_with("std.alloc.") {
                let relative = source.strip_prefix("std.alloc.").unwrap();
                let stdlib_rel = format!("stdlib/alloc/{relative}.aru");
                if let Some(p) = stdlib_cache.get_or_resolve(&stdlib_rel) {
                    queue.push(p);
                }
            }
        }
    }

    while let Some(path) = queue.pop() {
        if !visited.insert(path.clone()) {
            continue;
        }
        match std::fs::read_to_string(&path) {
            Ok(source) => {
                match cache.get_or_parse(&path, &source) {
                    Ok(program) => {
                        let module_name = program.module.as_ref().map(|m| m.path.join("."));
                        let is_current = module_name.as_deref() == current_module;

                        if !is_current {
                            let mut resolved = arandu_resolve::ResolvedNames::default();

                            if let Some(ref mod_name) = module_name {
                                for decl_id in &program.decls {
                                    let decl = program.pool.decl(*decl_id);
                                    match decl {
                                        arandu_parser::TopLevelDecl::Enum(d) => {
                                            if let Some(sym) = checker
                                                .symbols
                                                .lookup_module_member(mod_name, &d.name)
                                            {
                                                resolved.define(d.span, sym);
                                            }
                                        }
                                        arandu_parser::TopLevelDecl::Struct(d) => {
                                            if let Some(sym) = checker
                                                .symbols
                                                .lookup_module_member(mod_name, &d.name)
                                            {
                                                resolved.define(d.span, sym);
                                            }
                                        }
                                        arandu_parser::TopLevelDecl::Func(d) => match &d.name {
                                            arandu_parser::FuncName::Free { span, name } => {
                                                if let Some(sym) = checker
                                                    .symbols
                                                    .lookup_module_member(mod_name, name)
                                                {
                                                    resolved.define(*span, sym);
                                                }
                                            }
                                            arandu_parser::FuncName::Method {
                                                span,
                                                receiver,
                                                name,
                                            } => {
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

                            let res = arandu_resolve::resolve_with_symbols(
                                checker.symbols.clone(),
                                resolved,
                                arandu_resolve::DocCommentMap::default(),
                                Vec::new(),
                                program,
                            );

                            let tc_result = crate::type_check(res, program);

                            let checker_sym_count = checker.symbols.iter().count();
                            let tc_sym_count = tc_result.symbols.iter().count();
                            if tc_sym_count > checker_sym_count {
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
                                    if let Some(p) = stdlib_cache.get_or_resolve(&stdlib_rel) {
                                        queue.push(p);
                                    }
                                } else if source.starts_with("std.alloc.") {
                                    let relative = source.strip_prefix("std.alloc.").unwrap();
                                    let stdlib_rel = format!("stdlib/alloc/{relative}.aru");
                                    if let Some(p) = stdlib_cache.get_or_resolve(&stdlib_rel) {
                                        queue.push(p);
                                    }
                                }
                            }
                        }
                    }
                    Err(err) => {
                        checker.diagnostics.push(err.into());
                    }
                }
            }
            Err(err) => {
                checker.diagnostics.push(arandu_middle::Diagnostic::error(
                    arandu_middle::DiagCode::ICET001,
                    format!("failed to read stdlib file {}: {}", path.display(), err),
                    arandu_lexer::Span::new(0, 0, 0),
                ));
            }
        }
    }
}

pub(crate) fn register_prelude(
    checker: &mut TypeChecker<'_>,
    program: &Program,
    cache: &mut ParseCache,
    stdlib_cache: &mut StdlibPathCache,
) {
    let any_id = checker.intern(ArType::Primitive(Primitive::Any));
    let void_id = checker.intern(ArType::Void);
    let str_id = checker.intern(ArType::Primitive(Primitive::Str));
    let err_literal_id = checker.intern(ArType::Err);

    let result_any_err = checker.intern(ArType::Result(any_id, err_literal_id));
    let result_void_err = checker.intern(ArType::Result(void_id, err_literal_id));

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
                let ty_id = checker.intern(ty);
                checker.record_decl_type(symbol_id, ty_id);
            }
        }
    }

    if checker.session_mode == SessionMode::Shared {
        let current_module = program.module.as_ref().map(|m| m.path.join("."));
        load_stdlib_signatures(
            checker,
            program,
            current_module.as_deref(),
            cache,
            stdlib_cache,
        );
    }
}
