use super::{
    Attribute, ConstDecl, EnumDecl, EnumPayload, EnumVariant, ExternDecl, FieldDecl, FuncDecl,
    FuncName, FuncSignature, ImportDecl, ImportItem, InterfaceDecl, ModuleDecl, ParseError,
    ParseErrorCode, Parser, StructDecl, TokenKind, TopLevelDecl, TypeAliasDecl, Visibility,
    is_contextual_module_segment,
};

impl<'a> Parser<'a> {
    pub(super) fn expect_optional_semicolon_after_module_path(&mut self) -> Result<(), ParseError> {
        if self.at_kind_name("SEMICOLON") {
            self.expect_semicolon()?;
        } else {
            let last_segment_is_contextual = is_contextual_module_segment(&self.previous().kind);
            let next_starts_top_level = self.at_kind_name("EOF")
                || self.at_kind_name("KW_IMPORT")
                || matches!(
                    self.current().kind,
                    TokenKind::At
                        | TokenKind::KwPublic
                        | TokenKind::KwConst
                        | TokenKind::KwType
                        | TokenKind::KwAsync
                        | TokenKind::KwFunc
                        | TokenKind::KwStruct
                        | TokenKind::KwEnum
                        | TokenKind::KwInterface
                        | TokenKind::KwExtern
                );
            if !(last_segment_is_contextual && next_starts_top_level) {
                self.expect_semicolon()?;
            }
        }
        Ok(())
    }

    pub(super) fn parse_module(&mut self) -> Result<ModuleDecl, ParseError> {
        self.collect_doc_comments();
        let docs = self.take_pending_docs();
        let start = self.mark();
        self.expect_name("KW_MODULE")?;
        let path = self.parse_module_path()?;
        self.expect_optional_semicolon_after_module_path()?;
        let module = ModuleDecl {
            span: self.span_from_mark(start),
            path,
        };
        self.attach_docs(docs, module.span);
        Ok(module)
    }

    pub(super) fn parse_import(&mut self) -> Result<ImportDecl, ParseError> {
        self.collect_doc_comments();
        let docs = self.take_pending_docs();
        let start = self.mark();
        self.expect_name("KW_IMPORT")?;
        if self.eat_name("LBRACE") {
            let items = self.parse_comma_separated_list("RBRACE", 1, |parser| {
                let item_start = parser.mark();
                let name = parser.expect_import_name()?;
                let alias = if parser.eat_name("KW_AS") {
                    Some(parser.expect_import_name()?)
                } else {
                    None
                };
                Ok(ImportItem {
                    span: parser.span_from_mark(item_start),
                    name,
                    alias,
                })
            })?;
            self.skip_semicolons();
            self.expect_name("RBRACE")?;
            self.expect_name("KW_FROM")?;
            let from = self.parse_module_path()?;
            self.expect_optional_semicolon_after_module_path()?;
            let import = ImportDecl::Named {
                span: self.span_from_mark(start),
                items,
                from,
            };
            self.attach_docs(docs, import.span());
            return Ok(import);
        }

        let path = self.parse_module_path()?;
        self.expect_optional_semicolon_after_module_path()?;
        let import = ImportDecl::Module {
            span: self.span_from_mark(start),
            path,
        };
        self.attach_docs(docs, import.span());
        Ok(import)
    }

    pub(super) fn parse_top_level_decl(&mut self) -> Result<TopLevelDecl, ParseError> {
        let start = self.mark();
        match self.try_parse_top_level_decl() {
            Ok(decl) => Ok(decl),
            Err(err) => {
                self.diagnostics.push(err);
                self.synchronize_top_level();
                Ok(TopLevelDecl::Error(self.span_from_mark(start)))
            }
        }
    }

    pub(super) fn try_parse_top_level_decl(&mut self) -> Result<TopLevelDecl, ParseError> {
        self.collect_doc_comments();
        let docs = self.take_pending_docs();
        let attrs = self.parse_attributes()?;
        let visibility = self.parse_visibility();
        let decl = match self.current().kind {
            TokenKind::KwConst => Ok(TopLevelDecl::Const(self.parse_const(attrs, visibility)?)),
            TokenKind::KwType => Ok(TopLevelDecl::TypeAlias(
                self.parse_type_alias(attrs, visibility)?,
            )),
            TokenKind::KwAsync | TokenKind::KwFunc => Ok(TopLevelDecl::Func(
                self.parse_func(attrs, visibility)?,
            )),
            TokenKind::KwStruct => Ok(TopLevelDecl::Struct(
                self.parse_struct_decl(attrs, visibility)?,
            )),
            TokenKind::KwEnum => Ok(TopLevelDecl::Enum(
                self.parse_enum_decl(attrs, visibility)?,
            )),
            TokenKind::KwInterface => Ok(TopLevelDecl::Interface(
                self.parse_interface_decl(attrs, visibility)?,
            )),
            TokenKind::KwExtern => Ok(TopLevelDecl::Extern(
                self.parse_extern_decl(attrs)?,
            )),
            _ => Err(ParseError::new(
                ParseErrorCode::ExpectedTopLevelDecl,
                "expected top-level declaration",
                self.current(),
                self.file_id,
                self.source,
            )),
        }?;
        self.attach_docs(docs, decl.span());
        Ok(decl)
    }

    pub(super) fn parse_const(
        &mut self,
        attrs: Vec<Attribute>,
        visibility: Visibility,
    ) -> Result<ConstDecl, ParseError> {
        let start = self.mark();
        self.expect_name("KW_CONST")?;
        let name = self.expect_name_like()?;
        let ty = if self.can_start_type() {
            Some(self.parse_type()?)
        } else {
            None
        };
        self.expect_name("EQUAL")?;
        let value = self.parse_expr(0)?;
        self.expect_semicolon()?;
        Ok(ConstDecl {
            span: self.span_from_mark(start),
            attrs,
            visibility,
            name,
            ty,
            value,
        })
    }

    pub(super) fn parse_type_alias(
        &mut self,
        attrs: Vec<Attribute>,
        visibility: Visibility,
    ) -> Result<TypeAliasDecl, ParseError> {
        let start = self.mark();
        self.expect_name("KW_TYPE")?;
        let name = self.expect_ident_type()?;
        let generic_params = self.parse_generic_params()?;
        self.expect_name("EQUAL")?;
        let ty = self.parse_type()?;
        self.expect_semicolon()?;
        Ok(TypeAliasDecl {
            span: self.span_from_mark(start),
            attrs,
            visibility,
            name,
            generic_params,
            ty,
        })
    }

    pub(super) fn parse_func(
        &mut self,
        attrs: Vec<Attribute>,
        visibility: Visibility,
    ) -> Result<FuncDecl, ParseError> {
        let start = self.mark();
        let is_async = self.eat_name("KW_ASYNC");
        self.expect_name("KW_FUNC")?;
        let name = self.parse_func_name()?;
        let generic_params = self.parse_generic_params()?;
        self.expect_name("LPAREN")?;
        let method_receiver = match &name {
            FuncName::Method { receiver, .. } => Some(receiver),
            FuncName::Free { .. } => None,
        };
        let params = self.parse_params(method_receiver)?;
        self.expect_name("RPAREN")?;
        let result = if self.can_start_type() || self.at_kind_name("LPAREN") {
            Some(self.parse_result_type()?)
        } else {
            None
        };
        let where_clause = self.parse_where_clause("LBRACE")?;
        let body = self.parse_block()?;
        Ok(FuncDecl {
            span: self.span_from_mark(start),
            attrs,
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

    pub(super) fn parse_struct_decl(
        &mut self,
        attrs: Vec<Attribute>,
        visibility: Visibility,
    ) -> Result<StructDecl, ParseError> {
        let start = self.mark();
        self.expect_name("KW_STRUCT")?;
        let name = self.expect_ident_type()?;
        let generic_params = self.parse_generic_params()?;
        let where_clause = self.parse_where_clause("LBRACE")?;
        self.expect_name("LBRACE")?;
        let mut fields = Vec::new();
        while !self.at_kind_name("RBRACE") {
            self.skip_semicolons();
            if self.at_kind_name("RBRACE") {
                break;
            }
            if self.at_kind_name("EOF") {
                self.diagnostics.push(ParseError::new(
                    ParseErrorCode::ExpectedToken,
                    "expected '}'",
                    self.current(),
                    self.file_id,
                    self.source,
                ));
                break;
            }
            fields.push(self.parse_field_decl(true)?);
        }
        self.expect_name("RBRACE")?;
        self.skip_semicolons();
        Ok(StructDecl {
            span: self.span_from_mark(start),
            attrs,
            visibility,
            name,
            generic_params,
            where_clause,
            fields,
        })
    }

    pub(super) fn parse_field_decl(
        &mut self,
        require_semicolon: bool,
    ) -> Result<FieldDecl, ParseError> {
        self.collect_doc_comments();
        let docs = self.take_pending_docs();
        let start = self.mark();
        let attrs = self.parse_attributes()?;
        let visibility = self.parse_visibility();
        let name = self.expect_ident_value()?;
        let ty = self.parse_type()?;
        if require_semicolon || self.at_kind_name("SEMICOLON") {
            self.expect_semicolon()?;
        }
        let field = FieldDecl {
            span: self.span_from_mark(start),
            attrs,
            visibility,
            name,
            ty,
        };
        self.attach_docs(docs, field.span);
        Ok(field)
    }

    pub(super) fn parse_enum_decl(
        &mut self,
        attrs: Vec<Attribute>,
        visibility: Visibility,
    ) -> Result<EnumDecl, ParseError> {
        let start = self.mark();
        self.expect_name("KW_ENUM")?;
        let name = self.expect_ident_type()?;
        let generic_params = self.parse_generic_params()?;
        let where_clause = self.parse_where_clause("LBRACE")?;
        self.expect_name("LBRACE")?;
        let mut variants = Vec::new();
        while !self.at_kind_name("RBRACE") {
            self.skip_semicolons();
            if self.at_kind_name("RBRACE") {
                break;
            }
            if self.at_kind_name("EOF") {
                self.diagnostics.push(ParseError::new(
                    ParseErrorCode::ExpectedToken,
                    "expected '}'",
                    self.current(),
                    self.file_id,
                    self.source,
                ));
                break;
            }
            variants.push(self.parse_enum_variant()?);
            if self.eat_name("COMMA") {
                continue;
            }
            self.skip_semicolons();
        }
        self.expect_name("RBRACE")?;
        self.skip_semicolons();
        Ok(EnumDecl {
            span: self.span_from_mark(start),
            attrs,
            visibility,
            name,
            generic_params,
            where_clause,
            variants,
        })
    }

    pub(super) fn parse_enum_variant(&mut self) -> Result<EnumVariant, ParseError> {
        self.collect_doc_comments();
        let docs = self.take_pending_docs();
        let start = self.mark();
        let attrs = self.parse_attributes()?;
        let name = self.expect_ident_type()?;
        let payload = if self.eat_name("LPAREN") {
            let payload_start = self.pos.saturating_sub(1);
            let types = self.parse_comma_separated_list("RPAREN", 0, super::Parser::parse_type)?;
            self.expect_name("RPAREN")?;
            let range = self.pool.alloc_type_expr_list(&types);
            Some(EnumPayload::Tuple {
                span: self.span_from_mark(payload_start),
                types: range,
            })
        } else if self.eat_name("LBRACE") {
            let payload_start = self.pos.saturating_sub(1);
            let fields = self
                .parse_comma_separated_list("RBRACE", 0, |parser| parser.parse_field_decl(false))?;
            self.expect_name("RBRACE")?;
            Some(EnumPayload::Struct {
                span: self.span_from_mark(payload_start),
                fields,
            })
        } else {
            None
        };
        let variant = EnumVariant {
            span: self.span_from_mark(start),
            attrs,
            name,
            payload,
        };
        self.attach_docs(docs, variant.span);
        Ok(variant)
    }

    pub(super) fn parse_interface_decl(
        &mut self,
        attrs: Vec<Attribute>,
        visibility: Visibility,
    ) -> Result<InterfaceDecl, ParseError> {
        let start = self.mark();
        self.expect_name("KW_INTERFACE")?;
        let name = self.expect_ident_type()?;
        let generic_params = self.parse_generic_params()?;
        let where_clause = self.parse_where_clause("LBRACE")?;
        let members = self.parse_braced_member_list(|parser| {
            let attrs = parser.parse_attributes()?;
            parser.parse_func_signature(attrs)
        })?;
        Ok(InterfaceDecl {
            span: self.span_from_mark(start),
            attrs,
            visibility,
            name,
            generic_params,
            where_clause,
            members,
        })
    }

    pub(super) fn parse_extern_decl(
        &mut self,
        attrs: Vec<Attribute>,
    ) -> Result<ExternDecl, ParseError> {
        let start = self.mark();
        self.expect_name("KW_EXTERN")?;
        let abi = self.parse_abi_literal()?;
        let members = self.parse_braced_member_list(|parser| {
            let attrs = parser.parse_attributes()?;
            parser.parse_func_signature(attrs)
        })?;
        Ok(ExternDecl {
            span: self.span_from_mark(start),
            attrs,
            abi,
            members,
        })
    }

    pub(super) fn parse_abi_literal(&mut self) -> Result<String, ParseError> {
        self.expect_name("STRING_START")?;
        let abi = match &self.current().kind {
            TokenKind::StringText => {
                let text = self.current_text().to_string();
                self.consume();
                text
            }
            _ => {
                return Err(ParseError::new(
                    ParseErrorCode::ExpectedToken,
                    "expected static ABI string",
                    self.current(),
                    self.file_id,
                    self.source,
                ));
            }
        };
        self.expect_name("STRING_END")?;
        Ok(abi)
    }

    pub(super) fn parse_func_signature(
        &mut self,
        attrs: Vec<Attribute>,
    ) -> Result<FuncSignature, ParseError> {
        self.collect_doc_comments();
        let docs = self.take_pending_docs();
        let start = self.mark();
        self.expect_name("KW_FUNC")?;
        let name = self.expect_ident_value()?;
        let generic_params = self.parse_generic_params()?;
        self.expect_name("LPAREN")?;
        let params = self.parse_params(None)?;
        self.expect_name("RPAREN")?;
        let result = if self.can_start_type() || self.at_kind_name("LPAREN") {
            Some(self.parse_result_type()?)
        } else {
            None
        };
        let where_clause = self.parse_where_clause("SEMICOLON")?;
        let signature = FuncSignature {
            span: self.span_from_mark(start),
            attrs,
            name,
            generic_params,
            params,
            result,
            where_clause,
        };
        self.attach_docs(docs, signature.span);
        Ok(signature)
    }

    pub(super) fn parse_attributes(&mut self) -> Result<Vec<Attribute>, ParseError> {
        let mut attrs = Vec::new();
        while self.eat_name("AT") {
            let start = self.pos.saturating_sub(1);
            let name = self.expect_ident_value()?;
            let args = if self.eat_name("LPAREN") {
                let args = self.parse_arguments()?;
                self.expect_name("RPAREN")?;
                args
            } else {
                Vec::new()
            };
            attrs.push(Attribute {
                span: self.span_from_mark(start),
                name,
                args,
            });
            self.skip_semicolons();
        }
        Ok(attrs)
    }

    pub(super) fn parse_comma_separated_list<T, F>(
        &mut self,
        end_name: &str,
        min_items: usize,
        mut parse_item: F,
    ) -> Result<Vec<T>, ParseError>
    where
        F: FnMut(&mut Self) -> Result<T, ParseError>,
    {
        if self.at_kind_name(end_name) {
            if min_items == 0 {
                return Ok(Vec::new());
            }
            return Err(ParseError::new(
                ParseErrorCode::ExpectedToken,
                format!("expected item before {end_name}"),
                self.current(),
                self.file_id,
                self.source,
            ));
        }

        let mut items = Vec::new();
        loop {
            items.push(parse_item(self)?);
            if !self.eat_name("COMMA") {
                break;
            }
            if self.at_kind_name(end_name) {
                break;
            }
        }

        if items.len() < min_items {
            return Err(ParseError::new(
                ParseErrorCode::ExpectedToken,
                format!("expected at least {min_items} item(s) before {end_name}"),
                self.current(),
                self.file_id,
                self.source,
            ));
        }

        Ok(items)
    }

    pub(super) fn parse_braced_member_list<T, F>(
        &mut self,
        mut parse_item: F,
    ) -> Result<Vec<T>, ParseError>
    where
        F: FnMut(&mut Self) -> Result<T, ParseError>,
    {
        self.expect_name("LBRACE")?;
        let mut items = Vec::new();
        while !self.at_kind_name("RBRACE") {
            self.skip_semicolons();
            if self.at_kind_name("RBRACE") {
                break;
            }
            if self.at_kind_name("EOF") {
                self.diagnostics.push(ParseError::new(
                    ParseErrorCode::ExpectedToken,
                    "expected '}'",
                    self.current(),
                    self.file_id,
                    self.source,
                ));
                break;
            }
            items.push(parse_item(self)?);
            self.expect_semicolon()?;
        }
        self.expect_name("RBRACE")?;
        self.skip_semicolons();
        Ok(items)
    }

    pub(super) fn parse_visibility(&mut self) -> Visibility {
        if self.eat_name("KW_PUBLIC") {
            Visibility::Public
        } else {
            Visibility::Private
        }
    }

    pub(super) fn parse_module_path(&mut self) -> Result<Vec<String>, ParseError> {
        let mut path = vec![self.expect_module_segment()?];
        while self.eat_name("DOT") {
            path.push(self.expect_module_segment()?);
        }
        Ok(path)
    }

    pub(super) fn parse_func_name(&mut self) -> Result<FuncName, ParseError> {
        let start = self.pos;
        if matches!(
            self.current().kind,
            TokenKind::IdentType | TokenKind::IdentValue
        ) {
            if let Ok(receiver) = self.parse_type_name()
                && self.eat_name("DOT")
            {
                let name = self.expect_ident_value()?;
                return Ok(FuncName::Method {
                    span: self.span_from_mark(start),
                    receiver,
                    name,
                });
            }
            self.pos = start;
        }

        let name = self.expect_ident_value()?;
        Ok(FuncName::Free {
            span: self.span_from_mark(start),
            name,
        })
    }

    pub(super) fn expect_import_name(&mut self) -> Result<String, ParseError> {
        self.expect_name_like()
    }
}
