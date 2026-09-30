//! Narrow CTFE lowering/evaluation queries. General runtime `func_amir` remains
//! a projection of whole-program lowering; this path admits scalar, non-generic
//! functions only and never calls that final lowering query.

use std::sync::Arc;

use crate::db::HashEq;
use crate::{ArandCompilerDb, SourceFile};
use arandu_middle::ctfe::ConstValue;
use arandu_middle::{Diagnostic, SymbolId};
use arandu_mir::ctfe::{Budget, CtfeFunction, EvalError, EvalErrorKind, FunctionProvider};

#[derive(Debug, Clone)]
pub enum BuildFailure {
    MissingFunction,
    ImportsNotStaged,
    GenericFunction,
    Diagnostics(Vec<Diagnostic>),
    Evaluation(EvalErrorKind),
}

#[derive(Debug, Clone)]
pub struct CtfeLowering {
    pub result: Result<Arc<CtfeFunction>, BuildFailure>,
}

#[salsa::tracked]
#[tracing::instrument(level = "trace", target = "arandu_query", skip(db), fields(
    query = "ctfe_func_amir", file = ?file.file_id(db), func = ?symbol,
))]
pub fn ctfe_func_amir(
    db: &dyn ArandCompilerDb,
    file: SourceFile,
    symbol: SymbolId,
) -> HashEq<CtfeLowering> {
    if symbol.file_id != *file.file_id(db) {
        return HashEq::new(CtfeLowering {
            result: Err(BuildFailure::MissingFunction),
        });
    }
    let body = crate::passes::item_source_input(db, file, symbol);
    // Imported signatures currently obtain borrow interfaces through final,
    // program-wide lowering. Do not smuggle that dependency into CTFE staging.
    if !body.program.imports.is_empty() {
        return HashEq::new(CtfeLowering {
            result: Err(BuildFailure::ImportsNotStaged),
        });
    }
    let layout = *db.target_config().data_layout(db);
    if let Err(error) =
        arandu_middle::ctfe::IntegerType::new(arandu_middle::types::Primitive::USize, layout)
    {
        return HashEq::new(CtfeLowering {
            result: Err(BuildFailure::Evaluation(EvalErrorKind::Value(error))),
        });
    }
    let item = crate::passes::item_body_typeck(db, file, symbol);
    let signatures = crate::passes::module_signatures(db, file);
    let build = || -> Result<Arc<CtfeFunction>, BuildFailure> {
        if item.type_info.generic_params.contains_key(&symbol) {
            return Err(BuildFailure::GenericFunction);
        }
        // Arc-backed fields are shared; only the selected body contributes to
        // HIR/AMIR. There is deliberately no monomorphizer or imported-body walk.
        let mut checked = (**item).clone();
        let mut span = None;
        body.program.for_each_decl_recursive(|_, declaration| {
            if arandu_semantics::primary_def_key(declaration)
                .and_then(|key| checked.resolved.definitions.get(&key))
                == Some(&symbol)
            {
                span = Some(arandu_semantics::item_source_span(declaration));
            }
        });
        let span = span.ok_or(BuildFailure::MissingFunction)?;
        // Resolution diagnostics include all bodies; do not reject this unit
        // because a sibling has an unresolved name. This evaluation is not a
        // certificate that the rest of the file is a valid runtime program.
        checked.diagnostics.extend(
            signatures
                .diagnostics
                .iter()
                .filter(|diagnostic| {
                    diagnostic.span.file_id == span.file_id
                        && diagnostic.span.start >= span.start
                        && diagnostic.span.end <= span.end
                })
                .cloned(),
        );
        let hir = arandu_semantics::lower_function_to_hir(&mut checked, &body.program, symbol)
            .map_err(BuildFailure::Diagnostics)?
            .ok_or(BuildFailure::MissingFunction)?;
        db.unwind_if_revision_cancelled();
        let (mut program, diagnostics) =
            arandu_mir::lower_to_amir_with_interfaces(&mut checked, &hir, layout.pointer_width())
                .map_err(BuildFailure::Diagnostics)?;
        if diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == arandu_middle::Severity::Error)
        {
            return Err(BuildFailure::Diagnostics(diagnostics));
        }
        let function = program.funcs.pop().ok_or(BuildFailure::MissingFunction)?;
        if !program.funcs.is_empty() || function.symbol != symbol {
            return Err(BuildFailure::MissingFunction);
        }
        CtfeFunction::new(
            function,
            program.literal_pool,
            &checked.type_info.type_interner,
            layout,
        )
        .map(Arc::new)
        .map_err(BuildFailure::Evaluation)
    };
    HashEq::new(CtfeLowering { result: build() })
}

/// Arguments and policy are part of query identity, not mutable evaluator state.
#[salsa::interned]
pub struct CtfeRequest<'db> {
    pub file: SourceFile,
    pub symbol: SymbolId,
    #[returns(ref)]
    pub arguments: Vec<ConstValue>,
    pub budget: Budget,
}

struct QueryProvider<'a> {
    db: &'a dyn ArandCompilerDb,
}

impl FunctionProvider for QueryProvider<'_> {
    fn function(&self, symbol: SymbolId) -> Result<Arc<CtfeFunction>, EvalErrorKind> {
        self.db.unwind_if_revision_cancelled();
        let file = self
            .db
            .source_file_by_id(symbol.file_id)
            .ok_or(EvalErrorKind::MissingFunction(symbol))?;
        let lowered = ctfe_func_amir(self.db, file, symbol);
        match &lowered.result {
            Ok(unit) => Ok(Arc::clone(unit)),
            Err(_) => Err(EvalErrorKind::UnavailableFunction(symbol)),
        }
    }
}

#[salsa::tracked]
#[tracing::instrument(
    level = "trace",
    target = "arandu_query",
    skip(db, request),
    fields(query = "ctfe_eval")
)]
pub fn ctfe_eval<'db>(
    db: &'db dyn ArandCompilerDb,
    request: CtfeRequest<'db>,
) -> Result<ConstValue, EvalError> {
    // Anchor the source identity even if a stale symbol points elsewhere.
    let file = request.file(db);
    let symbol = *request.symbol(db);
    if symbol.file_id != *file.file_id(db) {
        return Err(EvalError {
            kind: EvalErrorKind::MissingFunction(symbol),
            function: symbol,
            block: arandu_middle::amir::BlockId(0),
            span: arandu_middle::Span::new(*file.file_id(db), 0, 0),
        });
    }
    arandu_mir::ctfe::evaluate(
        &QueryProvider { db },
        symbol,
        request.arguments(db),
        *request.budget(db),
        || {
            // Salsa cancellation unwinds the query, so it cannot be cached as a
            // normal CTFE error. Pure VM callers can instead supply a boolean hook.
            db.unwind_if_revision_cancelled();
            false
        },
    )
}
