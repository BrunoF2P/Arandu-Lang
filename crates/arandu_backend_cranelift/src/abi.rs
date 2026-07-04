//! ABI helpers for the Cranelift backend.
//!
//! Utilities for mapping Arandu types to Cranelift calling conventions and
//! building [`Signature`]s used when declaring and calling functions.

use crate::types::clif_types;
use arandu_semantics::passes::type_checker::types::ArType;
use cranelift_codegen::ir::{AbiParam, Signature, Type};
use cranelift_codegen::isa::CallConv;

/// Returns the appropriate Cranelift [`CallConv`] for the given target triple.
///
/// Uses `WindowsFastcall` on Windows and `SystemV` on all other platforms.
#[must_use]
pub fn call_conv_for_target(triple: &target_lexicon::Triple) -> CallConv {
    match triple.operating_system {
        target_lexicon::OperatingSystem::Windows => CallConv::WindowsFastcall,
        _ => CallConv::SystemV,
    }
}

/// Builds a Cranelift [`Signature`] from Arandu parameter and return types.
///
/// Each [`ArType`] is expanded into one or more Cranelift IR types via
/// [`clif_types`] (e.g. `str` expands to `[ptr, i64]`).
#[must_use]
pub fn build_signature(
    params: &[ArType],
    return_type: &ArType,
    call_conv: CallConv,
    ptr_type: Type,
) -> Signature {
    let mut sig = Signature::new(call_conv);
    for param in params {
        for &ty in &clif_types(param, ptr_type) {
            sig.params.push(AbiParam::new(ty));
        }
    }
    for &ty in &clif_types(return_type, ptr_type) {
        sig.returns.push(AbiParam::new(ty));
    }
    sig
}
