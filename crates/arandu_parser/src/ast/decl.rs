use super::{Block, DeclId, Expr, IndexRange, TypeExprId, TypeName};
use arandu_lexer::Span;

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub span: Span,
    pub module: Option<ModuleDecl>,
    pub imports: Vec<ImportDecl>,
    pub decls: Vec<DeclId>,
    pub docs: Vec<DocCommentAttachment>,
    pub pool: super::AstPool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DocCommentAttachment {
    pub span: Span,
    pub text: String,
    pub target_span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModuleDecl {
    pub span: Span,
    pub path: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImportDecl {
    /// `import path.to.module as alias`
    ModuleAlias {
        span: Span,
        path: Vec<String>,
        alias: String,
    },
    /// `from path.to.module import { Item, Other }`
    Named {
        span: Span,
        items: Vec<ImportItem>,
        path: Vec<String>,
    },
    /// `from "external" import { Item }`
    ExternalNamed {
        span: Span,
        items: Vec<ImportItem>,
        source: String,
    },
    /// `import "external" as alias`
    ExternalAlias {
        span: Span,
        source: String,
        alias: String,
    },
}

impl ImportDecl {
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            ImportDecl::ModuleAlias { span, .. }
            | ImportDecl::Named { span, .. }
            | ImportDecl::ExternalNamed { span, .. }
            | ImportDecl::ExternalAlias { span, .. } => *span,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportItem {
    pub span: Span,
    pub name: String,
    pub alias: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TopLevelDecl {
    Const(ConstDecl),
    TypeAlias(TypeAliasDecl),
    Func(FuncDecl),
    Struct(StructDecl),
    Enum(EnumDecl),
    Interface(InterfaceDecl),
    Extern(ExternDecl),
    Error(Span),
}

impl TopLevelDecl {
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            TopLevelDecl::Const(decl) => decl.span,
            TopLevelDecl::TypeAlias(decl) => decl.span,
            TopLevelDecl::Func(decl) => decl.span,
            TopLevelDecl::Struct(decl) => decl.span,
            TopLevelDecl::Enum(decl) => decl.span,
            TopLevelDecl::Interface(decl) => decl.span,
            TopLevelDecl::Extern(decl) => decl.span,
            TopLevelDecl::Error(span) => *span,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Attribute {
    pub span: Span,
    pub name: String,
    pub args: Vec<Expr>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Private,
    Public,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GenericParam {
    pub span: Span,
    pub name: String,
    pub constraints: Vec<TypeName>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WhereItem {
    pub span: Span,
    pub name: String,
    pub constraints: Vec<TypeName>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConstDecl {
    pub span: Span,
    pub attrs: Vec<Attribute>,
    pub visibility: Visibility,
    pub name: String,
    pub ty: Option<TypeExprId>,
    pub value: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeAliasDecl {
    pub span: Span,
    pub attrs: Vec<Attribute>,
    pub visibility: Visibility,
    pub name: String,
    pub generic_params: Vec<GenericParam>,
    pub ty: TypeExprId,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FuncDecl {
    pub span: Span,
    pub attrs: Vec<Attribute>,
    pub visibility: Visibility,
    pub is_async: bool,
    pub name: FuncName,
    pub generic_params: Vec<GenericParam>,
    pub params: Vec<Param>,
    pub result: Option<super::ResultType>,
    pub where_clause: Vec<WhereItem>,
    pub body: Block,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FuncName {
    Free {
        span: Span,
        name: String,
    },
    Method {
        span: Span,
        receiver: TypeName,
        name: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct FuncSignature {
    pub span: Span,
    pub attrs: Vec<Attribute>,
    pub name: String,
    pub generic_params: Vec<GenericParam>,
    pub params: Vec<Param>,
    pub result: Option<super::ResultType>,
    pub where_clause: Vec<WhereItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructDecl {
    pub span: Span,
    pub attrs: Vec<Attribute>,
    pub visibility: Visibility,
    pub name: String,
    pub generic_params: Vec<GenericParam>,
    pub where_clause: Vec<WhereItem>,
    pub fields: Vec<FieldDecl>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldDecl {
    pub span: Span,
    pub attrs: Vec<Attribute>,
    pub visibility: Visibility,
    pub name: String,
    pub ty: TypeExprId,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumDecl {
    pub span: Span,
    pub attrs: Vec<Attribute>,
    pub visibility: Visibility,
    pub name: String,
    pub generic_params: Vec<GenericParam>,
    pub where_clause: Vec<WhereItem>,
    pub variants: Vec<EnumVariant>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumVariant {
    pub span: Span,
    pub attrs: Vec<Attribute>,
    pub name: String,
    pub payload: Option<EnumPayload>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EnumPayload {
    Tuple { span: Span, types: IndexRange },
    Struct { span: Span, fields: Vec<FieldDecl> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceDecl {
    pub span: Span,
    pub attrs: Vec<Attribute>,
    pub visibility: Visibility,
    pub name: String,
    pub generic_params: Vec<GenericParam>,
    pub where_clause: Vec<WhereItem>,
    pub members: Vec<FuncSignature>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExternDecl {
    pub span: Span,
    pub attrs: Vec<Attribute>,
    pub abi: String,
    pub members: Vec<FuncSignature>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub span: Span,
    pub attrs: Vec<Attribute>,
    pub ownership: Option<Ownership>,
    pub name: String,
    pub ty: TypeExprId,
    pub is_variadic: bool,
    /// `true` when this parameter is the method receiver (`self`).
    pub is_receiver: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ownership {
    Own,
    Mut,
    Shared,
}
