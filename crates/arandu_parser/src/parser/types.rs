use super::*;

impl Parser {
    pub(super) fn parse_generic_params(&mut self) -> Result<Vec<GenericParam>, ParseError> {
        if !self.eat_name("LT") {
            return Ok(Vec::new());
        }
        let params = self.parse_comma_separated_list("GT", 1, |parser| {
            let start = parser.mark();
            let name = parser.expect_ident_type()?;
            let constraints = if parser.eat_name("COLON") {
                parser.parse_constraint_list()?
            } else {
                Vec::new()
            };
            Ok(GenericParam {
                span: parser.span_from_mark(start),
                name,
                constraints,
            })
        })?;
        self.expect_name("GT")?;
        Ok(params)
    }

    pub(super) fn parse_generic_args(&mut self) -> Result<Vec<TypeExpr>, ParseError> {
        self.expect_name("LT")?;
        let args = self.parse_comma_separated_list("GT", 1, |parser| parser.parse_type())?;
        self.expect_name("GT")?;
        Ok(args)
    }

    pub(super) fn parse_where_clause(
        &mut self,
        end_name: &str,
    ) -> Result<Vec<WhereItem>, ParseError> {
        if !self.eat_name("KW_WHERE") {
            return Ok(Vec::new());
        }
        let mut items = Vec::new();
        loop {
            let start = self.mark();
            let name = self.expect_ident_type()?;
            self.expect_name("COLON")?;
            let constraints = self.parse_constraint_list()?;
            items.push(WhereItem {
                span: self.span_from_mark(start),
                name,
                constraints,
            });
            if !self.eat_name("COMMA") {
                break;
            }
            if self.at_kind_name(end_name) {
                break;
            }
        }
        Ok(items)
    }

    pub(super) fn parse_constraint_list(&mut self) -> Result<Vec<TypeName>, ParseError> {
        let mut constraints = vec![self.parse_type_name()?];
        while self.eat_name("PLUS") {
            constraints.push(self.parse_type_name()?);
        }
        Ok(constraints)
    }

    pub(super) fn parse_params(&mut self) -> Result<Vec<Param>, ParseError> {
        let params = self.parse_comma_separated_list("RPAREN", 0, |parser| {
            let start = parser.mark();
            let attrs = parser.parse_attributes()?;
            let ownership = if parser.eat_name("KW_OWN") {
                Some(Ownership::Own)
            } else if parser.eat_name("KW_MUT") {
                Some(Ownership::Mut)
            } else {
                None
            };
            let name = parser.expect_ident_value()?;
            let ty = parser.parse_type()?;
            let is_variadic = parser.eat_name("ELLIPSIS");
            Ok(Param {
                span: parser.span_from_mark(start),
                attrs,
                ownership,
                name,
                ty,
                is_variadic,
            })
        })?;
        Ok(params)
    }

    pub(super) fn parse_result_type(&mut self) -> Result<ResultType, ParseError> {
        let start = self.mark();
        if self.eat_name("LPAREN") {
            let types =
                self.parse_comma_separated_list("RPAREN", 2, |parser| parser.parse_type())?;
            self.expect_name("RPAREN")?;
            Ok(ResultType::Multi {
                span: self.span_from_mark(start),
                types,
            })
        } else {
            let ty = self.parse_type()?;
            Ok(ResultType::Single {
                span: self.span_from_mark(start),
                ty,
            })
        }
    }

    pub(super) fn parse_type(&mut self) -> Result<TypeExpr, ParseError> {
        let start = self.mark();
        let mut ty = self.parse_type_primary()?;
        if self.eat_name("QUESTION") {
            ty = TypeExpr::Nullable {
                span: self.span_from_mark(start),
                inner: Box::new(ty),
            };
        }
        Ok(ty)
    }

    pub(super) fn parse_type_primary(&mut self) -> Result<TypeExpr, ParseError> {
        let start = self.mark();
        if self.eat_name("KW_PTR") {
            self.expect_name("LBRACKET")?;
            let inner = self.parse_type()?;
            self.expect_name("RBRACKET")?;
            return Ok(TypeExpr::Pointer {
                span: self.span_from_mark(start),
                inner: Box::new(inner),
            });
        }
        if self.eat_name("LBRACKET") {
            if self.eat_name("RBRACKET") {
                let inner = self.parse_type_primary()?;
                return Ok(TypeExpr::Slice {
                    span: self.span_from_mark(start),
                    inner: Box::new(inner),
                });
            }
            let size = match &self.current().kind {
                TokenKind::IntDec(value) => {
                    let value = value.clone();
                    self.consume();
                    value
                }
                _ => {
                    return Err(ParseError::new(
                        ParseErrorCode::ExpectedToken,
                        "expected array size",
                        self.current(),
                    ));
                }
            };
            self.expect_name("RBRACKET")?;
            let elem = self.parse_type_primary()?;
            return Ok(TypeExpr::Array {
                span: self.span_from_mark(start),
                size,
                elem: Box::new(elem),
            });
        }
        if self.eat_name("KW_FUNC") {
            self.expect_name("LPAREN")?;
            let mut params = Vec::new();
            if !self.at_kind_name("RPAREN") {
                loop {
                    params.push(self.parse_type()?);
                    if !self.eat_name("COMMA") {
                        break;
                    }
                    if self.at_kind_name("RPAREN") {
                        break;
                    }
                }
            }
            self.expect_name("RPAREN")?;
            let result = if self.can_start_type() || self.at_kind_name("LPAREN") {
                Some(Box::new(self.parse_result_type()?))
            } else {
                None
            };
            return Ok(TypeExpr::Func {
                span: self.span_from_mark(start),
                params,
                result,
            });
        }
        if self.eat_name("LPAREN") {
            let ty = self.parse_type()?;
            self.expect_name("RPAREN")?;
            return Ok(TypeExpr::Group {
                span: self.span_from_mark(start),
                inner: Box::new(ty),
            });
        }
        if let Some(name) = primitive_type_name(&self.current().kind) {
            self.consume();
            return Ok(TypeExpr::Primitive {
                span: self.span_from_mark(start),
                name: name.to_string(),
            });
        }
        if matches!(
            self.current().kind,
            TokenKind::IdentValue(_) | TokenKind::IdentType(_)
        ) {
            let name = self.parse_type_name()?;
            let args = if self.at_kind_name("LT") {
                self.parse_generic_args()?
            } else {
                Vec::new()
            };
            return Ok(TypeExpr::Named {
                span: self.span_from_mark(start),
                name,
                args,
            });
        }
        Err(ParseError::new(
            ParseErrorCode::ExpectedType,
            "expected type",
            self.current(),
        ))
    }

    pub(super) fn parse_type_name(&mut self) -> Result<TypeName, ParseError> {
        let start = self.mark();
        let mut path = Vec::new();
        while matches!(self.current().kind, TokenKind::IdentValue(_))
            && self
                .tokens
                .get(self.pos + 1)
                .is_some_and(|token| matches!(token.kind, TokenKind::Dot))
        {
            path.push(self.expect_ident_value()?);
            self.expect_name("DOT")?;
        }
        path.push(self.expect_ident_type()?);
        Ok(TypeName {
            span: self.span_from_mark(start),
            path,
        })
    }

    pub(super) fn can_start_type(&self) -> bool {
        matches!(
            self.current().kind,
            TokenKind::KwPtr | TokenKind::LBracket | TokenKind::KwFunc | TokenKind::LParen
        ) || is_type_token(&self.current().kind)
    }

    pub(super) fn looks_like_generic_call_or_block_suffix(&self) -> bool {
        self.find_matching_gt(self.pos)
            .and_then(|gt| self.tokens.get(gt + 1))
            .is_some_and(|token| matches!(token.kind, TokenKind::LParen | TokenKind::LBrace))
    }

    pub(super) fn looks_like_bare_generic_args(&self) -> bool {
        self.find_matching_gt(self.pos).is_some()
    }

    pub(super) fn looks_like_generic_args_boundary(&self) -> bool {
        self.find_matching_gt(self.pos)
            .and_then(|gt| self.tokens.get(gt + 1))
            .is_none_or(|token| {
                matches!(
                    token.kind,
                    TokenKind::Semicolon
                        | TokenKind::Comma
                        | TokenKind::RParen
                        | TokenKind::RBracket
                        | TokenKind::RBrace
                        | TokenKind::Eof
                )
            })
    }

    pub(super) fn find_matching_gt(&self, start: usize) -> Option<usize> {
        if !matches!(self.tokens.get(start)?.kind, TokenKind::Lt) {
            return None;
        }

        let mut depth = 0usize;
        for (index, token) in self.tokens.iter().enumerate().skip(start) {
            match token.kind {
                TokenKind::Lt => depth += 1,
                TokenKind::Gt => {
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
