use crate::TypeCheckResult;
use crate::diagnostics::Diagnostic;
use crate::hir::{
    HirConst, HirDecl, HirEnum, HirEnumVariant, HirExtern, HirFunc, HirFuncSignature, HirInterface,
    HirParam, HirStruct, HirStructField, HirTypeAlias,
};
use crate::passes::lowering::require_def_symbol;
use crate::passes::type_checker::types::ArType;
use arandu_parser::TopLevelDecl;
use arandu_parser::ast_pool::AstPool;

pub(crate) fn lower_decl(
    type_check: &TypeCheckResult,
    pool: &AstPool,
    hir_pool: &mut crate::hir::HirPool,
    decl: &TopLevelDecl,
) -> Result<HirDecl, Diagnostic> {
    match decl {
        TopLevelDecl::Const(d) => {
            let symbol = require_def_symbol(&type_check.resolved, d.span)?;
            let ty = type_check
                .type_info
                .decl_type(symbol)
                .cloned()
                .unwrap_or(ArType::Error);
            let value_vid = super::expr::lower_expr(type_check, pool, hir_pool, d.value)?;
            Ok(HirDecl::Const(HirConst {
                symbol,
                ty,
                value: value_vid,
                span: d.span,
            }))
        }
        TopLevelDecl::TypeAlias(d) => {
            let symbol = require_def_symbol(&type_check.resolved, d.span)?;
            let target = type_check
                .type_info
                .decl_type(symbol)
                .cloned()
                .unwrap_or(ArType::Error);
            Ok(HirDecl::TypeAlias(HirTypeAlias {
                symbol,
                target,
                span: d.span,
            }))
        }
        TopLevelDecl::Func(d) => {
            let name_span = match &d.name {
                arandu_parser::FuncName::Free { span, .. } => *span,
                arandu_parser::FuncName::Method { span, .. } => *span,
            };
            let symbol = require_def_symbol(&type_check.resolved, name_span)?;
            let decl_ty = type_check
                .type_info
                .decl_type(symbol)
                .cloned()
                .unwrap_or(ArType::Error);
            let return_type = match decl_ty {
                ArType::Func(_, ret) => {
                    arandu_middle::types::type_interner::with_resolved_type(ret, |t| t.clone())
                }
                other => other,
            };
            let mut params = Vec::new();
            for p in &d.params {
                let p_symbol = require_def_symbol(&type_check.resolved, p.span)?;
                let p_ty = type_check
                    .type_info
                    .decl_type(p_symbol)
                    .cloned()
                    .unwrap_or(ArType::Error);
                params.push(HirParam {
                    symbol: p_symbol,
                    ty: p_ty,
                    span: p.span,
                    is_receiver: p.is_receiver,
                    receiver_kind: p.ownership.map(super::stmt::ownership_to_receiver_kind),
                });
            }
            let params = hir_pool.alloc_param_list(&params);
            Ok(HirDecl::Func(HirFunc {
                symbol,
                params,
                return_type,
                body: Some(super::stmt::lower_block(
                    type_check, pool, hir_pool, &d.body,
                )?),
                span: d.span,
            }))
        }
        TopLevelDecl::Struct(d) => {
            let symbol = require_def_symbol(&type_check.resolved, d.span)?;
            let mut fields = Vec::new();
            if let Some(struct_fields_map) = type_check.type_info.struct_fields.get(&symbol) {
                for f in &d.fields {
                    let field_symbol = require_def_symbol(&type_check.resolved, f.span)?;
                    let field_ty = struct_fields_map
                        .get(&f.name)
                        .cloned()
                        .unwrap_or(ArType::Error);
                    fields.push(HirStructField {
                        symbol: field_symbol,
                        ty: field_ty,
                        span: f.span,
                    });
                }
            }
            let fields = hir_pool.alloc_struct_field_list(&fields);
            Ok(HirDecl::Struct(HirStruct {
                symbol,
                fields,
                span: d.span,
            }))
        }
        TopLevelDecl::Enum(d) => {
            let symbol = require_def_symbol(&type_check.resolved, d.span)?;
            let mut variants = Vec::new();
            for v in &d.variants {
                let v_symbol = require_def_symbol(&type_check.resolved, v.span)?;
                let payload =
                    type_check
                        .type_info
                        .enum_variants
                        .get(&v_symbol)
                        .and_then(|(_, shape)| match shape {
                            crate::passes::type_checker::EnumPayloadShape::Unit => None,
                            crate::passes::type_checker::EnumPayloadShape::Tuple(tys) => {
                                if tys.is_empty() {
                                    None
                                } else if tys.len() == 1 {
                                    Some(tys[0].clone())
                                } else {
                                    let tys_ids = tys
                                        .iter()
                                        .map(|t| {
                                            arandu_middle::types::type_interner::intern_type(
                                                t.clone(),
                                            )
                                        })
                                        .collect();
                                    Some(ArType::Tuple(tys_ids))
                                }
                            }
                        });
                variants.push(HirEnumVariant {
                    symbol: v_symbol,
                    payload,
                    span: v.span,
                });
            }
            let variants = hir_pool.alloc_enum_variant_list(&variants);
            Ok(HirDecl::Enum(HirEnum {
                symbol,
                variants,
                span: d.span,
            }))
        }
        TopLevelDecl::Interface(d) => {
            let symbol = require_def_symbol(&type_check.resolved, d.span)?;
            Ok(HirDecl::Interface(HirInterface {
                symbol,
                span: d.span,
            }))
        }
        TopLevelDecl::Extern(d) => {
            let mut members = Vec::new();
            for m in &d.members {
                let symbol = require_def_symbol(&type_check.resolved, m.span)?;
                let m_ty = type_check
                    .type_info
                    .decl_type(symbol)
                    .cloned()
                    .unwrap_or(ArType::Error);
                let return_type = match m_ty {
                    ArType::Func(_, ret) => {
                        arandu_middle::types::type_interner::with_resolved_type(ret, |t| t.clone())
                    }
                    other => other,
                };
                let mut params = Vec::new();
                for p in &m.params {
                    let p_symbol = require_def_symbol(&type_check.resolved, p.span)?;
                    let p_ty = type_check
                        .type_info
                        .decl_type(p_symbol)
                        .cloned()
                        .unwrap_or(ArType::Error);
                    params.push(HirParam {
                        symbol: p_symbol,
                        ty: p_ty,
                        span: p.span,
                        is_receiver: p.is_receiver,
                        receiver_kind: p.ownership.map(super::stmt::ownership_to_receiver_kind),
                    });
                }
                let params = hir_pool.alloc_param_list(&params);
                members.push(HirFuncSignature {
                    symbol,
                    params,
                    return_type,
                    span: m.span,
                });
            }
            let members = hir_pool.alloc_func_signature_list(&members);
            Ok(HirDecl::Extern(HirExtern {
                abi: d.abi.clone(),
                members,
                span: d.span,
            }))
        }
        TopLevelDecl::Error(_) => unreachable!("syntax error in HIR lowering"),
    }
}
