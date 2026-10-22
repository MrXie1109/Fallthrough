//! Statements and the control forms built out of them.
//!
//! Bindings, blocks, `if`/`while`/`for`/`switch`, `raise`, `try` and the
//! initialisers a binding may write.

use super::*;

impl Parser {
    /// Parses a statement.
    pub fn parse_stmt(&mut self, stream: &mut TokenStream, ast: &mut Ast) -> Option<StmtId> {
        // `export` makes a name visible to importers, and a local name belongs
        // to one call frame. Reported here, then the statement it prefixed is
        // parsed so the rest of it still reads.
        if stream.at(TokenKind::Keyword(Keyword::Export)) {
            let exported = stream.advance();
            self.push_diagnostic(
                Diagnostic::error("`export` applies to a module-level declaration, not a local")
                    .with_code(DiagnosticCode::parser(2))
                    .with_span(exported.span)
                    .with_note("a local name belongs to one call frame"),
            );
            return self.parse_stmt(stream, ast);
        }
        let span = stream.current_span();
        let kind = match stream.current_kind() {
            // `{ .. }` on its own: a block in statement position, which is how
            // a scope is written where no `if` or loop supplies one -- and how
            // a `switch` arm gets its own.
            TokenKind::Punct(Punctuation::OpenBrace) => {
                let block = self.parse_block(stream, ast)?;
                let end = stream.current_span();
                let id = ast
                    .builder()
                    .alloc_stmt(StmtKind::Block(block), span.to(end));
                return Some(id);
            }
            TokenKind::Keyword(Keyword::Defer) => {
                stream.advance();
                let block = self.parse_block(stream, ast)?;
                StmtKind::Defer(block)
            }
            TokenKind::Keyword(Keyword::Return) => {
                stream.advance();
                let value = if stream.at(TokenKind::Punct(Punctuation::Semi)) {
                    None
                } else {
                    Some(self.parse_expr(stream, ast)?)
                };
                self.expect_punct(stream, Punctuation::Semi, "`;`")?;
                StmtKind::Return(ReturnStmt { value })
            }
            TokenKind::Keyword(Keyword::Break) => {
                stream.advance();
                self.expect_punct(stream, Punctuation::Semi, "`;`")?;
                StmtKind::Break
            }
            TokenKind::Keyword(Keyword::Continue) => {
                stream.advance();
                self.expect_punct(stream, Punctuation::Semi, "`;`")?;
                StmtKind::Continue
            }
            TokenKind::Keyword(Keyword::Fallthrough) => {
                stream.advance();
                self.expect_punct(stream, Punctuation::Semi, "`;`")?;
                StmtKind::Fallthrough
            }
            TokenKind::Keyword(Keyword::StaticAssert) => {
                StmtKind::StaticAssert(self.parse_static_assert(stream, ast)?)
            }
            TokenKind::Keyword(Keyword::If) => self.parse_if(stream, ast)?,
            TokenKind::Keyword(Keyword::While) => self.parse_while(stream, ast)?,
            TokenKind::Keyword(Keyword::For) => self.parse_for(stream, ast)?,
            TokenKind::Keyword(Keyword::Switch) => self.parse_switch(stream, ast)?,
            TokenKind::Keyword(Keyword::Raise) => self.parse_raise(stream, ast)?,
            TokenKind::Keyword(Keyword::Try) => self.parse_try(stream, ast)?,
            // A binding begins with a name followed by a type or `auto`.
            // Both are identifiers, so the decision needs two tokens of
            // lookahead and comes before the fallthrough case.
            TokenKind::Ident if self.looks_like_binding(stream) => self.parse_bind(stream, ast)?,
            // An expression statement: an expression followed by `;`.
            _ => {
                let start = stream.current_span();
                let expr = self.parse_expr(stream, ast)?;
                let semi = self.expect_punct(stream, Punctuation::Semi, "`;`")?;
                let span = start.to(semi.span);
                let _ = span;
                StmtKind::Expr(ExprStmt { expr })
            }
        };
        Some(ast.builder().alloc_stmt(kind, span))
    }

    /// Parses a brace-delimited block.
    pub fn parse_block(&mut self, stream: &mut TokenStream, ast: &mut Ast) -> Option<BlockId> {
        let open = self.expect_punct(stream, Punctuation::OpenBrace, "`{`")?;
        let mut statements = Vec::new();
        while !stream.at(TokenKind::Punct(Punctuation::CloseBrace)) && !stream.at_eof() {
            let before = stream.position();
            if let Some(stmt) = self.parse_stmt(stream, ast) {
                statements.push(stmt);
            } else {
                self.recover_to_statement(stream);
            }
            if stream.position() == before && !stream.at_eof() {
                // Guarantee progress; see `parse_items`.
                stream.advance();
            }
        }
        let close = self.expect_punct(stream, Punctuation::CloseBrace, "`}`")?;
        Some(
            ast.builder()
                .alloc_block(statements, open.span.to(close.span)),
        )
    }

    /// Skips tokens until something that can start a statement appears.
    ///
    /// Recovery is token-at-a-time: one token is discarded per attempt. It does
    /// not synchronise on the next statement keyword, even though
    /// `Keyword::starts_statement` exists in the lexer, so a malformed statement
    /// is skipped piece by piece rather than resumed at the next statement.
    pub fn recover_to_statement(&mut self, stream: &mut TokenStream) {
        if !stream.at_eof() {
            stream.advance();
        }
    }

    /// Returns `true` if the tokens at the cursor begin a binding.
    ///
    /// A binding is `name` followed by a type or `auto`. Types are written with
    /// prefix operators, so the second token can be `*`, `&`, `[`, `const`, or
    /// a name — and an expression can also begin with a name and continue with
    /// an operator, so the two are told apart by what follows.
    ///
    /// The distinction matters at `x * y`: as a statement that is a binding of
    /// a `*y`, but it could equally be a multiplication whose result is
    /// discarded. A binding requires the name to be followed by a *type*, and
    /// the only ambiguous case is a name followed by a name, which cannot be a
    /// valid expression at all.
    pub(super) fn looks_like_binding(&self, stream: &TokenStream) -> bool {
        // `name auto`
        if stream.peek_kind(1) == TokenKind::Keyword(Keyword::Auto) {
            return true;
        }
        // `[` is ambiguous between a binding and an index: `a [3]int` declares
        // an array while `a[3]` subscripts one. They are told apart by what
        // follows the `]` -- a type after it means a binding, and an operator
        // or the end of the statement means an index.
        if stream.peek_kind(1) == TokenKind::Punct(Punctuation::OpenBracket) {
            return self.bracket_is_a_type(stream);
        }
        // `name type(expr)` -- a type asked of an expression.
        if stream.peek_kind(1) == TokenKind::Keyword(Keyword::Type)
            && stream.peek_kind(2) == TokenKind::Punct(Punctuation::OpenParen)
        {
            return true;
        }
        // `name Type` where the type starts with a name or a type operator.
        // A name followed by a name is never a valid expression.
        matches!(
            stream.peek_kind(1),
            TokenKind::Ident
                | TokenKind::Keyword(Keyword::Const)
                | TokenKind::Keyword(Keyword::Fn)
                | TokenKind::Punct(Punctuation::Star)
                | TokenKind::Punct(Punctuation::StarStar)
                | TokenKind::Punct(Punctuation::Amp)
                // `x &&auto = ..` and `x &&T` are rvalue references. The
                // reading follows the one `&` already takes: `a & b;` is a
                // binding of a reference to `b`, so `a && b;` is a binding of
                // an rvalue reference to it. A logical `&&` is written where a
                // value is wanted, which a discarded comparison is not.
                | TokenKind::Punct(Punctuation::AmpAmp)
                // `x ?int` -- an optional type, so the name before it is the
                // binding's name.
                | TokenKind::Punct(Punctuation::Question)
        )
    }

    /// Returns `true` if the `[` at offset 1 begins an array *type*.
    ///
    /// The bracket is skipped to its `]`, and what follows decides: a type name
    /// or a type operator means `a [3]int`, while anything a subscript can be
    /// followed by -- `;`, `=`, an operator, a `)` -- means `a[3]`.
    ///
    /// Scanning for the matching `]` is safe here because an array length is a
    /// constant expression, which cannot itself contain an unparenthesised
    /// bracket.
    pub(super) fn bracket_is_a_type(&self, stream: &TokenStream) -> bool {
        let mut offset = 1;
        // Bound the scan: a length long enough to run past this is malformed,
        // and treating it as an index lets the expression parser report it.
        while offset < 64 {
            match stream.peek_kind(offset) {
                TokenKind::Punct(Punctuation::CloseBracket) => {
                    // A second bracket group is another *dimension* of an array
                    // type -- `m [2][3]int` -- so the decision waits for the
                    // bracket after it. Deciding here read `m[i][j] = 5;` as the
                    // binding `m` of type `[i][j]` and then reported the missing
                    // element type at the `=`.
                    if stream.peek_kind(offset + 1)
                        == TokenKind::Punct(Punctuation::OpenBracket)
                    {
                        offset += 1;
                        continue;
                    }
                    // What follows the last `]` decides.
                    return matches!(
                        stream.peek_kind(offset + 1),
                        TokenKind::Ident
                            | TokenKind::Keyword(Keyword::Const)
                            | TokenKind::Keyword(Keyword::Fn)
                            | TokenKind::Punct(Punctuation::Star)
                            | TokenKind::Punct(Punctuation::StarStar)
                            | TokenKind::Punct(Punctuation::Amp)
                            | TokenKind::Punct(Punctuation::AmpAmp)
                            | TokenKind::Punct(Punctuation::Question)
                    );
                }
                TokenKind::Eof => return false,
                _ => offset += 1,
            }
        }
        false
    }

    /// Parses a variable binding.
    ///
    /// ```text
    /// name type;              // zero-initialised
    /// name type = expr;       // initialised from an expression
    /// name type = .{ .. };    // initialised from a designated initialiser
    /// name auto = expr;       // type deduced from the initialiser
    /// ```
    ///
    /// The two `auto` forms that cannot be deduced (`name auto;` and
    /// `name auto = .{ .. };`) are parsed successfully and rejected later, so
    /// the diagnostic can say why. See [`fallc_ast::BindStmt`].
    pub(super) fn parse_bind(&mut self, stream: &mut TokenStream, ast: &mut Ast) -> Option<StmtKind> {
        self.parse_bind_inner(stream, ast, true)
    }

    /// Parses a binding, optionally consuming its trailing `;`.
    ///
    /// A `for` initialiser is a binding without one, because the `;` there
    /// separates the loop's clauses rather than ending a statement.
    pub(super) fn parse_bind_inner(
        &mut self,
        stream: &mut TokenStream,
        ast: &mut Ast,
        expect_semi: bool,
    ) -> Option<StmtKind> {
        let name_token = self.parse_ident(stream, "a binding name")?;
        let name = self.token_text(stream, &name_token);

        let ty = if stream.at(TokenKind::Keyword(Keyword::Auto)) {
            let span = stream.current_span();
            stream.advance();
            BindingType::Auto(span)
        } else {
            BindingType::Written(self.parse_type(stream, ast)?)
        };

        // `mutable` is a property of a field: a `const` member function needs
        // it so that one field can still be written. A binding has no such
        // exception to make, so the word is refused where it would do nothing.
        if stream.at(TokenKind::Keyword(Keyword::Mutable)) {
            self.diagnostics.push(
                Diagnostic::error("`mutable` is a property of a field, not of a binding")
                    .with_code(DiagnosticCode::parser(2))
                    .with_span(stream.current_span())
                    .with_note(
                        "a `const` member function needs it so that one field can still be \
                         written; a binding has no such exception to make",
                    ),
            );
            return None;
        }
        let (mut is_static, mut is_thread_local, mut align) = (false, false, None);
        // The new spelling writes the storage class as attributes after the
        // declaration: `count int @static = 0;`. They mean what the old
        // modifiers meant, so they are folded into the same fields and nothing
        // downstream has to know both spellings.
        let suffix_attrs = self.parse_attributes(stream, ast)?;
        let mut exported = false;
        for attr in &suffix_attrs {
            match attr.name.as_str() {
                "static" => is_static = true,
                "thread_local" => is_thread_local = true,
                "export" => exported = true,
                "align" => {
                    let Some(value) = attr.arguments.first().copied() else {
                        continue;
                    };
                    align = Some(fallc_ast::AlignSpec {
                        alignment: value,
                        span: attr.span,
                    });
                }
                _ => {}
            }
        }
        // Per-thread storage outlives the call, so a local marked `thread_local`
        // is a static local as well; the language gives `thread_local` alone the
        // storage class a reader would expect.
        let is_static = is_static || is_thread_local;

        let is_uninitialized = suffix_attrs
            .iter()
            .any(|attr| attr.name == "uninitialized");
        if is_uninitialized && stream.at(TokenKind::Punct(Punctuation::Eq)) {
            self.diagnostics.push(
                Diagnostic::error(
                    "`@uninitialized` cannot have an initialiser: the attribute means nothing \
                     is written before the program does",
                )
                .with_code(DiagnosticCode::parser(2))
                .with_span(stream.current_span())
                .with_note("drop the attribute, or drop `= value`"),
            );
            return None;
        }
        let init = if stream.eat(TokenKind::Punct(Punctuation::Eq)).is_some() {
            if stream.at(TokenKind::Punct(Punctuation::DotBrace)) {
                Some(Initializer::Designated(
                    self.parse_designated_init(stream, ast)?,
                ))
            } else {
                Some(Initializer::Expr(self.parse_expr(stream, ast)?))
            }
        } else {
            None
        };

        if expect_semi {
            self.expect_punct(stream, Punctuation::Semi, "`;`")?;
        }
        Some(StmtKind::Bind(BindStmt {
            name,
            name_span: name_token.span,
            ty,
            init,
            is_static,
            is_thread_local,
            align,
            exported,
            is_uninitialized,
            attrs: suffix_attrs,
        }))
    }

    /// Parses `.{ .. }`.
    ///
    /// The elements are expressions, in field order, and a nested `.{ .. }` is
    /// one of them, so nesting needs no special case. The raw text between the
    /// braces is kept as well, so a diagnostic can quote what was written.
    pub(super) fn parse_designated_init(
        &mut self,
        stream: &mut TokenStream,
        ast: &mut Ast,
    ) -> Option<DesignatedInit> {
        let open = self.expect_punct(stream, Punctuation::DotBrace, "`.{`")?;
        self.parse_init_body(stream, ast, open.span)
    }

    /// Parses the `{ .. }` of a typed initialiser, `Point{ .. }`.
    pub(super) fn parse_braced_init(
        &mut self,
        stream: &mut TokenStream,
        ast: &mut Ast,
    ) -> Option<DesignatedInit> {
        let open = self.expect_punct(stream, Punctuation::OpenBrace, "`{`")?;
        self.parse_init_body(stream, ast, open.span)
    }

    /// Parses an initialiser's elements, given the span of its opener.
    pub(super) fn parse_init_body(
        &mut self,
        stream: &mut TokenStream,
        ast: &mut Ast,
        open_span: Span,
    ) -> Option<DesignatedInit> {
        let inner_start = open_span.hi;

        // The elements, parsed as expressions: a prefix by position, then
        // `.field = value` for the ones named. A trailing comma is allowed, so
        // a one-element list can be written unambiguously.
        let mut elements = Vec::new();
        let mut named = Vec::new();
        if !stream.at(TokenKind::Punct(Punctuation::CloseBrace)) {
            loop {
                if self.at_named_argument(stream) {
                    named.push(self.parse_named_argument(stream, ast)?);
                } else {
                    if !named.is_empty() {
                        self.diagnostics.push(
                            Diagnostic::error(
                                "a positional element cannot follow a named one",
                            )
                            .with_code(DiagnosticCode::parser(2))
                            .with_span(stream.current_span())
                            .with_note(
                                "write every positional element first, then `.field = value` \
                                 for the ones named",
                            ),
                        );
                    }
                    elements.push(self.parse_expr_no_comma(stream, ast)?);
                }
                if stream.eat(TokenKind::Punct(Punctuation::Comma)).is_none() {
                    break;
                }
                if stream.at(TokenKind::Punct(Punctuation::CloseBrace)) {
                    break;
                }
            }
        }
        let close = self.expect_punct(stream, Punctuation::CloseBrace, "`}`")?;
        let close_span = close.span;

        let text = self
            .source_text(inner_start, close_span.lo)
            .trim()
            .to_owned();
        // The expression id is filled in by the caller when the initialiser is
        // nested, since only then is it an expression.
        let _ = &mut elements;
        Some(DesignatedInit {
            expr: fallc_ast::ExprId::DUMMY,
            text,
            elements,
            named,
            span: open_span.to(close_span),
        })
    }

    /// Returns the source text in `[lo, hi)`, or empty if unavailable.
    pub(super) fn source_text(&self, lo: u32, hi: u32) -> &str {
        self.source
            .as_ref()
            .and_then(|text| text.get(lo as usize..hi as usize))
            .unwrap_or("")
    }

    /// Parses `if (cond) block (else if (cond) block)* (else block)?`.
    ///
    /// An `else if` chain is built as a nested `if` statement inside the outer
    /// statement's else branch, so the shape is uniform at every level.
    ///
    /// `if constexpr (cond)` is the same statement with the marker set: the
    /// condition must be a constant, and the branch not taken is not checked.
    /// Parses the attributes a branch may carry, `@likely` and `@unlikely`.
    ///
    /// They are hints about which way execution usually goes: caching and code
    /// layout use them, semantics do not. A name that is an attribute
    /// elsewhere (`@inline`, `@export`) is a mistake here and is reported here;
    /// the checker never sees it.
    fn parse_branch_attributes(
        &mut self,
        stream: &mut TokenStream,
        ast: &mut Ast,
        allow_constexpr: bool,
    ) -> Option<Vec<fallc_ast::Attr>> {
        let attrs = self.parse_attributes(stream, ast)?;
        let mut hints = Vec::new();
        for attr in attrs {
            let is_hint = attr.name == "likely" || attr.name == "unlikely";
            let is_constexpr = allow_constexpr && attr.name == "constexpr";
            if is_hint || is_constexpr {
                if !attr.arguments.is_empty() {
                    self.diagnostics.push(
                        Diagnostic::error(format!("`@{}` takes no arguments", attr.name))
                            .with_code(DiagnosticCode::parser(2))
                            .with_span(attr.span),
                    );
                }
                hints.push(attr);
            } else {
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "`@{}` is not an attribute of this branch",
                        attr.name
                    ))
                    .with_code(DiagnosticCode::parser(2))
                    .with_span(attr.span)
                    .with_note(if allow_constexpr {
                        "an `if` condition takes `@constexpr`, `@likely` or `@unlikely`"
                    } else {
                        "this branch takes `@likely` or `@unlikely`"
                    }),
                );
            }
        }
        Some(hints)
    }

    pub(super) fn parse_if(&mut self, stream: &mut TokenStream, ast: &mut Ast) -> Option<StmtKind> {
        let start = stream.current_span();
        stream.advance(); // `if`
        let is_constexpr = false;
        let condition = self.parse_paren_condition(stream, ast)?;
        // `if (cond) @constexpr { .. }` is the attribute spelling of the old
        // `if constexpr (cond)`: the condition must be a constant, and the
        // branch not taken is not checked. `@constexpr` is only meaningful on
        // an `if`; `@likely`/`@unlikely` are accepted on every branch.
        let condition_attrs = self.parse_branch_attributes(stream, ast, true)?;
        let is_constexpr = is_constexpr
            || condition_attrs.iter().any(|attr| attr.name == "constexpr");
        let then_block = self.parse_block(stream, ast)?;

        let mut else_attrs = Vec::new();
        let else_branch = if stream.at(TokenKind::Keyword(Keyword::Else)) {
            stream.advance();
            if stream.at(TokenKind::Keyword(Keyword::If)) {
                // `else if` becomes a nested statement.
                let nested_span = stream.current_span();
                let nested_kind = self.parse_if(stream, ast)?;
                let nested = ast.builder().alloc_stmt(nested_kind, nested_span);
                Some(ElseBranch::If(nested))
            } else {
                else_attrs = self.parse_branch_attributes(stream, ast, false)?;
                Some(ElseBranch::Block(self.parse_block(stream, ast)?))
            }
        } else {
            None
        };

        let _ = start;
        Some(StmtKind::If(IfStmt {
            is_constexpr,
            condition,
            condition_attrs,
            then_block,
            else_branch,
            else_attrs,
        }))
    }

    /// Parses `while (cond) block (else block)?`.
    pub(super) fn parse_while(&mut self, stream: &mut TokenStream, ast: &mut Ast) -> Option<StmtKind> {
        stream.advance(); // `while`
        let condition = self.parse_paren_condition(stream, ast)?;
        let body = self.parse_block(stream, ast)?;
        let else_block = self.parse_optional_else(stream, ast)?;
        Some(StmtKind::While(LoopStmt {
            condition,
            body,
            else_block,
        }))
    }

    /// Parses a C-style `for (init; cond; update) block` or the range form
    /// `for (elem type : range) block`, each with an optional `else`.
    ///
    /// The two forms share a prefix and a suffix, and differ only after the
    /// first item. The decision is made after parsing the binding, by looking
    /// for `:` (range form) or `;` (C-style).
    pub(super) fn parse_for(&mut self, stream: &mut TokenStream, ast: &mut Ast) -> Option<StmtKind> {
        let start = stream.advance().span; // `for`
        self.expect_punct(stream, Punctuation::OpenParen, "`(`")?;

        // The range form starts with a name followed by a type, so it is found
        // by reading those speculatively and looking for the `:`. Reading them
        // is the only reliable test: `for (x &int : a)` starts with a type
        // operator, and `for (i int = 0; ..)` has the same prefix and then a
        // `=` instead.
        let header = self.speculate(stream, |parser, stream| {
            let name_token = parser.parse_ident(stream, "a loop variable")?;
            let name = parser.token_text(stream, &name_token);
            let ty = if stream.at(TokenKind::Keyword(Keyword::Auto)) {
                let auto = stream.advance().span;
                fallc_ast::BindingType::Auto(auto)
            } else {
                fallc_ast::BindingType::Written(parser.parse_type(stream, ast)?)
            };
            if stream.at(TokenKind::Punct(Punctuation::Colon)) {
                Some((name, name_token.span, ty))
            } else {
                None
            }
        });

        let kind = if let Some((binding_name, name_span, binding_type)) = header {
            self.expect_punct(stream, Punctuation::Colon, "`:`")?;
            let range = self.parse_expr(stream, ast)?;
            self.expect_punct(stream, Punctuation::CloseParen, "`)`")?;
            let body = self.parse_block(stream, ast)?;
            let else_block = self.parse_optional_else(stream, ast)?;
            let end = match else_block {
                Some(block) => ast.block(block).span,
                None => ast.block(body).span,
            };
            self.desugar_for_each(
                ast,
                start,
                start.to(end),
                binding_name,
                name_span,
                binding_type,
                range,
                body,
                else_block,
            )?
        } else {
            // C-style. Each clause may be empty.
            let init = if stream.at(TokenKind::Punct(Punctuation::Semi)) {
                None
            } else if stream.at(TokenKind::Ident)
                && matches!(
                    stream.peek_kind(1),
                    TokenKind::Ident | TokenKind::Keyword(Keyword::Auto)
                )
            {
                // A binding declaration, which is what normally appears here.
                let start = stream.current_span();
                // The binding's own `;` is the clause separator, so it is not
                // consumed here.
                let kind = self.parse_bind_inner(stream, ast, false)?;
                let span = start.to(stream.current_span());
                Some(ast.builder().alloc_stmt(kind, span))
            } else {
                // Any expression may stand here, as it may anywhere a
                // statement is allowed: `for (i = 0; ..)` is a loop that starts
                // from an existing variable, and `for (reset(); ..)` is one
                // that starts by calling something. The clause's own `;` is the
                // separator, so it is not consumed here.
                let start = stream.current_span();
                let expr = self.parse_expr(stream, ast)?;
                let span = start.to(ast.expr(expr).span);
                Some(
                    ast.builder()
                        .alloc_stmt(StmtKind::Expr(ExprStmt { expr }), span),
                )
            };
            self.expect_punct(stream, Punctuation::Semi, "`;`")?;

            let condition = if stream.at(TokenKind::Punct(Punctuation::Semi)) {
                None
            } else {
                Some(self.parse_expr(stream, ast)?)
            };
            self.expect_punct(stream, Punctuation::Semi, "`;`")?;

            let update = if stream.at(TokenKind::Punct(Punctuation::CloseParen)) {
                None
            } else {
                Some(self.parse_expr(stream, ast)?)
            };
            self.expect_punct(stream, Punctuation::CloseParen, "`)`")?;

            let body = self.parse_block(stream, ast)?;
            let else_block = self.parse_optional_else(stream, ast)?;
            StmtKind::ForC(ForCStmt {
                init,
                condition,
                update,
                body,
                else_block,
            })
        };
        Some(kind)
    }

    /// Rewrites `for (x T : range) { .. }` into the ordinary statements that
    /// perform it.
    ///
    /// # The five operations
    ///
    /// A range loop is C++'s: the range is asked for a beginning and an end,
    /// the position is compared with the end, advanced with `++`, and read with
    /// `*`. Every one of those is an operation the language already has, so the
    /// loop is *desugared* rather than given a lowering of its own -- which is
    /// also what makes a type iterable by providing five methods and nothing
    /// else.
    ///
    /// ```text
    /// {
    ///     auto $range = <range>;      // only when the range is not a place
    ///     auto $it = $range.begin();
    ///     auto $end = $range.end();
    ///     for (; $it != $end; ++$it) {
    ///         x T = *$it;
    ///         <body>
    ///     } else { <else> }
    /// }
    /// ```
    ///
    /// # Why the names carry a `$`
    ///
    /// The three names are made up by the compiler, and `$` cannot appear in an
    /// identifier a program writes, so they can neither shadow nor be shadowed
    /// by one of the program's own.
    ///
    /// # Why the range is sometimes not bound
    ///
    /// `for (c char : s)` iterates `s` itself: calling the two operations on
    /// the expression twice is free and correct when the expression only names
    /// storage, and it means a write through the loop variable reaches `s`.
    /// A range that is a *value* -- `for (c char : make())` -- is bound once so
    /// that it is evaluated once.
    ///
    /// # Why the loop variable is a binding inside the body
    ///
    /// `x T = *$it;` is the same binding form as everywhere else, so `x int`
    /// copies the element and `x &int` names the element itself, and the
    /// declaration's own type rules decide how a reference becomes a value.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn desugar_for_each(
        &mut self,
        ast: &mut Ast,
        start: Span,
        whole: Span,
        binding_name: String,
        name_span: Span,
        binding_type: fallc_ast::BindingType,
        range: ExprId,
        body: BlockId,
        else_block: Option<BlockId>,
    ) -> Option<StmtKind> {
        // Every made-up node needs a span of its own: the checker records what
        // it learned about an expression *by span*, so two nodes sharing one
        // would overwrite each other's answer. The spans grow a byte at a time
        // from the `for` keyword, which keeps them inside the statement --
        // every one of them points at the loop, which is where a reader would
        // look anyway.
        // The widths run *down* from the statement's own length, so the nodes
        // allocated first -- the two calls, which are the ones that fail when a
        // type is not iterable -- are the ones whose span covers the whole
        // loop, and the caret points at something a reader can see.
        let file = start.file;
        let widest = whole.hi.saturating_sub(start.lo).max(1);
        let mut taken = 0u32;
        let mut span = || {
            let width = widest.saturating_sub(taken).max(1);
            taken += 1;
            Span::new(file, start.lo, start.lo + width)
        };
        let n = |text: &str| -> String { text.to_owned() };

        let mut statements: Vec<StmtId> = Vec::new();
        // The range: the expression itself when it only names storage, and a
        // copy bound once when it is a value.
        let receiver = if self.names_storage(ast, range) {
            range
        } else {
            let bind_span = span();
            let name_span = span();
            let auto_span = span();
            let name = ast.builder().alloc_expr(ExprKind::Name(n("$range")), name_span);
            statements.push(ast.builder().alloc_stmt(
                StmtKind::Bind(BindStmt {
                    name: n("$range"),
                    name_span,
                    ty: fallc_ast::BindingType::Auto(auto_span),
                    init: Some(fallc_ast::Initializer::Expr(range)),
                    is_static: false,
                    is_thread_local: false,
                    align: None,
                    exported: false,
                    is_uninitialized: false,
                    attrs: Vec::new(),
                }),
                bind_span,
            ));
            name
        };

        // `$range.begin()` and `$range.end()`. The receiver node is used twice,
        // which is a small departure from a strict tree: both calls name the
        // same storage, and copying the expression would mean a second node for
        // the same thing.
        let mut call_on = |field: &str, receiver: ExprId| -> ExprId {
            let field_span = span();
            let call_span = span();
            let callee = ast.builder().alloc_expr(
                ExprKind::Field(FieldExpr {
                    object: receiver,
                    field: n(field),
                    through_pointer: false,
                }),
                field_span,
            );
            ast.builder().alloc_expr(
                ExprKind::Call(CallExpr {
                    callee,
                    args: Vec::new(),
                    named: Vec::new(),
                    template_args: Vec::new(),
                    explicit_template_arguments: false,
                }),
                call_span,
            )
        };
        let begin = call_on("begin", receiver);
        let end = call_on("end", receiver);

        // `auto $it = ..;` and `auto $end = ..;`, in the block the loop lives
        // in, so that both outlive it.
        let mut bind_made_up = |name: &str, init: ExprId| -> StmtId {
            let bind_span = span();
            let name_span = span();
            let auto_span = span();
            ast.builder().alloc_stmt(
                StmtKind::Bind(BindStmt {
                    name: n(name),
                    name_span,
                    ty: fallc_ast::BindingType::Auto(auto_span),
                    init: Some(fallc_ast::Initializer::Expr(init)),
                    is_static: false,
                    is_thread_local: false,
                    align: None,
                    exported: false,
                    is_uninitialized: false,
                    attrs: Vec::new(),
                }),
                bind_span,
            )
        };
        statements.push(bind_made_up("$it", begin));
        statements.push(bind_made_up("$end", end));

        // `$it != $end`.
        let condition = {
            let lhs_span = span();
            let lhs = ast.builder().alloc_expr(ExprKind::Name(n("$it")), lhs_span);
            let rhs_span = span();
            let rhs = ast.builder().alloc_expr(ExprKind::Name(n("$end")), rhs_span);
            let binary_span = span();
            ast.builder().alloc_expr(
                ExprKind::Binary(BinaryExpr {
                    op: BinaryOp::Ne,
                    lhs,
                    rhs,
                }),
                binary_span,
            )
        };
        // `++$it`, which is the prefix form: the loop wants the position
        // advanced, and the value of `$it++` is of no use to anyone.
        let update = {
            let operand_span = span();
            let operand = ast.builder().alloc_expr(ExprKind::Name(n("$it")), operand_span);
            let incdec_span = span();
            ast.builder().alloc_expr(
                ExprKind::IncDec(IncDecExpr {
                    op: IncDecOp::Increment,
                    postfix: false,
                    operand,
                }),
                incdec_span,
            )
        };
        // `x T = *$it;`, the declaration the program wrote, with the element as
        // its initialiser.
        let loop_variable = {
            let operand_span = span();
            let operand = ast.builder().alloc_expr(ExprKind::Name(n("$it")), operand_span);
            let deref_span = span();
            let element = ast.builder().alloc_expr(
                ExprKind::Unary(UnaryExpr {
                    op: UnaryOp::Deref,
                    operand,
                }),
                deref_span,
            );
            let bind_span = span();
            ast.builder().alloc_stmt(
                StmtKind::Bind(BindStmt {
                    name: binding_name,
                    name_span,
                    ty: binding_type,
                    init: Some(fallc_ast::Initializer::Expr(element)),
                    is_static: false,
                    is_thread_local: false,
                    align: None,
                    exported: false,
                    is_uninitialized: false,
                    attrs: Vec::new(),
                }),
                bind_span,
            )
        };

        // The body keeps the statements the program wrote, with the loop
        // variable's declaration in front of them.
        let mut body_statements = vec![loop_variable];
        body_statements.extend(ast.block(body).statements.iter().copied());
        let body_span = ast.block(body).span;
        let body = ast.builder().alloc_block(body_statements, body_span);

        let loop_stmt = ast.builder().alloc_stmt(
            StmtKind::ForC(ForCStmt {
                // No initialiser: the position was taken above, where it also
                // has to be visible to the condition.
                init: None,
                condition: Some(condition),
                update: Some(update),
                body,
                else_block,
            }),
            whole,
        );
        statements.push(loop_stmt);
        let block = ast.builder().alloc_block(statements, whole);
        Some(StmtKind::Block(block))
    }

    /// Returns `true` if the expression only names storage that already exists.
    ///
    /// Used to decide whether an expression may be *re-read*: a range loop calls
    /// two operations on it, and re-evaluating `s` is free while re-evaluating
    /// `f()` would call `f` twice.
    pub(super) fn names_storage(&self, ast: &Ast, expr: ExprId) -> bool {
        match &ast.expr(expr).kind {
            ExprKind::Name(_) => true,
            // A field is a place when the object is one: `this._items` names
            // storage, while `f().field` would evaluate `f` again.
            ExprKind::Field(field) => self.names_storage(ast, field.object),
            // An index is a place only when nothing in it has to be evaluated
            // twice: `a[i]` is, `a[f()]` is not.
            ExprKind::Index(index) => {
                self.names_storage(ast, index.object) && self.names_storage(ast, index.index)
            }
            ExprKind::Unary(unary) if unary.op == fallc_ast::UnaryOp::Deref => {
                self.names_storage(ast, unary.operand)
            }
            _ => false,
        }
    }

    /// Parses `switch (expr) { arm* }`.
    ///
    /// Arms may appear in any order, and `default` may appear anywhere. A case
    /// body is a statement list that ends at the next `case`, `default`, or the
    /// closing brace.
    pub(super) fn parse_switch(&mut self, stream: &mut TokenStream, ast: &mut Ast) -> Option<StmtKind> {
        stream.advance(); // `switch`
        let scrutinee = self.parse_paren_condition(stream, ast)?;
        self.expect_punct(stream, Punctuation::OpenBrace, "`{`")?;

        let mut arms = Vec::new();
        while !stream.at(TokenKind::Punct(Punctuation::CloseBrace)) && !stream.at_eof() {
            let arm_start = stream.current_span();
            let labels = if stream.at(TokenKind::Keyword(Keyword::Case)) {
                stream.advance();
                // The comma here is a *separator* between labels, not the comma
                // operator, so each label is parsed without comma. Parsing them
                // with `parse_expr` would fold `case 1, 2:` into one comma
                // expression and silently drop the first label.
                let mut labels = vec![self.parse_expr_no_comma(stream, ast)?];
                while stream.eat(TokenKind::Punct(Punctuation::Comma)).is_some() {
                    labels.push(self.parse_expr_no_comma(stream, ast)?);
                }
                Some(labels)
            } else if stream.at(TokenKind::Keyword(Keyword::Default)) {
                stream.advance();
                None
            } else {
                self.report_unexpected(stream, "`case` or `default`");
                self.recover_to_statement(stream);
                continue;
            };
            let arm_attrs = self.parse_branch_attributes(stream, ast, false)?;
            self.expect_punct(stream, Punctuation::Colon, "`:`")?;

            let mut body = Vec::new();
            while !stream.at(TokenKind::Keyword(Keyword::Case))
                && !stream.at(TokenKind::Keyword(Keyword::Default))
                && !stream.at(TokenKind::Punct(Punctuation::CloseBrace))
                && !stream.at_eof()
            {
                let before = stream.position();
                if let Some(stmt) = self.parse_stmt(stream, ast) {
                    body.push(stmt);
                } else {
                    self.recover_to_statement(stream);
                }
                if stream.position() == before && !stream.at_eof() {
                    stream.advance();
                }
            }

            let arm_end = if body.is_empty() {
                arm_start
            } else {
                ast.stmt(*body.last().expect("body is non-empty")).span
            };
            arms.push(SwitchArm {
                labels,
                body,
                span: arm_start.to(arm_end),
                attrs: arm_attrs,
            });
        }
        self.expect_punct(stream, Punctuation::CloseBrace, "`}`")?;
        Some(StmtKind::Switch(SwitchStmt { scrutinee, arms }))
    }

    /// Parses `raise "type": "what";` or the bare `raise;`.
    pub(super) fn parse_raise(&mut self, stream: &mut TokenStream, ast: &mut Ast) -> Option<StmtKind> {
        stream.advance(); // `raise`
        let node = if stream.at(TokenKind::Punct(Punctuation::Semi)) {
            RaiseStmt {
                exception_type: None,
                message: None,
            }
        } else {
            let exception_type = self.parse_expr(stream, ast)?;
            // `raise "Type";` gives the type and no message; `raise "Type":
            // "what";` adds one. The message is the part a program composes, so
            // it is the part that may be left out.
            let message = if stream.at(TokenKind::Punct(Punctuation::Colon)) {
                stream.advance();
                Some(self.parse_expr(stream, ast)?)
            } else {
                None
            };
            RaiseStmt {
                exception_type: Some(exception_type),
                message,
            }
        };
        self.expect_punct(stream, Punctuation::Semi, "`;`")?;
        Some(StmtKind::Raise(node))
    }

    /// Parses `try block (except block)? (else block)? (finally block)?`.
    ///
    /// Every clause but the `try` body is optional, and they must appear in
    /// that order.
    pub(super) fn parse_try(&mut self, stream: &mut TokenStream, ast: &mut Ast) -> Option<StmtKind> {
        stream.advance(); // `try`
        let body = self.parse_block(stream, ast)?;

        // Several `except` clauses may follow, in the order they are tried:
        // `except ("A") { .. } except ("B") { .. } except { .. }`. A clause
        // with names catches only those; a bare clause catches everything.
        let mut except_clauses = Vec::new();
        let mut else_block = None;
        let mut finally_block = None;

        while stream.at(TokenKind::Keyword(Keyword::Except)) {
            let start = stream.advance().span;
            let mut names = Vec::new();
            if stream.at(TokenKind::Punct(Punctuation::OpenParen)) {
                stream.advance();
                loop {
                    names.push(self.parse_expr(stream, ast)?);
                    if stream.at(TokenKind::Punct(Punctuation::Comma)) {
                        stream.advance();
                        continue;
                    }
                    break;
                }
                self.expect_punct(stream, Punctuation::CloseParen, "`)`")?;
            }
            let block = self.parse_block(stream, ast)?;
            let span = start.to(ast.block(block).span);
            except_clauses.push(fallc_ast::ExceptClause { names, block, span });
        }
        if stream.at(TokenKind::Keyword(Keyword::Else)) {
            stream.advance();
            else_block = Some(self.parse_block(stream, ast)?);
        }
        if stream.at(TokenKind::Keyword(Keyword::Finally)) {
            stream.advance();
            finally_block = Some(self.parse_block(stream, ast)?);
        }
        // A `try` with no clause at all still parses; whether it is useful is
        // a semantic question, not a syntactic one.

        Some(StmtKind::Try(TryStmt {
            body,
            except_clauses,
            else_block,
            finally_block,
        }))
    }

    /// Parses `( expr )`, as used by `if`, `while`, `for`, and `switch`.
    pub(super) fn parse_paren_condition(&mut self, stream: &mut TokenStream, ast: &mut Ast) -> Option<ExprId> {
        self.expect_punct(stream, Punctuation::OpenParen, "`(`")?;
        let condition = self.parse_expr(stream, ast)?;
        self.expect_punct(stream, Punctuation::CloseParen, "`)`")?;
        Some(condition)
    }

    /// Parses an optional `else block`, as the loop statements may carry.
    pub(super) fn parse_optional_else(
        &mut self,
        stream: &mut TokenStream,
        ast: &mut Ast,
    ) -> Option<Option<BlockId>> {
        if stream.at(TokenKind::Keyword(Keyword::Else)) {
            stream.advance();
            Some(Some(self.parse_block(stream, ast)?))
        } else {
            Some(None)
        }
    }

    // ---------------------------------------------------------- expressions

}
