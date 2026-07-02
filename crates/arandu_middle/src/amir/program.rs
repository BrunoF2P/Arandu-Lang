use super::block::AmirBasicBlock;
use super::local::{AmirLocal, AmirReceiver, AmirTemp, TempId};
use super::stmt::{AmirStmt, AmirStmtTable, InstrId};
use crate::SymbolId;
use crate::cfg::ControlFlowGraph;
use crate::layout::DenseRange;
use crate::literal_pool::AmirLiteralPool;
use crate::types::ArType;

#[derive(Debug)]
pub struct AmirProgram {
    pub funcs: Vec<AmirFunc>,
    pub literal_pool: AmirLiteralPool,
    pub extern_funcs:
        rustc_hash::FxHashMap<crate::SymbolId, (Vec<crate::types::ArType>, crate::types::ArType)>,
}

#[derive(Debug)]
pub struct AmirFunc {
    pub symbol: SymbolId,
    pub return_type: ArType,
    pub receiver: Option<AmirReceiver>,
    pub params: Vec<TempId>,
    pub locals: Vec<AmirLocal>,
    pub temps: Vec<AmirTemp>,
    pub blocks: Vec<AmirBasicBlock>,
    pub stmts: AmirStmtTable,
    pub cfg: ControlFlowGraph,
}

impl AmirFunc {
    #[must_use]
    pub fn block(&self, block: super::block::BlockId) -> &AmirBasicBlock {
        &self.blocks[block.as_usize()]
    }

    pub fn block_mut(&mut self, block: super::block::BlockId) -> &mut AmirBasicBlock {
        &mut self.blocks[block.as_usize()]
    }

    #[must_use]
    pub fn stmt(&self, id: InstrId) -> &AmirStmt {
        self.stmts.get(id).expect("invalid AMIR instruction id")
    }

    pub fn stmt_mut(&mut self, id: InstrId) -> &mut AmirStmt {
        self.stmts.get_mut(id).expect("invalid AMIR instruction id")
    }

    pub fn block_stmt_ids(
        &self,
        block: super::block::BlockId,
    ) -> impl Iterator<Item = InstrId> + '_ {
        self.block(block).statements.iter_ids::<InstrId>()
    }

    pub fn block_stmts(
        &self,
        block: super::block::BlockId,
    ) -> impl Iterator<Item = &AmirStmt> + '_ {
        self.block_stmt_ids(block).map(|id| self.stmt(id))
    }

    #[must_use]
    pub fn successors(&self, block: super::block::BlockId) -> &[super::block::BlockId] {
        &self.cfg.successors[block.as_usize()]
    }

    #[must_use]
    pub fn predecessors(&self, block: super::block::BlockId) -> &[super::block::BlockId] {
        &self.cfg.predecessors[block.as_usize()]
    }

    pub fn append_stmt_to_block(
        &mut self,
        block: super::block::BlockId,
        stmt: AmirStmt,
    ) -> InstrId {
        let id = self.stmts.push(stmt);
        extend_block_range(&mut self.blocks[block.as_usize()].statements, id);
        id
    }
}

pub fn extend_block_range(range: &mut DenseRange, id: InstrId) {
    let idx = id.as_usize();
    if range.is_empty() {
        *range = DenseRange::new(idx, 1);
        return;
    }
    debug_assert_eq!(
        range.end_usize(),
        idx,
        "AMIR block statements must be appended contiguously"
    );
    range.len += 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::amir::{AmirBasicBlock, AmirConstant, AmirOperand, AmirTerminator};
    use crate::types::ArType;

    fn block(id: usize) -> AmirBasicBlock {
        AmirBasicBlock {
            id: super::super::block::BlockId::from_usize(id),
            params: Vec::new(),
            statements: DenseRange::empty(),
            terminator: AmirTerminator::Return,
        }
    }

    fn func() -> AmirFunc {
        AmirFunc {
            symbol: SymbolId(0),
            return_type: ArType::Void,
            receiver: None,
            params: Vec::new(),
            locals: Vec::new(),
            temps: Vec::new(),
            blocks: vec![block(0), block(1)],
            stmts: AmirStmtTable::new(),
            cfg: ControlFlowGraph::default(),
        }
    }

    #[test]
    fn appending_statements_allocates_dense_ids_and_block_ranges() {
        let mut func = func();
        let first = func.append_stmt_to_block(
            super::super::block::BlockId::from_usize(0),
            AmirStmt::Free(AmirOperand::Constant(AmirConstant::Bool(true))),
        );
        let second = func.append_stmt_to_block(
            super::super::block::BlockId::from_usize(0),
            AmirStmt::StorageLive(super::super::local::LocalId::from_usize(0)),
        );
        let third = func.append_stmt_to_block(
            super::super::block::BlockId::from_usize(1),
            AmirStmt::StorageDead(super::super::local::LocalId::from_usize(0)),
        );

        assert_eq!(first, InstrId::from_usize(0));
        assert_eq!(second, InstrId::from_usize(1));
        assert_eq!(third, InstrId::from_usize(2));
        assert_eq!(
            func.block(super::super::block::BlockId::from_usize(0))
                .statements,
            DenseRange::new(0, 2)
        );
        assert_eq!(
            func.block(super::super::block::BlockId::from_usize(1))
                .statements,
            DenseRange::new(2, 1)
        );
        assert_eq!(
            func.block_stmt_ids(super::super::block::BlockId::from_usize(0))
                .collect::<Vec<_>>(),
            vec![InstrId::from_usize(0), InstrId::from_usize(1)]
        );
        assert_eq!(
            func.stmts.kinds.as_slice(),
            &[
                super::super::stmt::AmirStmtKind::Free,
                super::super::stmt::AmirStmtKind::StorageLive,
                super::super::stmt::AmirStmtKind::StorageDead,
            ]
        );
    }
}
