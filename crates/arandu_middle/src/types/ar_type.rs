use super::primitive::Primitive;
use super::type_interner::{TypeId, TypeInterner};
use crate::{SymbolId, SymbolTable};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ArType {
    /// Primitive types: int, float, bool, str, ...\
    Primitive(Primitive),

    /// Named type with optional generic arguments: User, List<int>
    Named(SymbolId, Vec<TypeId>),

    /// Function type: func(int, str) bool
    Func(Vec<TypeId>, TypeId),

    /// Nullable wrapper: str?
    Nullable(TypeId),

    /// Slice type: []int
    Slice(TypeId),

    /// Fixed-size array: [4]float
    Array(u64, TypeId),

    /// Pointer type: ptr[Vec2]
    Ptr(TypeId),

    /// Multi-value tuple (non-`Result` returns only)
    Tuple(Vec<TypeId>),

    /// `Result<T, E>` — canonical success/error type
    Result(TypeId, TypeId),

    /// `Option<T>` — optional value (`T?` is `Nullable`, not `Option`)
    Option(TypeId),

    /// `Coroutine[T]` — coroutine machine state type returning T
    Coroutine(TypeId),

    /// `Range<T>` — representing ranges like 0..n
    Range(TypeId),

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
    pub fn display(&self, symbols: &SymbolTable, interner: &TypeInterner) -> String {
        match self {
            ArType::Primitive(p) => p.to_string(),
            ArType::Named(id, args) => {
                let name = &symbols.get(*id).name;
                if args.is_empty() {
                    name.clone()
                } else {
                    let args_str: Vec<String> = args
                        .iter()
                        .map(|&a| interner.resolve(a).display(symbols, interner))
                        .collect();
                    format!("{}<{}>", name, args_str.join(", "))
                }
            }
            ArType::Func(params, ret) => {
                let params_str: Vec<String> = params
                    .iter()
                    .map(|&p| interner.resolve(p).display(symbols, interner))
                    .collect();
                let ret_ty = interner.resolve(*ret);
                let is_void = matches!(ret_ty, ArType::Void);
                let ret_str = ret_ty.display(symbols, interner);
                if is_void {
                    format!("func({})", params_str.join(", "))
                } else {
                    format!("func({}) {}", params_str.join(", "), ret_str)
                }
            }
            ArType::Nullable(inner) => {
                let inner_str = interner.resolve(*inner).display(symbols, interner);
                format!("{}?", inner_str)
            }
            ArType::Slice(inner) => {
                let inner_str = interner.resolve(*inner).display(symbols, interner);
                format!("[]{}", inner_str)
            }
            ArType::Array(size, inner) => {
                let inner_str = interner.resolve(*inner).display(symbols, interner);
                format!("[{}]{}", size, inner_str)
            }
            ArType::Ptr(inner) => {
                let inner_str = interner.resolve(*inner).display(symbols, interner);
                format!("ptr[{}]", inner_str)
            }
            ArType::Tuple(types) => {
                let parts: Vec<String> = types
                    .iter()
                    .map(|&t| interner.resolve(t).display(symbols, interner))
                    .collect();
                format!("({})", parts.join(", "))
            }
            ArType::Result(ok, err) => {
                let ok_str = interner.resolve(*ok).display(symbols, interner);
                let err_str = interner.resolve(*err).display(symbols, interner);
                format!("Result<{}, {}>", ok_str, err_str)
            }
            ArType::Option(inner) => {
                let inner_str = interner.resolve(*inner).display(symbols, interner);
                format!("Option<{}>", inner_str)
            }
            ArType::Coroutine(inner) => {
                let inner_str = interner.resolve(*inner).display(symbols, interner);
                format!("Coroutine<{}>", inner_str)
            }
            ArType::Range(inner) => {
                let inner_str = interner.resolve(*inner).display(symbols, interner);
                format!("Range<{}>", inner_str)
            }
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

    #[must_use]
    pub fn is_copy_v01(&self) -> bool {
        match self {
            ArType::Primitive(p) => {
                p.is_numeric()
                    || matches!(
                        p,
                        Primitive::Bool | Primitive::Char | Primitive::Byte | Primitive::Any
                    )
            }
            ArType::IntLiteral | ArType::FloatLiteral | ArType::Ptr(_) | ArType::Nullable(_) => {
                true
            }
            ArType::Error | ArType::Void | ArType::Err => true,
            ArType::Named(_, _)
            | ArType::Func(_, _)
            | ArType::Slice(_)
            | ArType::Array(_, _)
            | ArType::Tuple(_)
            | ArType::Result(_, _)
            | ArType::Option(_)
            | ArType::Coroutine(_)
            | ArType::Range(_) => false,
        }
    }
}
