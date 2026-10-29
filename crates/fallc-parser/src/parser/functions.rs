//! Functions, parameters and types.
//!
//! A function's parameter list is where a type meets a name, so the type
//! grammar is parsed here too, together with the fold forms that make a type
//! out of an expression.

use super::*;

impl Parser {
    /// Parses `fn name(params) ret { body }` or `fn name(params) ret;`.
    ///
    /// The semicolon form declares a function implemented elsewhere — a C
    /// function, for instance — so it has no body and no block.
    pub(super) fn parse_function(
        &mut self,
        stream: &mut TokenStream,
        ast: &mut Ast,
        start: Span,
        template_params: Vec<TemplateParam>,
        prefix: DeclarationPrefix,
    ) -> Option<ItemId> {
        let DeclarationPrefix {
            exported,
            abi,
            is_constexpr,
        } = prefix;
        stream.advance(); // `fn`

        let name_token = self.parse_ident(stream, "a function name")?;
        let name = self.token_text(stream, &name_token);
        let (params, variadic) = self.parse_params(stream, ast)?;

        // A definition may leave the return type out: it then means `auto`,
        // deduced from the body's `return` statements. A declaration has no body
        // to deduce from, so it must write its type -- `void` included, as
        // `@extern_C fn exit(status int) void;` does. That is also what a
        // defaulted or deleted function is: a declaration.
        let return_type = if stream.at(TokenKind::Punct(Punctuation::OpenBrace)) {
            TypeSyntax::Auto(stream.current_span())
        } else if stream.at(TokenKind::Punct(Punctuation::Semi))
            || stream.at(TokenKind::Punct(Punctuation::Eq))
        {
            self.diagnostics.push(
                Diagnostic::error("a function declaration must write its return type")
                    .with_code(DiagnosticCode::parser(2))
                    .with_span(stream.current_span())
                    .with_note(
                        "write `void` for a function that returns no value; `auto` needs a body \
                         to deduce from",
                    ),
            );
            TypeSyntax::Named(NamedType {
                // An empty name means "no value". The checker recognises it, so
                // there is no separate node for a missing return type.
                path: String::new(),
                arguments: Vec::new(),
                is_expansion: false,
                span: stream.current_span(),
            })
        } else {
            self.parse_type(stream, ast)?
        };

        // `except` after the return type: the body may raise an exception.
        //
        // `except(cond)` adds a claim -- it may raise only when `cond` holds --
        // and `cond` is a constant the checker decides. A keyword rather than
        // punctuation: the annotation is a statement about the function, and
        // `??` is kept for the two expression forms, which is where the work
        // happens.
        let throws = if stream.at(TokenKind::Keyword(Keyword::Except)) {
            let start = stream.advance().span;
            // `except(auto)` asks the compiler to decide whether the function
            // can raise, instead of the declaration saying so. `auto` is a
            // keyword, so it cannot be a condition expression.
            let mut is_auto = false;
            let condition = if stream.at(TokenKind::Punct(Punctuation::OpenParen)) {
                stream.advance();
                if stream.at(TokenKind::Keyword(Keyword::Auto)) {
                    stream.advance();
                    is_auto = true;
                    self.expect_punct(stream, Punctuation::CloseParen, "`)`")?;
                    None
                } else {
                    let condition = self.parse_expr_no_comma(stream, ast)?;
                    self.expect_punct(stream, Punctuation::CloseParen, "`)`")?;
                    Some(condition)
                }
            } else {
                None
            };
            Some(fallc_ast::Throws {
                condition,
                is_auto,
                span: start,
            })
        } else {
            None
        };

        // A `;` means a declaration with no body.
        let (body, is_external, span) = if stream.at(TokenKind::Punct(Punctuation::Semi)) {
            let semi = stream.advance();
            (None, true, start.to(semi.span))
        } else {
            let body = self.parse_block(stream, ast)?;
            let span = start.to(ast.block(body).span);
            (Some(body), false, span)
        };

        // `fn __setup__() void { .. }` is the module's setup: a function in
        // form, so that it is declared, checked and called like one, and a
        // distinct item in the tree because what runs it is the program's
        // start rather than a call anyone wrote.
        if name == "__setup__"
            && let Some(body) = body
        {
            let span = start.to(ast.block(body).span);
            return Some(ast.builder().alloc_item(ItemKind::Setup(body), span));
        }
        Some(ast.builder().alloc_item(
            ItemKind::Function(FunctionItem {
                is_constexpr,
                abi,
                exported,
                throws,
                template_params,
                is_external,
                variadic,
                name,
                // Filled in once every declaration is known.
                overloaded: false,
                name_span: name_token.span,
                params,
                return_type,
                body,
            }),
            span,
        ))
    }

    /// Parses `static_assert(cond);` or `static_assert(cond, "message");`.
    ///
    /// The message is not an arbitrary expression: it is what the compiler
    /// prints when the claim fails, so it has to be known while compiling, and
    /// a string literal is the only form that is.
    pub(super) fn parse_static_assert(
        &mut self,
        stream: &mut TokenStream,
        ast: &mut Ast,
    ) -> Option<StaticAssert> {
        let keyword = stream.advance();
        self.expect_punct(stream, Punctuation::OpenParen, "`(`")?;
        // The condition is parsed without the comma operator, so that `,`
        // separates it from the message rather than joining the two.
        let condition = self.parse_expr_no_comma(stream, ast)?;
        let message = if stream.eat(TokenKind::Punct(Punctuation::Comma)).is_some() {
            let argument = self.parse_expr_no_comma(stream, ast)?;
            match &ast.expr(argument).kind {
                ExprKind::StringLiteral(text) | ExprKind::RawStringLiteral(text) => {
                    let body = if matches!(&ast.expr(argument).kind, ExprKind::RawStringLiteral(_))
                    {
                        // A raw message is taken verbatim; its own rule finds
                        // the text, and no escape is decoded.
                        fallc_ast::literal::string_body(text).to_owned()
                    } else {
                        let bytes = fallc_ast::literal::decode_escapes(
                            fallc_ast::literal::string_body(text),
                        );
                        String::from_utf8_lossy(&bytes).into_owned()
                    };
                    Some(body)
                }
                _ => {
                    self.diagnostics.push(
                        Diagnostic::error(
                            "a `static_assert` message must be a string literal",
                        )
                        .with_code(DiagnosticCode::parser(2))
                        .with_span(ast.expr(argument).span),
                    );
                    None
                }
            }
        } else {
            None
        };
        let close = self.expect_punct(stream, Punctuation::CloseParen, "`)`")?;
        let semi = self.expect_punct(stream, Punctuation::Semi, "`;`")?;
        Some(StaticAssert {
            condition,
            message,
            span: keyword.span.to(close.span).to(semi.span),
        })
    }

    /// Parses `( p T, q U )`.
    ///
    /// A parameter's name may be omitted, in which case it is positional only
    /// and cannot be referred to -- which is what a selector parameter is, such
    /// as the `*double` in `__as__`. A `= expr` gives a default, defaults must
    /// come last, and a trailing `...` makes the function variadic.
    pub(super) fn parse_params(
        &mut self,
        stream: &mut TokenStream,
        ast: &mut Ast,
    ) -> Option<(Vec<Param>, bool)> {
        self.expect_punct(stream, Punctuation::OpenParen, "`(`")?;
        let mut params = Vec::new();
        let mut variadic = false;

        if !stream.at(TokenKind::Punct(Punctuation::CloseParen)) {
            loop {
                // `...` ends the parameter list and accepts any further
                // arguments. It must be last, so anything after it is an error.
                if stream.at(TokenKind::Punct(Punctuation::DotDotDot)) {
                    stream.advance();
                    variadic = true;
                    break;
                }
                // A parameter's name may be omitted, in which case the
                // parameter is positional only and cannot be referred to. This
                // is what a selector parameter is: `fn __as__(this *const
                // Fraction, *double) double` names the target type and never
                // uses it, so there is nothing to name.
                //
                // The name is present when the token is an identifier that is
                // not itself the start of a type, so `int x` and `x int` are
                // told apart by looking at what follows.
                let name_token = if stream.at(TokenKind::Keyword(Keyword::This)) {
                    // `this` is a keyword, but it is also the required name of
                    // a member function's receiver, so it is accepted here and
                    // nowhere else. A parameter named `this` is what makes a
                    // function a method.
                    Some(stream.advance())
                } else if stream.at(TokenKind::Ident) && !self.ident_starts_a_type(stream) {
                    Some(self.parse_ident(stream, "a parameter name")?)
                } else {
                    None
                };
                // `values ...Ts` marks a pack: this parameter takes any
                // number of arguments instead of one.
                let is_pack = stream.eat(TokenKind::Punct(Punctuation::DotDotDot)).is_some();
                // A parameter list is where `[]T` means `*T`.
                let ty = self.parse_parameter_type(stream, ast)?;
                // A default follows `=`. Defaults must come last, as in C++,
                // which is checked below rather than here so that the
                // diagnostic can say why.
                let default = if stream.eat(TokenKind::Punct(Punctuation::Eq)).is_some() {
                    Some(self.parse_expr_no_comma(stream, ast)?)
                } else {
                    None
                };
                let end_span = default.map_or_else(|| ty.span(), |expr| ast.expr(expr).span);
                let name_span = name_token
                    .as_ref()
                    .map_or_else(|| ty.span(), |token| token.span);
                params.push(Param {
                    name: name_token
                        .as_ref()
                        .map_or_else(String::new, |token| self.token_text(stream, token)),
                    name_span,
                    ty,
                    default,
                    is_pack,
                    span: name_span.to(end_span),
                });
                if stream.eat(TokenKind::Punct(Punctuation::Comma)).is_none() {
                    break;
                }
            }
        }
        self.expect_punct(stream, Punctuation::CloseParen, "`)`")?;

        // A default may not be followed by a parameter without one, or a call
        // could not say which parameter it meant.
        if let Some(at) = params
            .iter()
            .position(|param| param.default.is_some())
            .filter(|&at| params[at..].iter().any(|param| param.default.is_none()))
        {
            // Reported at the parameter that broke the rule, which is the one
            // after the first default rather than the first default itself.
            let offender = params[at..]
                .iter()
                .find(|param| param.default.is_none())
                .expect("the filter checked one exists");
            let span = offender.span;
            self.push_diagnostic(
                Diagnostic::error("a parameter without a default cannot follow one with a default")
                    .with_code(DiagnosticCode::parser(4))
                    .with_span(span)
                    .with_note("reorder the parameters so the defaulted ones come last"),
            );
        }

        // A pack's element count is the call's, so a written default would have
        // nothing to default: the number of elements is exactly the number of
        // arguments. Rejected rather than ignored, because the default would
        // otherwise never run and the declaration would read as if it might.
        if let Some(pack) = params.iter().find(|param| param.is_pack)
            && pack.default.is_some()
        {
            self.push_diagnostic(
                Diagnostic::error("a parameter pack cannot have a default argument")
                    .with_code(DiagnosticCode::parser(6))
                    .with_span(pack.span)
                    .with_note(
                        "how many elements a pack has is the call's decision, so there is \
                         nothing for a default to stand in for",
                    ),
            );
        }

        // A pack takes every remaining argument, so a parameter after one
        // would have no argument left to receive: the call could not say how
        // many elements the pack has. Reported at the parameter that broke the
        // rule rather than at the pack.
        if let Some(at) = params.iter().position(|param| param.is_pack)
            && at + 1 != params.len()
        {
            self.push_diagnostic(
                Diagnostic::error("a parameter pack must be the last parameter")
                    .with_code(DiagnosticCode::parser(5))
                    .with_span(params[at + 1].span)
                    .with_note(
                        "the pack takes every remaining argument, so a parameter after it \
                         would never receive one",
                    ),
            );
        }

        Some((params, variadic))
    }

    /// Parses a type annotation.
    ///
    /// Types are written **postfix**: the name comes first, the type after it.
    /// The type itself is one of the prefix forms the grammar in
    /// `Parser::parse_type_impl` spells out: `const`/`*`/`&` applied to a
    /// type, `[N]T` or `[]T`, `fn(..) R`, or a path such as `int` or
    /// `std.Exception`.
    pub fn parse_type(&mut self, stream: &mut TokenStream, ast: &mut Ast) -> Option<TypeSyntax> {
        self.parse_type_impl(stream, ast, false)
    }

    /// Parses a type in a parameter list, where `[]T` means `*T`.
    pub(super) fn parse_parameter_type(
        &mut self,
        stream: &mut TokenStream,
        ast: &mut Ast,
    ) -> Option<TypeSyntax> {
        self.parse_type_impl(stream, ast, true)
    }

    /// Parses a type.
    ///
    /// # Grammar
    ///
    /// Types are written with **prefix** operators, unlike declarations, whose
    /// type follows the name:
    ///
    /// ```text
    /// T        := 'const' T | '*' T | '*' 'const' T | 'const' '*' T
    ///           | '&' T | '&' 'const' T | '[' N ']' T | '[' ']' T | '?' T | path
    /// ```
    ///
    /// The two `const` forms are distinct and cannot be merged:
    ///
    /// * `*const T` — a pointer to a constant `T`;
    /// * `const *T` — a constant pointer to a mutable `T`.
    ///
    /// So `const` binds to whatever follows it. `const *const T` combines both.
    /// `in_parameter` records whether `[]T` should be read as a pointer, which
    /// is only meaningful in a parameter list.
    pub(super) fn parse_type_impl(
        &mut self,
        stream: &mut TokenStream,
        ast: &mut Ast,
        in_parameter: bool,
    ) -> Option<TypeSyntax> {
        // `?T` -- a value that may be absent. The question mark is the sugar;
        // the type itself is `Optional<T>` from `std`, so a program that writes
        // `?int` says the same thing as `Optional<int>` with fewer words.
        if stream.at(TokenKind::Punct(Punctuation::Question)) {
            let question = stream.advance();
            let inner = self.parse_type_impl(stream, ast, in_parameter)?;
            let span = question.span.to(inner.span());
            return Some(TypeSyntax::Named(NamedType {
                path: "Optional".to_owned(),
                arguments: vec![TemplateArgumentSyntax::Type(inner)],
                is_expansion: false,
                span,
            }));
        }

        // `fn(int) int` -- a function type, which is a type like any other.
        // `*fn(int) int` is a pointer to one, built by the pointer cases above
        // from this base.
        if stream.at(TokenKind::Keyword(Keyword::Fn)) {
            let keyword = stream.advance();
            // The ABI, as in `fn "C" (int) int`, is the string a declaration
            // writes after `extern`.
            let abi = if stream.at(TokenKind::String) {
                let token = stream.advance();
                let text = self.token_text(stream, &token);
                Some(
                    text.strip_prefix('"')
                        .and_then(|rest| rest.strip_suffix('"'))
                        .unwrap_or(&text)
                        .to_owned(),
                )
            } else {
                None
            };
            self.expect_punct(stream, Punctuation::OpenParen, "`(`")?;
            let mut parameters = Vec::new();
            let mut variadic = false;
            while !stream.at(TokenKind::Punct(Punctuation::CloseParen)) {
                if stream.at(TokenKind::Punct(Punctuation::DotDotDot)) {
                    stream.advance();
                    variadic = true;
                    break;
                }
                // In a parameter list `[]T` is `*T`, exactly as it is in a
                // declaration's.
                parameters.push(self.parse_type_impl(stream, ast, true)?);
                if stream.at(TokenKind::Punct(Punctuation::Comma)) {
                    stream.advance();
                    continue;
                }
                break;
            }
            self.expect_punct(stream, Punctuation::CloseParen, "`)`")?;
            let returns = self.parse_type_impl(stream, ast, false)?;
            // `fn(int) int except` -- the type of a function that may raise.
            // It is part of the type because it is part of the calling
            // convention, so a pointer to one is not a pointer to a plain
            // function. The marker binds to the type it follows, so in
            // `*fn(int) int except` it is the function that may raise.
            let throws = if stream.at(TokenKind::Keyword(Keyword::Except)) {
                let except = stream.advance();
                // `except(cond)` is a claim about *arguments*, and a type has
                // none to claim about.
                if stream.at(TokenKind::Punct(Punctuation::OpenParen)) {
                    self.push_diagnostic(
                        Diagnostic::error(
                            "a function type carries `except` without a condition",
                        )
                        .with_code(DiagnosticCode::parser(2))
                        .with_span(except.span)
                        .with_note(
                            "`except(cond)` says when a particular function may raise; a type \
                             can only say that a call of it may",
                        ),
                    );
                }
                true
            } else {
                false
            };
            let span = keyword.span.to(returns.span());
            // `fn(int) int` is the function's address, which is what a value of
            // that type holds: there is nothing to store but where it lives.
            // Writing `*fn(int) int` says the same thing, so the bare form is
            // read as the pointer and the two spellings name one type.
            let function = TypeSyntax::Function(fallc_ast::FunctionTypeSyntax {
                abi,
                parameters,
                variadic,
                throws,
                returns: Box::new(returns),
                span,
            });
            return Some(TypeSyntax::Pointer(Box::new(function)));
        }

        // `auto`, alone or under a modifier: `&auto`, `*auto`, `const auto`.
        // What it stands for is deduced where the type is used, so the
        // grammar only has to accept it as a base.
        if stream.at(TokenKind::Keyword(Keyword::Auto)) {
            return Some(TypeSyntax::Auto(stream.advance().span));
        }

        // `const`
        if stream.at(TokenKind::Keyword(Keyword::Const)) {
            stream.advance();
            // `const *T` is a constant pointer rather than a constant type.
            if stream.at(TokenKind::Punct(Punctuation::Star)) {
                stream.advance();
                let inner = self.parse_type_impl(stream, ast, in_parameter)?;
                return Some(TypeSyntax::ConstPointer(Box::new(inner)));
            }
            let inner = self.parse_type_impl(stream, ast, in_parameter)?;
            return Some(TypeSyntax::Const(Box::new(inner)));
        }

        // `**T` -- a pointer to a pointer.
        //
        // The lexer reads `**` as one token, because that is how it is written
        // and how exponentiation is spelled. In a type position it is two
        // pointers' worth of meaning, exactly as `&&T` is one rvalue reference,
        // so it is split here rather than left to fail.
        if stream.at(TokenKind::Punct(Punctuation::StarStar)) {
            stream.advance();
            if stream.at(TokenKind::Keyword(Keyword::Const)) {
                stream.advance();
                let inner = self.parse_type_impl(stream, ast, in_parameter)?;
                return Some(TypeSyntax::Pointer(Box::new(TypeSyntax::PointerToConst(
                    Box::new(inner),
                ))));
            }
            let inner = self.parse_type_impl(stream, ast, in_parameter)?;
            return Some(TypeSyntax::Pointer(Box::new(TypeSyntax::Pointer(Box::new(
                inner,
            )))));
        }

        // `*T` and `*const T`
        if stream.at(TokenKind::Punct(Punctuation::Star)) {
            stream.advance();
            if stream.at(TokenKind::Keyword(Keyword::Const)) {
                stream.advance();
                let inner = self.parse_type_impl(stream, ast, in_parameter)?;
                return Some(TypeSyntax::PointerToConst(Box::new(inner)));
            }
            let inner = self.parse_type_impl(stream, ast, in_parameter)?;
            // `fn(..) ..` already denotes the function's address, so `*fn(..)
            // ..` would say it twice; the two spellings name one type, and the
            // bare one is what a value of it holds.
            if matches!(
                inner,
                TypeSyntax::Pointer(ref inner) if is_function_type(inner)
            ) {
                return Some(inner);
            }
            return Some(TypeSyntax::Pointer(Box::new(inner)));
        }

        // `&T` and `&const T`
        if stream.at(TokenKind::Punct(Punctuation::Amp)) {
            stream.advance();
            if stream.at(TokenKind::Keyword(Keyword::Const)) {
                stream.advance();
                let inner = self.parse_type_impl(stream, ast, in_parameter)?;
                return Some(TypeSyntax::ConstReference(Box::new(inner)));
            }
            let inner = self.parse_type_impl(stream, ast, in_parameter)?;
            return Some(TypeSyntax::Reference(Box::new(inner)));
        }

        // `&&T` and `&&const T` -- the rvalue references.
        //
        // `&&` is one token, because that is how the lexer reads it and how it
        // is written. In a type position it is two references' worth of
        // meaning, not a logical operator: which one it is depends only on
        // where it stands, and the two never compete, because there is no
        // expression that a type could be.
        if stream.at(TokenKind::Punct(Punctuation::AmpAmp)) {
            stream.advance();
            if stream.at(TokenKind::Keyword(Keyword::Const)) {
                stream.advance();
                let inner = self.parse_type_impl(stream, ast, in_parameter)?;
                return Some(TypeSyntax::ConstRvalueReference(Box::new(inner)));
            }
            let inner = self.parse_type_impl(stream, ast, in_parameter)?;
            return Some(TypeSyntax::RvalueReference(Box::new(inner)));
        }

        // `volatile T`, which may be combined with `const` in either order.
        if stream.at(TokenKind::Keyword(Keyword::Volatile)) {
            stream.advance();
            let inner = self.parse_type_impl(stream, ast, in_parameter)?;
            return Some(TypeSyntax::Volatile(Box::new(inner)));
        }

        // `[N]T` and `[]T`
        if stream.at(TokenKind::Punct(Punctuation::OpenBracket)) {
            stream.advance();
            if stream.at(TokenKind::Punct(Punctuation::CloseBracket)) {
                stream.advance();
                let inner = self.parse_type_impl(stream, ast, in_parameter)?;
                return Some(TypeSyntax::UnsizedArray(Box::new(inner)));
            }
            // The length is a constant expression. A comma here would be a
            // separator, not an operator.
            let len = self.parse_expr_no_comma(stream, ast)?;
            let len_span = ast.expr(len).span;
            self.expect_punct(stream, Punctuation::CloseBracket, "`]`")?;
            let element = self.parse_type_impl(stream, ast, in_parameter)?;
            let _ = in_parameter;
            return Some(TypeSyntax::Array(ArrayType {
                len,
                len_span,
                element: Box::new(element),
            }));
        }

        // `type(expr)` -- the type of an expression.
        //
        // The keyword that *declares* a type also *asks* for one. `type(a)` is
        // what `a` is, and `type((a))` is what the expression `a` is: a
        // reference, because the expression names storage. The extra
        // parentheses are the whole difference, so they are noticed here rather
        // than parsed away.
        if stream.at(TokenKind::Keyword(Keyword::Type))
            && stream.peek_kind(1) == TokenKind::Punct(Punctuation::OpenParen)
        {
            let keyword = stream.advance();
            stream.advance(); // `(`
            // `type(auto)` -- `decltype(auto)`: the initialiser's own type,
            // references kept. `auto` alone is the *value* type, so the two
            // differ exactly in whether a place stays a reference.
            if stream.at(TokenKind::Keyword(Keyword::Auto)) {
                stream.advance();
                let close = self.expect_punct(stream, Punctuation::CloseParen, "`)`")?;
                return Some(TypeSyntax::DecltypeAuto(keyword.span.to(close.span)));
            }
            let parenthesized = stream.at(TokenKind::Punct(Punctuation::OpenParen));
            let operand = self.parse_expr_no_comma(stream, ast)?;
            let close = self.expect_punct(stream, Punctuation::CloseParen, "`)`")?;
            return Some(TypeSyntax::TypeOf(TypeOfType {
                operand,
                parenthesized,
                span: keyword.span.to(close.span),
            }));
        }

        // A named type, possibly a dotted path such as `std.Exception`.
        let first = self.parse_ident(stream, "a type")?;
        let mut span = first.span;
        let mut text = self.token_text(stream, &first);
        while stream.at(TokenKind::Punct(Punctuation::Dot)) {
            let dot = stream.advance();
            let segment = self.parse_ident(stream, "a type path segment")?;
            text.push('.');
            text.push_str(&self.token_text(stream, &segment));
            span = span.to(dot.span).to(segment.span);
        }
        // `Array<int>` -- a member of a family of types.
        //
        // In a *type* position a `<` can only open template arguments: there is
        // no expression here for it to be a comparison in. That is what tells
        // this apart from `f<int>(..)`, where the lookahead has to guess.
        let mut arguments = Vec::new();
        if stream.at(TokenKind::Punct(Punctuation::Lt)) {
            arguments = self.parse_template_arguments(stream, ast)?;
            if let Some(last) = arguments.last() {
                let _ = last;
            }
            span = span.to(stream.current_span());
        }
        // `Args...` expands a parameter pack. The `...` belongs to the name and
        // is written where the elements are wanted -- `Box<Args...>` is one
        // `Box` per element -- so it is recorded rather than consumed here.
        let is_expansion = if stream.at(TokenKind::Punct(Punctuation::DotDotDot)) {
            let dots = stream.advance();
            span = span.to(dots.span);
            true
        } else {
            false
        };
        let mut ty = TypeSyntax::Named(NamedType {
            path: text,
            arguments,
            is_expansion,
            span,
        });
        // `Array<int>.Iterator` -- a type declared inside another one. The dot
        // is read after the argument list, because that is where the parser
        // knows the outer type is complete.
        while stream.at(TokenKind::Punct(Punctuation::Dot)) {
            let dot = stream.advance();
            let segment = self.parse_ident(stream, "a nested type name")?;
            let member_span = span.to(dot.span).to(segment.span);
            ty = TypeSyntax::Member(fallc_ast::TypeMember {
                outer: Box::new(ty),
                name: self.token_text(stream, &segment),
                span: member_span,
            });
            span = member_span;
        }
        Some(ty)
    }

    /// Recognises `(init op ... op pack)` after the two fixed forms fail.
    ///
    /// The starting value is read as a unary expression first, and the whole
    /// shape is then checked; anything else is put back for the ordinary
    /// parenthesised expression. This is the one fold form whose beginning is
    /// not distinctive, so it is the one that needs to be tried and undone.
    pub(super) fn parse_leading_fold(
        &mut self,
        stream: &mut TokenStream,
        ast: &mut Ast,
    ) -> Option<Option<FoldExpr>> {
        let checkpoint = stream.checkpoint();
        let outer = stream.position();
        let mut attempt = |parser: &mut Parser, stream: &mut TokenStream| -> Option<FoldExpr> {
            stream.advance(); // `(`
            let init = parser.parse_postfix_inner(stream, ast, true)?;
            let TokenKind::Punct(operator) = stream.current_kind() else {
                return None;
            };
            let op = binary_op_of_punct(operator)?;
            stream.advance(); // the operator
            if stream.current_kind() != TokenKind::Punct(Punctuation::DotDotDot) {
                return None;
            }
            stream.advance(); // `...`
            if stream.current_kind() != TokenKind::Punct(operator) {
                return None;
            }
            stream.advance(); // the operator again
            if stream.current_kind() != TokenKind::Ident {
                return None;
            }
            let name = stream.advance();
            if stream.current_kind() != TokenKind::Punct(Punctuation::CloseParen) {
                return None;
            }
            Some(FoldExpr {
                pack: PackRef {
                    pack: parser.token_text(stream, &name),
                    pack_span: name.span,
                },
                op,
                kind: FoldKind::BinaryLeft,
                init: Some(init),
            })
        };
        let diagnostics_before = self.diagnostics.len();
        match attempt(self, stream) {
            Some(fold) => Some(Some(fold)),
            None => {
                // Nothing matched: the tokens are the ordinary expression's
                // again, and anything the attempt reported goes with them.
                stream.rewind(checkpoint);
                let _ = outer;
                self.diagnostics.truncate(diagnostics_before);
                Some(None)
            }
        }
    }

    /// Recognises `name op ...` as the inside of a fold, without consuming it
    /// unless it matches.
    ///
    /// Looked at rather than parsed: `values` is an ordinary name and `+` an
    /// ordinary operator, so the only thing that makes this a fold is the
    /// `...` where an operand would be.
    pub(super) fn parse_fold(&mut self, stream: &mut TokenStream, ast: &mut Ast) -> Option<Option<FoldExpr>> {
        // `(... op pack)`: the `...` comes first, which no expression can.
        if stream.peek_kind(1) == TokenKind::Punct(Punctuation::DotDotDot) {
            let TokenKind::Punct(operator) = stream.peek_kind(2) else {
                return Some(None);
            };
            let Some(op) = binary_op_of_punct(operator) else {
                return Some(None);
            };
            if stream.peek_kind(3) != TokenKind::Ident {
                return Some(None);
            }
            stream.advance(); // `(`
            stream.advance(); // `...`
            stream.advance(); // the operator
            let name = stream.advance();
            if stream.peek_kind(0) != TokenKind::Punct(Punctuation::CloseParen) {
                return Some(None);
            }
            return Some(Some(FoldExpr {
                pack: PackRef {
                    pack: self.token_text(stream, &name),
                    pack_span: name.span,
                },
                op,
                kind: FoldKind::UnaryLeft,
                init: None,
            }));
        }
        // The cursor is on the `(`, so the name is one ahead.
        if stream.peek_kind(1) != TokenKind::Ident {
            return Some(None);
        }
        let TokenKind::Punct(operator) = stream.peek_kind(2) else {
            return Some(None);
        };
        let Some(op) = binary_op_of_punct(operator) else {
            return Some(None);
        };
        if stream.peek_kind(3) != TokenKind::Punct(Punctuation::DotDotDot) {
            return Some(None);
        }
        // `(pack op ...)` -- and, when another operator follows, the binary
        // form `(pack op ... op init)`: the elements are combined from the
        // right, with `init` under the last one.
        stream.advance(); // `(`
        let name = stream.advance();
        stream.advance(); // the operator
        stream.advance(); // `...`
        let mut kind = FoldKind::UnaryRight;
        let mut init = None;
        if let TokenKind::Punct(next) = stream.peek_kind(0)
            && next != Punctuation::CloseParen
            && binary_op_of_punct(next) == Some(op)
        {
            stream.advance(); // the operator again
            init = Some(self.parse_expr_no_comma(stream, ast)?);
            kind = FoldKind::BinaryRight;
        }
        Some(Some(FoldExpr {
            pack: PackRef {
                pack: self.token_text(stream, &name),
                pack_span: name.span,
            },
            op,
            kind,
            init,
        }))
    }

    /// Parses an identifier, reporting if the next token is not one.
    pub(super) fn parse_ident(&mut self, stream: &mut TokenStream, wanted: &str) -> Option<Token> {
        if stream.at(TokenKind::Ident) {
            return Some(stream.advance());
        }
        self.report_unexpected(stream, wanted);
        None
    }

    /// Consumes one `>` or reports an error.
    ///
    /// Uses [`TokenStream::split_gt`], so a nested `>>` needs no space.
    pub(super) fn expect_gt(&mut self, stream: &mut TokenStream) -> Option<Token> {
        match stream.split_gt() {
            Some(token) => Some(token),
            None => {
                self.report_unexpected(stream, "`>`");
                None
            }
        }
    }

    /// Consumes a specific punctuation mark or reports an error.
    pub(super) fn expect_punct(
        &mut self,
        stream: &mut TokenStream,
        punct: Punctuation,
        wanted: &str,
    ) -> Option<Token> {
        match stream.eat(TokenKind::Punct(punct)) {
            Some(token) => Some(token),
            None => {
                self.report_unexpected(stream, wanted);
                None
            }
        }
    }

    /// Returns the source text of `token`.
    ///
    /// Identifier and type spelling comes from the source slice, so this needs
    /// the text the token spans. The parser keeps a copy of the file text for
    /// exactly this purpose, set by [`Parser::parse_file`].
    pub(super) fn token_text(&self, stream: &TokenStream, token: &Token) -> String {
        let _ = stream;
        // A token that carries its own spelling is preferred: a macro expands
        // to tokens marked with the span of the `$NAME` that produced them, so
        // the source text at that span is the invocation and not what it stood
        // for. Scanned tokens spell the same either way.
        if let Some(text) = token.spelling() {
            return text.to_owned();
        }
        // Without source text the spelling is unavailable. That only happens
        // when a caller drives the parser without giving it the file contents;
        // the token kinds are still correct.
        self.source
            .as_ref()
            .and_then(|text| text.get(token.span.lo as usize..token.span.hi as usize))
            .map(ToOwned::to_owned)
            .unwrap_or_default()
    }

    // ----------------------------------------------------------- statements

}

/// Whether a written type is the bare `fn(..) ..`, which already means an
/// address.
fn is_function_type(ty: &TypeSyntax) -> bool {
    matches!(ty, TypeSyntax::Function(_))
}
