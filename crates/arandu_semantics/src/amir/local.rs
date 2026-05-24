use crate::SymbolId;
use crate::hir::ReceiverKind;
use crate::newtype_index;
use crate::passes::type_checker::types::ArType;

newtype_index!(LocalId);
newtype_index!(TempId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AmirReceiver {
    pub temp: TempId,
    pub kind: ReceiverKind,
}

#[derive(Debug, Clone)]
pub struct AmirLocal {
    pub id: LocalId,
    pub ty: ArType,
    pub symbol: Option<SymbolId>,
}

#[derive(Debug, Clone)]
pub struct AmirTemp {
    pub id: TempId,
    pub ty: ArType,
}
