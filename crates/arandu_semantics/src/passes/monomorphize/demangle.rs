use arandu_middle::symbol_table::SymbolTable;
use arandu_middle::types::{ArType, TypeInterner};

use super::graph::InstantiationKey;

pub fn mangle_symbol(
    key: &InstantiationKey,
    interner: &TypeInterner,
    symbols: &SymbolTable,
) -> String {
    let name = &symbols.get(key.symbol).name;
    let mut mangled = format!("_A${name}$I");
    for (i, &tid) in key.type_args.iter().enumerate() {
        if i > 0 {
            mangled.push('_');
        }
        mangled.push('_');
        mangle_type_into(&mut mangled, interner.resolve(tid), symbols);
    }
    mangled.push_str("_$E");
    mangled
}

#[must_use]
pub fn demangle_symbol(mangled: &str) -> Option<String> {
    let inner = mangled.strip_prefix("_A$")?.strip_suffix("$E")?;
    let (name, rest) = inner.split_once("$I")?;
    let types_part = rest.trim_matches('_');
    if types_part.is_empty() {
        Some(name.to_string())
    } else {
        let types: Vec<&str> = types_part.split('_').filter(|s| !s.is_empty()).collect();
        Some(format!("{}<{}>", name, types.join(", ")))
    }
}

fn mangle_type_into(out: &mut String, ty: &ArType, symbols: &SymbolTable) {
    match ty {
        ArType::Primitive(p) => out.push_str(p.as_str()),
        ArType::Named(id, args) => {
            out.push_str(&symbols.get(*id).name);
            for &arg in args {
                out.push('_');
                arandu_middle::types::type_interner::with_resolved_type(arg, |arg_ty| {
                    mangle_type_into(out, arg_ty, symbols);
                });
            }
        }
        ArType::Nullable(inner) => {
            out.push_str("opt_");
            arandu_middle::types::type_interner::with_resolved_type(*inner, |inner_ty| {
                mangle_type_into(out, inner_ty, symbols);
            });
        }
        ArType::Ptr(inner) => {
            out.push_str("ptr_");
            arandu_middle::types::type_interner::with_resolved_type(*inner, |inner_ty| {
                mangle_type_into(out, inner_ty, symbols);
            });
        }
        ArType::Slice(inner) => {
            out.push_str("slice_");
            arandu_middle::types::type_interner::with_resolved_type(*inner, |inner_ty| {
                mangle_type_into(out, inner_ty, symbols);
            });
        }
        ArType::Array(n, inner) => {
            out.push_str(&format!("arr{n}_"));
            arandu_middle::types::type_interner::with_resolved_type(*inner, |inner_ty| {
                mangle_type_into(out, inner_ty, symbols);
            });
        }
        ArType::Tuple(items) => {
            out.push_str("tup");
            for &item in items {
                out.push('_');
                arandu_middle::types::type_interner::with_resolved_type(item, |item_ty| {
                    mangle_type_into(out, item_ty, symbols);
                });
            }
        }
        ArType::Func(params, ret) => {
            out.push_str("fn");
            for &param in params {
                out.push('_');
                arandu_middle::types::type_interner::with_resolved_type(param, |param_ty| {
                    mangle_type_into(out, param_ty, symbols);
                });
            }
            out.push_str("_R_");
            arandu_middle::types::type_interner::with_resolved_type(*ret, |ret_ty| {
                mangle_type_into(out, ret_ty, symbols);
            });
        }
        ArType::Result(ok, err) => {
            out.push_str("res_");
            arandu_middle::types::type_interner::with_resolved_type(*ok, |ok_ty| {
                mangle_type_into(out, ok_ty, symbols);
            });
            out.push('_');
            arandu_middle::types::type_interner::with_resolved_type(*err, |err_ty| {
                mangle_type_into(out, err_ty, symbols);
            });
        }
        ArType::Option(inner) => {
            out.push_str("option_");
            arandu_middle::types::type_interner::with_resolved_type(*inner, |inner_ty| {
                mangle_type_into(out, inner_ty, symbols);
            });
        }
        ArType::Coroutine(inner) => {
            out.push_str("coro_");
            arandu_middle::types::type_interner::with_resolved_type(*inner, |inner_ty| {
                mangle_type_into(out, inner_ty, symbols);
            });
        }
        ArType::Range(inner) => {
            out.push_str("range_");
            arandu_middle::types::type_interner::with_resolved_type(*inner, |inner_ty| {
                mangle_type_into(out, inner_ty, symbols);
            });
        }
        ArType::Void => out.push_str("void"),
        ArType::Err => out.push_str("err"),
        ArType::IntLiteral => out.push_str("int"),
        ArType::FloatLiteral => out.push_str("float"),
        ArType::Error => out.push_str("error"),
    }
}
