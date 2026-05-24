use super::{
    BinaryOp, Block, CatchHandler, Expr, FieldInit, LambdaBody, LambdaParam, ParseError,
    ParseErrorCode, Parser, Stmt, StringPart, TokenKind, TypeExpr, UnaryOp, merge_text_parts,
    span_between,
};

impl<'a> Parser<'a> {
    pub(super) fn parse_expr(&mut self, min_bp: u8) -> Result<Expr, ParseError> {
        let start = self.mark();
        match self.try_parse_expr(min_bp) {
            Ok(expr) => Ok(expr),
            Err(err) => {
                self.diagnostics.push(err);
                // No synchronize_expr yet, to avoid eating too much.
                // It just falls back to Expr::Error so parent parses can fail/recover naturally.
                Ok(Expr::Error(self.span_from_mark(start)))
            }
        }
    }

    pub(super) fn try_parse_expr(&mut self, min_bp: u8) -> Result<Expr, ParseError> {
        let mut left = self.parse_prefix()?;
        loop {
            if self.at_kind_name("LT") && self.looks_like_generic_call_or_block_suffix() {
                let generic_start = left.span();
                let args = self.parse_generic_args()?;
                left = Expr::Generic {
                    span: span_between(generic_start, self.previous().span),
                    callee: Box::new(left),
                    args,
                };
                if self.at_kind_name("LPAREN") {
                    left = self.finish_call(left)?;
                } else if self.allow_block_calls {
                    left = self.finish_trailing_block_call(left)?;
                } else {
                    return Err(ParseError::new(
                        ParseErrorCode::ExpectedToken,
                        "generic block calls are not valid in this context",
                        self.current(),
                        self.source,
                    ));
                }
                continue;
            }
            if self.at_kind_name("LT")
                && self.looks_like_bare_generic_args()
                && self.looks_like_generic_args_boundary()
            {
                return Err(ParseError::new(
                    ParseErrorCode::ExpectedToken,
                    "generic arguments in expressions must be followed by call arguments or block",
                    self.current(),
                    self.source,
                ));
            }
            if self.at_kind_name("LPAREN") {
                left = self.finish_call(left)?;
                continue;
            }
            if self.allow_block_calls && self.at_kind_name("LBRACE") {
                left = self.finish_trailing_block_call(left)?;
                continue;
            }
            if self.at_kind_name("LBRACKET") {
                let span_start = left.span();
                self.consume();
                let index = self.parse_expr(0)?;
                self.expect_name("RBRACKET")?;
                left = Expr::Index {
                    span: span_between(span_start, self.previous().span),
                    base: Box::new(left),
                    index: Box::new(index),
                };
                continue;
            }
            if self.eat_name("SAFE_INDEX_START") {
                let span_start = left.span();
                let index = self.parse_expr(0)?;
                self.expect_name("RBRACKET")?;
                left = Expr::SafeIndex {
                    span: span_between(span_start, self.previous().span),
                    base: Box::new(left),
                    index: Box::new(index),
                };
                continue;
            }
            if self.eat_name("DOT") {
                let span_start = left.span();
                let field = self.expect_ident_value()?;
                left = Expr::Field {
                    span: span_between(span_start, self.previous().span),
                    base: Box::new(left),
                    field,
                };
                continue;
            }
            if self.eat_name("SAFE_DOT") {
                let span_start = left.span();
                let field = self.expect_ident_value()?;
                left = Expr::SafeField {
                    span: span_between(span_start, self.previous().span),
                    base: Box::new(left),
                    field,
                };
                continue;
            }
            if self.eat_name("QUESTION") {
                let span_start = left.span();
                left = Expr::Try {
                    span: span_between(span_start, self.previous().span),
                    expr: Box::new(left),
                };
                continue;
            }

            if self.at_kind_name("KW_CATCH") {
                let left_bp = 10;
                let right_bp = 10;
                if left_bp < min_bp {
                    break;
                }
                let span_start = left.span();
                self.consume();
                let handler = if self.eat_name("PIPE") {
                    let handler_start = self.pos.saturating_sub(1);
                    let error = self.expect_ident_value()?;
                    self.expect_name("PIPE")?;
                    let block = self.parse_block()?;
                    CatchHandler::Block {
                        span: self.span_from_mark(handler_start),
                        error,
                        block,
                    }
                } else {
                    let handler_start = self.mark();
                    let expr = self.parse_expr(right_bp)?;
                    CatchHandler::Expr {
                        span: self.span_from_mark(handler_start),
                        expr: Box::new(expr),
                    }
                };
                left = Expr::Catch {
                    span: span_between(span_start, self.previous().span),
                    expr: Box::new(left),
                    handler,
                };
                continue;
            }

            if self.at_kind_name("KW_AS") {
                let left_bp = 140;
                if left_bp < min_bp {
                    break;
                }
                let span_start = left.span();
                self.consume();
                let ty = self.parse_type()?;
                left = Expr::Cast {
                    span: span_between(span_start, ty.span()),
                    expr: Box::new(left),
                    ty,
                };
                continue;
            }

            let Some((op, left_bp, right_bp)) = self.current_binary() else {
                break;
            };
            if left_bp < min_bp {
                break;
            }
            if matches!(op, BinaryOp::RangeExclusive | BinaryOp::RangeInclusive)
                && matches!(
                    left,
                    Expr::Binary {
                        op: BinaryOp::RangeExclusive | BinaryOp::RangeInclusive,
                        ..
                    }
                )
            {
                return Err(ParseError::new(
                    ParseErrorCode::ExpectedToken,
                    "chained ranges require parentheses",
                    self.current(),
                    self.source,
                ));
            }
            let span_start = left.span();
            self.consume();
            let right = self.parse_expr(right_bp)?;
            let span = span_between(span_start, right.span());
            left = if op == BinaryOp::NullCoalesce {
                Expr::NullCoalesce {
                    span,
                    left: Box::new(left),
                    right: Box::new(right),
                }
            } else {
                Expr::Binary {
                    span,
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                }
            };
        }
        Ok(left)
    }

    pub(super) fn parse_prefix(&mut self) -> Result<Expr, ParseError> {
        let start = self.mark();
        match &self.current().kind {
            TokenKind::Minus => {
                self.consume();
                let expr = self.parse_expr(150)?;
                Ok(Expr::Unary {
                    span: self.span_from_mark(start),
                    op: UnaryOp::Neg,
                    expr: Box::new(expr),
                })
            }
            TokenKind::Bang => {
                self.consume();
                let expr = self.parse_expr(150)?;
                Ok(Expr::Unary {
                    span: self.span_from_mark(start),
                    op: UnaryOp::Not,
                    expr: Box::new(expr),
                })
            }
            TokenKind::Tilde => {
                self.consume();
                let expr = self.parse_expr(150)?;
                Ok(Expr::Unary {
                    span: self.span_from_mark(start),
                    op: UnaryOp::BitNot,
                    expr: Box::new(expr),
                })
            }
            TokenKind::KwAwait => {
                self.consume();
                let expr = self.parse_expr(150)?;
                Ok(Expr::Unary {
                    span: self.span_from_mark(start),
                    op: UnaryOp::Await,
                    expr: Box::new(expr),
                })
            }
            TokenKind::KwAlloc => {
                self.consume();
                let expr = self.parse_expr(150)?;
                Ok(Expr::Alloc {
                    span: self.span_from_mark(start),
                    expr: Box::new(expr),
                })
            }
            TokenKind::KwAsync => {
                self.consume();
                let block = self.parse_block()?;
                Ok(Expr::AsyncBlock {
                    span: self.span_from_mark(start),
                    block,
                })
            }
            TokenKind::KwUnsafe => {
                self.consume();
                let block = self.parse_block()?;
                Ok(Expr::UnsafeBlock {
                    span: self.span_from_mark(start),
                    block,
                })
            }
            TokenKind::KwIf => self.parse_if_expr(),
            TokenKind::KwMatch => self.parse_match_expr(),
            TokenKind::KwSelf => {
                self.consume();
                Ok(Expr::Path {
                    span: self.span_from_mark(start),
                    path: vec!["self".to_string()],
                })
            }
            TokenKind::IdentValue => {
                let name = self.expect_ident_value()?;
                Ok(Expr::Path {
                    span: self.span_from_mark(start),
                    path: vec![name],
                })
            }
            TokenKind::IdentType => self.parse_type_led_expr(),
            TokenKind::IntDec
            | TokenKind::IntHex
            | TokenKind::IntBin
            | TokenKind::IntOct => {
                let value = self.current_text().to_string();
                self.consume();
                Ok(Expr::Int {
                    span: self.span_from_mark(start),
                    value,
                })
            }
            TokenKind::Float => {
                let value = self.current_text().to_string();
                self.consume();
                Ok(Expr::Float {
                    span: self.span_from_mark(start),
                    value,
                })
            }
            TokenKind::BoolTrue => {
                self.consume();
                Ok(Expr::Bool {
                    span: self.span_from_mark(start),
                    value: true,
                })
            }
            TokenKind::BoolFalse => {
                self.consume();
                Ok(Expr::Bool {
                    span: self.span_from_mark(start),
                    value: false,
                })
            }
            TokenKind::Char => {
                let value = self.current().char_content(self.source).to_string();
                self.consume();
                Ok(Expr::Char {
                    span: self.span_from_mark(start),
                    value,
                })
            }
            TokenKind::Nil => {
                self.consume();
                Ok(Expr::Nil {
                    span: self.span_from_mark(start),
                })
            }
            TokenKind::StringStart => self.parse_string_like("STRING_START", "STRING_END"),
            TokenKind::MultilineStringStart => {
                self.parse_string_like("MULTILINE_STRING_START", "MULTILINE_STRING_END")
            }
            TokenKind::RawString => {
                let value = self.current().raw_string_content(self.source).to_string();
                self.consume();
                Ok(Expr::InterpolatedString {
                    span: self.span_from_mark(start),
                    parts: vec![StringPart::Text {
                        span: self.span_from_mark(start),
                        text: value,
                    }],
                })
            }
            TokenKind::LParen if self.looks_like_lambda_expr() => self.parse_lambda(),
            TokenKind::LParen => {
                self.consume();
                let expr = self.parse_expr(0)?;
                self.expect_name("RPAREN")?;
                Ok(Expr::Group {
                    span: self.span_from_mark(start),
                    expr: Box::new(expr),
                })
            }
            TokenKind::LBracket => self.parse_array(),
            _ => Err(ParseError::new(
                ParseErrorCode::ExpectedExpression,
                "expected expression",
                self.current(),
                self.source,
            )),
        }
    }

    pub(super) fn finish_call(&mut self, callee: Expr) -> Result<Expr, ParseError> {
        let span_start = callee.span();
        self.expect_name("LPAREN")?;
        let args = self.parse_arguments()?;
        self.expect_name("RPAREN")?;
        let trailing_block = if self.at_kind_name("LBRACE") {
            Some(self.parse_block()?)
        } else {
            None
        };
        let end = trailing_block
            .as_ref()
            .map_or_else(|| self.previous().span, |block| block.span);
        Ok(Expr::Call {
            span: span_between(span_start, end),
            callee: Box::new(callee),
            args,
            trailing_block,
        })
    }

    pub(super) fn finish_trailing_block_call(&mut self, callee: Expr) -> Result<Expr, ParseError> {
        let span_start = callee.span();
        let trailing_block = self.parse_block()?;
        Ok(Expr::Call {
            span: span_between(span_start, trailing_block.span),
            callee: Box::new(callee),
            args: Vec::new(),
            trailing_block: Some(trailing_block),
        })
    }

    pub(super) fn parse_if_expr(&mut self) -> Result<Expr, ParseError> {
        let start = self.mark();
        self.expect_name("KW_IF")?;
        let condition = self.parse_condition()?;
        let then_block = self.parse_block()?;
        self.expect_name("KW_ELSE")?;
        let else_block = if self.at_kind_name("KW_IF") {
            let nested = self.parse_if_expr()?;
            Block {
                span: nested.span(),
                statements: vec![Stmt::Expr {
                    span: nested.span(),
                    expr: Box::new(nested),
                }],
            }
        } else {
            self.parse_block()?
        };
        Ok(Expr::If {
            span: self.span_from_mark(start),
            condition: Box::new(condition),
            then_block,
            else_block,
        })
    }

    pub(super) fn parse_array(&mut self) -> Result<Expr, ParseError> {
        let start = self.mark();
        self.expect_name("LBRACKET")?;
        let mut items = Vec::new();
        if !self.at_kind_name("RBRACKET") {
            loop {
                items.push(self.parse_expr(0)?);
                if !self.eat_name("COMMA") {
                    break;
                }
                if self.at_kind_name("RBRACKET") {
                    break;
                }
            }
        }
        self.expect_name("RBRACKET")?;
        Ok(Expr::Array {
            span: self.span_from_mark(start),
            items,
        })
    }

    pub(super) fn parse_lambda(&mut self) -> Result<Expr, ParseError> {
        let start = self.mark();
        self.expect_name("LPAREN")?;
        let mut params = Vec::new();
        if !self.at_kind_name("RPAREN") {
            loop {
                let param_start = self.mark();
                let name = self.expect_ident_value()?;
                let ty = if self.can_start_type() {
                    Some(self.parse_type()?)
                } else {
                    None
                };
                params.push(LambdaParam {
                    span: self.span_from_mark(param_start),
                    name,
                    ty,
                });
                if !self.eat_name("COMMA") {
                    break;
                }
                if self.at_kind_name("RPAREN") {
                    break;
                }
            }
        }
        self.expect_name("RPAREN")?;
        self.expect_name("FAT_ARROW")?;
        let body = if self.at_kind_name("LBRACE") {
            let block = self.parse_block()?;
            LambdaBody::Block {
                span: block.span,
                block,
            }
        } else {
            let body_start = self.mark();
            let expr = self.parse_expr(0)?;
            LambdaBody::Expr {
                span: self.span_from_mark(body_start),
                expr: Box::new(expr),
            }
        };
        Ok(Expr::Lambda {
            span: self.span_from_mark(start),
            params,
            body,
        })
    }

    pub(super) fn parse_type_led_expr(&mut self) -> Result<Expr, ParseError> {
        let start = self.mark();
        let ty = self.parse_type()?;
        if self.eat_name("LBRACE") {
            let mut fields = Vec::new();
            if !self.at_kind_name("RBRACE") {
                loop {
                    let field_start = self.mark();
                    let name = self.expect_ident_value()?;
                    self.expect_name("COLON")?;
                    let value = self.parse_expr(0)?;
                    fields.push(FieldInit {
                        span: self.span_from_mark(field_start),
                        name,
                        value,
                    });
                    if !self.eat_name("COMMA") {
                        break;
                    }
                    if self.at_kind_name("RBRACE") {
                        break;
                    }
                }
            }
            self.expect_name("RBRACE")?;
            return Ok(Expr::StructLiteral {
                span: self.span_from_mark(start),
                ty,
                fields,
            });
        }
        if let TypeExpr::Named { name, args, .. } = ty
            && args.is_empty()
            && self.eat_name("DOT")
        {
            let member = self.expect_name_like()?;
            return Ok(Expr::TypePath {
                span: self.span_from_mark(start),
                type_name: name,
                member,
            });
        }
        Err(ParseError::new(
            ParseErrorCode::ExpectedExpression,
            "expected type-qualified expression or struct literal",
            self.current(),
            self.source,
        ))
    }

    pub(super) fn parse_match_expr(&mut self) -> Result<Expr, ParseError> {
        let start = self.mark();
        self.expect_name("KW_MATCH")?;
        let value = self.parse_expr_without_block_calls(0)?;
        self.expect_name("LBRACE")?;
        let mut arms = Vec::new();
        while !self.at_kind_name("RBRACE") {
            self.skip_semicolons();
            if self.at_kind_name("RBRACE") {
                break;
            }
            arms.push(self.parse_match_arm()?);
        }
        self.expect_name("RBRACE")?;
        Ok(Expr::Match {
            span: self.span_from_mark(start),
            value: Box::new(value),
            arms,
        })
    }

    pub(super) fn parse_string_like(
        &mut self,
        start_name: &str,
        end_name: &str,
    ) -> Result<Expr, ParseError> {
        let start = self.mark();
        self.expect_name(start_name)?;
        let mut parts = Vec::new();
        while !self.at_kind_name(end_name) {
            match &self.current().kind {
                TokenKind::StringText | TokenKind::StringEscape => {
                    let span = self.current().span;
                    parts.push(StringPart::Text {
                        span,
                        text: self.current_text().to_string(),
                    });
                    self.consume();
                }
                TokenKind::InterpStart => {
                    let part_start = self.mark();
                    self.consume();
                    let expr = self.parse_expr(0)?;
                    self.expect_name("INTERP_END")?;
                    parts.push(StringPart::Expr {
                        span: self.span_from_mark(part_start),
                        expr: Box::new(expr),
                    });
                }
                _ => {
                    return Err(ParseError::new(
                        ParseErrorCode::ExpectedExpression,
                        "expected string part",
                        self.current(),
                        self.source,
                    ));
                }
            }
        }
        self.expect_name(end_name)?;
        Ok(Expr::InterpolatedString {
            span: self.span_from_mark(start),
            parts: merge_text_parts(parts),
        })
    }

    pub(super) fn parse_arguments(&mut self) -> Result<Vec<Expr>, ParseError> {
        let mut args = Vec::new();
        if self.at_kind_name("RPAREN") {
            return Ok(args);
        }
        loop {
            args.push(self.parse_expr(0)?);
            if !self.eat_name("COMMA") {
                break;
            }
            if self.at_kind_name("RPAREN") {
                break;
            }
        }
        Ok(args)
    }

    pub(super) fn current_binary(&self) -> Option<(BinaryOp, u8, u8)> {
        let (op, bp) = match self.current().kind {
            TokenKind::NullCoalesce => (BinaryOp::NullCoalesce, 20),
            TokenKind::LogicalOr => (BinaryOp::Or, 30),
            TokenKind::LogicalAnd => (BinaryOp::And, 40),
            TokenKind::EqualEqual => (BinaryOp::Equal, 50),
            TokenKind::BangEqual => (BinaryOp::NotEqual, 50),
            TokenKind::Lt => (BinaryOp::Lt, 60),
            TokenKind::Gt => (BinaryOp::Gt, 60),
            TokenKind::LtEqual => (BinaryOp::LtEqual, 60),
            TokenKind::GtEqual => (BinaryOp::GtEqual, 60),
            TokenKind::RangeExclusive => (BinaryOp::RangeExclusive, 70),
            TokenKind::RangeInclusive => (BinaryOp::RangeInclusive, 70),
            TokenKind::Pipe => (BinaryOp::BitOr, 80),
            TokenKind::Caret => (BinaryOp::BitXor, 90),
            TokenKind::Amp => (BinaryOp::BitAnd, 100),
            TokenKind::ShiftLeft => (BinaryOp::ShiftLeft, 110),
            TokenKind::ShiftRight => (BinaryOp::ShiftRight, 110),
            TokenKind::Plus => (BinaryOp::Add, 120),
            TokenKind::Minus => (BinaryOp::Sub, 120),
            TokenKind::Star => (BinaryOp::Mul, 130),
            TokenKind::Slash => (BinaryOp::Div, 130),
            TokenKind::Percent => (BinaryOp::Mod, 130),
            _ => return None,
        };
        Some((op, bp, bp + 1))
    }

    pub(super) fn looks_like_lambda_expr(&self) -> bool {
        self.find_matching_rparen(self.pos)
            .and_then(|rparen| self.tokens.get(rparen + 1))
            .is_some_and(|token| matches!(token.kind, TokenKind::FatArrow))
    }

    pub(super) fn find_matching_rparen(&self, start: usize) -> Option<usize> {
        if !matches!(self.tokens.get(start)?.kind, TokenKind::LParen) {
            return None;
        }

        let mut depth = 0usize;
        for (index, token) in self.tokens.iter().enumerate().skip(start) {
            match token.kind {
                TokenKind::LParen => depth += 1,
                TokenKind::RParen => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Some(index);
                    }
                }
                TokenKind::Eof => return None,
                _ => {}
            }
        }
        None
    }
}
