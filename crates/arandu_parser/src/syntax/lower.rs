//! Green-guided lower: walk typed top-level items; RD at each item seek.
//!
//! **Body without full-file RD:** simple `STMT` nodes can be hand-lowered from
//! tokens ([`try_hand_lower_stmt`]); simple free `FUNC_ITEM`s with hand-lowerable
//! bodies skip RD entirely ([`try_hand_lower_func_item`]). Complex forms fall
//! back to seek + RD.

use super::SyntaxTree;
use super::kind::{SyntaxKind, SyntaxNode};
use crate::ast::ast_pool::{AstPool, ExprId, ExprKind, StmtId};
use crate::parser::{ParseError, ParseOutput, Parser};
use crate::{
    BinaryOp, BindingItem, Block, FuncDecl, FuncName, Program, ResultType, Stmt, TopLevelDecl,
    TypeExpr, TypeName, Visibility,
};
use arandu_lexer::{Span, Token, TokenKind};
use smallvec::smallvec;
use smol_str::SmolStr;
use std::sync::Arc;

/// Summary of structured green content (no heap AST).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GreenStructure {
    pub top_level_items: usize,
    pub func_items: usize,
    pub struct_items: usize,
    pub blocks: usize,
    pub stmts: usize,
    pub typed_items: usize,
}

/// Walk the CST and count structured nodes.
#[must_use]
pub fn inspect_green_structure(tree: &SyntaxTree) -> GreenStructure {
    let mut s = GreenStructure::default();
    for item in tree.items() {
        s.top_level_items += 1;
        if item.kind() != SyntaxKind::ITEM {
            s.typed_items += 1;
        }
        match item.kind() {
            SyntaxKind::FUNC_ITEM => s.func_items += 1,
            SyntaxKind::STRUCT_ITEM => s.struct_items += 1,
            _ => {}
        }
        for n in item.descendants() {
            match n.kind() {
                SyntaxKind::BLOCK => s.blocks += 1,
                SyntaxKind::STMT => s.stmts += 1,
                _ => {}
            }
        }
    }
    s
}

/// True when every top-level child is a typed `*_ITEM` (not generic `ITEM`).
#[must_use]
pub fn is_fully_typed_toplevel(tree: &SyntaxTree) -> bool {
    let items = tree.items();
    !items.is_empty() && items.iter().all(|n| n.kind() != SyntaxKind::ITEM)
}

/// First `FUNC_ITEM` node, if any.
#[must_use]
pub fn first_func_item(tree: &SyntaxTree) -> Option<super::kind::SyntaxNode> {
    tree.items()
        .into_iter()
        .find(|n| n.kind() == SyntaxKind::FUNC_ITEM)
}

/// Body `BLOCK` of a function item node.
#[must_use]
pub fn func_body_block(func: &super::kind::SyntaxNode) -> Option<super::kind::SyntaxNode> {
    func.children().find(|n| n.kind() == SyntaxKind::BLOCK)
}

/// Count `STMT` children inside a `BLOCK` (direct or nested one level).
#[must_use]
pub fn block_stmt_count(block: &super::kind::SyntaxNode) -> usize {
    block
        .children()
        .filter(|n| n.kind() == SyntaxKind::STMT)
        .count()
}

/// Green-guided lower: walk top-level green items and parse each with RD at its offset.
///
/// Falls back to a full linear `parse_program` if the tree has no top-level items
/// or the walk cannot consume the file cleanly.
pub fn lower_from_green(tree: &SyntaxTree, file_id: u32) -> Result<Program, ParseError> {
    let output = lower_from_green_recovering(tree, file_id);
    if let Some(err) = output.diagnostics.into_iter().next() {
        Err(err)
    } else {
        Ok(output.program)
    }
}

/// Recovering green-guided lower (keeps diagnostics).
#[must_use]
pub fn lower_from_green_recovering(tree: &SyntaxTree, file_id: u32) -> ParseOutput {
    let mut lex_diags: Vec<ParseError> = tree
        .lex_diagnostics()
        .iter()
        .copied()
        .map(|err| ParseError::from_lex(err, file_id))
        .collect();

    let items = tree.items();
    if items.is_empty() {
        // Empty / unstructured: full linear parse.
        return crate::parser::parse_token_stream(
            tree.text(),
            Arc::clone(tree.tokens_arc()),
            file_id,
            lex_diags,
        );
    }

    let mut parser = Parser::new(tree.text(), Arc::clone(tree.tokens_arc())).with_file_id(file_id);
    let start = parser.mark();
    let mut module = None;
    let mut imports = Vec::new();
    let mut decls = Vec::new();
    let mut walk_ok = true;

    for item in &items {
        let off = u32::from(item.text_range().start());
        parser.seek_to_item_start(off);
        parser.skip_semicolons();
        parser.collect_doc_comments();

        match item.kind() {
            SyntaxKind::MODULE_ITEM => match parser.parse_module() {
                Ok(m) => module = Some(m),
                Err(err) => {
                    parser.report_error(err);
                    parser.synchronize_top_level();
                    walk_ok = false;
                }
            },
            SyntaxKind::IMPORT_ITEM => match parser.parse_import() {
                Ok(import) => imports.push(import),
                Err(err) => {
                    parser.report_error(err);
                    parser.synchronize_top_level();
                    walk_ok = false;
                }
            },
            SyntaxKind::FUNC_ITEM => {
                // Prefer green body + simple signature without RD when possible.
                if let Some(func) = try_hand_lower_func_item(
                    &mut parser.pool,
                    tree.text(),
                    tree.tokens(),
                    item,
                    file_id,
                ) {
                    let end = u32::from(item.text_range().end());
                    parser.seek_to_byte(end);
                    let decl_id = parser.pool.alloc_decl(TopLevelDecl::Func(func));
                    decls.push(decl_id);
                } else {
                    match parser.parse_top_level_decl() {
                        Ok(decl) => {
                            let decl_id = parser.pool.alloc_decl(decl);
                            decls.push(decl_id);
                        }
                        Err(err) => {
                            parser.report_error(err);
                            parser.synchronize_top_level();
                            walk_ok = false;
                        }
                    }
                }
            }
            SyntaxKind::STRUCT_ITEM
            | SyntaxKind::ENUM_ITEM
            | SyntaxKind::INTERFACE_ITEM
            | SyntaxKind::CONST_ITEM
            | SyntaxKind::TYPE_ALIAS_ITEM
            | SyntaxKind::EXTERN_ITEM
            | SyntaxKind::ITEM => match parser.parse_top_level_decl() {
                Ok(decl) => {
                    let decl_id = parser.pool.alloc_decl(decl);
                    decls.push(decl_id);
                }
                Err(err) => {
                    parser.report_error(err);
                    parser.synchronize_top_level();
                    walk_ok = false;
                }
            },
            _ => {
                walk_ok = false;
            }
        }
    }

    // Prefer full linear RD when walk is incomplete or reported errors.
    // Green walk still builds structure (STMT/BLOCK) for IDE; AST correctness
    // stays on the proven full-token RD path for complex prefixes/attrs.
    let decl_like = items
        .iter()
        .filter(|n| !matches!(n.kind(), SyntaxKind::MODULE_ITEM | SyntaxKind::IMPORT_ITEM))
        .count();
    let import_like = items
        .iter()
        .filter(|n| n.kind() == SyntaxKind::IMPORT_ITEM)
        .count();
    let need_fallback = !walk_ok
        || !parser.diagnostics.is_empty()
        || decls.len() != decl_like
        || imports.len() != import_like
        || (items.iter().any(|n| n.kind() == SyntaxKind::MODULE_ITEM) && module.is_none());

    if need_fallback {
        return crate::parser::parse_token_stream(
            tree.text(),
            Arc::clone(tree.tokens_arc()),
            file_id,
            lex_diags,
        );
    }

    let program = Program {
        span: parser.span_from_mark(start),
        module,
        imports,
        decls,
        docs: std::mem::take(&mut parser.docs),
        pool: std::mem::take(&mut parser.pool),
    };

    lex_diags.extend(std::mem::take(&mut parser.diagnostics));
    ParseOutput {
        program,
        diagnostics: lex_diags,
    }
}

/// Number of top-level decls that are not `Error` shells.
#[must_use]
pub fn decl_count(program: &Program) -> usize {
    program
        .decls
        .iter()
        .filter(|id| !matches!(program.pool.decl(**id), TopLevelDecl::Error(_)))
        .count()
}

/// Collect non-EOF, non-inserted-semicolon tokens inside `[start, end)`.
fn tokens_in_range(tokens: &[Token], start: u32, end: u32) -> Vec<&Token> {
    tokens
        .iter()
        .filter(|t| {
            !matches!(t.kind, TokenKind::Eof)
                && t.start >= start
                && t.start < end
                && !(t.kind == TokenKind::Semicolon && t.inserted)
        })
        .collect()
}

fn token_text<'a>(source: &'a str, t: &Token) -> Option<&'a str> {
    let ts = t.start as usize;
    let te = t.start.saturating_add(t.len) as usize;
    source.get(ts..te.min(source.len()))
}

fn token_span(file_id: u32, t: &Token) -> Span {
    Span::new(file_id, t.start, t.start + t.len)
}

/// Binary op binding power (left, right) for simple arithmetic.
fn bin_bp(kind: TokenKind) -> Option<(u8, u8, BinaryOp)> {
    match kind {
        TokenKind::Plus => Some((1, 2, BinaryOp::Add)),
        TokenKind::Minus => Some((1, 2, BinaryOp::Sub)),
        TokenKind::Star => Some((3, 4, BinaryOp::Mul)),
        TokenKind::Slash => Some((3, 4, BinaryOp::Div)),
        _ => None,
    }
}

/// Hand-lower a simple expression (int, path, binary `+ - * /`).
///
/// Returns `(expr, next_index)` over `toks`.
fn try_hand_lower_expr(
    pool: &mut AstPool,
    source: &str,
    toks: &[&Token],
    mut pos: usize,
    min_bp: u8,
    file_id: u32,
) -> Option<(ExprId, usize)> {
    if pos >= toks.len() {
        return None;
    }
    let t = toks[pos];
    let mut left = match t.kind {
        TokenKind::IntDec | TokenKind::IntHex | TokenKind::IntBin | TokenKind::IntOct => {
            let text = token_text(source, t)?;
            pos += 1;
            pool.alloc_expr(
                ExprKind::Int {
                    value: SmolStr::new(text),
                },
                token_span(file_id, t),
            )
        }
        TokenKind::IdentValue => {
            let text = token_text(source, t)?;
            pos += 1;
            pool.alloc_expr(
                ExprKind::Path {
                    path: smallvec![SmolStr::new(text)],
                },
                token_span(file_id, t),
            )
        }
        TokenKind::LParen => {
            pos += 1;
            let (inner, next) = try_hand_lower_expr(pool, source, toks, pos, 0, file_id)?;
            pos = next;
            if pos >= toks.len() || !matches!(toks[pos].kind, TokenKind::RParen) {
                return None;
            }
            let close = toks[pos];
            pos += 1;
            let span = Span::new(file_id, t.start, close.start + close.len);
            pool.alloc_expr(ExprKind::Group { expr: inner }, span)
        }
        _ => return None,
    };

    loop {
        if pos >= toks.len() {
            break;
        }
        let Some((l_bp, r_bp, op)) = bin_bp(toks[pos].kind) else {
            break;
        };
        if l_bp < min_bp {
            break;
        }
        pos += 1;
        let (right, next) = try_hand_lower_expr(pool, source, toks, pos, r_bp, file_id)?;
        pos = next;
        let left_span = pool.expr_span(left);
        let right_span = pool.expr_span(right);
        let span = Span::new(file_id, left_span.start, right_span.end);
        left = pool.alloc_expr(ExprKind::Binary { op, left, right }, span);
    }

    Some((left, pos))
}

/// Hand-lower the full token slice as one expression (must consume all).
fn try_hand_lower_expr_all(
    pool: &mut AstPool,
    source: &str,
    toks: &[&Token],
    file_id: u32,
) -> Option<ExprId> {
    if toks.is_empty() {
        return None;
    }
    let (expr, end) = try_hand_lower_expr(pool, source, toks, 0, 0, file_id)?;
    if end != toks.len() {
        return None;
    }
    Some(expr)
}

/// Map a primitive type token kind to its source name.
fn primitive_type_token_name(kind: TokenKind) -> Option<&'static str> {
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

/// Hand-lower a simple type token (`int`, `IdentType`, single-segment name).
fn try_hand_lower_type(
    pool: &mut AstPool,
    source: &str,
    t: &Token,
    file_id: u32,
) -> Option<crate::ast::ast_pool::TypeExprId> {
    let span = token_span(file_id, t);
    if let Some(name) = primitive_type_token_name(t.kind) {
        return Some(pool.alloc_type_expr(TypeExpr::Primitive {
            span,
            name: SmolStr::new_static(name),
        }));
    }
    match t.kind {
        TokenKind::IdentType | TokenKind::IdentValue => {
            let text = token_text(source, t)?;
            let name = TypeName {
                span,
                path: smallvec![SmolStr::new(text)],
            };
            let args = pool.alloc_type_expr_list(&[]);
            Some(pool.alloc_type_expr(TypeExpr::Named { span, name, args }))
        }
        _ => None,
    }
}

/// Hand-lower a green `STMT` without calling RD (simple patterns only).
///
/// Supports `break`/`continue`, `return`/`return <expr>` (int, path, binary
/// `+ - * /`), and `let [mut] name = <expr>` (optional trailing `;`).
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
    // Drop trailing explicit semicolon for matching.
    if toks
        .last()
        .is_some_and(|t| matches!(t.kind, TokenKind::Semicolon))
    {
        toks.pop();
    }
    if toks.is_empty() {
        return None;
    }
    let span = Span::new(file_id, s, e);
    match toks[0].kind {
        TokenKind::KwBreak if toks.len() == 1 => Some(pool.alloc_stmt(Stmt::Break { span })),
        TokenKind::KwContinue if toks.len() == 1 => Some(pool.alloc_stmt(Stmt::Continue { span })),
        TokenKind::KwReturn => {
            let values = if toks.len() == 1 {
                Vec::new()
            } else {
                let expr = try_hand_lower_expr_all(pool, source, &toks[1..], file_id)?;
                vec![expr]
            };
            Some(pool.alloc_stmt(Stmt::Return { span, values }))
        }
        TokenKind::KwLet => {
            // let [mut] name = expr
            let mut i = 1usize;
            if i >= toks.len() {
                return None;
            }
            let mutable = if matches!(toks[i].kind, TokenKind::KwMut) {
                i += 1;
                true
            } else {
                false
            };
            if i >= toks.len() || !matches!(toks[i].kind, TokenKind::IdentValue) {
                return None;
            }
            let name_tok = toks[i];
            let name = SmolStr::new(token_text(source, name_tok)?);
            let name_span = token_span(file_id, name_tok);
            i += 1;
            // Optional `: type` — skip for hand path when present (type-annotated let).
            if i < toks.len() && matches!(toks[i].kind, TokenKind::Colon) {
                return None;
            }
            if i >= toks.len() || !matches!(toks[i].kind, TokenKind::Equal) {
                return None;
            }
            i += 1;
            let value = try_hand_lower_expr_all(pool, source, &toks[i..], file_id)?;
            let binding = BindingItem {
                span: name_span,
                mutable,
                name,
                ty: None,
            };
            Some(pool.alloc_stmt(Stmt::VarDecl {
                span,
                bindings: vec![binding],
                value,
            }))
        }
        _ => None,
    }
}

/// Hand-lower every direct `STMT` child of a green `BLOCK`.
///
/// Requires a real closing `}` so incomplete sources fall back to RD diagnostics.
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
    // Incomplete `{` without `}`: refuse (green may still form a BLOCK node).
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

/// Hand-lower a simple free `FUNC_ITEM` (no attrs/async/generics/where) when the
/// body block is fully hand-lowerable.
///
/// Signature shape: `func name([p: T, ...])[: R] { ... }`
#[must_use]
pub fn try_hand_lower_func_item(
    pool: &mut AstPool,
    source: &str,
    tokens: &[Token],
    func: &SyntaxNode,
    file_id: u32,
) -> Option<FuncDecl> {
    let block_node = func_body_block(func)?;
    let body = try_hand_lower_block(pool, source, tokens, &block_node, file_id)?;

    let fr = func.text_range();
    let fs = u32::from(fr.start());
    let fe = u32::from(fr.end());
    let bs = u32::from(block_node.text_range().start());

    let sig_toks = tokens_in_range(tokens, fs, bs);
    if sig_toks.is_empty() {
        return None;
    }
    let mut i = 0usize;
    // Reject visibility / async / attrs (not hand-supported).
    if matches!(
        sig_toks[i].kind,
        TokenKind::KwPublic | TokenKind::KwAsync | TokenKind::At
    ) {
        return None;
    }
    if !matches!(sig_toks[i].kind, TokenKind::KwFunc) {
        return None;
    }
    i += 1;
    if i >= sig_toks.len() || !matches!(sig_toks[i].kind, TokenKind::IdentValue) {
        return None;
    }
    let name_tok = sig_toks[i];
    let name = SmolStr::new(token_text(source, name_tok)?);
    let name_span = token_span(file_id, name_tok);
    i += 1;
    // No generics.
    if i < sig_toks.len() && matches!(sig_toks[i].kind, TokenKind::Lt) {
        return None;
    }
    if i >= sig_toks.len() || !matches!(sig_toks[i].kind, TokenKind::LParen) {
        return None;
    }
    i += 1;

    let mut params = Vec::new();
    if i < sig_toks.len() && !matches!(sig_toks[i].kind, TokenKind::RParen) {
        loop {
            if i >= sig_toks.len() || !matches!(sig_toks[i].kind, TokenKind::IdentValue) {
                return None;
            }
            let p_name_tok = sig_toks[i];
            let p_name = SmolStr::new(token_text(source, p_name_tok)?);
            let p_start = p_name_tok.start;
            i += 1;
            if i >= sig_toks.len() || !matches!(sig_toks[i].kind, TokenKind::Colon) {
                return None;
            }
            i += 1;
            if i >= sig_toks.len() {
                return None;
            }
            let ty_tok = sig_toks[i];
            let ty = try_hand_lower_type(pool, source, ty_tok, file_id)?;
            i += 1;
            let p_span = Span::new(file_id, p_start, ty_tok.start + ty_tok.len);
            params.push(crate::Param {
                span: p_span,
                attrs: smallvec![],
                ownership: None,
                name: p_name,
                ty,
                is_variadic: false,
                is_receiver: false,
            });
            if i < sig_toks.len() && matches!(sig_toks[i].kind, TokenKind::Comma) {
                i += 1;
                continue;
            }
            break;
        }
    }
    if i >= sig_toks.len() || !matches!(sig_toks[i].kind, TokenKind::RParen) {
        return None;
    }
    i += 1;

    let result = if i < sig_toks.len() && matches!(sig_toks[i].kind, TokenKind::Colon) {
        i += 1;
        if i >= sig_toks.len() {
            return None;
        }
        let ty_tok = sig_toks[i];
        let ty = try_hand_lower_type(pool, source, ty_tok, file_id)?;
        let ty_span = token_span(file_id, ty_tok);
        i += 1;
        Some(ResultType::Single {
            span: ty_span,
            ty,
        })
    } else {
        None
    };

    // Signature must be fully consumed (no trailing where/etc.).
    if i != sig_toks.len() {
        return None;
    }

    Some(FuncDecl {
        span: Span::new(file_id, fs, fe),
        attrs: smallvec![],
        visibility: Visibility::Private,
        is_async: false,
        name: FuncName::Free {
            span: name_span,
            name,
        },
        generic_params: smallvec![],
        params,
        result,
        where_clause: smallvec![],
        body,
    })
}

/// Count how many green STMTs under `tree` hand-lower without RD.
#[must_use]
pub fn count_hand_lowerable_stmts(tree: &SyntaxTree) -> (usize, usize) {
    let mut total = 0usize;
    let mut hand = 0usize;
    let mut pool = AstPool::new();
    for item in tree.items() {
        for n in item.descendants() {
            if n.kind() != SyntaxKind::STMT {
                continue;
            }
            total += 1;
            if try_hand_lower_stmt(&mut pool, tree.text(), tree.tokens(), &n, 0).is_some() {
                hand += 1;
            }
        }
    }
    (total, hand)
}

/// Count top-level `FUNC_ITEM`s that fully hand-lower (signature + body).
#[must_use]
pub fn count_hand_lowerable_funcs(tree: &SyntaxTree) -> (usize, usize) {
    let mut total = 0usize;
    let mut hand = 0usize;
    let mut pool = AstPool::new();
    for item in tree.items() {
        if item.kind() != SyntaxKind::FUNC_ITEM {
            continue;
        }
        total += 1;
        if try_hand_lower_func_item(&mut pool, tree.text(), tree.tokens(), &item, 0).is_some() {
            hand += 1;
        }
    }
    (total, hand)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::parse_syntax;

    #[test]
    fn structured_func_has_block_and_stmts() {
        let tree = parse_syntax("func main(): int {\n    return 1\n}\n");
        let s = inspect_green_structure(&tree);
        assert_eq!(s.func_items, 1);
        assert_eq!(s.blocks, 1);
        assert!(
            s.stmts >= 1,
            "expected STMT under BLOCK for return, got {s:?}"
        );
        assert!(is_fully_typed_toplevel(&tree));
        let f = first_func_item(&tree).expect("func");
        let b = func_body_block(&f).expect("block");
        assert!(block_stmt_count(&b) >= 1);
    }

    #[test]
    fn two_funcs_two_blocks() {
        let src = "func a(): int { return 1 }\nfunc b(): int { return 2 }\n";
        let tree = parse_syntax(src);
        let s = inspect_green_structure(&tree);
        assert_eq!(s.func_items, 2);
        assert_eq!(s.blocks, 2);
        assert!(s.stmts >= 2);
    }

    #[test]
    fn struct_item_has_block() {
        let tree = parse_syntax("struct Point {\n    x: int\n}\n");
        let s = inspect_green_structure(&tree);
        assert_eq!(s.struct_items, 1);
        assert!(s.blocks >= 1);
    }

    #[test]
    fn green_guided_lower_matches_full_rd() {
        let src = "func alpha(): int {\n    return 1\n}\n\nfunc beta(): int {\n    return 2\n}\n";
        let tree = parse_syntax(src);
        let via_walk = lower_from_green(&tree, 0).expect("walk lower");
        let via_rd = crate::syntax::build::lower_syntax_to_program_rd_only(&tree, 0).expect("rd");
        assert_eq!(via_walk.decls.len(), via_rd.decls.len());
        assert_eq!(decl_count(&via_walk), 2);
    }

    #[test]
    fn green_guided_lower_module_import_func() {
        let src = "module m\nimport io as io\nfunc main(): int { return 0 }\n";
        let tree = parse_syntax(src);
        let prog = lower_from_green(&tree, 0).expect("lower");
        assert!(prog.module.is_some());
        assert_eq!(prog.imports.len(), 1);
        assert_eq!(prog.decls.len(), 1);
    }

    #[test]
    fn hand_lower_return_stmt() {
        let src = "func main(): int {\n    return 1\n}\n";
        let tree = parse_syntax(src);
        let (total, hand) = count_hand_lowerable_stmts(&tree);
        assert!(total >= 1, "stmts={total}");
        assert!(hand >= 1, "hand-lowerable={hand} total={total}");
    }

    #[test]
    fn hand_lower_let_and_binary_return() {
        let src = "func main(): int {\n    let x = 1\n    let mut y = x + 2\n    return y * 3\n}\n";
        let tree = parse_syntax(src);
        let (total, hand) = count_hand_lowerable_stmts(&tree);
        assert!(total >= 3, "stmts={total}");
        assert_eq!(hand, total, "all stmts hand-lowerable, hand={hand} total={total}");

        let (ftotal, fhand) = count_hand_lowerable_funcs(&tree);
        assert_eq!(ftotal, 1);
        assert_eq!(fhand, 1, "func should fully hand-lower");
    }

    #[test]
    fn hand_lower_func_integrated_in_green_walk() {
        let src = "func add(a: int, b: int): int {\n    return a + b\n}\n";
        let tree = parse_syntax(src);
        let prog = lower_from_green(&tree, 0).expect("hand/green lower");
        assert_eq!(decl_count(&prog), 1);
        match prog.pool.decl(prog.decls[0]) {
            TopLevelDecl::Func(f) => {
                assert_eq!(f.params.len(), 2);
                assert_eq!(f.body.statements.len(), 1);
                assert!(matches!(
                    prog.pool.stmt(f.body.statements[0]),
                    Stmt::Return { .. }
                ));
            }
            other => panic!("expected Func, got {other:?}"),
        }
    }

    #[test]
    fn hand_lower_func_matches_rd_decl_count() {
        let src = "func main(): int {\n    let x = 1 + 2\n    return x\n}\n";
        let tree = parse_syntax(src);
        let via_walk = lower_from_green(&tree, 0).expect("walk");
        let via_rd = crate::syntax::build::lower_syntax_to_program_rd_only(&tree, 0).expect("rd");
        assert_eq!(via_walk.decls.len(), via_rd.decls.len());
        assert_eq!(decl_count(&via_walk), 1);
        let (ftotal, fhand) = count_hand_lowerable_funcs(&tree);
        assert_eq!((ftotal, fhand), (1, 1));
    }

    #[test]
    fn event_green_has_expr_nodes() {
        let tree = parse_syntax("func main(): int { return 1 }\n");
        let exprs = tree
            .root()
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::EXPR)
            .count();
        assert!(exprs >= 1, "expected EXPR nodes from event sink");
    }
}
