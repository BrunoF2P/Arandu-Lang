#![allow(clippy::unnecessary_wraps)] // recovery parsers use Result for local error handling

mod ast;
mod parser;

pub use ast::*;
pub use parser::{
    ParseError, ParseErrorCode, ParseOutput, Parser, parse, parse_recovering,
    parse_recovering_with_file_id, parse_to_string, parse_with_file_id,
};
