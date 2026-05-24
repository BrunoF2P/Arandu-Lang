use arandu_lexer::Span;

#[derive(Debug, Clone, PartialEq)]
pub enum ResultType {
    Single { span: Span, ty: TypeExpr },
    Multi { span: Span, types: Vec<TypeExpr> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeName {
    pub span: Span,
    pub path: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeExpr {
    Primitive {
        span: Span,
        name: String,
    },
    Named {
        span: Span,
        name: TypeName,
        args: Vec<TypeExpr>,
    },
    Nullable {
        span: Span,
        inner: Box<TypeExpr>,
    },
    Pointer {
        span: Span,
        inner: Box<TypeExpr>,
    },
    Slice {
        span: Span,
        inner: Box<TypeExpr>,
    },
    Array {
        span: Span,
        size: String,
        elem: Box<TypeExpr>,
    },
    Func {
        span: Span,
        params: Vec<TypeExpr>,
        result: Option<Box<ResultType>>,
    },
    Group {
        span: Span,
        inner: Box<TypeExpr>,
    },
}

impl TypeExpr {
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            TypeExpr::Primitive { span, .. }
            | TypeExpr::Named { span, .. }
            | TypeExpr::Nullable { span, .. }
            | TypeExpr::Pointer { span, .. }
            | TypeExpr::Slice { span, .. }
            | TypeExpr::Array { span, .. }
            | TypeExpr::Func { span, .. }
            | TypeExpr::Group { span, .. } => *span,
        }
    }
}
