use super::block::AmirBasicBlock;
use super::local::{AmirLocal, AmirReceiver, AmirTemp, TempId};
use crate::SymbolId;
use crate::literal_pool::AmirLiteralPool;
use crate::passes::type_checker::types::ArType;

#[derive(Debug)]
pub struct AmirProgram {
    pub funcs: Vec<AmirFunc>,
    pub literal_pool: AmirLiteralPool,
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
}
