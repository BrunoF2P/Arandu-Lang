use crate::index_vec::IndexVec;

crate::newtype_index!(HirExprId);
crate::newtype_index!(HirStmtId);
crate::newtype_index!(HirBlockId);

/// A compact, index-backed storage for HIR nodes. This is an initial
/// skeleton to begin the A5 migration; more helpers (reserve ranges,
/// contiguous allocation, serialization) will be added in follow-ups.
#[derive(Debug, Default)]
pub struct HirPool {
    pub exprs: IndexVec<HirExprId, super::HirExpr>,
    pub stmts: IndexVec<HirStmtId, super::HirStmt>,
    pub blocks: IndexVec<HirBlockId, super::HirBlock>,
}

impl HirPool {
    #[must_use]
    pub fn new() -> Self {
        Self {
            exprs: IndexVec::new(),
            stmts: IndexVec::new(),
            blocks: IndexVec::new(),
        }
    }

    pub fn alloc_expr(&mut self, expr: super::HirExpr) -> HirExprId {
        self.exprs.push(expr)
    }

    #[must_use]
    pub fn expr(&self, id: HirExprId) -> &super::HirExpr {
        self.exprs.get(id).expect("invalid HirExprId")
    }

    pub fn expr_mut(&mut self, id: HirExprId) -> &mut super::HirExpr {
        self.exprs.get_mut(id).expect("invalid HirExprId")
    }

    pub fn alloc_stmt(&mut self, stmt: super::HirStmt) -> HirStmtId {
        self.stmts.push(stmt)
    }

    #[must_use]
    pub fn stmt(&self, id: HirStmtId) -> &super::HirStmt {
        self.stmts.get(id).expect("invalid HirStmtId")
    }

    pub fn alloc_block(&mut self, block: super::HirBlock) -> HirBlockId {
        self.blocks.push(block)
    }

    /// Allocate a sequence of expressions and return their generated IDs.
    pub fn alloc_expr_list<I>(&mut self, iter: I) -> Vec<HirExprId>
    where
        I: IntoIterator<Item = super::HirExpr>,
    {
        iter.into_iter().map(|e| self.alloc_expr(e)).collect()
    }

    /// Allocate a sequence of statements and return their generated IDs.
    pub fn alloc_stmt_list<I>(&mut self, iter: I) -> Vec<HirStmtId>
    where
        I: IntoIterator<Item = super::HirStmt>,
    {
        iter.into_iter().map(|s| self.alloc_stmt(s)).collect()
    }

    /// Iterate over all expressions with their IDs.
    pub fn exprs_iter(&self) -> impl Iterator<Item = (HirExprId, &super::HirExpr)> {
        self.exprs
            .raw
            .iter()
            .enumerate()
            .map(|(i, v)| (HirExprId::from_usize(i), v))
    }

    /// Iterate over all statements with their IDs.
    pub fn stmts_iter(&self) -> impl Iterator<Item = (HirStmtId, &super::HirStmt)> {
        self.stmts
            .raw
            .iter()
            .enumerate()
            .map(|(i, v)| (HirStmtId::from_usize(i), v))
    }

    /// Iterate over all blocks with their IDs.
    pub fn blocks_iter(&self) -> impl Iterator<Item = (HirBlockId, &super::HirBlock)> {
        self.blocks
            .raw
            .iter()
            .enumerate()
            .map(|(i, v)| (HirBlockId::from_usize(i), v))
    }

    #[must_use]
    pub fn block(&self, id: HirBlockId) -> &super::HirBlock {
        self.blocks.get(id).expect("invalid HirBlockId")
    }
}
