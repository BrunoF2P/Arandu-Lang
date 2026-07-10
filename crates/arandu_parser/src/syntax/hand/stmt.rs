//! Hand-lower statements and blocks.

use super::cursor::{Cursor, HandCtx, drop_trailing_semi, set_op_from_token, tokens_in_range};
use super::expr::{try_hand_lower_expr, try_hand_lower_expr_all};
use super::ty::parse_type;
use crate::ast::ast_pool::{AstPool, StmtId};
use crate::syntax::kind::{SyntaxKind, SyntaxNode};
use crate::{
    BindingItem, Block, Condition, DeferBody, ForBinding, ForClause, Place, PlaceSuffix, SimpleStmt,
    Stmt,
};
use arandu_lexer::{Span, Token, TokenKind};
use smol_str::SmolStr;

/// Hand-lower a green `STMT` without RD.
#[must_use]
pub fn try_hand_lower_stmt(
    pool: &mut AstPool,
    source: &str,
    tokens: &[Token],
    stmt: &SyntaxNode,
    file_id: u32,
) -> Option<StmtId> {
    let r = stmt.text_range();
    let s = u32::from(r.start());
    let e = u32::from(r.end());
    let mut toks = tokens_in_range(tokens, s, e);
    drop_trailing_semi(&mut toks);
    if toks.is_empty() {
        return None;
    }
    // Prefer token bounds (green STMT ranges often include leading whitespace).
    let first = toks[0];
    let last = toks[toks.len() - 1];
    let span = Span::new(file_id, first.start, last.start + last.len);
    let mut ctx = HandCtx {
        pool,
        source,
        file_id,
    };
    lower_stmt(&mut ctx, tokens, stmt, &toks, span)
}

fn lower_stmt(
    ctx: &mut HandCtx<'_>,
    tokens: &[Token],
    stmt: &SyntaxNode,
    toks: &[&Token],
    span: Span,
) -> Option<StmtId> {
    match toks[0].kind {
        TokenKind::KwBreak if toks.len() == 1 => Some(ctx.pool.alloc_stmt(Stmt::Break { span })),
        TokenKind::KwContinue if toks.len() == 1 => {
            Some(ctx.pool.alloc_stmt(Stmt::Continue { span }))
        }
        TokenKind::KwReturn => {
            let values = if toks.len() == 1 {
                Vec::new()
            } else {
                // multi-return: a, b
                let mut cur = Cursor::new(&toks[1..]);
                let mut values = vec![try_hand_lower_expr(ctx, &mut cur, 0)?];
                while cur.eat(TokenKind::Comma) {
                    values.push(try_hand_lower_expr(ctx, &mut cur, 0)?);
                }
                if !cur.at_end() {
                    return None;
                }
                values
            };
            Some(ctx.pool.alloc_stmt(Stmt::Return { span, values }))
        }
        TokenKind::KwFree => {
            let expr = try_hand_lower_expr_all(ctx.pool, ctx.source, &toks[1..], ctx.file_id)?;
            Some(ctx.pool.alloc_stmt(Stmt::Free { span, expr }))
        }
        TokenKind::KwLet => lower_let(ctx, toks, span),
        TokenKind::KwIf => lower_if(ctx, tokens, stmt, toks, span),
        TokenKind::KwWhile => lower_while(ctx, tokens, stmt, toks, span),
        TokenKind::KwFor => lower_for(ctx, tokens, stmt, toks, span),
        TokenKind::KwDefer => lower_defer(ctx, tokens, stmt, toks, span, false),
        TokenKind::KwErrdefer => lower_defer(ctx, tokens, stmt, toks, span, true),
        TokenKind::KwUnsafe => lower_unsafe(ctx, tokens, stmt, toks, span),
        TokenKind::KwSet => lower_set(ctx, toks, span, true),
        TokenKind::IdentValue | TokenKind::KwSelf => {
            if let Some(id) = lower_set(ctx, toks, span, false) {
                return Some(id);
            }
            let expr = try_hand_lower_expr_all(ctx.pool, ctx.source, toks, ctx.file_id)?;
            // match expr as stmt
            if matches!(ctx.pool.expr(expr), crate::ast::ast_pool::ExprKind::Match { .. }) {
                Some(ctx.pool.alloc_stmt(Stmt::Match { span, expr }))
            } else {
                Some(ctx.pool.alloc_stmt(Stmt::Expr { span, expr }))
            }
        }
        TokenKind::KwMatch => {
            let expr = try_hand_lower_expr_all(ctx.pool, ctx.source, toks, ctx.file_id)?;
            Some(ctx.pool.alloc_stmt(Stmt::Match { span, expr }))
        }
        TokenKind::Minus
        | TokenKind::Bang
        | TokenKind::Tilde
        | TokenKind::KwAwait
        | TokenKind::IntDec
        | TokenKind::IntHex
        | TokenKind::IntBin
        | TokenKind::IntOct
        | TokenKind::Float
        | TokenKind::BoolTrue
        | TokenKind::BoolFalse
        | TokenKind::Char
        | TokenKind::Nil
        | TokenKind::LParen
        | TokenKind::LBracket
        | TokenKind::StringStart
        | TokenKind::MultilineStringStart
        | TokenKind::RawString
        | TokenKind::IdentType => {
            let expr = try_hand_lower_expr_all(ctx.pool, ctx.source, toks, ctx.file_id)?;
            Some(ctx.pool.alloc_stmt(Stmt::Expr { span, expr }))
        }
        _ => None,
    }
}

fn lower_let(ctx: &mut HandCtx<'_>, toks: &[&Token], span: Span) -> Option<StmtId> {
    let mut cur = Cursor::new(toks);
    cur.expect(TokenKind::KwLet)?;
    let bind_start_tok = cur.peek()?;
    let mutable = cur.eat(TokenKind::KwMut);
    let name_tok = cur.expect(TokenKind::IdentValue)?;
    let name = SmolStr::new(ctx.text(name_tok)?);
    let bind_start = if mutable {
        bind_start_tok.start
    } else {
        name_tok.start
    };
    let ty = if cur.eat(TokenKind::Colon) {
        Some(parse_type(ctx, &mut cur)?)
    } else {
        None
    };
    let bind_end = ty
        .map(|id| ctx.pool.type_expr_span(id).end)
        .unwrap_or(name_tok.start + name_tok.len);
    cur.expect(TokenKind::Equal)?;
    let value = try_hand_lower_expr(ctx, &mut cur, 0)?;
    if !cur.at_end() {
        return None;
    }
    let binding = BindingItem {
        span: ctx.span(bind_start, bind_end),
        mutable,
        name,
        ty,
    };
    Some(ctx.pool.alloc_stmt(Stmt::VarDecl {
        span,
        bindings: vec![binding],
        value,
    }))
}

fn lower_set(
    ctx: &mut HandCtx<'_>,
    toks: &[&Token],
    span: Span,
    explicit_set: bool,
) -> Option<StmtId> {
    let mut cur = Cursor::new(toks);
    if explicit_set {
        cur.expect(TokenKind::KwSet)?;
    }
    let place = parse_place(ctx, &mut cur)?;
    let op_tok = cur.peek()?;
    let op = set_op_from_token(op_tok.kind)?;
    cur.bump();
    let value = try_hand_lower_expr(ctx, &mut cur, 0)?;
    if !cur.at_end() {
        return None;
    }
    Some(ctx.pool.alloc_stmt(Stmt::Set {
        span,
        places: vec![place],
        op,
        value,
    }))
}

fn parse_place(ctx: &mut HandCtx<'_>, cur: &mut Cursor<'_>) -> Option<Place> {
    let root_tok = cur.peek()?;
    let root = match root_tok.kind {
        TokenKind::KwSelf => {
            cur.bump();
            SmolStr::new_static("self")
        }
        TokenKind::IdentValue => {
            let n = SmolStr::new(ctx.text(root_tok)?);
            cur.bump();
            n
        }
        _ => return None,
    };
    let start = root_tok.start;
    let mut end = root_tok.start + root_tok.len;
    let mut suffixes = Vec::new();
    loop {
        if cur.eat(TokenKind::Dot) {
            let field_tok = cur.expect(TokenKind::IdentValue)?;
            let name = SmolStr::new(ctx.text(field_tok)?);
            end = field_tok.start + field_tok.len;
            suffixes.push(PlaceSuffix::Field {
                span: ctx.token_span(field_tok),
                name,
            });
        } else if cur.eat(TokenKind::LBracket) {
            let idx_start = cur.peek().map(|t| t.start).unwrap_or(end);
            let expr = try_hand_lower_expr(ctx, cur, 0)?;
            let close = cur.expect(TokenKind::RBracket)?;
            end = close.start + close.len;
            suffixes.push(PlaceSuffix::Index {
                span: ctx.span(idx_start, end),
                expr,
            });
        } else {
            break;
        }
    }
    Some(Place {
        span: ctx.span(start, end),
        root,
        suffixes,
    })
}

/// Hand-lower every direct `STMT` of a green `BLOCK` (requires real `}`).
#[must_use]
pub fn try_hand_lower_block(
    pool: &mut AstPool,
    source: &str,
    tokens: &[Token],
    block: &SyntaxNode,
    file_id: u32,
) -> Option<Block> {
    let r = block.text_range();
    let bs = u32::from(r.start());
    let be = u32::from(r.end());
    let has_rbrace = tokens.iter().any(|t| {
        matches!(t.kind, TokenKind::RBrace) && !t.inserted && t.start > bs && t.start <= be
    });
    if !has_rbrace {
        return None;
    }
    let span = Span::new(file_id, bs, be);
    let mut statements = Vec::new();
    for child in block.children() {
        if child.kind() != SyntaxKind::STMT {
            continue;
        }
        statements.push(try_hand_lower_stmt(pool, source, tokens, &child, file_id)?);
    }
    Some(Block { span, statements })
}

fn block_children(stmt: &SyntaxNode) -> Vec<SyntaxNode> {
    stmt.children()
        .filter(|n| n.kind() == SyntaxKind::BLOCK)
        .collect()
}

fn lower_if(
    ctx: &mut HandCtx<'_>,
    tokens: &[Token],
    stmt: &SyntaxNode,
    toks: &[&Token],
    span: Span,
) -> Option<StmtId> {
    let blocks = block_children(stmt);
    if blocks.is_empty() {
        return None;
    }
    let cond_end = toks
        .iter()
        .position(|t| matches!(t.kind, TokenKind::LBrace))
        .filter(|&p| p > 0)?;
    let cond_expr = try_hand_lower_expr_all(ctx.pool, ctx.source, &toks[1..cond_end], ctx.file_id)?;
    let condition = Condition::Expr {
        span: ctx.pool.expr_span(cond_expr),
        expr: cond_expr,
    };
    let then_block = try_hand_lower_block(ctx.pool, ctx.source, tokens, &blocks[0], ctx.file_id)?;
    let then_start = u32::from(blocks[0].text_range().start());
    let then_end = u32::from(blocks[0].text_range().end());

    let else_block = if let Some(else_idx) = toks.iter().position(|t| {
        matches!(t.kind, TokenKind::KwElse) && t.start >= then_start
    }) {
        // else if → nested If stmt in synthetic block
        if toks
            .get(else_idx + 1)
            .is_some_and(|t| matches!(t.kind, TokenKind::KwIf))
        {
            let rest = &toks[else_idx + 1..];
            // Build nested if using remaining green blocks (all after first).
            if blocks.len() < 2 {
                return None;
            }
            let nested = lower_if_from_parts(ctx, tokens, &blocks[1..], rest, span)?;
            let nested_id = ctx.pool.alloc_stmt(nested);
            Some(Block {
                span: ctx.span(
                    toks[else_idx].start,
                    u32::from(blocks.last()?.text_range().end()),
                ),
                statements: vec![nested_id],
            })
        } else if blocks.len() >= 2 {
            Some(try_hand_lower_block(
                ctx.pool,
                ctx.source,
                tokens,
                &blocks[1],
                ctx.file_id,
            )?)
        } else {
            return None;
        }
    } else {
        None
    };
    let _ = then_end;
    Some(ctx.pool.alloc_stmt(Stmt::If {
        span,
        condition,
        then_block,
        else_block,
    }))
}

/// Nested `if` starting at toks[0] == KwIf, blocks are its then/else blocks in order.
fn lower_if_from_parts(
    ctx: &mut HandCtx<'_>,
    tokens: &[Token],
    blocks: &[SyntaxNode],
    toks: &[&Token],
    outer_span: Span,
) -> Option<Stmt> {
    if blocks.is_empty() || toks.is_empty() || !matches!(toks[0].kind, TokenKind::KwIf) {
        return None;
    }
    let cond_end = toks
        .iter()
        .position(|t| matches!(t.kind, TokenKind::LBrace))
        .filter(|&p| p > 0)?;
    let cond_expr = try_hand_lower_expr_all(ctx.pool, ctx.source, &toks[1..cond_end], ctx.file_id)?;
    let condition = Condition::Expr {
        span: ctx.pool.expr_span(cond_expr),
        expr: cond_expr,
    };
    let then_block = try_hand_lower_block(ctx.pool, ctx.source, tokens, &blocks[0], ctx.file_id)?;
    let then_start = u32::from(blocks[0].text_range().start());
    let else_block = if let Some(else_idx) = toks.iter().position(|t| {
        matches!(t.kind, TokenKind::KwElse) && t.start >= then_start
    }) {
        if toks
            .get(else_idx + 1)
            .is_some_and(|t| matches!(t.kind, TokenKind::KwIf))
        {
            if blocks.len() < 2 {
                return None;
            }
            let nested = lower_if_from_parts(ctx, tokens, &blocks[1..], &toks[else_idx + 1..], outer_span)?;
            let nested_id = ctx.pool.alloc_stmt(nested);
            Some(Block {
                span: outer_span,
                statements: vec![nested_id],
            })
        } else if blocks.len() >= 2 {
            Some(try_hand_lower_block(
                ctx.pool,
                ctx.source,
                tokens,
                &blocks[1],
                ctx.file_id,
            )?)
        } else {
            None
        }
    } else {
        None
    };
    Some(Stmt::If {
        span: outer_span,
        condition,
        then_block,
        else_block,
    })
}

fn lower_while(
    ctx: &mut HandCtx<'_>,
    tokens: &[Token],
    stmt: &SyntaxNode,
    toks: &[&Token],
    span: Span,
) -> Option<StmtId> {
    let block = block_children(stmt).into_iter().next()?;
    let cond_end = toks
        .iter()
        .position(|t| matches!(t.kind, TokenKind::LBrace))
        .filter(|&p| p > 0)?;
    let cond_expr = try_hand_lower_expr_all(ctx.pool, ctx.source, &toks[1..cond_end], ctx.file_id)?;
    let condition = Condition::Expr {
        span: ctx.pool.expr_span(cond_expr),
        expr: cond_expr,
    };
    let body = try_hand_lower_block(ctx.pool, ctx.source, tokens, &block, ctx.file_id)?;
    Some(ctx.pool.alloc_stmt(Stmt::While {
        span,
        condition,
        body,
    }))
}

fn lower_for(
    ctx: &mut HandCtx<'_>,
    tokens: &[Token],
    stmt: &SyntaxNode,
    toks: &[&Token],
    span: Span,
) -> Option<StmtId> {
    let block = block_children(stmt).into_iter().next()?;
    // for [mut] x [, y] in expr { body }
    let mut cur = Cursor::new(toks);
    cur.expect(TokenKind::KwFor)?;
    // detect for-in vs c-style: look for `in` before first `{`
    let brace = toks
        .iter()
        .position(|t| matches!(t.kind, TokenKind::LBrace))?;
    let head = &toks[1..brace];
    let has_in = head.iter().any(|t| matches!(t.kind, TokenKind::KwIn));
    let clause = if has_in {
        let mut h = Cursor::new(head);
        let mut bindings = Vec::new();
        loop {
            let mutable = h.eat(TokenKind::KwMut);
            let name_tok = h.expect(TokenKind::IdentValue)?;
            let name = SmolStr::new(ctx.text(name_tok)?);
            bindings.push(ForBinding {
                span: ctx.token_span(name_tok),
                mutable,
                name,
            });
            if h.eat(TokenKind::Comma) {
                continue;
            }
            break;
        }
        h.expect(TokenKind::KwIn)?;
        let iterable = try_hand_lower_expr(ctx, &mut h, 0)?;
        if !h.at_end() {
            return None;
        }
        ForClause::In {
            span: ctx.span(toks[1].start, ctx.pool.expr_span(iterable).end),
            bindings,
            iterable,
        }
    } else {
        // C-style for init; cond; step — best-effort with `;` separators
        let mut parts: Vec<&[&Token]> = Vec::new();
        let mut start = 0usize;
        for (i, t) in head.iter().enumerate() {
            if matches!(t.kind, TokenKind::Semicolon) {
                parts.push(&head[start..i]);
                start = i + 1;
            }
        }
        parts.push(&head[start..]);
        if parts.len() != 3 {
            return None;
        }
        let init = if parts[0].is_empty() {
            None
        } else {
            Some(lower_simple_stmt(ctx, parts[0])?)
        };
        let condition = if parts[1].is_empty() {
            None
        } else {
            Some(try_hand_lower_expr_all(
                ctx.pool,
                ctx.source,
                parts[1],
                ctx.file_id,
            )?)
        };
        let step = if parts[2].is_empty() {
            None
        } else {
            Some(lower_simple_stmt(ctx, parts[2])?)
        };
        ForClause::CStyle {
            span: ctx.span(
                toks.get(1).map(|t| t.start).unwrap_or(span.start),
                toks[brace].start,
            ),
            init,
            condition,
            step,
        }
    };
    let body = try_hand_lower_block(ctx.pool, ctx.source, tokens, &block, ctx.file_id)?;
    let _ = cur;
    Some(ctx.pool.alloc_stmt(Stmt::For {
        span,
        clause,
        body,
    }))
}

fn lower_simple_stmt(ctx: &mut HandCtx<'_>, toks: &[&Token]) -> Option<SimpleStmt> {
    if toks.is_empty() {
        return None;
    }
    let span = ctx.span(toks[0].start, toks.last()?.start + toks.last()?.len);
    if matches!(toks[0].kind, TokenKind::KwLet) {
        let id = lower_let(ctx, toks, span)?;
        let Stmt::VarDecl {
            span,
            bindings,
            value,
        } = ctx.pool.stmt(id).clone()
        else {
            return None;
        };
        return Some(SimpleStmt::VarDecl {
            span,
            bindings,
            value,
        });
    }
    if let Some(id) = lower_set(ctx, toks, span, matches!(toks[0].kind, TokenKind::KwSet))
        && let Stmt::Set {
            span,
            places,
            op,
            value,
        } = ctx.pool.stmt(id).clone()
    {
        return Some(SimpleStmt::Set {
            span,
            places,
            op,
            value,
        });
    }
    let expr = try_hand_lower_expr_all(ctx.pool, ctx.source, toks, ctx.file_id)?;
    Some(SimpleStmt::Expr { span, expr })
}

fn lower_defer(
    ctx: &mut HandCtx<'_>,
    tokens: &[Token],
    stmt: &SyntaxNode,
    toks: &[&Token],
    span: Span,
    is_err: bool,
) -> Option<StmtId> {
    let blocks = block_children(stmt);
    let body = if let Some(block) = blocks.first() {
        let b = try_hand_lower_block(ctx.pool, ctx.source, tokens, block, ctx.file_id)?;
        DeferBody::Block {
            span: b.span,
            block: b,
        }
    } else {
        // defer expr;
        let expr = try_hand_lower_expr_all(ctx.pool, ctx.source, &toks[1..], ctx.file_id)?;
        DeferBody::Expr {
            span: ctx.pool.expr_span(expr),
            expr,
        }
    };
    Some(ctx.pool.alloc_stmt(if is_err {
        Stmt::ErrDefer { span, body }
    } else {
        Stmt::Defer { span, body }
    }))
}

fn lower_unsafe(
    ctx: &mut HandCtx<'_>,
    tokens: &[Token],
    stmt: &SyntaxNode,
    _toks: &[&Token],
    span: Span,
) -> Option<StmtId> {
    let block = block_children(stmt).into_iter().next()?;
    let block = try_hand_lower_block(ctx.pool, ctx.source, tokens, &block, ctx.file_id)?;
    Some(ctx.pool.alloc_stmt(Stmt::Unsafe { span, block }))
}

