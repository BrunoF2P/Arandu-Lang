//! F1b — green-guided lower: walk typed top-level items, RD at each seek position.
//!
//! One shared [`Parser`] / [`AstPool`] for the whole file; the green tree only
//! drives **where** to resume parsing (no dual independent AST).

use super::SyntaxTree;
use super::kind::SyntaxKind;
use crate::parser::{ParseError, ParseOutput, Parser};
use crate::{Program, TopLevelDecl};
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
            SyntaxKind::FUNC_ITEM
            | SyntaxKind::STRUCT_ITEM
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
}
