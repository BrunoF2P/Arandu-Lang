use arandu_parser::{Program, TopLevelDecl};

use super::super::TypeChecker;
use super::super::types::ArType;

pub(crate) fn collect_type_shapes(checker: &mut TypeChecker<'_>, program: &Program) {
    for decl_id in &program.decls {
        let decl = checker.pool.decl(*decl_id);
        if let TopLevelDecl::Struct(struct_decl) = decl {
            let mut fields = rustc_hash::FxHashMap::default();
            let mut field_symbols = rustc_hash::FxHashMap::default();
            for field in &struct_decl.fields {
                let field_ty = super::super::types::lower_type_expr(
                    field.ty,
                    checker.pool,
                    &checker.symbols,
                    checker.symbols.global_scope(),
                    &checker.resolved,
                );
                let field_key = crate::NodeKey::from(field.span);
                if let Some(field_symbol) = checker.resolved.definitions.get(&field_key) {
                    field_symbols.insert(field.name.clone(), *field_symbol);
                }
                fields.insert(field.name.clone(), field_ty);
            }
            let struct_key = crate::NodeKey::from(struct_decl.span);
            if let Some(symbol_id) = checker.resolved.definitions.get(&struct_key) {
                checker.type_info.struct_fields.insert(*symbol_id, fields);
                checker
                    .type_info
                    .struct_field_symbols
                    .insert(*symbol_id, field_symbols);
                let params = super::super::types::collect_generic_param_symbols(
                    checker,
                    &struct_decl.generic_params,
                );
                if !params.is_empty() {
                    checker.type_info.generic_params.insert(*symbol_id, params);
                }
            }
        } else if let TopLevelDecl::Enum(enum_decl) = decl {
            let enum_key = crate::NodeKey::from(enum_decl.span);
            if let Some(enum_symbol_id) = checker.resolved.definitions.get(&enum_key) {
                for variant in &enum_decl.variants {
                    let shape = match &variant.payload {
                        None => super::super::EnumPayloadShape::Unit,
                        Some(arandu_parser::EnumPayload::Tuple { types, .. }) => {
                            let tys = checker.pool.type_expr_list(*types)
                                .iter()
                                .map(|&ty_expr| {
                                    super::super::types::lower_type_expr(
                                        ty_expr,
                                        checker.pool,
                                        &checker.symbols,
                                        checker.symbols.global_scope(),
                                        &checker.resolved,
                                    )
                                })
                                .collect();
                            super::super::EnumPayloadShape::Tuple(tys)
                        }
                        _ => super::super::EnumPayloadShape::Unit,
                    };
                    let variant_key = crate::NodeKey::from(variant.span);
                    if let Some(variant_symbol_id) = checker.resolved.definitions.get(&variant_key)
                    {
                        checker
                            .type_info
                            .enum_variants
                            .insert(*variant_symbol_id, (*enum_symbol_id, shape.clone()));
                    }
                    if let Some(assoc_symbol_id) = checker
                        .symbols
                        .lookup_associated_member(&enum_decl.name, &variant.name)
                    {
                        checker
                            .type_info
                            .enum_variants
                            .insert(assoc_symbol_id, (*enum_symbol_id, shape));
                    }
                }
            }
        }
    }

    // T029: Recursive struct size validation
    let struct_ids: Vec<crate::SymbolId> = checker.type_info.struct_fields.keys().copied().collect();
    for struct_id in struct_ids {
        let mut visiting = rustc_hash::FxHashSet::default();
        let mut visited = rustc_hash::FxHashSet::default();
        check_recursive(struct_id, struct_id, &mut visiting, &mut visited, checker);
    }
}

fn check_recursive(
    root_struct_id: crate::SymbolId,
    current_struct_id: crate::SymbolId,
    visiting: &mut rustc_hash::FxHashSet<crate::SymbolId>,
    visited: &mut rustc_hash::FxHashSet<crate::SymbolId>,
    checker: &mut TypeChecker<'_>,
) {
    if visited.contains(&current_struct_id) {
        return;
    }
    if visiting.contains(&current_struct_id) {
        let struct_symbol = checker.symbols.get(root_struct_id);
        let struct_name = struct_symbol.name.clone();
        let diag = crate::Diagnostic::error(
            crate::DiagCode::T029RecursiveStructInfiniteSize,
            format!("recursive type '{struct_name}' has infinite size"),
            struct_symbol.span,
        )
        .with_label(struct_symbol.span, "recursive type has infinite size")
        .with_hint("insert indirection (e.g. use a pointer type like `ptr[T]`) to make it finite");
        checker.diagnostics.push(diag);
        return;
    }

    visiting.insert(current_struct_id);

    if let Some(fields) = checker.type_info.struct_fields.get(&current_struct_id).cloned() {
        for field_ty in fields.values() {
            visit_type(root_struct_id, field_ty, visiting, visited, checker);
        }
    }

    visiting.remove(&current_struct_id);
    visited.insert(current_struct_id);
}

fn visit_type(
    root_struct_id: crate::SymbolId,
    ty: &ArType,
    visiting: &mut rustc_hash::FxHashSet<crate::SymbolId>,
    visited: &mut rustc_hash::FxHashSet<crate::SymbolId>,
    checker: &mut TypeChecker<'_>,
) {
    match ty {
        ArType::Named(id, _) if checker.type_info.struct_fields.contains_key(id) => {
            check_recursive(root_struct_id, *id, visiting, visited, checker);
        }
        ArType::Tuple(tys) => {
            for t in tys {
                visit_type(root_struct_id, t, visiting, visited, checker);
            }
        }
        ArType::Array(_, inner) | ArType::Slice(inner) | ArType::Nullable(inner) | ArType::Option(inner) => {
            visit_type(root_struct_id, inner, visiting, visited, checker);
        }
        ArType::Result(ok, err) => {
            visit_type(root_struct_id, ok, visiting, visited, checker);
            visit_type(root_struct_id, err, visiting, visited, checker);
        }
        _ => {}
    }
}


pub(crate) fn collect_signature_types(checker: &mut TypeChecker<'_>, program: &Program) {
    for decl_id in &program.decls {
        let decl = checker.pool.decl(*decl_id);
        match decl {
            TopLevelDecl::Func(func_decl) => {
                let ret_ty = if let Some(result) = &func_decl.result {
                    super::super::types::lower_result_type(
                        result,
                        checker.pool,
                        &checker.symbols,
                        checker.symbols.global_scope(),
                        &checker.resolved,
                    )
                } else {
                    ArType::Void
                };

                let mut param_types = Vec::new();
                for param in &func_decl.params {
                    let param_ty = super::super::types::lower_type_expr(
                        param.ty,
                        checker.pool,
                        &checker.symbols,
                        checker.symbols.global_scope(),
                        &checker.resolved,
                    );
                    param_types.push(param_ty);
                }

                let name_span = match &func_decl.name {
                    arandu_parser::FuncName::Free { span, .. } => *span,
                    arandu_parser::FuncName::Method { span, .. } => *span,
                };
                let name_key = crate::NodeKey::from(name_span);
                if let Some(symbol_id) = checker.resolved.definitions.get(&name_key).copied() {
                    let func_ty = ArType::Func(param_types, Box::new(ret_ty));
                    checker.record_decl_type(symbol_id, func_ty);
                    let params = super::super::types::collect_generic_param_symbols(
                        checker,
                        &func_decl.generic_params,
                    );
                    if !params.is_empty() {
                        checker.type_info.generic_params.insert(symbol_id, params);
                    }
                }
            }
            TopLevelDecl::Extern(extern_decl) => {
                for member in &extern_decl.members {
                    let ret_ty = if let Some(result) = &member.result {
                        super::super::types::lower_result_type(
                            result,
                            checker.pool,
                            &checker.symbols,
                            checker.symbols.global_scope(),
                            &checker.resolved,
                        )
                    } else {
                        ArType::Void
                    };

                    let mut param_types = Vec::new();
                    for param in &member.params {
                        let param_ty = super::super::types::lower_type_expr(
                            param.ty,
                            checker.pool,
                            &checker.symbols,
                            checker.symbols.global_scope(),
                            &checker.resolved,
                        );
                        param_types.push(param_ty);
                    }

                    let name_key = crate::NodeKey::from(member.span);
                    if let Some(symbol_id) = checker.resolved.definitions.get(&name_key).copied() {
                        let func_ty = ArType::Func(param_types, Box::new(ret_ty));
                        checker.record_decl_type(symbol_id, func_ty);
                    }
                }
            }
            _ => {}
        }
    }
}
