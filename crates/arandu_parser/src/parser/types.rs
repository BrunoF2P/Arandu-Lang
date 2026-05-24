use super::{
    GenericParam, Ownership, Param, ParseError, ParseErrorCode, Parser, ResultType, TokenKind,
    TypeExpr, TypeName, WhereItem, is_type_token, primitive_type_name,
};

fn type_expr_is_err_slot(ty: &TypeExpr) -> bool {
    match ty {
        TypeExpr::Nullable { inner, .. } => type_expr_is_err_slot(inner),
        TypeExpr::Primitive { name, .. } => name == "Err",
        _ => false,
    }
}

fn result_type_must_use_result_generic(result: &ResultType) -> bool {
    match result {
        ResultType::Single { ty, .. } => type_expr_is_err_slot(ty),
        ResultType::Multi { types, .. } => types.len() == 2 && type_expr_is_err_slot(&types[1]),
    }
}

impl<'a> Parser<'a> {
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
        let args = self.parse_comma_separated_list("GT", 1, super::Parser::parse_type)?;
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

    fn parse_ownership(&mut self) -> Option<Ownership> {
        if self.eat_name("KW_OWN") {
            Some(Ownership::Own)
        } else if self.eat_name("KW_MUT") {
            Some(Ownership::Mut)
        } else if self.eat_name("KW_SHARED") {
            Some(Ownership::Shared)
        } else {
            None
        }
    }

    pub(super) fn parse_params(
        &mut self,
        method_receiver: Option<&TypeName>,
    ) -> Result<Vec<Param>, ParseError> {
        let params = self.parse_comma_separated_list("RPAREN", 0, |parser| {
            let start = parser.mark();
            let attrs = parser.parse_attributes()?;
            let ownership = parser.parse_ownership();
            let name = if parser.eat_name("KW_SELF") {
                "self".to_string()
            } else {
                parser.expect_ident_value()?
            };
            let is_receiver = name == "self";
            let ty = if parser.can_start_type() {
                parser.parse_type()?
            } else if is_receiver {
                let receiver = method_receiver.ok_or_else(|| {
                    ParseError::new(
                        ParseErrorCode::ExpectedType,
                        "receiver parameter 'self' requires an explicit type here",
                        parser.current(),
                        parser.source,
                    )
                })?;
                TypeExpr::Named {
                    span: receiver.span,
                    name: receiver.clone(),
                    args: Vec::new(),
                }
            } else {
                return Err(ParseError::new(
                    ParseErrorCode::ExpectedType,
                    "expected parameter type",
                    parser.current(),
                    parser.source,
                ));
            };
            let is_variadic = parser.eat_name("ELLIPSIS");
            Ok(Param {
                span: parser.span_from_mark(start),
                attrs,
                ownership,
                name,
                ty,
                is_variadic,
                is_receiver,
            })
        })?;
        Ok(params)
    }

    pub(super) fn parse_result_type(&mut self) -> Result<ResultType, ParseError> {
        let start = self.mark();
        let result = if self.eat_name("LPAREN") {
            let types = self.parse_comma_separated_list("RPAREN", 2, super::Parser::parse_type)?;
            self.expect_name("RPAREN")?;
            ResultType::Multi {
                span: self.span_from_mark(start),
                types,
            }
        } else {
            let ty = self.parse_type()?;
            ResultType::Single {
                span: self.span_from_mark(start),
                ty,
            }
        };
        if result_type_must_use_result_generic(&result) {
            let token = self.current().clone();
            return Err(ParseError::new(
                ParseErrorCode::InvalidResultReturn,
                "function result must use `Result<T, E>` syntax",
                &token,
                self.source,
            ));
        }
        Ok(result)
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
                TokenKind::IntDec => {
                    let value = self.current_text().to_string();
                    self.consume();
                    value
                }
                _ => {
                    return Err(ParseError::new(
                        ParseErrorCode::ExpectedToken,
                        "expected array size",
                        self.current(),
                        self.source,
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
            TokenKind::IdentValue | TokenKind::IdentType
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
            self.source,
        ))
    }

    pub(super) fn parse_type_name(&mut self) -> Result<TypeName, ParseError> {
        let start = self.mark();
        let mut path = Vec::new();
        while matches!(self.current().kind, TokenKind::IdentValue)
            && self
                .tokens
                .get(self.pos + 1)
                .is_some_and(|token| matches!(token.kind, TokenKind::Dot))
        {
            path.push(self.expect_ident_value()?);
            self.expect_name("DOT")?;
        }
        let last = match &self.current().kind {
            TokenKind::IdentType => {
                let name = self.current_text().to_string();
                self.consume();
                name
            }
            TokenKind::IdentValue if self.current_text() == "void" => {
                let name = self.current_text().to_string();
                self.consume();
                name
            }
            _ => self.expect_ident_type()?,
        };
        path.push(last);
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
