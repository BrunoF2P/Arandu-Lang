use std::fmt;

// ── Primitive types ──────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Primitive {
    Int,
    Uint,
    Float,
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
    Bool,
    Byte,
    Char,
    Str,
    Any,
}

impl Primitive {
    #[must_use]
    pub fn is_numeric(self) -> bool {
        matches!(
            self,
            Primitive::Int
                | Primitive::Uint
                | Primitive::Float
                | Primitive::I8
                | Primitive::I16
                | Primitive::I32
                | Primitive::I64
                | Primitive::U8
                | Primitive::U16
                | Primitive::U32
                | Primitive::U64
                | Primitive::F32
                | Primitive::F64
                | Primitive::Byte
        )
    }

    #[must_use]
    pub fn is_integer(self) -> bool {
        matches!(
            self,
            Primitive::Int
                | Primitive::Uint
                | Primitive::I8
                | Primitive::I16
                | Primitive::I32
                | Primitive::I64
                | Primitive::U8
                | Primitive::U16
                | Primitive::U32
                | Primitive::U64
                | Primitive::Byte
        )
    }

    #[must_use]
    pub fn is_float(self) -> bool {
        matches!(self, Primitive::Float | Primitive::F32 | Primitive::F64)
    }

    #[must_use]
    pub fn is_signed(self) -> bool {
        matches!(
            self,
            Primitive::Int
                | Primitive::I8
                | Primitive::I16
                | Primitive::I32
                | Primitive::I64
                | Primitive::Float
                | Primitive::F32
                | Primitive::F64
        )
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Primitive::Int => "int",
            Primitive::Uint => "uint",
            Primitive::Float => "float",
            Primitive::I8 => "i8",
            Primitive::I16 => "i16",
            Primitive::I32 => "i32",
            Primitive::I64 => "i64",
            Primitive::U8 => "u8",
            Primitive::U16 => "u16",
            Primitive::U32 => "u32",
            Primitive::U64 => "u64",
            Primitive::F32 => "f32",
            Primitive::F64 => "f64",
            Primitive::Bool => "bool",
            Primitive::Byte => "byte",
            Primitive::Char => "char",
            Primitive::Str => "str",
            Primitive::Any => "any",
        }
    }

    /// Parse a primitive type name from an AST `TypeExpr::Primitive`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Primitive> {
        match name {
            "int" => Some(Primitive::Int),
            "uint" => Some(Primitive::Uint),
            "float" => Some(Primitive::Float),
            "i8" => Some(Primitive::I8),
            "i16" => Some(Primitive::I16),
            "i32" => Some(Primitive::I32),
            "i64" => Some(Primitive::I64),
            "u8" => Some(Primitive::U8),
            "u16" => Some(Primitive::U16),
            "u32" => Some(Primitive::U32),
            "u64" => Some(Primitive::U64),
            "f32" => Some(Primitive::F32),
            "f64" => Some(Primitive::F64),
            "bool" => Some(Primitive::Bool),
            "byte" => Some(Primitive::Byte),
            "char" => Some(Primitive::Char),
            "str" => Some(Primitive::Str),
            "any" => Some(Primitive::Any),
            _ => None,
        }
    }
}

impl fmt::Display for Primitive {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
