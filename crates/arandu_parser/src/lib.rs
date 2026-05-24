#![allow(clippy::unnecessary_wraps)] // recovery parsers use Result for local error handling

mod ast;
mod parser;

pub use ast::*;
pub use parser::{
    ParseError, ParseErrorCode, ParseOutput, Parser, parse, parse_recovering, parse_to_string,
};
