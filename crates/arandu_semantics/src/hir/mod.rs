mod pattern;
mod pretty;
pub use pattern::{HirFieldPattern, HirPattern};
pub use pretty::HirPrettyCtx;

use crate::ops::{BinaryOp, SetOp, UnaryOp};
use crate::passes::type_checker::types::ArType;
use crate::{SymbolId, SymbolTable};
use arandu_lexer::Span;
use smallvec::SmallVec;

/// Resolve a display name from the symbol table (B3: names are not duplicated on HIR nodes).
#[must_use]
pub fn symbol_name(symbols: &SymbolTable, id: SymbolId) -> &str {
    &symbols.get(id).name
}

#[derive(Debug)]
pub struct HirProgram {
    pub span: Span,
    pub module: Option<String>,
    pub decls: Vec<HirDecl>,
}

#[derive(Debug)]
#[non_exhaustive]
pub enum HirDecl {
    Const(HirConst),
    TypeAlias(HirTypeAlias),
    Func(HirFunc),
    Struct(HirStruct),
    Enum(HirEnum),
    Interface(HirInterface),
    Extern(HirExtern),
}

impl HirDecl {
    pub fn span(&self) -> Span {
        match self {
            HirDecl::Const(decl) => decl.span,
            HirDecl::TypeAlias(decl) => decl.span,
            HirDecl::Func(decl) => decl.span,
            HirDecl::Struct(decl) => decl.span,
            HirDecl::Enum(decl) => decl.span,
            HirDecl::Interface(decl) => decl.span,
            HirDecl::Extern(decl) => decl.span,
        }
    }
}

#[derive(Debug)]
pub struct HirConst {
    pub symbol: SymbolId,
    pub ty: ArType,
    pub value: HirExpr,
    pub span: Span,
}

#[derive(Debug)]
pub struct HirTypeAlias {
    pub symbol: SymbolId,
    pub target: ArType,
    pub span: Span,
}

#[derive(Debug)]
pub struct HirFunc {
    pub symbol: SymbolId,
    pub params: Vec<HirParam>,
    pub return_type: ArType,
    pub body: Option<HirBlock>,
    pub span: Span,
}

#[derive(Debug)]
pub struct HirParam {
    pub symbol: SymbolId,
    pub ty: ArType,
    pub span: Span,
}

#[derive(Debug)]
pub struct HirStruct {
    pub symbol: SymbolId,
    pub fields: Vec<HirStructField>,
    pub span: Span,
}

#[derive(Debug)]
pub struct HirStructField {
    pub symbol: SymbolId,
    pub ty: ArType,
    pub span: Span,
}

#[derive(Debug)]
pub struct HirEnum {
    pub symbol: SymbolId,
    pub variants: Vec<HirEnumVariant>,
    pub span: Span,
}

#[derive(Debug)]
pub struct HirEnumVariant {
    pub symbol: SymbolId,
    pub payload: Option<ArType>,
    pub span: Span,
}

#[derive(Debug)]
pub struct HirInterface {
    pub symbol: SymbolId,
    pub span: Span,
}

#[derive(Debug)]
pub struct HirExtern {
    pub abi: String,
    pub members: Vec<HirFuncSignature>,
    pub span: Span,
}

#[derive(Debug)]
pub struct HirFuncSignature {
    pub symbol: SymbolId,
    pub params: Vec<HirParam>,
    pub return_type: ArType,
    pub span: Span,
}

#[derive(Debug)]
pub struct HirBlock {
    pub statements: Vec<HirStmt>,
    pub span: Span,
}

#[derive(Debug)]
pub struct HirStmt {
    pub kind: HirStmtKind,
    pub span: Span,
}

#[derive(Debug)]
#[non_exhaustive]
#[allow(clippy::large_enum_variant)]
pub enum HirStmtKind {
    VarDecl {
        bindings: Vec<HirBindingItem>,
        value: HirExpr,
    },
    Set {
        places: Vec<HirPlace>,
        op: SetOp,
        value: HirExpr,
    },
    Return {
        values: Vec<HirExpr>,
    },
    Break,
    Continue,
    Free(HirExpr),
    Expr(HirExpr),
    If {
        condition: HirCondition,
        then_block: HirBlock,
        else_block: Option<HirBlock>,
    },
    For {
        clause: HirForClause,
        body: HirBlock,
    },
    While {
        condition: HirCondition,
        body: HirBlock,
    },
    Match {
        value: HirExpr,
        arms: Vec<HirMatchArm>,
    },
    Defer(HirBlock),
    ErrDefer(HirBlock),
    Unsafe(HirBlock),
}

#[derive(Debug)]
#[non_exhaustive]
pub enum HirCondition {
    Expr(HirExpr),
    Is { expr: HirExpr, pattern: HirPattern },
}

#[derive(Debug)]
#[non_exhaustive]
#[allow(clippy::large_enum_variant)]
pub enum HirForClause {
    In {
        span: Span,
        bindings: Vec<HirForBinding>,
        iterable: HirExpr,
    },
    CStyle {
        span: Span,
        init: Option<HirSimpleStmt>,
        condition: Option<HirExpr>,
        step: Option<HirSimpleStmt>,
    },
}

#[derive(Debug)]
pub struct HirForBinding {
    pub symbol: SymbolId,
    pub ty: ArType,
    pub span: Span,
}

#[derive(Debug)]
#[non_exhaustive]
pub enum HirSimpleStmt {
    VarDecl {
        bindings: Vec<HirBindingItem>,
        value: HirExpr,
    },
    Set {
        places: Vec<HirPlace>,
        op: SetOp,
        value: HirExpr,
    },
    Expr(HirExpr),
}

#[derive(Debug)]
pub struct HirBindingItem {
    pub symbol: SymbolId,
    pub ty: ArType,
    pub span: Span,
}

#[derive(Debug)]
pub struct HirPlace {
    pub root_symbol: SymbolId,
    pub suffixes: SmallVec<[HirPlaceSuffix; 2]>,
    pub ty: ArType,
    pub span: Span,
}

#[derive(Debug)]
#[non_exhaustive]
#[allow(clippy::large_enum_variant)]
pub enum HirPlaceSuffix {
    Field {
        span: Span,
        name: String,
        ty: ArType,
    },
    Index {
        span: Span,
        expr: HirExpr,
        ty: ArType,
    },
}

#[derive(Debug)]
pub struct HirExpr {
    pub kind: HirExprKind,
    pub ty: ArType,
    pub span: Span,
}

#[derive(Debug)]
#[non_exhaustive]
pub enum HirExprKind {
    Path {
        symbol: SymbolId,
    },
    TypePath {
        type_symbol: SymbolId,
        member_symbol: SymbolId,
    },
    Generic {
        callee: Box<HirExpr>,
        args: Vec<ArType>,
    },
    Field {
        base: Box<HirExpr>,
        field: String,
    },
    SafeField {
        base: Box<HirExpr>,
        field: String,
    },
    Index {
        base: Box<HirExpr>,
        index: Box<HirExpr>,
    },
    SafeIndex {
        base: Box<HirExpr>,
        index: Box<HirExpr>,
    },
    Try {
        expr: Box<HirExpr>,
    },
    Call {
        callee: Box<HirExpr>,
        args: Vec<HirExpr>,
        trailing_block: Option<HirBlock>,
    },
    StructLiteral {
        struct_symbol: SymbolId,
        fields: Vec<HirFieldInit>,
    },
    Array {
        items: Vec<HirExpr>,
    },
    Lambda {
        params: Vec<HirLambdaParam>,
        body: HirLambdaBody,
    },
    Alloc {
        expr: Box<HirExpr>,
    },
    AsyncBlock {
        block: HirBlock,
    },
    UnsafeBlock {
        block: HirBlock,
    },
    If {
        condition: Box<HirCondition>,
        then_block: HirBlock,
        else_block: HirBlock,
    },
    Match {
        value: Box<HirExpr>,
        arms: Vec<HirMatchArm>,
    },
    Catch {
        expr: Box<HirExpr>,
        handler: HirCatchHandler,
    },
    NullCoalesce {
        left: Box<HirExpr>,
        right: Box<HirExpr>,
    },
    Cast {
        expr: Box<HirExpr>,
        target_ty: ArType,
    },
    Unary {
        op: UnaryOp,
        expr: Box<HirExpr>,
    },
    Binary {
        op: BinaryOp,
        left: Box<HirExpr>,
        right: Box<HirExpr>,
    },
    Int(String),
    Float(String),
    Bool(bool),
    Char(String),
    Str(String),
    Nil,
}

#[derive(Debug)]
pub struct HirFieldInit {
    pub span: Span,
    pub name: String,
    pub value: HirExpr,
}

#[derive(Debug)]
pub struct HirLambdaParam {
    pub span: Span,
    pub symbol: SymbolId,
    pub ty: ArType,
}

#[derive(Debug)]
#[non_exhaustive]
pub enum HirLambdaBody {
    Expr(Box<HirExpr>),
    Block(HirBlock),
}

#[derive(Debug)]
pub struct HirMatchArm {
    pub span: Span,
    pub pattern: HirPattern,
    pub guard: Option<HirExpr>,
    pub body: HirMatchArmBody,
}

#[derive(Debug)]
#[non_exhaustive]
pub enum HirMatchArmBody {
    Expr(Box<HirExpr>),
    Block(HirBlock),
}

#[derive(Debug)]
#[non_exhaustive]
pub enum HirCatchHandler {
    Expr(Box<HirExpr>),
    Block {
        error_symbol: Option<SymbolId>,
        error_name: Option<String>,
        block: HirBlock,
    },
}

impl HirProgram {
    pub fn pretty_print(&self, ctx: &HirPrettyCtx<'_>) -> String {
        pretty::print_program(self, ctx)
    }
}

fn check_span(span: Span) -> Result<(), String> {
    if span.start == 0 && span.end == 0 {
        return Ok(());
    }
    if span.start > span.end {
        return Err(format!(
            "Invalid span: start {} is greater than end {}",
            span.start, span.end
        ));
    }
    Ok(())
}

impl HirProgram {
    pub fn validate_invariants(&self, symbols: &SymbolTable) -> Result<(), String> {
        check_span(self.span)?;
        for decl in &self.decls {
            check_span(decl.span())?;
            match decl {
                HirDecl::Const(c) => {
                    let _sym = symbols.get(c.symbol);
                    c.value.validate_invariants(symbols)?;
                }
                HirDecl::TypeAlias(_) => {}
                HirDecl::Func(f) => {
                    check_span(f.span)?;
                    for param in &f.params {
                        check_span(param.span)?;
                    }
                    if let Some(body) = &f.body {
                        body.validate_invariants(symbols)?;
                    }
                }
                HirDecl::Struct(s) => {
                    check_span(s.span)?;
                    for field in &s.fields {
                        check_span(field.span)?;
                    }
                }
                HirDecl::Enum(e) => {
                    check_span(e.span)?;
                    for var in &e.variants {
                        check_span(var.span)?;
                    }
                }
                HirDecl::Interface(i) => {
                    check_span(i.span)?;
                }
                HirDecl::Extern(ex) => {
                    check_span(ex.span)?;
                    for m in &ex.members {
                        check_span(m.span)?;
                    }
                }
            }
        }
        Ok(())
    }
}

impl HirBlock {
    pub fn validate_invariants(&self, symbols: &SymbolTable) -> Result<(), String> {
        check_span(self.span)?;
        let mut last_start = 0;
        for stmt in &self.statements {
            check_span(stmt.span)?;
            if stmt.span.start != 0 || stmt.span.end != 0 {
                if stmt.span.start < last_start {
                    return Err(format!(
                        "Block statement order out of sequence: span start {} is less than last start {}",
                        stmt.span.start, last_start
                    ));
                }
                last_start = stmt.span.start;
            }
            stmt.validate_invariants(symbols)?;
        }
        Ok(())
    }
}

impl HirStmt {
    pub fn validate_invariants(&self, symbols: &SymbolTable) -> Result<(), String> {
        check_span(self.span)?;
        match &self.kind {
            HirStmtKind::VarDecl { bindings, value } => {
                for b in bindings {
                    check_span(b.span)?;
                    if matches!(b.ty, ArType::Error) {
                        return Err(format!(
                            "Variable declaration binding '{}' has Error type",
                            symbol_name(symbols, b.symbol)
                        ));
                    }
                }
                value.validate_invariants(symbols)?;
            }
            HirStmtKind::Set {
                places,
                op: _,
                value,
            } => {
                if places.is_empty() {
                    return Err("Set statement has no target places".to_string());
                }
                for p in places {
                    check_span(p.span)?;
                    if matches!(p.ty, ArType::Error) {
                        return Err(format!(
                            "Set target place '{}' has Error type",
                            symbol_name(symbols, p.root_symbol)
                        ));
                    }
                }
                value.validate_invariants(symbols)?;
            }
            HirStmtKind::Return { values } => {
                for v in values {
                    v.validate_invariants(symbols)?;
                }
            }
            HirStmtKind::Break | HirStmtKind::Continue => {}
            HirStmtKind::Free(expr) => {
                expr.validate_invariants(symbols)?;
            }
            HirStmtKind::Expr(expr) => {
                expr.validate_invariants(symbols)?;
            }
            HirStmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                condition.validate_invariants(symbols)?;
                then_block.validate_invariants(symbols)?;
                if let Some(eb) = else_block {
                    eb.validate_invariants(symbols)?;
                }
            }
            HirStmtKind::For { clause, body } => {
                match clause {
                    HirForClause::In {
                        bindings, iterable, ..
                    } => {
                        for b in bindings {
                            check_span(b.span)?;
                        }
                        iterable.validate_invariants(symbols)?;
                    }
                    HirForClause::CStyle {
                        init,
                        condition,
                        step,
                        ..
                    } => {
                        if let Some(i) = init {
                            i.validate_invariants(symbols)?;
                        }
                        if let Some(c) = condition {
                            c.validate_invariants(symbols)?;
                        }
                        if let Some(s) = step {
                            s.validate_invariants(symbols)?;
                        }
                    }
                }
                body.validate_invariants(symbols)?;
            }
            HirStmtKind::While { condition, body } => {
                condition.validate_invariants(symbols)?;
                body.validate_invariants(symbols)?;
            }
            HirStmtKind::Match { value, arms } => {
                value.validate_invariants(symbols)?;
                for arm in arms {
                    check_span(arm.span)?;
                    if let Some(g) = &arm.guard {
                        g.validate_invariants(symbols)?;
                    }
                    match &arm.body {
                        HirMatchArmBody::Expr(e) => e.validate_invariants(symbols)?,
                        HirMatchArmBody::Block(b) => b.validate_invariants(symbols)?,
                    }
                }
            }
            HirStmtKind::Defer(b) | HirStmtKind::ErrDefer(b) | HirStmtKind::Unsafe(b) => {
                b.validate_invariants(symbols)?;
            }
        }
        Ok(())
    }
}

impl HirCondition {
    pub fn validate_invariants(&self, symbols: &SymbolTable) -> Result<(), String> {
        match self {
            HirCondition::Expr(expr) => expr.validate_invariants(symbols),
            HirCondition::Is { expr, .. } => expr.validate_invariants(symbols),
        }
    }
}

impl HirSimpleStmt {
    pub fn validate_invariants(&self, symbols: &SymbolTable) -> Result<(), String> {
        match self {
            HirSimpleStmt::VarDecl { bindings, value } => {
                for b in bindings {
                    check_span(b.span)?;
                    if matches!(b.ty, ArType::Error) {
                        return Err(format!(
                            "Variable declaration binding '{}' has Error type",
                            symbol_name(symbols, b.symbol)
                        ));
                    }
                }
                value.validate_invariants(symbols)?;
            }
            HirSimpleStmt::Set {
                places,
                op: _,
                value,
            } => {
                for p in places {
                    check_span(p.span)?;
                }
                value.validate_invariants(symbols)?;
            }
            HirSimpleStmt::Expr(expr) => {
                expr.validate_invariants(symbols)?;
            }
        }
        Ok(())
    }
}

impl HirExpr {
    pub fn validate_invariants(&self, symbols: &SymbolTable) -> Result<(), String> {
        check_span(self.span)?;
        if matches!(self.ty, ArType::Error) {
            return Err("Expression has Error type".to_string());
        }
        match &self.kind {
            HirExprKind::Path { symbol } => {
                let _sym = symbols.get(*symbol);
            }
            HirExprKind::TypePath {
                type_symbol,
                member_symbol,
                ..
            } => {
                let _t_sym = symbols.get(*type_symbol);
                let _m_sym = symbols.get(*member_symbol);
            }
            HirExprKind::Generic { callee, .. } => {
                callee.validate_invariants(symbols)?;
            }
            HirExprKind::Field { base, .. } | HirExprKind::SafeField { base, .. } => {
                base.validate_invariants(symbols)?;
                if matches!(base.ty, ArType::Error) {
                    return Err("Field access base expression has Error type".to_string());
                }
            }
            HirExprKind::Index { base, index } | HirExprKind::SafeIndex { base, index } => {
                base.validate_invariants(symbols)?;
                index.validate_invariants(symbols)?;
            }
            HirExprKind::Try { expr } => {
                expr.validate_invariants(symbols)?;
            }
            HirExprKind::Call {
                callee,
                args,
                trailing_block,
            } => {
                callee.validate_invariants(symbols)?;
                if matches!(callee.ty, ArType::Error) {
                    return Err("Call callee expression has Error type".to_string());
                }
                for arg in args {
                    arg.validate_invariants(symbols)?;
                }
                if let Some(tb) = trailing_block {
                    tb.validate_invariants(symbols)?;
                }
            }
            HirExprKind::StructLiteral {
                struct_symbol,
                fields,
            } => {
                let _sym = symbols.get(*struct_symbol);
                for f in fields {
                    check_span(f.span)?;
                    f.value.validate_invariants(symbols)?;
                }
            }
            HirExprKind::Array { items } => {
                for item in items {
                    item.validate_invariants(symbols)?;
                }
            }
            HirExprKind::Lambda { params, body } => {
                for p in params {
                    check_span(p.span)?;
                    let _sym = symbols.get(p.symbol);
                }
                match body {
                    HirLambdaBody::Expr(e) => e.validate_invariants(symbols)?,
                    HirLambdaBody::Block(b) => b.validate_invariants(symbols)?,
                }
            }
            HirExprKind::Alloc { expr } => {
                expr.validate_invariants(symbols)?;
            }
            HirExprKind::AsyncBlock { block } | HirExprKind::UnsafeBlock { block } => {
                block.validate_invariants(symbols)?;
            }
            HirExprKind::If {
                condition,
                then_block,
                else_block,
            } => {
                condition.validate_invariants(symbols)?;
                then_block.validate_invariants(symbols)?;
                else_block.validate_invariants(symbols)?;
            }
            HirExprKind::Match { value, arms } => {
                value.validate_invariants(symbols)?;
                for arm in arms {
                    check_span(arm.span)?;
                    if let Some(g) = &arm.guard {
                        g.validate_invariants(symbols)?;
                    }
                    match &arm.body {
                        HirMatchArmBody::Expr(e) => e.validate_invariants(symbols)?,
                        HirMatchArmBody::Block(b) => b.validate_invariants(symbols)?,
                    }
                }
            }
            HirExprKind::Catch { expr, handler } => {
                expr.validate_invariants(symbols)?;
                match handler {
                    HirCatchHandler::Expr(e) => e.validate_invariants(symbols)?,
                    HirCatchHandler::Block { block, .. } => block.validate_invariants(symbols)?,
                }
            }
            HirExprKind::NullCoalesce { left, right } => {
                left.validate_invariants(symbols)?;
                right.validate_invariants(symbols)?;
            }
            HirExprKind::Cast { expr, .. } => {
                expr.validate_invariants(symbols)?;
            }
            HirExprKind::Unary { expr, .. } => {
                expr.validate_invariants(symbols)?;
            }
            HirExprKind::Binary { left, right, .. } => {
                left.validate_invariants(symbols)?;
                right.validate_invariants(symbols)?;
            }
            HirExprKind::Int(_)
            | HirExprKind::Float(_)
            | HirExprKind::Bool(_)
            | HirExprKind::Char(_)
            | HirExprKind::Str(_)
            | HirExprKind::Nil => {}
        }
        Ok(())
    }
}
