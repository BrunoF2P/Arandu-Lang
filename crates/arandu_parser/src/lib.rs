#![allow(clippy::unnecessary_wraps)] // recovery parsers use Result for local error handling

#[cfg(test)]
mod tests;

mod ast;
mod parser;
pub mod syntax;

pub use ast::*;
pub use parser::{
    ParseError, ParseErrorCode, ParseOutput, Parser, parse, parse_recovering,
    parse_recovering_with_file_id, parse_to_string, parse_with_file_id,
};
pub use syntax::{
    SyntaxKind, SyntaxNode, SyntaxTree, parse_dual, parse_dual_with_file_id, parse_syntax,
    parse_syntax_with_item_spans, reparse_edit,
};
