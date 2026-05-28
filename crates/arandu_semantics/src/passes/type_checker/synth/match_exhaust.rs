use std::collections::HashSet;

use arandu_lexer::Span;
use arandu_parser::{MatchArm, Pattern};

use super::super::TypeChecker;
use super::super::types::ArType;

fn pattern_covers_all(pat: &Pattern) -> bool {
    matches!(pat, Pattern::Wildcard { .. } | Pattern::Bind { .. })
}

fn variant_short_name(enum_name: &str, symbol_name: &str) -> String {
    symbol_name
        .strip_prefix(&format!("{enum_name}."))
        .unwrap_or(symbol_name)
        .to_string()
}

fn pattern_variant_short_name(pat: &Pattern) -> Option<String> {
    match pat {
        Pattern::Enum { variant, .. } => Some(variant.clone()),
        Pattern::TypeTuple { name, .. } => Some(
            name.rsplit_once('.')
                .map(|(_, short)| short.to_string())
                .unwrap_or_else(|| name.clone()),
        ),
        _ => None,
    }
}

fn enum_variant_short_names(
    checker: &TypeChecker<'_>,
    enum_id: crate::SymbolId,
) -> HashSet<String> {
    let enum_name = checker.symbols.get(enum_id).name.clone();
    let mut names = HashSet::new();
    let mut seen = HashSet::new();
    for (variant_id, (parent_enum, _)) in &checker.type_info.enum_variants {
        if *parent_enum != enum_id || !seen.insert(*variant_id) {
            continue;
        }
        names.insert(variant_short_name(
            &enum_name,
            &checker.symbols.get(*variant_id).name,
        ));
    }
    names
}

pub fn check_match_exhaustiveness(
    checker: &mut TypeChecker<'_>,
    value_ty: &ArType,
    arms: &[MatchArm],
    match_span: Span,
) {
    let ArType::Named(enum_id, _) = value_ty else {
        return;
    };
    if value_ty.is_error() {
        return;
    }

    let all_variants = enum_variant_short_names(checker, *enum_id);
    if all_variants.is_empty() {
        return;
    }

    if arms.iter().any(|arm| pattern_covers_all(&arm.pattern)) {
        return;
    }

    let mut covered = HashSet::new();
    for arm in arms {
        if let Some(name) = pattern_variant_short_name(&arm.pattern) {
            covered.insert(name);
        }
    }

    let mut missing: Vec<_> = all_variants.difference(&covered).cloned().collect();
    if missing.is_empty() {
        return;
    }
    missing.sort();

    checker.diagnostics.push(crate::Diagnostic::error(
        crate::DiagCode::T024NonExhaustiveMatch,
        format!(
            "non-exhaustive match: missing variant(s): {}",
            missing.join(", ")
        ),
        match_span,
    ));
}
