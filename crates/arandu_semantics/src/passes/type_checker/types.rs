use std::fmt;

use arandu_lexer::Span;
use arandu_parser::{ResultType, TypeExpr, TypeName};

use crate::{ResolvedNames, ScopeId, SymbolId, SymbolTable};

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

    pub fn is_float(self) -> bool {
        matches!(self, Primitive::Float | Primitive::F32 | Primitive::F64)
    }

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

// ── ArType — the core type representation ───────────────────────────

#[derive(Debug, Clone, PartialEq)]
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

    /// Multi-return tuple: (int, Err?)
    Tuple(Vec<ArType>),

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
    pub fn is_error(&self) -> bool {
        matches!(self, ArType::Error)
    }

    /// Returns true for any numeric primitive type.
    pub fn is_numeric(&self) -> bool {
        match self {
            ArType::Primitive(p) => p.is_numeric(),
            ArType::IntLiteral | ArType::FloatLiteral => true,
            _ => false,
        }
    }

    /// Returns true for integer primitives.
    pub fn is_integer(&self) -> bool {
        match self {
            ArType::Primitive(p) => p.is_integer(),
            ArType::IntLiteral => true,
            _ => false,
        }
    }

    /// Returns true for float primitives.
    pub fn is_float(&self) -> bool {
        match self {
            ArType::Primitive(p) => p.is_float(),
            ArType::FloatLiteral => true,
            _ => false,
        }
    }

    /// Returns true if this is an unresolved literal type.
    pub fn is_literal(&self) -> bool {
        matches!(self, ArType::IntLiteral | ArType::FloatLiteral)
    }

    /// Produce a human-readable name for this type.
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
            ArType::Err => "Err".to_string(),
            ArType::Void => "void".to_string(),
            ArType::IntLiteral => "int".to_string(),
            ArType::FloatLiteral => "float".to_string(),
            ArType::Error => "<error>".to_string(),
        }
    }

    /// Resolve a literal type to a concrete type. If the type is a literal
    /// and no context is available, defaults to `int` or `float`.
    pub fn default_literal(self) -> ArType {
        match self {
            ArType::IntLiteral => ArType::Primitive(Primitive::Int),
            ArType::FloatLiteral => ArType::Primitive(Primitive::Float),
            other => other,
        }
    }

    /// Check if this literal type can absorb the given target type.
    /// Used for literal context-absorption (Swift-style).
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

// ── Lowering from AST TypeExpr to ArType ────────────────────────────

/// Convert an AST `TypeExpr` into an internal `ArType`.
///
/// Uses the symbol table and resolved names to resolve named types to
/// their `SymbolId`. Returns `ArType::Error` for types that cannot be
/// resolved (the name resolver already reported the error).
pub fn lower_type_expr(
    expr: &TypeExpr,
    symbols: &SymbolTable,
    _scope: ScopeId,
    resolved: &ResolvedNames,
) -> ArType {
    match expr {
        TypeExpr::Primitive { name, .. } => {
            if name == "Err" {
                return ArType::Err;
            }
            match Primitive::from_name(name) {
                Some(p) => ArType::Primitive(p),
                None => ArType::Error,
            }
        }
        TypeExpr::Named { span, name, args } => {
            lower_named_type(*span, name, args, symbols, _scope, resolved)
        }
        TypeExpr::Nullable { inner, .. } => {
            let inner_ty = lower_type_expr(inner, symbols, _scope, resolved);
            ArType::Nullable(Box::new(inner_ty))
        }
        TypeExpr::Pointer { inner, .. } => {
            let inner_ty = lower_type_expr(inner, symbols, _scope, resolved);
            ArType::Ptr(Box::new(inner_ty))
        }
        TypeExpr::Slice { inner, .. } => {
            let inner_ty = lower_type_expr(inner, symbols, _scope, resolved);
            ArType::Slice(Box::new(inner_ty))
        }
        TypeExpr::Array { size, elem, .. } => {
            let elem_ty = lower_type_expr(elem, symbols, _scope, resolved);
            let n = size.parse::<u64>().unwrap_or(0);
            ArType::Array(n, Box::new(elem_ty))
        }
        TypeExpr::Func { params, result, .. } => {
            let param_types: Vec<ArType> = params
                .iter()
                .map(|p| lower_type_expr(p, symbols, _scope, resolved))
                .collect();
            let ret = match result {
                Some(r) => lower_result_type(r, symbols, _scope, resolved),
                None => ArType::Void,
            };
            ArType::Func(param_types, Box::new(ret))
        }
        TypeExpr::Group { inner, .. } => lower_type_expr(inner, symbols, _scope, resolved),
    }
}

/// Convert an AST `ResultType` into an `ArType`.
/// Single result → direct type, multi result → Tuple.
pub fn lower_result_type(
    result: &ResultType,
    symbols: &SymbolTable,
    scope: ScopeId,
    resolved: &ResolvedNames,
) -> ArType {
    match result {
        ResultType::Single { ty, .. } => lower_type_expr(ty, symbols, scope, resolved),
        ResultType::Multi { types, .. } => {
            let tys: Vec<ArType> = types
                .iter()
                .map(|t| lower_type_expr(t, symbols, scope, resolved))
                .collect();
            ArType::Tuple(tys)
        }
    }
}

fn lower_named_type(
    _span: Span,
    name: &TypeName,
    args: &[TypeExpr],
    symbols: &SymbolTable,
    scope: ScopeId,
    resolved: &ResolvedNames,
) -> ArType {
    // The name resolver already resolved this name — look up the symbol ID.
    let key = crate::NodeKey::from(name.span);
    if let Some(&symbol_id) = resolved.type_refs.get(&key) {
        let generic_args: Vec<ArType> = args
            .iter()
            .map(|a| lower_type_expr(a, symbols, scope, resolved))
            .collect();
        ArType::Named(symbol_id, generic_args)
    } else {
        // Name was not resolved — name resolver already emitted an error.
        ArType::Error
    }
}

// ── Unification ─────────────────────────────────────────────────────

/// Structural type equality check. Returns true if the two types unify.
///
/// - `Error` unifies with anything (poison propagation)
/// - `Any` unifies with anything (FFI/variadic)
/// - `IntLiteral` unifies with any numeric type
/// - `FloatLiteral` unifies with any float type
/// - Named types compare SymbolId + generic args
/// - Func types compare param count, params, and return
pub fn unify(a: &ArType, b: &ArType) -> bool {
    // Poison and Any always unify
    if a.is_error() || b.is_error() {
        return true;
    }
    if matches!(a, ArType::Primitive(Primitive::Any))
        || matches!(b, ArType::Primitive(Primitive::Any))
    {
        return true;
    }

    // Literal absorption
    if a.is_literal() && a.literal_absorbs(b) {
        return true;
    }
    if b.is_literal() && b.literal_absorbs(a) {
        return true;
    }
    // Two int literals or two float literals unify
    if matches!((a, b), (ArType::IntLiteral, ArType::IntLiteral)) {
        return true;
    }
    if matches!((a, b), (ArType::FloatLiteral, ArType::FloatLiteral)) {
        return true;
    }
    // IntLiteral and FloatLiteral: the int absorbs float context
    if matches!(
        (a, b),
        (ArType::IntLiteral, ArType::FloatLiteral) | (ArType::FloatLiteral, ArType::IntLiteral)
    ) {
        return true;
    }

    match (a, b) {
        (ArType::Primitive(pa), ArType::Primitive(pb)) => pa == pb,
        (ArType::Named(id_a, args_a), ArType::Named(id_b, args_b)) => {
            id_a == id_b
                && args_a.len() == args_b.len()
                && args_a.iter().zip(args_b).all(|(x, y)| unify(x, y))
        }
        (ArType::Func(params_a, ret_a), ArType::Func(params_b, ret_b)) => {
            params_a.len() == params_b.len()
                && params_a.iter().zip(params_b).all(|(x, y)| unify(x, y))
                && unify(ret_a, ret_b)
        }
        (ArType::Nullable(inner_a), ArType::Nullable(inner_b)) => unify(inner_a, inner_b),
        (ArType::Slice(inner_a), ArType::Slice(inner_b)) => unify(inner_a, inner_b),
        (ArType::Array(n_a, elem_a), ArType::Array(n_b, elem_b)) => {
            n_a == n_b && unify(elem_a, elem_b)
        }
        (ArType::Ptr(inner_a), ArType::Ptr(inner_b)) => unify(inner_a, inner_b),
        (ArType::Tuple(types_a), ArType::Tuple(types_b)) => {
            types_a.len() == types_b.len() && types_a.iter().zip(types_b).all(|(x, y)| unify(x, y))
        }
        (ArType::Err, ArType::Err) => true,
        (ArType::Void, ArType::Void) => true,
        _ => false,
    }
}

/// Given two types where at least one may be a literal, resolve to the
/// concrete type. This is used to determine the result type of binary
/// operations where literals are involved.
pub fn resolve_literal_pair(a: &ArType, b: &ArType) -> ArType {
    match (a, b) {
        // If one side is a concrete type and the other is a literal, use
        // the concrete type.
        (ArType::IntLiteral, other) | (other, ArType::IntLiteral) if !other.is_literal() => {
            other.clone()
        }
        (ArType::FloatLiteral, other) | (other, ArType::FloatLiteral) if !other.is_literal() => {
            other.clone()
        }
        // Two int literals → default to int
        (ArType::IntLiteral, ArType::IntLiteral) => ArType::Primitive(Primitive::Int),
        // Two float literals → default to float
        (ArType::FloatLiteral, ArType::FloatLiteral) => ArType::Primitive(Primitive::Float),
        // Int + Float literals → float wins
        (ArType::IntLiteral, ArType::FloatLiteral) | (ArType::FloatLiteral, ArType::IntLiteral) => {
            ArType::Primitive(Primitive::Float)
        }
        // Neither is a literal — just return a
        _ => a.clone(),
    }
}
