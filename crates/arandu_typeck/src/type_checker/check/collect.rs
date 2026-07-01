use arandu_parser::{Program, TopLevelDecl};

use crate::type_checker::TypeChecker;
use crate::type_checker::types::ArType;

pub(crate) fn collect_type_shapes(checker: &mut TypeChecker<'_>, program: &Program) {
    for decl_id in &program.decls {
        let decl = checker.pool.decl(*decl_id);
        match decl {
            TopLevelDecl::Struct(struct_decl) => {
                let mut fields = rustc_hash::FxHashMap::default();
                let mut field_symbols = rustc_hash::FxHashMap::default();
                let mut field_indices = rustc_hash::FxHashMap::default();
                for (idx, field) in struct_decl.fields.iter().enumerate() {
                    let field_ty =
                        checker.lower_type_expr(field.ty, checker.symbols.global_scope());
                    let field_key = crate::NodeKey::from(field.span);
                    if let Some(field_symbol) = checker.resolved.definitions.get(&field_key) {
                        field_symbols.insert(field.name.clone(), *field_symbol);
                    }
                    fields.insert(field.name.clone(), field_ty);
                    field_indices.insert(field.name.clone(), idx);
                }
                let struct_key = crate::NodeKey::from(struct_decl.span);
                if let Some(symbol_id) = checker.resolved.definitions.get(&struct_key).copied() {
                    checker.type_info.struct_fields.insert(symbol_id, fields);
                    checker
                        .type_info
                        .struct_field_symbols
                        .insert(symbol_id, field_symbols);
                    checker
                        .type_info
                        .struct_field_indices
                        .insert(symbol_id, field_indices);
                    let params = super::super::types::extract_generic_param_symbols(
                        checker,
                        &struct_decl.generic_params,
                    );
                    if !params.is_empty() {
                        checker.type_info.generic_params.insert(symbol_id, params);
                    }
                }
            }
            TopLevelDecl::Enum(enum_decl) => {
                let enum_key = crate::NodeKey::from(enum_decl.span);
                let Some(enum_symbol_id) = checker.resolved.definitions.get(&enum_key).copied()
                else {
                    continue;
                };
                let params = super::super::types::extract_generic_param_symbols(
                    checker,
                    &enum_decl.generic_params,
                );
                if !params.is_empty() {
                    checker
                        .type_info
                        .generic_params
                        .insert(enum_symbol_id, params);
                }

                for (tag, variant) in enum_decl.variants.iter().enumerate() {
                    let shape = match &variant.payload {
                        None => super::super::EnumPayloadShape::Unit,
                        Some(arandu_parser::EnumPayload::Tuple { types, .. }) => {
                            let type_list = checker.pool.type_expr_list(*types).to_vec();
                            let tys = type_list
                                .iter()
                                .map(|&ty_expr| {
                                    checker.lower_type_expr(ty_expr, checker.symbols.global_scope())
                                })
                                .collect();
                            super::super::EnumPayloadShape::Tuple(tys)
                        }
                        _ => super::super::EnumPayloadShape::Unit,
                    };
                    let variant_key = crate::NodeKey::from(variant.span);
                    if let Some(variant_symbol_id) =
                        checker.resolved.definitions.get(&variant_key).copied()
                    {
                        checker
                            .type_info
                            .enum_variants
                            .insert(variant_symbol_id, (enum_symbol_id, shape.clone()));
                        checker
                            .type_info
                            .record_enum_variant_tag(variant_symbol_id, tag);
                    }
                }
            }
            TopLevelDecl::Const(const_decl) => {
                if let Some(ty_expr) = const_decl.ty {
                    let const_ty = checker.lower_type_expr(ty_expr, checker.symbols.global_scope());
                    let const_key = crate::NodeKey::from(const_decl.span);
                    if let Some(symbol_id) = checker.resolved.definitions.get(&const_key).copied() {
                        let const_id = checker.intern(const_ty);
                        checker.record_decl_type(symbol_id, const_id);
                    }
                }
            }
            TopLevelDecl::TypeAlias(alias_decl) => {
                let alias_ty =
                    checker.lower_type_expr(alias_decl.ty, checker.symbols.global_scope());
                let alias_key = crate::NodeKey::from(alias_decl.span);
                if let Some(symbol_id) = checker.resolved.definitions.get(&alias_key).copied() {
                    let alias_id = checker.intern(alias_ty);
                    checker.record_decl_type(symbol_id, alias_id);
                    let params = super::super::types::extract_generic_param_symbols(
                        checker,
                        &alias_decl.generic_params,
                    );
                    if !params.is_empty() {
                        checker.type_info.generic_params.insert(symbol_id, params);
                    }
                }
            }
            TopLevelDecl::Func(func_decl) => {
                let name_span = match func_decl.name {
                    arandu_parser::FuncName::Free { span, .. } => span,
                    arandu_parser::FuncName::Method { span, .. } => span,
                };
                let name_key = crate::NodeKey::from(name_span);
                if let Some(symbol_id) = checker.resolved.definitions.get(&name_key).copied() {
                    let params = super::super::types::extract_generic_param_symbols(
                        checker,
                        &func_decl.generic_params,
                    );
                    if !params.is_empty() {
                        checker.type_info.generic_params.insert(symbol_id, params);
                    }
                }
            }
            _ => {}
        }
    }
}

pub(crate) fn collect_signature_types(checker: &mut TypeChecker<'_>, program: &Program) {
    for decl_id in &program.decls {
        let decl = checker.pool.decl(*decl_id);
        match decl {
            TopLevelDecl::Func(func_decl) => {
                let ret_ty = if let Some(result) = &func_decl.result {
                    checker.lower_result_type(result, checker.symbols.global_scope())
                } else {
                    ArType::Void
                };

                let mut param_types = Vec::new();
                for param in &func_decl.params {
                    let param_ty =
                        checker.lower_type_expr(param.ty, checker.symbols.global_scope());
                    param_types.push(checker.intern(param_ty));
                }

                let name_span = match func_decl.name {
                    arandu_parser::FuncName::Free { span, .. } => span,
                    arandu_parser::FuncName::Method { span, .. } => span,
                };
                let name_key = crate::NodeKey::from(name_span);
                if let Some(symbol_id) = checker.resolved.definitions.get(&name_key).copied() {
                    let params = super::super::types::extract_generic_param_symbols(
                        checker,
                        &func_decl.generic_params,
                    );
                    if !params.is_empty() {
                        checker
                            .type_info
                            .generic_params
                            .insert(symbol_id, params.clone());
                    }
                    if let arandu_parser::FuncName::Method { .. } = &func_decl.name
                        && let Some(first_param) = func_decl.params.first()
                        && first_param.name.as_str() == "self"
                        && let Some(first_ty_id) = param_types.first_mut()
                    {
                        let lowered_first_ty = checker.resolve(*first_ty_id).clone();
                        if let ArType::Named(struct_id, ref args) = lowered_first_ty
                            && args.is_empty()
                            && !params.is_empty()
                        {
                            let mut new_args = Vec::new();
                            for &param_sym in &params {
                                let arg_ty = ArType::Named(param_sym, vec![]);
                                new_args.push(checker.intern(arg_ty));
                            }
                            let new_first_ty = ArType::Named(struct_id, new_args);
                            *first_ty_id = checker.intern(new_first_ty);
                        }
                    }
                    let ret_id = checker.intern(ret_ty);
                    let func_ty = ArType::Func(param_types, ret_id);
                    let func_id = checker.intern(func_ty);
                    checker.record_decl_type(symbol_id, func_id);
                }
            }
            TopLevelDecl::Extern(extern_decl) => {
                for member in &extern_decl.members {
                    let ret_ty = if let Some(result) = &member.result {
                        checker.lower_result_type(result, checker.symbols.global_scope())
                    } else {
                        ArType::Void
                    };

                    let mut param_types = Vec::new();
                    for param in &member.params {
                        let param_ty =
                            checker.lower_type_expr(param.ty, checker.symbols.global_scope());
                        param_types.push(checker.intern(param_ty));
                    }

                    let name_key = crate::NodeKey::from(member.span);
                    if let Some(symbol_id) = checker.resolved.definitions.get(&name_key).copied() {
                        let ret_id = checker.intern(ret_ty);
                        let func_ty = ArType::Func(param_types, ret_id);
                        let func_id = checker.intern(func_ty);
                        checker.record_decl_type(symbol_id, func_id);
                        let params = super::super::types::extract_generic_param_symbols(
                            checker,
                            &member.generic_params,
                        );
                        if !params.is_empty() {
                            checker.type_info.generic_params.insert(symbol_id, params);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}
