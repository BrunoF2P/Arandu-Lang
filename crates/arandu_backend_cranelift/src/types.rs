use arandu_semantics::passes::type_checker::types::ArType;
use arandu_semantics::passes::type_checker::types::Primitive;
use cranelift_codegen::ir::Type;
use cranelift_codegen::ir::types::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClifType {
    Concrete(Type),
    Void,
}

impl ClifType {
    #[must_use]
    pub fn concrete(self) -> Option<Type> {
        match self {
            Self::Concrete(ty) => Some(ty),
            Self::Void => None,
        }
    }
}

#[must_use]
pub fn ar_type_is_unsigned_integer(ty: &ArType) -> bool {
    matches!(
        ty,
        ArType::Primitive(p) if matches!(
            p,
            Primitive::Uint
                | Primitive::U8
                | Primitive::U16
                | Primitive::U32
                | Primitive::U64
                | Primitive::Byte
        )
    )
}

#[must_use]
pub fn clif_type(ty: &ArType, ptr_type: Type) -> ClifType {
    match ty {
        ArType::Primitive(p) => match p {
            Primitive::Int | Primitive::Uint => ClifType::Concrete(I32),
            Primitive::Float => ClifType::Concrete(F64),
            Primitive::I8 | Primitive::U8 | Primitive::Byte => ClifType::Concrete(I8),
            Primitive::I16 | Primitive::U16 => ClifType::Concrete(I16),
            Primitive::I32 | Primitive::U32 => ClifType::Concrete(I32),
            Primitive::I64 | Primitive::U64 => ClifType::Concrete(I64),
            Primitive::F32 => ClifType::Concrete(F32),
            Primitive::F64 => ClifType::Concrete(F64),
            Primitive::Bool => ClifType::Concrete(I8),
            Primitive::Char => ClifType::Concrete(I32),
            Primitive::Str => {
                // TODO(SL_C): Str é fat pointer (ptr, len).
                // Por ora mapeado como I64 (só ptr). Vai quebrar quando
                // funções que retornam/recebem str forem implementadas.
                ClifType::Concrete(ptr_type)
            }
            Primitive::Any => ClifType::Concrete(ptr_type),
        },
        ArType::Ptr(_) | ArType::Nullable(_) | ArType::Slice(_) | ArType::Array(_, _) => {
            ClifType::Concrete(ptr_type)
        }
        ArType::Void | ArType::Err | ArType::Error => ClifType::Void,
        ArType::IntLiteral => ClifType::Concrete(I32),
        ArType::FloatLiteral => ClifType::Concrete(F64),
        ArType::Named(_, _) => {
            // Structs are passed by pointer or custom ABI representation, for now pointer.
            ClifType::Concrete(ptr_type)
        }
        ArType::Func(_, _) => ClifType::Concrete(ptr_type),
        ArType::Tuple(_)
        | ArType::Result(_, _)
        | ArType::Option(_)
        | ArType::Coroutine(_)
        | ArType::Range(_) => {
            // Composite types map to pointers for JIT passing.
            ClifType::Concrete(ptr_type)
        }
    }
}
