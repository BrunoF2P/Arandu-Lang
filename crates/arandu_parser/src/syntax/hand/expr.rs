//! Hand-lower expressions (Pratt + postfix).

use super::cursor::{Cursor, HandCtx, bin_bp};
use super::ty::parse_type;
use crate::ast::ast_pool::{ExprId, ExprKind};
use crate::{StringPart, UnaryOp};
use arandu_lexer::{Token, TokenKind};
use smallvec::smallvec;
use smol_str::SmolStr;

/// Parse expression from token slice; must consume all tokens.
pub fn try_hand_lower_expr_all(
    pool: &mut crate::ast::ast_pool::AstPool,
    source: &str,
    toks: &[&Token],
    file_id: u32,
) -> Option<ExprId> {
    if toks.is_empty() {
        return None;
    }
    let mut ctx = HandCtx {
        pool,
        source,
        file_id,
    };
    let mut cur = Cursor::new(toks);
    let expr = try_hand_lower_expr(&mut ctx, &mut cur, 0)?;
    if !cur.at_end() {
        return None;
    }
    Some(expr)
}

/// Parse expression with minimum binding power.
pub fn try_hand_lower_expr(
    ctx: &mut HandCtx<'_>,
    cur: &mut Cursor<'_>,
    min_bp: u8,
) -> Option<ExprId> {
    let mut left = parse_unary_primary_post(ctx, cur)?;

    while let Some(kind) = cur.peek_kind() {
        let Some((l_bp, r_bp, op)) = bin_bp(kind) else {
            break;
        };
        if l_bp < min_bp {
            break;
        }
        cur.bump();
        let right = try_hand_lower_expr(ctx, cur, r_bp)?;
        let left_span = ctx.pool.expr_span(left);
        let right_span = ctx.pool.expr_span(right);
        let span = ctx.span(left_span.start, right_span.end);
        if op == crate::BinaryOp::NullCoalesce {
            left = ctx
                .pool
                .alloc_expr(ExprKind::NullCoalesce { left, right }, span);
        } else {
            left = ctx
                .pool
                .alloc_expr(ExprKind::Binary { op, left, right }, span);
        }
    }

    Some(left)
}

fn parse_unary_primary_post(ctx: &mut HandCtx<'_>, cur: &mut Cursor<'_>) -> Option<ExprId> {
    let t = cur.peek()?;
    match t.kind {
        TokenKind::Minus | TokenKind::Bang | TokenKind::Tilde | TokenKind::KwAwait => {
            let op = match t.kind {
                TokenKind::Minus => UnaryOp::Neg,
                TokenKind::Bang => UnaryOp::Not,
                TokenKind::Tilde => UnaryOp::BitNot,
                TokenKind::KwAwait => UnaryOp::Await,
                _ => unreachable!(),
            };
            let start = t.start;
            cur.bump();
            // Unary binds tighter than most binary; use high min_bp on recursive.
            let expr = try_hand_lower_expr(ctx, cur, 100)?;
            let end = ctx.pool.expr_span(expr).end;
            Some(
                ctx.pool
                    .alloc_expr(ExprKind::Unary { op, expr }, ctx.span(start, end)),
            )
        }
        _ => parse_primary_post(ctx, cur),
    }
}

fn parse_primary_post(ctx: &mut HandCtx<'_>, cur: &mut Cursor<'_>) -> Option<ExprId> {
    let mut left = parse_primary(ctx, cur)?;

    loop {
        match cur.peek_kind() {
            Some(TokenKind::LParen) => {
                cur.bump();
                let mut args = Vec::new();
                if cur.peek_kind() != Some(TokenKind::RParen) {
                    loop {
                        args.push(try_hand_lower_expr(ctx, cur, 0)?);
                        if cur.eat(TokenKind::Comma) {
                            continue;
                        }
                        break;
                    }
                }
                let close = cur.expect(TokenKind::RParen)?;
                let args_range = ctx.pool.alloc_expr_list(&args);
                let left_span = ctx.pool.expr_span(left);
                let span = ctx.span(left_span.start, close.start + close.len);
                left = ctx.pool.alloc_expr(
                    ExprKind::Call {
                        callee: left,
                        args: args_range,
                        trailing_block: None,
                    },
                    span,
                );
            }
            Some(TokenKind::Dot) => {
                cur.bump();
                let field_tok = cur.peek()?;
                if !matches!(field_tok.kind, TokenKind::IdentValue | TokenKind::IdentType) {
                    return None;
                }
                let field = SmolStr::new(ctx.text(field_tok)?);
                cur.bump();
                let left_span = ctx.pool.expr_span(left);
                let span = ctx.span(left_span.start, field_tok.start + field_tok.len);
                left = ctx
                    .pool
                    .alloc_expr(ExprKind::Field { base: left, field }, span);
            }
            Some(TokenKind::SafeDot) => {
                cur.bump();
                let field_tok = cur.expect(TokenKind::IdentValue)?;
                let field = SmolStr::new(ctx.text(field_tok)?);
                let left_span = ctx.pool.expr_span(left);
                let span = ctx.span(left_span.start, field_tok.start + field_tok.len);
                left = ctx
                    .pool
                    .alloc_expr(ExprKind::SafeField { base: left, field }, span);
            }
            Some(TokenKind::LBracket) => {
                cur.bump();
                let index = try_hand_lower_expr(ctx, cur, 0)?;
                let close = cur.expect(TokenKind::RBracket)?;
                let left_span = ctx.pool.expr_span(left);
                let span = ctx.span(left_span.start, close.start + close.len);
                left = ctx
                    .pool
                    .alloc_expr(ExprKind::Index { base: left, index }, span);
            }
            Some(TokenKind::SafeIndexStart) => {
                cur.bump();
                let index = try_hand_lower_expr(ctx, cur, 0)?;
                let close = cur.expect(TokenKind::RBracket)?;
                let left_span = ctx.pool.expr_span(left);
                let span = ctx.span(left_span.start, close.start + close.len);
                left = ctx
                    .pool
                    .alloc_expr(ExprKind::SafeIndex { base: left, index }, span);
            }
            Some(TokenKind::Question) => {
                // postfix try `expr?`
                let q = cur.bump()?;
                let left_span = ctx.pool.expr_span(left);
                let span = ctx.span(left_span.start, q.start + q.len);
                left = ctx.pool.alloc_expr(ExprKind::Try { expr: left }, span);
            }
            Some(TokenKind::KwAs) => {
                cur.bump();
                let ty = parse_type(ctx, cur)?;
                let left_span = ctx.pool.expr_span(left);
                let ty_end = ctx.pool.type_expr_span(ty).end;
                let span = ctx.span(left_span.start, ty_end);
                left = ctx
                    .pool
                    .alloc_expr(ExprKind::Cast { expr: left, ty }, span);
            }
            _ => break,
        }
    }

    Some(left)
}

fn parse_primary(ctx: &mut HandCtx<'_>, cur: &mut Cursor<'_>) -> Option<ExprId> {
    let t = cur.peek()?;
    let start = t.start;
    match t.kind {
        TokenKind::IntDec | TokenKind::IntHex | TokenKind::IntBin | TokenKind::IntOct => {
            let value = SmolStr::new(ctx.text(t)?);
            cur.bump();
            Some(
                ctx.pool
                    .alloc_expr(ExprKind::Int { value }, ctx.token_span(t)),
            )
        }
        TokenKind::Float => {
            let value = SmolStr::new(ctx.text(t)?);
            cur.bump();
            Some(
                ctx.pool
                    .alloc_expr(ExprKind::Float { value }, ctx.token_span(t)),
            )
        }
        TokenKind::BoolTrue | TokenKind::BoolFalse => {
            cur.bump();
            Some(ctx.pool.alloc_expr(
                ExprKind::Bool {
                    value: matches!(t.kind, TokenKind::BoolTrue),
                },
                ctx.token_span(t),
            ))
        }
        TokenKind::Char => {
            let value = SmolStr::new(t.char_content(ctx.source));
            cur.bump();
            Some(
                ctx.pool
                    .alloc_expr(ExprKind::Char { value }, ctx.token_span(t)),
            )
        }
        TokenKind::Nil => {
            cur.bump();
            Some(ctx.pool.alloc_expr(ExprKind::Nil, ctx.token_span(t)))
        }
        TokenKind::IdentValue | TokenKind::KwSelf => {
            let text = if matches!(t.kind, TokenKind::KwSelf) {
                "self"
            } else {
                ctx.text(t)?
            };
            cur.bump();
            // multi-segment path: a.b only as field in postfix; bare path is single
            Some(ctx.pool.alloc_expr(
                ExprKind::Path {
                    path: smallvec![SmolStr::new(text)],
                },
                ctx.token_span(t),
            ))
        }
        TokenKind::IdentType => {
            // Type-led: Type.member or Type path
            let text = ctx.text(t)?;
            cur.bump();
            if cur.eat(TokenKind::Dot) {
                let mem = cur.peek()?;
                if !matches!(mem.kind, TokenKind::IdentValue | TokenKind::IdentType) {
                    return None;
                }
                let member = SmolStr::new(ctx.text(mem)?);
                cur.bump();
                let type_name = TypeNameish {
                    span: ctx.token_span(t),
                    path: smallvec![SmolStr::new(text)],
                };
                let span = ctx.span(start, mem.start + mem.len);
                return Some(ctx.pool.alloc_expr(
                    ExprKind::TypePath {
                        type_name: crate::TypeName {
                            span: type_name.span,
                            path: type_name.path,
                        },
                        member,
                    },
                    span,
                ));
            }
            Some(ctx.pool.alloc_expr(
                ExprKind::Path {
                    path: smallvec![SmolStr::new(text)],
                },
                ctx.token_span(t),
            ))
        }
        TokenKind::LParen => {
            cur.bump();
            // lambda `(a, b) => expr` or group
            if looks_like_lambda(cur) {
                return parse_lambda(ctx, cur, start);
            }
            let inner = try_hand_lower_expr(ctx, cur, 0)?;
            let close = cur.expect(TokenKind::RParen)?;
            Some(ctx.pool.alloc_expr(
                ExprKind::Group { expr: inner },
                ctx.span(start, close.start + close.len),
            ))
        }
        TokenKind::StringStart => parse_string(ctx, cur, start, TokenKind::StringEnd),
        TokenKind::MultilineStringStart => {
            parse_string(ctx, cur, start, TokenKind::MultilineStringEnd)
        }
        TokenKind::RawString => {
            let value = SmolStr::new(t.raw_string_content(ctx.source));
            cur.bump();
            let span = ctx.token_span(t);
            let part = StringPart::Text {
                span,
                text: value,
            };
            let part_id = ctx.pool.alloc_string_part(part);
            let range = ctx.pool.alloc_string_part_list(&[part_id]);
            Some(
                ctx.pool
                    .alloc_expr(ExprKind::InterpolatedString { parts: range }, span),
            )
        }
        TokenKind::LBracket => {
            // array literal [a, b]
            cur.bump();
            let mut items = Vec::new();
            if cur.peek_kind() != Some(TokenKind::RBracket) {
                loop {
                    items.push(try_hand_lower_expr(ctx, cur, 0)?);
                    if cur.eat(TokenKind::Comma) {
                        continue;
                    }
                    break;
                }
            }
            let close = cur.expect(TokenKind::RBracket)?;
            let range = ctx.pool.alloc_expr_list(&items);
            Some(ctx.pool.alloc_expr(
                ExprKind::Array { items: range },
                ctx.span(start, close.start + close.len),
            ))
        }
        _ => None,
    }
}

struct TypeNameish {
    span: arandu_lexer::Span,
    path: smallvec::SmallVec<[SmolStr; 3]>,
}

fn looks_like_lambda(cur: &Cursor<'_>) -> bool {
    // Scan from current (after LParen already consumed) for `) =>`
    let mut depth = 1i32;
    let mut i = 0usize;
    while let Some(t) = cur.peek_at(i) {
        match t.kind {
            TokenKind::LParen => depth += 1,
            TokenKind::RParen => {
                depth -= 1;
                if depth == 0 {
                    return cur
                        .peek_at(i + 1)
                        .is_some_and(|n| matches!(n.kind, TokenKind::FatArrow));
                }
            }
            TokenKind::Eof => return false,
            _ => {}
        }
        i += 1;
        if i > 64 {
            return false;
        }
    }
    false
}

fn parse_lambda(ctx: &mut HandCtx<'_>, cur: &mut Cursor<'_>, start: u32) -> Option<ExprId> {
    use crate::{LambdaBody, LambdaParam};
    let mut params = Vec::new();
    if cur.peek_kind() != Some(TokenKind::RParen) {
        loop {
            let name_tok = cur.expect(TokenKind::IdentValue)?;
            let name = SmolStr::new(ctx.text(name_tok)?);
            let p_start = name_tok.start;
            let ty = if cur.peek_kind().is_some_and(can_start_type) {
                Some(parse_type(ctx, cur)?)
            } else {
                None
            };
            let p_end = ty
                .map(|id| ctx.pool.type_expr_span(id).end)
                .unwrap_or(name_tok.start + name_tok.len);
            params.push(LambdaParam {
                span: ctx.span(p_start, p_end),
                name,
                ty,
            });
            if cur.eat(TokenKind::Comma) {
                continue;
            }
            break;
        }
    }
    cur.expect(TokenKind::RParen)?;
    cur.expect(TokenKind::FatArrow)?;
    let expr = try_hand_lower_expr(ctx, cur, 0)?;
    let end = ctx.pool.expr_span(expr).end;
    let param_ids: Vec<_> = params
        .into_iter()
        .map(|p| ctx.pool.alloc_lambda_param(p))
        .collect();
    let params_range = ctx.pool.alloc_lambda_param_list(&param_ids);
    let body = LambdaBody::Expr {
        span: ctx.pool.expr_span(expr),
        expr,
    };
    Some(ctx.pool.alloc_expr(
        ExprKind::Lambda {
            params: params_range,
            body,
        },
        ctx.span(start, end),
    ))
}

fn can_start_type(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::IdentType
            | TokenKind::IdentValue
            | TokenKind::KwPtr
            | TokenKind::LBracket
            | TokenKind::LParen
    ) || super::ty::primitive_type_token_name(kind).is_some()
}

fn parse_string(
    ctx: &mut HandCtx<'_>,
    cur: &mut Cursor<'_>,
    start: u32,
    end_kind: TokenKind,
) -> Option<ExprId> {
    cur.bump(); // start
    let mut parts = Vec::new();
    loop {
        let t = cur.peek()?;
        match t.kind {
            k if k == end_kind => {
                let end_tok = cur.bump()?;
                let range = ctx.pool.alloc_string_part_list(&parts);
                return Some(ctx.pool.alloc_expr(
                    ExprKind::InterpolatedString { parts: range },
                    ctx.span(start, end_tok.start + end_tok.len),
                ));
            }
            TokenKind::StringText => {
                let text = SmolStr::new(ctx.text(t)?);
                let span = ctx.token_span(t);
                cur.bump();
                parts.push(ctx.pool.alloc_string_part(StringPart::Text { span, text }));
            }
            TokenKind::InterpStart => {
                let interp_start = cur.bump()?;
                let expr = try_hand_lower_expr(ctx, cur, 0)?;
                let mut end = ctx.pool.expr_span(expr).end;
                // optional end interp token
                if matches!(
                    cur.peek_kind(),
                    Some(TokenKind::RBrace) | Some(TokenKind::InterpEnd)
                ) {
                    let close = cur.bump()?;
                    end = close.start + close.len;
                }
                // RD spans the whole `${…}` including braces.
                let span = ctx.span(interp_start.start, end);
                parts.push(ctx.pool.alloc_string_part(StringPart::Expr { span, expr }));
            }
            _ => {
                // treat unknown as text if possible
                let text = SmolStr::new(ctx.text(t).unwrap_or(""));
                let span = ctx.token_span(t);
                cur.bump();
                parts.push(ctx.pool.alloc_string_part(StringPart::Text { span, text }));
            }
        }
    }
}
