use arandu_parser::{Program, TopLevelDecl};

use super::super::TypeChecker;
use super::super::types::ArType;

pub(crate) fn collect_type_shapes(checker: &mut TypeChecker, program: &Program) {
    for decl in &program.decls {
        if let TopLevelDecl::Struct(struct_decl) = decl {
            let mut fields = std::collections::HashMap::new();
            for field in &struct_decl.fields {
                let field_ty = super::super::types::lower_type_expr(
                    &field.ty,
                    &checker.symbols,
                    checker.symbols.global_scope(),
                    &checker.resolved,
                );
                fields.insert(field.name.clone(), field_ty);
            }
            let struct_key = crate::NodeKey::from(struct_decl.span);
            if let Some(symbol_id) = checker.resolved.definitions.get(&struct_key) {
                checker.type_info.struct_fields.insert(*symbol_id, fields);
                let params = super::super::types::collect_generic_param_symbols(
                    checker,
                    &struct_decl.generic_params,
                );
                if !params.is_empty() {
                    checker
                        .type_info
                        .generic_params
                        .insert(*symbol_id, params);
                }
            }
        } else if let TopLevelDecl::Enum(enum_decl) = decl {
            let enum_key = crate::NodeKey::from(enum_decl.span);
            if let Some(enum_symbol_id) = checker.resolved.definitions.get(&enum_key) {
                for variant in &enum_decl.variants {
                    let shape = match &variant.payload {
                        None => super::super::EnumPayloadShape::Unit,
                        Some(arandu_parser::EnumPayload::Tuple { types, .. }) => {
                            let tys = types
                                .iter()
                                .map(|ty_expr| {
                                    super::super::types::lower_type_expr(
                                        ty_expr,
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
}

pub(crate) fn collect_signature_types(checker: &mut TypeChecker, program: &Program) {
    for decl in &program.decls {
        match decl {
            TopLevelDecl::Func(func_decl) => {
                let ret_ty = if let Some(result) = &func_decl.result {
                    super::super::types::lower_result_type(
                        result,
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
                        &param.ty,
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
                if let Some(symbol_id) = checker.resolved.definitions.get(&name_key) {
                    let func_ty = ArType::Func(param_types, Box::new(ret_ty));
                    checker.type_info.decl_types.insert(*symbol_id, func_ty);
                    let params = super::super::types::collect_generic_param_symbols(
                        checker,
                        &func_decl.generic_params,
                    );
                    if !params.is_empty() {
                        checker
                            .type_info
                            .generic_params
                            .insert(*symbol_id, params);
                    }
                }
            }
            TopLevelDecl::Extern(extern_decl) => {
                for member in &extern_decl.members {
                    let ret_ty = if let Some(result) = &member.result {
                        super::super::types::lower_result_type(
                            result,
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
                            &param.ty,
                            &checker.symbols,
                            checker.symbols.global_scope(),
                            &checker.resolved,
                        );
                        param_types.push(param_ty);
                    }

                    let name_key = crate::NodeKey::from(member.span);
                    if let Some(symbol_id) = checker.resolved.definitions.get(&name_key) {
                        let func_ty = ArType::Func(param_types, Box::new(ret_ty));
                        checker.type_info.decl_types.insert(*symbol_id, func_ty);
                    }
                }
            }
            _ => {}
        }
    }
}
