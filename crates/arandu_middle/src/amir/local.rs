#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Primitive;

    #[test]
    fn local_id_roundtrip() {
        let id = LocalId::from_usize(42);
        assert_eq!(id.as_usize(), 42);
    }

    #[test]
    fn temp_id_roundtrip() {
        let id = TempId::from_usize(7);
        assert_eq!(id.as_usize(), 7);
    }

    #[test]
    fn amir_local_construction() {
        let local = AmirLocal {
            id: LocalId::from_usize(0),
            ty: ArType::Primitive(Primitive::Int),
            symbol: Some(SymbolId::new(0, 1)),
            span: Span::new(0, 0, 0),
            use_span: None,
        };
        assert_eq!(local.id.as_usize(), 0);
        assert!(local.use_span.is_none());
    }

    #[test]
    fn amir_temp_construction() {
        let temp = AmirTemp {
            id: TempId::from_usize(3),
            ty: ArType::Primitive(Primitive::Bool),
            span: Span::new(0, 0, 0),
        };
        assert_eq!(temp.id.as_usize(), 3);
    }

    #[test]
    fn amir_receiver_construction() {
        let recv = AmirReceiver {
            temp: TempId::from_usize(0),
            kind: ReceiverKind::Shared,
        };
        assert_eq!(recv.temp.as_usize(), 0);
        assert_eq!(recv.kind, ReceiverKind::Shared);
    }
}

use crate::SymbolId;
use crate::hir::ReceiverKind;
use crate::newtype_index;
use crate::types::ArType;
use arandu_lexer::Span;

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
    pub span: Span,
    pub use_span: Option<Span>,
}

#[derive(Debug, Clone)]
pub struct AmirTemp {
    pub id: TempId,
    pub ty: ArType,
    pub span: Span,
}
