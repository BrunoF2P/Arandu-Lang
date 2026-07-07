use crate::{ArandCompilerDb, SourceFile};
use arandu_middle::amir::{AmirFunc, BlockId};
use arandu_middle::SymbolId;
// Note: We'll need types for DataflowFacts and LivenessMap when we port the full move_checker logic
// For now, these are placeholder structs to establish the Salsa query shapes as defined in RFC A1.
use crate::db::HashEq;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataflowFacts {
    // Bitsets for definitely-initialized, moved, etc.
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LivenessMap {
    // Map of TempId/LocalId to their live ranges (intervals)
}

#[salsa::tracked]
#[tracing::instrument(level = "trace", target = "arandu_query", skip(db), fields(
    query = "func_amir",
    file = ?file.file_id(db),
    func = ?func_sym,
))]
pub fn func_amir(
    db: &dyn ArandCompilerDb,
    file: SourceFile,
    func_sym: SymbolId,
) -> HashEq<AmirFunc> {
    let amir_program = crate::passes::lower_amir(db, file);
    // Find the requested function in the lowered AMIR.
    let func = amir_program
        .funcs
        .iter()
        .find(|f| f.symbol == func_sym)
        .expect("Function not found in AMIR");
    HashEq::new(func.clone())
}

#[salsa::tracked]
#[tracing::instrument(level = "trace", target = "arandu_query", skip(db), fields(
    query = "block_dataflow_facts",
    file = ?file.file_id(db),
    func = ?func_sym,
    block = ?block,
))]
pub fn block_dataflow_facts(
    db: &dyn ArandCompilerDb,
    file: SourceFile,
    func_sym: SymbolId,
    block: BlockId,
) -> HashEq<DataflowFacts> {
    let _func = func_amir(db, file, func_sym);
    // placeholder logic
    HashEq::new(DataflowFacts {})
}

#[salsa::tracked]
#[tracing::instrument(level = "trace", target = "arandu_query", skip(db), fields(
    query = "liveness_facts",
    file = ?file.file_id(db),
    func = ?func_sym,
))]
pub fn liveness_facts(
    db: &dyn ArandCompilerDb,
    file: SourceFile,
    func_sym: SymbolId,
) -> HashEq<LivenessMap> {
    let _func = func_amir(db, file, func_sym);
    // compute_liveness_rpo(&func)
    HashEq::new(LivenessMap {})
}
