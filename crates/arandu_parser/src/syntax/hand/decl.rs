//! Hand-lower top-level declarations (module, import, func, struct, …).

use super::cursor::{Cursor, HandCtx, drop_trailing_semi, tokens_in_range};
use super::expr::try_hand_lower_expr;
use super::stmt::try_hand_lower_block;
use super::ty::{parse_dotted_ident_path, parse_result_type, parse_type};
use crate::ast::ast_pool::AstPool;
use crate::syntax::kind::{SyntaxKind, SyntaxNode};
use crate::{
    Attribute, ConstDecl, EnumDecl, EnumPayload, EnumVariant, ExternDecl, FieldDecl, FuncDecl,
    FuncName, FuncSignature, GenericParam, ImportDecl, ImportItem, InterfaceDecl, ModuleDecl,
    Ownership, Param, StructDecl, TopLevelDecl, TypeAliasDecl, TypeName, Visibility, WhereItem,
};
use arandu_lexer::{Span, Token, TokenKind};
use smallvec::{SmallVec, smallvec};
use smol_str::SmolStr;

/// Hand-lower any top-level green item into a [`TopLevelDecl`] (not module/import).
#[must_use]
pub fn try_hand_lower_top_level(
    pool: &mut AstPool,
    source: &str,
    tokens: &[Token],
    item: &SyntaxNode,
    file_id: u32,
) -> Option<TopLevelDecl> {
    match item.kind() {
        SyntaxKind::FUNC_ITEM => {
            try_hand_lower_func_item(pool, source, tokens, item, file_id).map(TopLevelDecl::Func)
        }
        SyntaxKind::STRUCT_ITEM => {
            try_hand_lower_struct(pool, source, tokens, item, file_id).map(TopLevelDecl::Struct)
        }
        SyntaxKind::ENUM_ITEM => {
            try_hand_lower_enum(pool, source, tokens, item, file_id).map(TopLevelDecl::Enum)
        }
        SyntaxKind::CONST_ITEM => {
            try_hand_lower_const(pool, source, tokens, item, file_id).map(TopLevelDecl::Const)
        }
        SyntaxKind::TYPE_ALIAS_ITEM => {
            try_hand_lower_type_alias(pool, source, tokens, item, file_id)
                .map(TopLevelDecl::TypeAlias)
        }
        SyntaxKind::INTERFACE_ITEM => try_hand_lower_interface(pool, source, tokens, item, file_id)
            .map(TopLevelDecl::Interface),
        SyntaxKind::EXTERN_ITEM => {
            try_hand_lower_extern(pool, source, tokens, item, file_id).map(TopLevelDecl::Extern)
        }
        SyntaxKind::ITEM => {
            // Heuristic: try func / const / struct by leading keyword after attrs.
            try_hand_lower_func_item(pool, source, tokens, item, file_id)
                .map(TopLevelDecl::Func)
                .or_else(|| {
                    try_hand_lower_const(pool, source, tokens, item, file_id)
                        .map(TopLevelDecl::Const)
                })
                .or_else(|| {
                    try_hand_lower_struct(pool, source, tokens, item, file_id)
                        .map(TopLevelDecl::Struct)
                })
        }
        _ => None,
    }
}

fn item_tokens<'a>(tokens: &'a [Token], item: &SyntaxNode) -> Vec<&'a Token> {
    let r = item.text_range();
    let mut toks = tokens_in_range(tokens, u32::from(r.start()), u32::from(r.end()));
    drop_trailing_semi(&mut toks);
    toks
}

/// Span from first..last significant token (matches RD marks better than green ranges,
/// which often include leading whitespace between items).
fn token_bounds_span(file_id: u32, toks: &[&Token]) -> Option<Span> {
    let first = *toks.first()?;
    let last = *toks.last()?;
    Some(Span::new(file_id, first.start, last.start + last.len))
}

fn parse_visibility(cur: &mut Cursor<'_>) -> Visibility {
    if cur.eat(TokenKind::KwPublic) {
        Visibility::Public
    } else {
        Visibility::Private
    }
}

/// `@name` or `@name(...)` — args must be hand-lowerable exprs.
fn parse_attributes(ctx: &mut HandCtx<'_>, cur: &mut Cursor<'_>) -> Option<Vec<Attribute>> {
    let mut attrs = Vec::new();
    while cur.peek_kind() == Some(TokenKind::At) {
        let at = cur.bump()?;
        let name_tok = cur.peek()?;
        if !matches!(name_tok.kind, TokenKind::IdentValue | TokenKind::IdentType) {
            return None;
        }
        let name = SmolStr::new(ctx.text(name_tok)?);
        let start = at.start;
        cur.bump();
        let mut args = Vec::new();
        let mut end = name_tok.start + name_tok.len;
        if cur.eat(TokenKind::LParen) {
            if cur.peek_kind() != Some(TokenKind::RParen) {
                loop {
                    // Attr args may be bare string literals
                    args.push(try_hand_lower_expr(ctx, cur, 0)?);
                    if cur.eat(TokenKind::Comma) {
                        continue;
                    }
                    break;
                }
            }
            let close = cur.expect(TokenKind::RParen)?;
            end = close.start + close.len;
        }
        attrs.push(Attribute {
            span: ctx.span(start, end),
            name,
            args,
        });
    }
    Some(attrs)
}

fn parse_generic_params(
    ctx: &mut HandCtx<'_>,
    cur: &mut Cursor<'_>,
) -> Option<SmallVec<[GenericParam; 2]>> {
    if !cur.eat(TokenKind::Lt) {
        return Some(smallvec![]);
    }
    let mut params = SmallVec::new();
    if cur.peek_kind() != Some(TokenKind::Gt) {
        loop {
            let name_tok = cur.peek()?;
            if !matches!(name_tok.kind, TokenKind::IdentType | TokenKind::IdentValue) {
                return None;
            }
            let name = SmolStr::new(ctx.text(name_tok)?);
            let p_start = name_tok.start;
            cur.bump();
            let mut constraints = SmallVec::new();
            let mut p_end = name_tok.start + name_tok.len;
            if cur.eat(TokenKind::Colon) {
                loop {
                    let path_start = cur.peek()?.start;
                    let path = parse_dotted_ident_path(ctx, cur)?;
                    let last = path.last()?;
                    let path_end = path_start + last.len() as u32;
                    p_end = path_end;
                    constraints.push(TypeName {
                        span: ctx.span(path_start, path_end),
                        path,
                    });
                    if cur.eat(TokenKind::Plus) {
                        continue;
                    }
                    break;
                }
            }
            params.push(GenericParam {
                span: ctx.span(p_start, p_end),
                name,
                constraints,
            });
            if cur.eat(TokenKind::Comma) {
                continue;
            }
            break;
        }
    }
    cur.expect(TokenKind::Gt)?;
    Some(params)
}

fn parse_where_clause(
    ctx: &mut HandCtx<'_>,
    cur: &mut Cursor<'_>,
) -> Option<SmallVec<[WhereItem; 2]>> {
    if !cur.eat(TokenKind::KwWhere) {
        return Some(smallvec![]);
    }
    let mut items = SmallVec::new();
    loop {
        let name_tok = cur.peek()?;
        if !matches!(name_tok.kind, TokenKind::IdentType | TokenKind::IdentValue) {
            break;
        }
        let name = SmolStr::new(ctx.text(name_tok)?);
        let start = name_tok.start;
        cur.bump();
        cur.expect(TokenKind::Colon)?;
        let mut constraints = SmallVec::new();
        loop {
            let path_start = cur.peek()?.start;
            let path = parse_dotted_ident_path(ctx, cur)?;
            // End of last path segment: approximate from last segment text len.
            let last = path.last()?;
            let path_end = path_start + last.len() as u32;
            constraints.push(TypeName {
                span: ctx.span(path_start, path_end),
                path,
            });
            if cur.eat(TokenKind::Plus) {
                continue;
            }
            break;
        }
        let item_end = constraints
            .last()
            .map(|c: &TypeName| c.span.end)
            .unwrap_or(start + name_tok.len);
        items.push(WhereItem {
            span: ctx.span(start, item_end),
            name,
            constraints,
        });
        if cur.eat(TokenKind::Comma) {
            continue;
        }
        break;
    }
    Some(items)
}

fn parse_params(
    ctx: &mut HandCtx<'_>,
    cur: &mut Cursor<'_>,
    method_receiver: Option<&TypeName>,
) -> Option<Vec<Param>> {
    let mut params = Vec::new();
    if cur.peek_kind() == Some(TokenKind::RParen) {
        return Some(params);
    }
    loop {
        let p_start_tok = cur.peek()?;
        let p_start = p_start_tok.start;
        // ownership keywords
        let ownership = match cur.peek_kind() {
            Some(TokenKind::KwOwn) => {
                cur.bump();
                Some(Ownership::Own)
            }
            Some(TokenKind::KwMut) => {
                cur.bump();
                Some(Ownership::Mut)
            }
            Some(TokenKind::KwShared) => {
                cur.bump();
                Some(Ownership::Shared)
            }
            _ => None,
        };
        let name_tok = cur.peek()?;
        let (name, is_receiver) = match name_tok.kind {
            TokenKind::KwSelf => {
                cur.bump();
                (SmolStr::new_static("self"), true)
            }
            TokenKind::IdentValue => {
                let n = SmolStr::new(ctx.text(name_tok)?);
                cur.bump();
                (n, false)
            }
            _ => return None,
        };
        let is_variadic = cur.eat(TokenKind::Ellipsis);
        let ty = if cur.eat(TokenKind::Colon) {
            parse_type(ctx, cur)?
        } else if is_receiver {
            if let Some(recv) = method_receiver {
                let args = ctx.pool.alloc_type_expr_list(&[]);
                ctx.pool.alloc_type_expr(crate::TypeExpr::Named {
                    span: recv.span,
                    name: recv.clone(),
                    args,
                })
            } else {
                let span = ctx.token_span(name_tok);
                let args = ctx.pool.alloc_type_expr_list(&[]);
                ctx.pool.alloc_type_expr(crate::TypeExpr::Named {
                    span,
                    name: TypeName {
                        span,
                        path: smallvec![SmolStr::new_static("Self")],
                    },
                    args,
                })
            }
        } else {
            return None;
        };
        // When self has an explicit `: Type`, use type end; if type span was
        // synthesized from the method receiver name (elsewhere), clamp to `self`.
        let ty_span = ctx.pool.type_expr_span(ty);
        let p_end = if is_receiver && ty_span.start < p_start {
            name_tok.start + name_tok.len
        } else {
            ty_span.end.max(name_tok.start + name_tok.len)
        };
        params.push(Param {
            span: ctx.span(p_start, p_end),
            attrs: smallvec![],
            ownership,
            name,
            ty,
            is_variadic,
            is_receiver,
        });
        if cur.eat(TokenKind::Comma) {
            continue;
        }
        break;
    }
    Some(params)
}

/// Free/method `func`, with `public`/`async`/attrs/generics/where when present.
#[must_use]
pub fn try_hand_lower_func_item(
    pool: &mut AstPool,
    source: &str,
    tokens: &[Token],
    func: &SyntaxNode,
    file_id: u32,
) -> Option<FuncDecl> {
    let block_node = func.children().find(|n| n.kind() == SyntaxKind::BLOCK)?;
    let body = try_hand_lower_block(pool, source, tokens, &block_node, file_id)?;

    let fr = func.text_range();
    let fs = u32::from(fr.start());
    let bs = u32::from(block_node.text_range().start());
    let sig_toks = tokens_in_range(tokens, fs, bs);
    if sig_toks.is_empty() {
        return None;
    }

    let mut ctx = HandCtx {
        pool,
        source,
        file_id,
    };
    let mut cur = Cursor::new(&sig_toks);
    let attrs = parse_attributes(&mut ctx, &mut cur)?;
    let visibility = parse_visibility(&mut cur);
    let is_async = cur.eat(TokenKind::KwAsync);
    cur.expect(TokenKind::KwFunc)?;
    let name = parse_func_name(&mut ctx, &mut cur)?;
    let generic_params = parse_generic_params(&mut ctx, &mut cur)?;
    cur.expect(TokenKind::LParen)?;
    let recv = match &name {
        FuncName::Method { receiver, .. } => Some(receiver.clone()),
        FuncName::Free { .. } => None,
    };
    let params = parse_params(&mut ctx, &mut cur, recv.as_ref())?;
    cur.expect(TokenKind::RParen)?;
    let result = if cur.eat(TokenKind::Colon) {
        Some(parse_result_type(&mut ctx, &mut cur)?)
    } else {
        None
    };
    let where_clause = parse_where_clause(&mut ctx, &mut cur)?;
    if !cur.at_end() {
        return None;
    }

    // Span: RD marks at `async`/`func` after attrs+visibility are consumed.
    let sig_start = sig_toks
        .iter()
        .find(|t| matches!(t.kind, TokenKind::KwAsync | TokenKind::KwFunc))
        .map(|t| t.start)
        .or_else(|| sig_toks.first().map(|t| t.start))
        .unwrap_or(fs);
    let body_end = {
        let br = block_node.text_range();
        let bs = u32::from(br.start());
        let be = u32::from(br.end());
        tokens
            .iter()
            .rev()
            .find(|t| {
                matches!(t.kind, TokenKind::RBrace)
                    && !t.inserted
                    && t.start >= bs
                    && t.start < be.max(bs + 1)
            })
            .map(|t| t.start + t.len)
            .unwrap_or(body.span.end)
    };
    Some(FuncDecl {
        span: Span::new(file_id, sig_start, body_end),
        attrs: attrs.into(),
        visibility,
        is_async,
        name,
        generic_params,
        params,
        result,
        where_clause,
        body,
    })
}

fn parse_func_name(ctx: &mut HandCtx<'_>, cur: &mut Cursor<'_>) -> Option<FuncName> {
    let first = cur.peek()?;
    // Method: Type.method or path.Type.method — require IdentType/Value . IdentValue
    // without consuming multi-seg as free name.
    if matches!(first.kind, TokenKind::IdentType | TokenKind::IdentValue) {
        // Look ahead for `.` + method name and then `(`/`<`
        if cur
            .peek_at(1)
            .is_some_and(|t| matches!(t.kind, TokenKind::Dot))
            && cur
                .peek_at(2)
                .is_some_and(|t| matches!(t.kind, TokenKind::IdentValue))
            && cur
                .peek_at(3)
                .is_some_and(|t| matches!(t.kind, TokenKind::LParen | TokenKind::Lt))
        {
            let recv_tok = cur.bump()?;
            let recv_name = SmolStr::new(ctx.text(recv_tok)?);
            cur.expect(TokenKind::Dot)?;
            let method_tok = cur.expect(TokenKind::IdentValue)?;
            let method = SmolStr::new(ctx.text(method_tok)?);
            return Some(FuncName::Method {
                span: ctx.span(recv_tok.start, method_tok.start + method_tok.len),
                receiver: TypeName {
                    span: ctx.token_span(recv_tok),
                    path: smallvec![recv_name],
                },
                name: method,
            });
        }
    }
    let name_tok = cur.expect(TokenKind::IdentValue)?;
    Some(FuncName::Free {
        span: ctx.token_span(name_tok),
        name: SmolStr::new(ctx.text(name_tok)?),
    })
}

/// `module a.b.c`
#[must_use]
pub fn try_hand_lower_module(
    source: &str,
    tokens: &[Token],
    item: &SyntaxNode,
    file_id: u32,
) -> Option<ModuleDecl> {
    let toks = item_tokens(tokens, item);
    if toks.is_empty() || !matches!(toks[0].kind, TokenKind::KwModule) {
        return None;
    }
    let mut throwaway = AstPool::new();
    let ctx = HandCtx {
        pool: &mut throwaway,
        source,
        file_id,
    };
    let mut cur = Cursor::new(&toks);
    cur.expect(TokenKind::KwModule)?;
    let path = parse_dotted_ident_path(&ctx, &mut cur)?;
    // Optional `;` terminator.
    let _ = cur.eat(TokenKind::Semicolon);
    if !cur.at_end() {
        return None;
    }
    // Same-line top-level without `;`/newline must be rejected (match RD).
    if let Some(last) = toks.last().copied() {
        let end = last.start + last.len;
        if let Some(next) = tokens
            .iter()
            .find(|t| !matches!(t.kind, TokenKind::Eof) && t.start >= end)
        {
            let between = source.get(end as usize..next.start as usize).unwrap_or("");
            let has_nl = between.contains('\n');
            let has_semi = toks.iter().any(|t| matches!(t.kind, TokenKind::Semicolon));
            if !has_nl && !has_semi && starts_top_level_decl(next.kind) {
                return None;
            }
        }
    }
    Some(ModuleDecl {
        span: token_bounds_span(file_id, &toks)?,
        path,
    })
}

fn starts_top_level_decl(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::KwConst
            | TokenKind::KwType
            | TokenKind::KwFunc
            | TokenKind::KwAsync
            | TokenKind::KwStruct
            | TokenKind::KwEnum
            | TokenKind::KwInterface
            | TokenKind::KwExtern
            | TokenKind::KwPublic
            | TokenKind::At
            | TokenKind::KwImport
            | TokenKind::KwFrom
    )
}

/// All import forms.
#[must_use]
pub fn try_hand_lower_import(
    source: &str,
    tokens: &[Token],
    item: &SyntaxNode,
    file_id: u32,
) -> Option<ImportDecl> {
    let toks = item_tokens(tokens, item);
    if toks.is_empty() {
        return None;
    }
    let mut pool = AstPool::new();
    let mut ctx = HandCtx {
        pool: &mut pool,
        source,
        file_id,
    };
    let mut cur = Cursor::new(&toks);
    let span = token_bounds_span(file_id, &toks)?;

    if cur.eat(TokenKind::KwFrom) {
        // from path import { A, B as C }  OR  from "ext" import { … }
        if cur.peek_kind() == Some(TokenKind::StringStart) {
            let source_s = parse_static_string(&mut ctx, &mut cur)?;
            cur.expect(TokenKind::KwImport)?;
            let items = parse_import_brace_list(&mut ctx, &mut cur)?;
            if !cur.at_end() {
                return None;
            }
            return Some(ImportDecl::ExternalNamed {
                span,
                items,
                source: source_s,
            });
        }
        let path = parse_dotted_ident_path(&ctx, &mut cur)?;
        cur.expect(TokenKind::KwImport)?;
        let items = parse_import_brace_list(&mut ctx, &mut cur)?;
        if !cur.at_end() {
            return None;
        }
        return Some(ImportDecl::Named { span, items, path });
    }

    if cur.eat(TokenKind::KwImport) {
        if cur.peek_kind() == Some(TokenKind::StringStart) {
            let source_s = parse_static_string(&mut ctx, &mut cur)?;
            cur.expect(TokenKind::KwAs)?;
            let alias_tok = cur.expect(TokenKind::IdentValue)?;
            let alias = SmolStr::new(ctx.text(alias_tok)?);
            if !cur.at_end() {
                return None;
            }
            return Some(ImportDecl::ExternalAlias {
                span,
                source: source_s,
                alias,
            });
        }
        let path = parse_dotted_ident_path(&ctx, &mut cur)?;
        let alias = if cur.eat(TokenKind::KwAs) {
            let alias_tok = cur
                .peek()
                .filter(|t| matches!(t.kind, TokenKind::IdentValue | TokenKind::IdentType))?;
            let alias = SmolStr::new(ctx.text(alias_tok)?);
            cur.bump();
            alias
        } else {
            // RD default: last path segment
            path.last()?.clone()
        };
        if !cur.at_end() {
            return None;
        }
        return Some(ImportDecl::ModuleAlias { span, path, alias });
    }
    None
}

fn parse_static_string(ctx: &mut HandCtx<'_>, cur: &mut Cursor<'_>) -> Option<SmolStr> {
    cur.expect(TokenKind::StringStart)?;
    let text_tok = cur.expect(TokenKind::StringText)?;
    let s = SmolStr::new(ctx.text(text_tok)?);
    cur.expect(TokenKind::StringEnd)?;
    Some(s)
}

fn parse_import_brace_list(ctx: &mut HandCtx<'_>, cur: &mut Cursor<'_>) -> Option<Vec<ImportItem>> {
    cur.expect(TokenKind::LBrace)?;
    let mut items = Vec::new();
    if cur.peek_kind() != Some(TokenKind::RBrace) {
        loop {
            let name_tok = cur.peek()?;
            if !matches!(name_tok.kind, TokenKind::IdentValue | TokenKind::IdentType) {
                return None;
            }
            let name = SmolStr::new(ctx.text(name_tok)?);
            let start = name_tok.start;
            cur.bump();
            let mut end = name_tok.start + name_tok.len;
            let alias = if cur.eat(TokenKind::KwAs) {
                let a = cur.peek()?;
                let alias = SmolStr::new(ctx.text(a)?);
                end = a.start + a.len;
                cur.bump();
                Some(alias)
            } else {
                None
            };
            items.push(ImportItem {
                span: ctx.span(start, end),
                name,
                alias,
            });
            if cur.eat(TokenKind::Comma) {
                continue;
            }
            break;
        }
    }
    cur.expect(TokenKind::RBrace)?;
    Some(items)
}

fn try_hand_lower_const(
    pool: &mut AstPool,
    source: &str,
    tokens: &[Token],
    item: &SyntaxNode,
    file_id: u32,
) -> Option<ConstDecl> {
    let toks = item_tokens(tokens, item);
    let mut ctx = HandCtx {
        pool,
        source,
        file_id,
    };
    let mut cur = Cursor::new(&toks);
    let attrs = parse_attributes(&mut ctx, &mut cur)?;
    let visibility = parse_visibility(&mut cur);
    cur.expect(TokenKind::KwConst)?;
    let name_tok = cur.peek()?;
    if !matches!(name_tok.kind, TokenKind::IdentValue | TokenKind::IdentType) {
        return None;
    }
    let name = SmolStr::new(ctx.text(name_tok)?);
    cur.bump();
    let ty = if cur.peek_kind().is_some_and(|k| k != TokenKind::Equal) {
        Some(parse_type(&mut ctx, &mut cur)?)
    } else {
        None
    };
    cur.expect(TokenKind::Equal)?;
    let value = try_hand_lower_expr(&mut ctx, &mut cur, 0)?;
    if !cur.at_end() {
        return None;
    }
    Some(ConstDecl {
        span: token_bounds_span(file_id, &toks)?,
        attrs: attrs.into(),
        visibility,
        name,
        ty,
        value,
    })
}

fn try_hand_lower_type_alias(
    pool: &mut AstPool,
    source: &str,
    tokens: &[Token],
    item: &SyntaxNode,
    file_id: u32,
) -> Option<TypeAliasDecl> {
    let toks = item_tokens(tokens, item);
    let mut ctx = HandCtx {
        pool,
        source,
        file_id,
    };
    let mut cur = Cursor::new(&toks);
    let attrs = parse_attributes(&mut ctx, &mut cur)?;
    let visibility = parse_visibility(&mut cur);
    cur.expect(TokenKind::KwType)?;
    let name_tok = cur.expect(TokenKind::IdentType).or_else(|| {
        cur.peek()
            .filter(|t| matches!(t.kind, TokenKind::IdentValue))
            .and_then(|_| cur.bump())
    })?;
    let name = SmolStr::new(ctx.text(name_tok)?);
    let generic_params = parse_generic_params(&mut ctx, &mut cur)?;
    cur.expect(TokenKind::Equal)?;
    let ty = parse_type(&mut ctx, &mut cur)?;
    if !cur.at_end() {
        return None;
    }
    Some(TypeAliasDecl {
        span: token_bounds_span(file_id, &toks)?,
        attrs: attrs.into(),
        visibility,
        name,
        generic_params,
        ty,
    })
}

fn try_hand_lower_struct(
    pool: &mut AstPool,
    source: &str,
    tokens: &[Token],
    item: &SyntaxNode,
    file_id: u32,
) -> Option<StructDecl> {
    let toks = item_tokens(tokens, item);
    let mut ctx = HandCtx {
        pool,
        source,
        file_id,
    };
    let mut cur = Cursor::new(&toks);
    let attrs = parse_attributes(&mut ctx, &mut cur)?;
    let visibility = parse_visibility(&mut cur);
    cur.expect(TokenKind::KwStruct)?;
    let name_tok = cur
        .peek()
        .filter(|t| matches!(t.kind, TokenKind::IdentType | TokenKind::IdentValue))?;
    let name = SmolStr::new(ctx.text(name_tok)?);
    cur.bump();
    let generic_params = parse_generic_params(&mut ctx, &mut cur)?;
    let where_clause = parse_where_clause(&mut ctx, &mut cur)?;
    cur.expect(TokenKind::LBrace)?;
    let mut fields = Vec::new();
    while cur.peek_kind() != Some(TokenKind::RBrace) && !cur.at_end() {
        while cur.eat(TokenKind::Semicolon) {}
        if cur.peek_kind() == Some(TokenKind::RBrace) {
            break;
        }
        fields.push(parse_field(&mut ctx, &mut cur, true)?);
    }
    cur.expect(TokenKind::RBrace)?;
    if !cur.at_end() {
        return None;
    }
    // Span: RD marks at `struct` after attrs+visibility.
    let start = toks
        .iter()
        .find(|t| matches!(t.kind, TokenKind::KwStruct))
        .map(|t| t.start)
        .or_else(|| toks.first().map(|t| t.start))?;
    let end = toks.last().map(|t| t.start + t.len)?;
    Some(StructDecl {
        span: Span::new(file_id, start, end),
        attrs: attrs.into(),
        visibility,
        name,
        generic_params,
        where_clause,
        fields,
    })
}

fn parse_field(
    ctx: &mut HandCtx<'_>,
    cur: &mut Cursor<'_>,
    require_semi: bool,
) -> Option<FieldDecl> {
    let field_start = cur.peek()?.start;
    let attrs = parse_attributes(ctx, cur)?;
    let visibility = parse_visibility(cur);
    let name_tok = cur.expect(TokenKind::IdentValue)?;
    let name = SmolStr::new(ctx.text(name_tok)?);
    cur.expect(TokenKind::Colon)?;
    let ty = parse_type(ctx, cur)?;
    if require_semi {
        let _ = cur.eat(TokenKind::Semicolon);
    } else {
        let _ = cur.eat(TokenKind::Comma);
        let _ = cur.eat(TokenKind::Semicolon);
    }
    let end = ctx.pool.type_expr_span(ty).end;
    Some(FieldDecl {
        span: ctx.span(field_start, end),
        attrs: attrs.into(),
        visibility,
        name,
        ty,
    })
}

fn try_hand_lower_enum(
    pool: &mut AstPool,
    source: &str,
    tokens: &[Token],
    item: &SyntaxNode,
    file_id: u32,
) -> Option<EnumDecl> {
    let toks = item_tokens(tokens, item);
    let mut ctx = HandCtx {
        pool,
        source,
        file_id,
    };
    let mut cur = Cursor::new(&toks);
    let attrs = parse_attributes(&mut ctx, &mut cur)?;
    let visibility = parse_visibility(&mut cur);
    cur.expect(TokenKind::KwEnum)?;
    let name_tok = cur
        .peek()
        .filter(|t| matches!(t.kind, TokenKind::IdentType | TokenKind::IdentValue))?;
    let name = SmolStr::new(ctx.text(name_tok)?);
    cur.bump();
    let generic_params = parse_generic_params(&mut ctx, &mut cur)?;
    let where_clause = parse_where_clause(&mut ctx, &mut cur)?;
    cur.expect(TokenKind::LBrace)?;
    let mut variants = Vec::new();
    while cur.peek_kind() != Some(TokenKind::RBrace) && !cur.at_end() {
        while cur.eat(TokenKind::Semicolon) || cur.eat(TokenKind::Comma) {}
        if cur.peek_kind() == Some(TokenKind::RBrace) {
            break;
        }
        variants.push(parse_enum_variant(&mut ctx, &mut cur)?);
        let _ = cur.eat(TokenKind::Comma);
        let _ = cur.eat(TokenKind::Semicolon);
    }
    cur.expect(TokenKind::RBrace)?;
    if !cur.at_end() {
        return None;
    }
    Some(EnumDecl {
        span: token_bounds_span(file_id, &toks)?,
        attrs: attrs.into(),
        visibility,
        name,
        generic_params,
        where_clause,
        variants,
    })
}

fn parse_enum_variant(ctx: &mut HandCtx<'_>, cur: &mut Cursor<'_>) -> Option<EnumVariant> {
    let attrs = parse_attributes(ctx, cur)?;
    let name_tok = cur
        .peek()
        .filter(|t| matches!(t.kind, TokenKind::IdentType | TokenKind::IdentValue))?;
    let name = SmolStr::new(ctx.text(name_tok)?);
    let start = name_tok.start;
    cur.bump();
    let payload = if cur.eat(TokenKind::LParen) {
        let mut types = Vec::new();
        if cur.peek_kind() != Some(TokenKind::RParen) {
            loop {
                types.push(parse_type(ctx, cur)?);
                if cur.eat(TokenKind::Comma) {
                    continue;
                }
                break;
            }
        }
        let close = cur.expect(TokenKind::RParen)?;
        let range = ctx.pool.alloc_type_expr_list(&types);
        Some(EnumPayload::Tuple {
            span: ctx.span(start, close.start + close.len),
            types: range,
        })
    } else if cur.eat(TokenKind::LBrace) {
        let mut fields = Vec::new();
        while cur.peek_kind() != Some(TokenKind::RBrace) && !cur.at_end() {
            fields.push(parse_field(ctx, cur, false)?);
            let _ = cur.eat(TokenKind::Comma);
        }
        let close = cur.expect(TokenKind::RBrace)?;
        Some(EnumPayload::Struct {
            span: ctx.span(start, close.start + close.len),
            fields,
        })
    } else {
        None
    };
    let end = match &payload {
        Some(EnumPayload::Tuple { span, .. } | EnumPayload::Struct { span, .. }) => span.end,
        None => name_tok.start + name_tok.len,
    };
    Some(EnumVariant {
        span: ctx.span(start, end),
        attrs: attrs.into(),
        name,
        payload,
    })
}

fn try_hand_lower_interface(
    pool: &mut AstPool,
    source: &str,
    tokens: &[Token],
    item: &SyntaxNode,
    file_id: u32,
) -> Option<InterfaceDecl> {
    let toks = item_tokens(tokens, item);
    let mut ctx = HandCtx {
        pool,
        source,
        file_id,
    };
    let mut cur = Cursor::new(&toks);
    let attrs = parse_attributes(&mut ctx, &mut cur)?;
    let visibility = parse_visibility(&mut cur);
    cur.expect(TokenKind::KwInterface)?;
    let name_tok = cur
        .peek()
        .filter(|t| matches!(t.kind, TokenKind::IdentType | TokenKind::IdentValue))?;
    let name = SmolStr::new(ctx.text(name_tok)?);
    cur.bump();
    let generic_params = parse_generic_params(&mut ctx, &mut cur)?;
    let where_clause = parse_where_clause(&mut ctx, &mut cur)?;
    cur.expect(TokenKind::LBrace)?;
    let mut members = Vec::new();
    while cur.peek_kind() != Some(TokenKind::RBrace) && !cur.at_end() {
        while cur.eat(TokenKind::Semicolon) {}
        if cur.peek_kind() == Some(TokenKind::RBrace) {
            break;
        }
        members.push(parse_func_signature(&mut ctx, &mut cur)?);
        let _ = cur.eat(TokenKind::Semicolon);
    }
    cur.expect(TokenKind::RBrace)?;
    if !cur.at_end() {
        return None;
    }
    Some(InterfaceDecl {
        span: token_bounds_span(file_id, &toks)?,
        attrs: attrs.into(),
        visibility,
        name,
        generic_params,
        where_clause,
        members,
    })
}

fn parse_func_signature(ctx: &mut HandCtx<'_>, cur: &mut Cursor<'_>) -> Option<FuncSignature> {
    let attrs = parse_attributes(ctx, cur)?;
    let start = cur.peek()?.start;
    cur.expect(TokenKind::KwFunc)?;
    let name_tok = cur.expect(TokenKind::IdentValue)?;
    let name = SmolStr::new(ctx.text(name_tok)?);
    let generic_params = parse_generic_params(ctx, cur)?;
    cur.expect(TokenKind::LParen)?;
    let params = parse_params(ctx, cur, None)?;
    cur.expect(TokenKind::RParen)?;
    let result = if cur.eat(TokenKind::Colon) {
        Some(parse_result_type(ctx, cur)?)
    } else {
        None
    };
    let where_clause = parse_where_clause(ctx, cur)?;
    let end = result
        .as_ref()
        .map(|r| match r {
            crate::ResultType::Single { span, .. } => span.end,
            crate::ResultType::Multi { span, .. } => span.end,
        })
        .unwrap_or(name_tok.start + name_tok.len);
    Some(FuncSignature {
        span: ctx.span(start, end),
        attrs: attrs.into(),
        name,
        generic_params,
        params,
        result,
        where_clause,
    })
}

fn try_hand_lower_extern(
    pool: &mut AstPool,
    source: &str,
    tokens: &[Token],
    item: &SyntaxNode,
    file_id: u32,
) -> Option<ExternDecl> {
    let toks = item_tokens(tokens, item);
    let mut ctx = HandCtx {
        pool,
        source,
        file_id,
    };
    let mut cur = Cursor::new(&toks);
    let attrs = parse_attributes(&mut ctx, &mut cur)?;
    cur.expect(TokenKind::KwExtern)?;
    let abi = parse_static_string(&mut ctx, &mut cur)?;
    cur.expect(TokenKind::LBrace)?;
    let mut members = Vec::new();
    while cur.peek_kind() != Some(TokenKind::RBrace) && !cur.at_end() {
        while cur.eat(TokenKind::Semicolon) {}
        if cur.peek_kind() == Some(TokenKind::RBrace) {
            break;
        }
        members.push(parse_func_signature(&mut ctx, &mut cur)?);
        let _ = cur.eat(TokenKind::Semicolon);
    }
    cur.expect(TokenKind::RBrace)?;
    if !cur.at_end() {
        return None;
    }
    // Span starts at `extern` (attrs attached separately), ends at last token.
    let start = toks
        .iter()
        .find(|t| matches!(t.kind, TokenKind::KwExtern))
        .map(|t| t.start)
        .or_else(|| toks.first().map(|t| t.start))?;
    let end = toks.last().map(|t| t.start + t.len)?;
    Some(ExternDecl {
        span: Span::new(file_id, start, end),
        attrs: attrs.into(),
        abi,
        members,
    })
}
