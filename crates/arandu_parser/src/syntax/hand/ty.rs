//! Hand-lower type expressions.

use super::cursor::{Cursor, HandCtx};
use crate::ast::ast_pool::TypeExprId;
use crate::{IndexRange, ResultType, TypeExpr, TypeName};
use arandu_lexer::{Token, TokenKind};
use smallvec::smallvec;
use smol_str::SmolStr;

#[must_use]
pub fn primitive_type_token_name(kind: TokenKind) -> Option<&'static str> {
    match kind {
        TokenKind::TypeInt => Some("int"),
        TokenKind::TypeUint => Some("uint"),
        TokenKind::TypeFloat => Some("float"),
        TokenKind::TypeI8 => Some("i8"),
        TokenKind::TypeI16 => Some("i16"),
        TokenKind::TypeI32 => Some("i32"),
        TokenKind::TypeI64 => Some("i64"),
        TokenKind::TypeU8 => Some("u8"),
        TokenKind::TypeU16 => Some("u16"),
        TokenKind::TypeU32 => Some("u32"),
        TokenKind::TypeU64 => Some("u64"),
        TokenKind::TypeF32 => Some("f32"),
        TokenKind::TypeF64 => Some("f64"),
        TokenKind::TypeBool => Some("bool"),
        TokenKind::TypeByte => Some("byte"),
        TokenKind::TypeChar => Some("char"),
        TokenKind::TypeStr => Some("str"),
        TokenKind::TypeAny => Some("any"),
        TokenKind::TypeErr => Some("Err"),
        _ => None,
    }
}

/// Single-token type (primitive or simple named).
pub fn try_hand_lower_type(
    pool: &mut crate::ast::ast_pool::AstPool,
    source: &str,
    t: &Token,
    file_id: u32,
) -> Option<TypeExprId> {
    let mut ctx = HandCtx {
        pool,
        source,
        file_id,
    };
    let toks = [t];
    let mut cur = Cursor::new(&toks);
    let ty = parse_type(&mut ctx, &mut cur)?;
    if !cur.at_end() {
        return None;
    }
    Some(ty)
}

/// Parse a type expression advancing `cur`.
pub fn parse_type(ctx: &mut HandCtx<'_>, cur: &mut Cursor<'_>) -> Option<TypeExprId> {
    let start_tok = cur.peek()?;
    let start = start_tok.start;

    // ptr T / *T-style via KwPtr
    if cur.eat(TokenKind::KwPtr) {
        let inner = parse_type(ctx, cur)?;
        let end = ctx.pool.type_expr_span(inner).end;
        return Some(ctx.pool.alloc_type_expr(TypeExpr::Pointer {
            span: ctx.span(start, end),
            inner,
        }));
    }

    // []T slice
    if cur.eat(TokenKind::LBracket) {
        if cur.eat(TokenKind::RBracket) {
            let inner = parse_type(ctx, cur)?;
            let end = ctx.pool.type_expr_span(inner).end;
            return Some(ctx.pool.alloc_type_expr(TypeExpr::Slice {
                span: ctx.span(start, end),
                inner,
            }));
        }
        // [N]T array — N is int literal
        let size_tok = cur.peek()?;
        if !matches!(
            size_tok.kind,
            TokenKind::IntDec | TokenKind::IntHex | TokenKind::IntBin | TokenKind::IntOct
        ) {
            return None;
        }
        let size = SmolStr::new(ctx.text(size_tok)?);
        cur.bump();
        cur.expect(TokenKind::RBracket)?;
        let elem = parse_type(ctx, cur)?;
        let end = ctx.pool.type_expr_span(elem).end;
        return Some(ctx.pool.alloc_type_expr(TypeExpr::Array {
            span: ctx.span(start, end),
            size,
            elem,
        }));
    }

    // (T) group or func type (T, U) -> R — keep group / simple for now
    if cur.eat(TokenKind::LParen) {
        let inner = parse_type(ctx, cur)?;
        let close = cur.expect(TokenKind::RParen)?;
        return Some(ctx.pool.alloc_type_expr(TypeExpr::Group {
            span: ctx.span(start, close.start + close.len),
            inner,
        }));
    }

    let t = cur.bump()?;
    let span = ctx.token_span(t);
    let mut ty = if let Some(name) = primitive_type_token_name(t.kind) {
        ctx.pool.alloc_type_expr(TypeExpr::Primitive {
            span,
            name: SmolStr::new_static(name),
        })
    } else {
        match t.kind {
            TokenKind::IdentType | TokenKind::IdentValue => {
                let text = ctx.text(t)?;
                let mut path = smallvec![SmolStr::new(text)];
                // dotted TypeName: a.b.C
                while cur.eat(TokenKind::Dot) {
                    let seg = cur.peek()?;
                    if !matches!(seg.kind, TokenKind::IdentType | TokenKind::IdentValue) {
                        return None;
                    }
                    path.push(SmolStr::new(ctx.text(seg)?));
                    cur.bump();
                }
                let name_end = path_end_span(ctx, t, &path);
                let name = TypeName {
                    span: ctx.span(start, name_end),
                    path,
                };
                let (args, args_end) = if cur.peek_kind() == Some(TokenKind::Lt) {
                    parse_generic_type_args(ctx, cur)?
                } else {
                    (ctx.pool.alloc_type_expr_list(&[]), name.span.end)
                };
                ctx.pool.alloc_type_expr(TypeExpr::Named {
                    span: ctx.span(start, args_end),
                    name,
                    args,
                })
            }
            _ => return None,
        }
    };

    // postfix ?
    if cur.eat(TokenKind::Question) {
        let end = ctx.pool.type_expr_span(ty).end + 1;
        ty = ctx.pool.alloc_type_expr(TypeExpr::Nullable {
            span: ctx.span(start, end),
            inner: ty,
        });
    }

    Some(ty)
}

fn path_end_span(_ctx: &HandCtx<'_>, first: &Token, path: &smallvec::SmallVec<[SmolStr; 3]>) -> u32 {
    // Best-effort: first token end if single segment.
    if path.len() == 1 {
        first.start + first.len
    } else {
        // without back-ref to last token, use first + rough
        first.start + first.len
    }
}

fn parse_generic_type_args(
    ctx: &mut HandCtx<'_>,
    cur: &mut Cursor<'_>,
) -> Option<(IndexRange, u32)> {
    cur.expect(TokenKind::Lt)?;
    let mut args = Vec::new();
    if cur.peek_kind() != Some(TokenKind::Gt) {
        loop {
            args.push(parse_type(ctx, cur)?);
            if cur.eat(TokenKind::Comma) {
                continue;
            }
            break;
        }
    }
    let gt = cur.expect(TokenKind::Gt)?;
    Some((
        ctx.pool.alloc_type_expr_list(&args),
        gt.start + gt.len,
    ))
}

/// `: T` result type (single).
pub fn parse_result_type(ctx: &mut HandCtx<'_>, cur: &mut Cursor<'_>) -> Option<ResultType> {
    let ty_start = cur.peek()?.start;
    let ty = parse_type(ctx, cur)?;
    let ty_end = ctx.pool.type_expr_span(ty).end;
    Some(ResultType::Single {
        span: ctx.span(ty_start, ty_end),
        ty,
    })
}

/// Dotted path of idents (`a.b.c`).
pub fn parse_dotted_ident_path(
    ctx: &HandCtx<'_>,
    cur: &mut Cursor<'_>,
) -> Option<smallvec::SmallVec<[SmolStr; 3]>> {
    let mut path = smallvec::SmallVec::new();
    loop {
        let t = cur.peek()?;
        if !matches!(t.kind, TokenKind::IdentValue | TokenKind::IdentType) {
            return None;
        }
        path.push(SmolStr::new(ctx.text(t)?));
        cur.bump();
        if cur.eat(TokenKind::Dot) {
            continue;
        }
        break;
    }
    if path.is_empty() {
        None
    } else {
        Some(path)
    }
}
