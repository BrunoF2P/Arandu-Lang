use super::primitive::Primitive;
use crate::{SymbolId, SymbolTable};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ArType {
    /// Primitive types: int, float, bool, str, ...
    Primitive(Primitive),

    /// Named type with optional generic arguments: User, List<int>
    Named(SymbolId, Vec<ArType>),

    /// Function type: func(int, str) bool
    Func(Vec<ArType>, Box<ArType>),

    /// Nullable wrapper: str?
    Nullable(Box<ArType>),

    /// Slice type: []int
    Slice(Box<ArType>),

    /// Fixed-size array: [4]float
    Array(u64, Box<ArType>),

    /// Pointer type: ptr[Vec2]
    Ptr(Box<ArType>),

    /// Multi-value tuple (non-`Result` returns only)
    Tuple(Vec<ArType>),

    /// `Result<T, E>` — canonical success/error type
    Result(Box<ArType>, Box<ArType>),

    /// `Option<T>` — optional value (`T?` is `Nullable`, not `Option`)
    Option(Box<ArType>),

    /// The `Err` type from the grammar
    Err,

    /// Functions with no return type
    Void,

    /// An integer literal that hasn't been assigned a concrete type yet.
    /// Absorbs the context type during check mode.
    IntLiteral,

    /// A float literal that hasn't been assigned a concrete type yet.
    /// Absorbs any float context (f32, f64, float).
    FloatLiteral,

    /// Poison type — operations on Error never produce new errors.
    /// This is the key to preventing cascading error messages.
    Error,
}

impl ArType {
    /// Returns true if this type is the poison type.
    #[must_use]
    pub fn is_error(&self) -> bool {
        matches!(self, ArType::Error)
    }

    /// Returns true for any numeric primitive type.
    #[must_use]
    pub fn is_numeric(&self) -> bool {
        match self {
            ArType::Primitive(p) => p.is_numeric(),
            ArType::IntLiteral | ArType::FloatLiteral => true,
            _ => false,
        }
    }

    /// Returns true for integer primitives.
    #[must_use]
    pub fn is_integer(&self) -> bool {
        match self {
            ArType::Primitive(p) => p.is_integer(),
            ArType::IntLiteral => true,
            _ => false,
        }
    }

    /// Returns true for float primitives.
    #[must_use]
    pub fn is_float(&self) -> bool {
        match self {
            ArType::Primitive(p) => p.is_float(),
            ArType::FloatLiteral => true,
            _ => false,
        }
    }

    /// Returns true if this is an unresolved literal type.
    #[must_use]
    pub fn is_literal(&self) -> bool {
        matches!(self, ArType::IntLiteral | ArType::FloatLiteral)
    }

    /// Produce a human-readable name for this type.
    #[must_use]
    pub fn display(&self, symbols: &SymbolTable) -> String {
        match self {
            ArType::Primitive(p) => p.to_string(),
            ArType::Named(id, args) => {
                let name = &symbols.get(*id).name;
                if args.is_empty() {
                    name.clone()
                } else {
                    let args_str: Vec<String> = args.iter().map(|a| a.display(symbols)).collect();
                    format!("{}<{}>", name, args_str.join(", "))
                }
            }
            ArType::Func(params, ret) => {
                let params_str: Vec<String> = params.iter().map(|p| p.display(symbols)).collect();
                let ret_str = ret.display(symbols);
                if **ret == ArType::Void {
                    format!("func({})", params_str.join(", "))
                } else {
                    format!("func({}) {}", params_str.join(", "), ret_str)
                }
            }
            ArType::Nullable(inner) => format!("{}?", inner.display(symbols)),
            ArType::Slice(inner) => format!("[]{}", inner.display(symbols)),
            ArType::Array(size, inner) => {
                format!("[{}]{}", size, inner.display(symbols))
            }
            ArType::Ptr(inner) => format!("ptr[{}]", inner.display(symbols)),
            ArType::Tuple(types) => {
                let parts: Vec<String> = types.iter().map(|t| t.display(symbols)).collect();
                format!("({})", parts.join(", "))
            }
            ArType::Result(ok, err) => {
                format!("Result<{}, {}>", ok.display(symbols), err.display(symbols))
            }
            ArType::Option(inner) => format!("Option<{}>", inner.display(symbols)),
            ArType::Err => "Err".to_string(),
            ArType::Void => "void".to_string(),
            ArType::IntLiteral => "int".to_string(),
            ArType::FloatLiteral => "float".to_string(),
            ArType::Error => "<error>".to_string(),
        }
    }

    /// Resolve a literal type to a concrete type. If the type is a literal
    /// and no context is available, defaults to `int` or `float`.
    #[must_use]
    pub fn default_literal(self) -> ArType {
        match self {
            ArType::IntLiteral => ArType::Primitive(Primitive::Int),
            ArType::FloatLiteral => ArType::Primitive(Primitive::Float),
            other => other,
        }
    }

    /// Check if this literal type can absorb the given target type.
    /// Used for literal context-absorption (Swift-style).
    #[must_use]
    pub fn literal_absorbs(&self, target: &ArType) -> bool {
        match (self, target) {
            // IntLiteral absorbs any numeric type
            (ArType::IntLiteral, ArType::Primitive(p)) => p.is_numeric(),
            // FloatLiteral absorbs any float type
            (ArType::FloatLiteral, ArType::Primitive(p)) => p.is_float(),
            _ => false,
        }
    }
}
