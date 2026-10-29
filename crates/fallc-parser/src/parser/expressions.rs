//! Expressions, from the lowest precedence up.
//!
//! `parse_expr_bp` is a Pratt loop: `parse_prefix` reads what an expression
//! starts with, `parse_postfix` reads what follows it, and the binding powers
//! live in `precedence.rs`.

use super::*;
use crate::precedence::RANGE_PRECEDENCE;

impl Parser {
    /// Parses an expression using the binding-power loop.
    ///
    /// The shape below is the finished algorithm; only the two primitives it
    /// calls are waiting on the grammar.
    pub fn parse_expr(&mut self, stream: &mut TokenStream, ast: &mut Ast) -> Option<ExprId> {
        let lhs = self.parse_assignment(stream, ast)?;

        // The comma operator: evaluate the left, discard it, yield the right.
        // It has the lowest precedence of all, below assignment.
        if stream.at(TokenKind::Punct(Punctuation::Comma)) {
            stream.advance();
            let rhs = self.parse_expr(stream, ast)?;
            let span = ast.expr(lhs).span.to(ast.expr(rhs).span);
            return Some(
                ast.builder()
                    .alloc_expr(ExprKind::Comma(CommaExpr { lhs, rhs }), span),
            );
        }

        Some(lhs)
    }

    /// Parses an assignment, or whatever is tighter if there is no `=`.
    ///
    /// Assignment is handled outside the Pratt loop because its left side must
    /// be an lvalue, and its precedence is below every binary operator. It is
    /// right-associative, so `a = b = c` parses as `a = (b = c)`.
    pub(super) fn parse_assignment(&mut self, stream: &mut TokenStream, ast: &mut Ast) -> Option<ExprId> {
        let lhs = self.parse_expr_bp(stream, ast, BindingPower::min())?;

        if let Some(punct) = stream.current_kind().punct()
            && punct.is_assignment()
        {
            stream.advance();
            let value = self.parse_assignment(stream, ast)?;
            let span = ast.expr(lhs).span.to(ast.expr(value).span);
            return Some(ast.builder().alloc_expr(
                ExprKind::Assign(AssignExpr {
                    target: lhs,
                    op: punct.compound_base().and_then(binary_op_of_punct),
                    value,
                }),
                span,
            ));
        }

        Some(lhs)
    }

    /// Parses an expression without consuming a top-level comma.
    ///
    /// The comma operator has the lowest precedence of all, so suppressing it
    /// means stopping one level below `parse_expr`. Contexts where a comma
    /// separates things — arguments, case labels, array lengths — must use
    /// this, or the separator is swallowed as an operator.
    pub fn parse_expr_no_comma(
        &mut self,
        stream: &mut TokenStream,
        ast: &mut Ast,
    ) -> Option<ExprId> {
        self.parse_assignment(stream, ast)
    }

    /// Parses an expression, consuming only operators that bind tighter than
    /// `min_bp`.
    pub fn parse_expr_bp(
        &mut self,
        stream: &mut TokenStream,
        ast: &mut Ast,
        min_bp: BindingPower,
    ) -> Option<ExprId> {
        let _ = self.enter(stream);
        let lhs = match self.parse_postfix(stream, ast) {
            Some(id) => id,
            None => {
                self.leave();
                return None;
            }
        };

        // The Pratt loop. Each iteration consumes one infix operator whose
        // left binding power is high enough for this call's minimum.
        //
        // Short-circuiting is a *lowering* concern, not a parsing one: `&&` and
        // `||` are ordinary left-associative binary operators here, and the
        // code generator turns them into control flow.
        let mut lhs = lhs;
        loop {
            let token = stream.peek(0).clone();

            // `cond ? then : otherwise`, and `call()??` for the postfix that
            // passes an exception on. One `?` is the conditional, two is the
            // postfix: the second question mark is what tells them apart, and
            // nothing has to be looked for past it.
            //
            // The middle operand is a *full* expression -- everything up to the
            // `:` belongs to it -- so it is parsed at the lowest precedence
            // rather than at this operator's. The right side recurses at the
            // same level, which is what makes the operator right-associative.
            if token.kind == TokenKind::Punct(Punctuation::Question)
                && stream.peek_kind(1) != TokenKind::Punct(Punctuation::Question)
            {
                if crate::precedence::CONDITIONAL_PRECEDENCE < min_bp.right {
                    break;
                }
                stream.advance();
                let condition = lhs;
                // The middle operand is a full expression: everything up to the
                // `:` belongs to it. This loop does not consume commas, so
                // parsing at the lowest binding power is what stops it at the
                // `:`, which is not an operator at all.
                let then_value = match self.parse_assignment(stream, ast) {
                    Some(id) => id,
                    None => break,
                };
                if self
                    .expect_punct(stream, Punctuation::Colon, "`:`")
                    .is_none()
                {
                    break;
                }
                // The right side recurses at the same level, which is what
                // makes `a ? b : c ? d : e` group to the right.
                let else_value = match self.parse_expr_bp(
                    stream,
                    ast,
                    BindingPower::new(crate::precedence::CONDITIONAL_PRECEDENCE, 0),
                ) {
                    Some(id) => id,
                    None => break,
                };
                let span = ast.expr(condition).span.to(ast.expr(else_value).span);
                lhs = ast.builder().alloc_expr(
                    ExprKind::Conditional(ConditionalExpr {
                        condition,
                        then_value,
                        else_value,
                    }),
                    span,
                );
                continue;
            }

            // A range: `low .. high` or `low ..= high`. It binds looser than
            // every binary operator, so `1 .. n + 1` is `1 .. (n + 1)`, and a
            // range cannot be chained: `1 .. 2 .. 3` has no meaning.
            if matches!(
                token.kind,
                TokenKind::Punct(Punctuation::DotDot | Punctuation::DotDotEq)
            ) {
                if RANGE_PRECEDENCE < min_bp.right {
                    break;
                }
                let inclusive = token.kind == TokenKind::Punct(Punctuation::DotDotEq);
                stream.advance();
                let low = lhs;
                let high = match self.parse_expr_bp(stream, ast, BindingPower::new(RANGE_PRECEDENCE, 1)) {
                    Some(id) => id,
                    None => break,
                };
                if matches!(
                    stream.peek(0).kind,
                    TokenKind::Punct(Punctuation::DotDot | Punctuation::DotDotEq)
                ) {
                    break;
                }
                let span = ast.expr(low).span.to(ast.expr(high).span);
                lhs = self.build_range(ast, low, high, inclusive, span);
                continue;
            }

            // Inside a template parameter header a `>` closes the header, and
            // is not a comparison: `template <n int = 4>` would otherwise read
            // `4 >` as a greater-than with nothing on the right.
            if self.in_template_header > 0
                && token.kind == TokenKind::Punct(Punctuation::Gt)
            {
                break;
            }
            let Some(info) = crate::precedence::infix(token.kind) else {
                break;
            };
            if info.power.left < min_bp.right {
                break;
            }
            let Some(op) = binary_op_of(token.kind) else {
                break;
            };
            stream.advance();
            let rhs = match self.parse_expr_bp(stream, ast, info.power) {
                Some(rhs) => rhs,
                None => break,
            };
            let span = ast.expr(lhs).span.to(ast.expr(rhs).span);
            lhs = ast
                .builder()
                .alloc_expr(ExprKind::Binary(BinaryExpr { op, lhs, rhs }), span);
        }

        self.leave();
        Some(lhs)
    }

    /// Whether the `{` at the cursor begins an array literal rather than a
    /// block.
    ///
    /// The question is answered by lookahead, comparing token kinds and nesting
    /// depth only: an array literal has an element followed by `,` at depth
    /// zero of its own braces, and a block reaches its `}` first because its
    /// statements end in `;` or because what follows the first expression is
    /// not a comma.
    fn brace_begins_a_literal(&self, stream: &TokenStream) -> bool {
        // Every `{`, `(`, `[` opens a level; the literal ends when the level
        // that started here closes.
        let mut depth = 0usize;
        let mut index = 1usize; // through the opening `{`
        // An element is either immediately followed by `,` at this level, or
        // the brace closes with a single element, which is also a literal.
        loop {
            match stream.peek(index).kind {
                TokenKind::Eof => return false,
                TokenKind::Punct(
                    Punctuation::OpenBrace | Punctuation::OpenParen | Punctuation::OpenBracket
                    | Punctuation::DotBrace,
                ) => depth += 1,
                TokenKind::Punct(
                    Punctuation::CloseBrace | Punctuation::CloseParen | Punctuation::CloseBracket,
                ) => {
                    if depth == 0 {
                        // The brace closed. `{}` and `{ element }` are literals;
                        // a block that got here had a `;`, which returned false.
                        return true;
                    }
                    depth -= 1;
                }
                TokenKind::Punct(Punctuation::Comma) if depth == 0 => return true,
                TokenKind::Punct(Punctuation::Semi) if depth == 0 => return false,
                _ => {}
            }
            index += 1;
        }
    }

    /// Parses an array literal, `{a, b, c}`.
    ///
    /// The elements are full expressions, separated by commas; the length is
    /// however many there are. Nothing here decides the element type -- that
    /// comes from the position the literal is written in, exactly as it does
    /// for `.{ .. }`.
    fn parse_array_literal(
        &mut self,
        stream: &mut TokenStream,
        ast: &mut Ast,
    ) -> Option<ExprId> {
        let open = stream.advance().span;
        let mut elements = Vec::new();
        while !stream.at(TokenKind::Punct(Punctuation::CloseBrace)) {
            if stream.at(TokenKind::Eof) {
                self.push_diagnostic(
                    Diagnostic::error("an array literal is not closed: `}` is missing")
                        .with_code(DiagnosticCode::parser(2))
                        .with_span(open),
                );
                return None;
            }
            let element = self.parse_expr_no_comma(stream, ast)?;
            elements.push(element);
            if stream.at(TokenKind::Punct(Punctuation::Comma)) {
                stream.advance();
                continue;
            }
            break;
        }
        let close = match self.expect_punct(stream, Punctuation::CloseBrace, "`}`") {
            Some(close) => close,
            None => return None,
        };
        let span = open.to(close.span);
        Some(
            ast.builder()
                .alloc_expr(ExprKind::ArrayLiteral(fallc_ast::ArrayLiteral { elements }), span),
        )
    }

    /// Builds the value of `low .. high` or `low ..= high`.
    ///
    /// A range is the standard library's `Range`, so it is lowered here to the
    /// construction `std.range.Range(low, high, step)`: `1 .. 10` walks 1
    /// through 9 and `1 ..= 10` walks 1 through 10, which is the same range
    /// with the stop one past the end in the second form. Writing the
    /// construction rather than a node of its own means every later stage --
    /// resolution, type checking and code generation -- handles a range as the
    /// constructor call it is.
    fn build_range(
        &mut self,
        ast: &mut Ast,
        low: ExprId,
        high: ExprId,
        inclusive: bool,
        span: fallc_span::Span,
    ) -> ExprId {
        let int_one = |ast: &mut Ast, span: fallc_span::Span| {
            ast.builder().alloc_expr(
                ExprKind::IntLiteral(fallc_ast::IntLiteral {
                    digits: "1".to_owned(),
                    suffix: Some("i64".to_owned()),
                }),
                span,
            )
        };
        let stop = if inclusive {
            // `..= high` stops one past the high end. The high end is widened to
            // the element type first, so `1 ..= 5` adds two `i64`s whatever
            // type the written literal would have had on its own.
            let widened = ast.builder().alloc_expr(
                ExprKind::Cast(fallc_ast::CastExpr {
                    operand: high,
                    ty: fallc_ast::TypeSyntax::Named(fallc_ast::NamedType {
                        path: "i64".to_owned(),
                        arguments: Vec::new(),
                        is_expansion: false,
                        span,
                    }),
                }),
                span,
            );
            let one = int_one(ast, span);
            ast.builder().alloc_expr(
                ExprKind::Binary(fallc_ast::BinaryExpr {
                    op: fallc_ast::BinaryOp::Add,
                    lhs: widened,
                    rhs: one,
                }),
                span,
            )
        } else {
            high
        };
        let step = int_one(ast, span);
        let std_module = ast
            .builder()
            .alloc_expr(ExprKind::Name("std".to_owned()), span);
        let range_module = ast.builder().alloc_expr(
            ExprKind::Field(fallc_ast::FieldExpr {
                object: std_module,
                field: "range".to_owned(),
                through_pointer: false,
            }),
            span,
        );
        let callee = ast.builder().alloc_expr(
            ExprKind::Field(fallc_ast::FieldExpr {
                object: range_module,
                field: "Range".to_owned(),
                through_pointer: false,
            }),
            span,
        );
        let call = fallc_ast::CallExpr {
            callee,
            args: vec![low, stop, step],
            named: Vec::new(),
            template_args: Vec::new(),
            explicit_template_arguments: false,
        };
        ast.builder().alloc_expr(ExprKind::Call(call), span)
    }

    /// Parses a prefix expression: a literal, a name, a parenthesised
    /// expression, an aggregate literal, `type(expr)`, `sizeof`/`alignof`, the
    /// `except(expr)` query, a prefix `++`/`--`, or a prefix operator.
    ///
    /// A prefix operator takes a postfix expression as its operand, so `&a[i]`
    /// addresses the element while `&x as *byte` is `(&x) as *byte`.
    /// `lambda(args) -> expr` or `lambda(args) { .. }`.
    ///
    /// A parameter's type may be left out: `lambda(x) -> x` takes each call's
    /// argument type, so the same lambda may be called with an `int` and with a
    /// `double`. A name is always written, because the body's use of the
    /// parameter is what makes it useful.
    fn parse_lambda(&mut self, stream: &mut TokenStream, ast: &mut Ast) -> Option<ExprId> {
        let start = stream.advance().span;
        self.expect_punct(stream, Punctuation::OpenParen, "`(`")?;
        let mut parameters = Vec::new();
        if !stream.at(TokenKind::Punct(Punctuation::CloseParen)) {
            loop {
                let name_token = self.parse_ident(stream, "a parameter name")?;
                let name = self.token_text(stream, &name_token);
                // A type is present unless the parameter ends here. The rules
                // are the function parameter list's: `x int` names the type
                // after the name, and a `,` or `)` right after the name means
                // the type was omitted.
                let ty = if stream.at(TokenKind::Punct(Punctuation::Comma))
                    || stream.at(TokenKind::Punct(Punctuation::CloseParen))
                {
                    None
                } else {
                    Some(self.parse_parameter_type(stream, ast)?)
                };
                let end_span = ty
                    .as_ref()
                    .map_or(name_token.span, fallc_ast::TypeSyntax::span);
                parameters.push(fallc_ast::LambdaParam {
                    name,
                    name_span: name_token.span,
                    ty,
                    span: name_token.span.to(end_span),
                });
                if stream.eat(TokenKind::Punct(Punctuation::Comma)).is_none() {
                    break;
                }
            }
        }
        let close = self.expect_punct(stream, Punctuation::CloseParen, "`)`")?;
        // The body is an expression after `->`, or a block. The arrow is the
        // one that already exists for pointer member access; here it only
        // introduces the result, because a lambda has no receiver.
        let body = if stream.eat(TokenKind::Punct(Punctuation::Arrow)).is_some() {
            let value = self.parse_expr_no_comma(stream, ast)?;
            fallc_ast::LambdaBody::Expression(value)
        } else if stream.at(TokenKind::Punct(Punctuation::OpenBrace)) {
            let block = self.parse_block(stream, ast)?;
            fallc_ast::LambdaBody::Block(block)
        } else {
            self.push_diagnostic(
                Diagnostic::error("a lambda needs a body: `-> expr` or `{ .. }`")
                    .with_code(DiagnosticCode::parser(2))
                    .with_span(close.span),
            );
            return None;
        };
        let end = match &body {
            fallc_ast::LambdaBody::Expression(value) => ast.expr(*value).span,
            fallc_ast::LambdaBody::Block(block) => ast.block(*block).span,
        };
        Some(ast.builder().alloc_expr(
            ExprKind::Lambda(fallc_ast::LambdaExpr {
                parameters,
                body,
                span: start.to(end),
            }),
            start.to(end),
        ))
    }

    pub fn parse_prefix(&mut self, stream: &mut TokenStream, ast: &mut Ast) -> Option<ExprId> {
        // `lambda(args) -> expr` or `lambda(args) { .. }` is an expression, so
        // it is read wherever an expression may begin.
        if stream.at(TokenKind::Keyword(Keyword::Lambda)) {
            return self.parse_lambda(stream, ast);
        }
        // `@__builtin_defined(X)` and its kind: a compile-time question. The
        // `@` is what tells it from a name, and the argument is read as text
        // because the builtin decides what it means -- a module name, a type,
        // a platform.
        if stream.at(TokenKind::Punct(Punctuation::At)) {
            let start = stream.advance().span;
            let name_token = self.parse_ident(stream, "a builtin name")?;
            // `@meta.defined` is written as a path: the namespace and the
            // question. It is one name, so the dot is part of it.
            let mut name = self.token_text(stream, &name_token);
            if stream.at(TokenKind::Punct(Punctuation::Dot))
                && stream.peek_kind(1) == TokenKind::Ident
            {
                stream.advance();
                let part = self.parse_ident(stream, "a builtin name")?;
                name = format!("{name}.{}", self.token_text(stream, &part));
            }
            self.expect_punct(stream, Punctuation::OpenParen, "`(`")?;
            let argument = self.parse_ident(stream, "what the builtin asks about")?;
            let argument_text = self.token_text(stream, &argument);
            let close = self.expect_punct(stream, Punctuation::CloseParen, "`)`")?;
            let span = start.to(close.span);
            return Some(ast.builder().alloc_expr(
                ExprKind::Builtin(fallc_ast::BuiltinQuery {
                    name,
                    argument: argument_text,
                    span,
                }),
                span,
            ));
        }
        // `if (cond) { .. } else { .. }` standing where a value is wanted.
        // The same keyword starts the statement form; the position decides
        // which it is, and this entry point is only reached from a position
        // that wants a value.
        if stream.at(TokenKind::Keyword(Keyword::If)) {
            stream.advance();
            self.expect_punct(stream, Punctuation::OpenParen, "`(`")?;
            let condition = self.parse_expr(stream, ast)?;
            self.expect_punct(stream, Punctuation::CloseParen, "`)`")?;
            let then_block = self.parse_block(stream, ast)?;
            let else_block = if stream.at(TokenKind::Keyword(Keyword::Else)) {
                stream.advance();
                Some(self.parse_block(stream, ast)?)
            } else {
                None
            };
            let end = match else_block {
                Some(block) => ast.block(block).span,
                None => ast.block(then_block).span,
            };
            let span = ast.expr(condition).span.to(end);
            return Some(ast.builder().alloc_expr(
                ExprKind::IfValue(fallc_ast::IfValueExpr {
                    condition,
                    then_block,
                    else_block,
                }),
                span,
            ));
        }
        // A `{` where a value is wanted begins one of two things, and they are
        // told apart by what comes after it: an array literal `{1, 2, 3}`, whose
        // elements are expressions separated by commas, or a block expression
        // `{ x = 1; x }`, whose contents are statements ending in `;` or a tail
        // expression.
        //
        // The separator decides it: an element followed by `,` is an array, and
        // anything else -- a `;`, a declaration, a `}` -- is a block. That is a
        // property of the text rather than of the position, so a literal inside
        // a statement still reads as a literal.
        if stream.at(TokenKind::Punct(Punctuation::OpenBrace))
            && self.brace_begins_a_literal(stream)
        {
            return self.parse_array_literal(stream, ast);
        }
        if stream.at(TokenKind::Punct(Punctuation::OpenBrace)) {
            let block = self.parse_block(stream, ast)?;
            let span = ast.block(block).span;
            return Some(
                ast.builder()
                    .alloc_expr(ExprKind::Block(block), span),
            );
        }
        // `type(expr)` where an expression is expected: a *type*, which only a
        // position that wants one -- a trait query's argument, today -- can
        // accept. The declaration form `x type(expr);` is a type, and is parsed
        // by the type grammar instead.
        if stream.at(TokenKind::Keyword(Keyword::Type))
            && stream.peek_kind(1) == TokenKind::Punct(Punctuation::OpenParen)
        {
            let start = stream.advance().span;
            self.expect_punct(stream, Punctuation::OpenParen, "`(`")?;
            let operand = self.parse_expr_no_comma(stream, ast)?;
            let close = self.expect_punct(stream, Punctuation::CloseParen, "`)`")?;
            return Some(
                ast.builder()
                    .alloc_expr(ExprKind::TypeOf(operand), start.to(close.span)),
            );
        }
        // `sizeof(T)` and `alignof(T)`.
        //
        // The operand is a *type*, always in parentheses: `sizeof int` would be
        // ambiguous with a variable called `sizeof` in any grammar that later
        // allows one, and the parentheses cost nothing. The result is a
        // constant, which is what makes it usable as an array length.
        for (keyword, is_size) in [
            (Keyword::Sizeof, true),
            (Keyword::Alignof, false),
        ] {
            if !stream.at(TokenKind::Keyword(keyword)) {
                continue;
            }
            let start = stream.advance().span;
            // `sizeof...(values)` counts a pack's elements. The `...` belongs
            // to `sizeof` rather than to the operand: a pack is a list of
            // values that exists only while the instantiation is known, so
            // there is no layout to measure, only a length to count.
            if is_size
                && stream.eat(TokenKind::Punct(Punctuation::DotDotDot)).is_some()
            {
                self.expect_punct(stream, Punctuation::OpenParen, "`(`")?;
                let name = stream.advance();
                let close = self.expect_punct(stream, Punctuation::CloseParen, "`)`")?;
                let span = start.to(close.span);
                let count = PackRef {
                    pack: self.token_text(stream, &name),
                    pack_span: name.span,
                };
                return Some(ast.builder().alloc_expr(ExprKind::SizeofPack(count), span));
            }
            self.expect_punct(stream, Punctuation::OpenParen, "`(`")?;
            let ty = self.parse_type(stream, ast)?;
            let close = self.expect_punct(stream, Punctuation::CloseParen, "`)`")?;
            let span = start.to(close.span);
            let kind = if is_size {
                ExprKind::Sizeof(ty)
            } else {
                ExprKind::Alignof(ty)
            };
            return Some(ast.builder().alloc_expr(kind, span));
        }

        // `except(expr)` -- a compile-time `bool`: may this expression raise?
        //
        // A prefix keyword rather than a call because what it asks about is the
        // *expression*, not a value: the expression is analysed and never
        // emitted, so `std.fake` can stand in for a value of a type that does
        // not exist anywhere yet.
        if stream.at(TokenKind::Keyword(Keyword::Except)) {
            let start = stream.advance().span;
            if !stream.at(TokenKind::Punct(Punctuation::OpenParen)) {
                // `except` alone is the block of a `try`, which the statement
                // parser has already taken care of; here it is the query, and
                // the query always has its operand in parentheses.
                let span = stream.current_span();
                self.push_diagnostic(
                    Diagnostic::error("`except` here is the query, so it needs `(expr)`")
                        .with_code(DiagnosticCode::parser(2))
                        .with_span(span),
                );
                return None;
            }
            stream.advance();
            let inner = self.parse_expr_no_comma(stream, ast)?;
            let close = self.expect_punct(stream, Punctuation::CloseParen, "`)`")?;
            return Some(ast.builder().alloc_expr(
                ExprKind::ThrowQuery(inner),
                start.to(close.span),
            ));
        }

        // A prefix `++` or `--` needs an lvalue, so it takes a postfix
        // expression rather than a full one.
        if let Some(op) = incdec_op_of(stream.current_kind()) {
            let start = stream.current_span();
            stream.advance();
            let operand = self.parse_postfix(stream, ast)?;
            let span = start.to(ast.expr(operand).span);
            return Some(ast.builder().alloc_expr(
                ExprKind::IncDec(IncDecExpr {
                    op,
                    postfix: false,
                    operand,
                }),
                span,
            ));
        }

        // A prefix operator takes a *postfix* expression as its operand, so
        // that `&a[i]` addresses the element, but not the `as` that follows:
        // `&x as *byte` is `(&x) as *byte`, not `&(x as *byte)`.
        //
        // Both are wanted and neither excludes the other, so the operand is
        // parsed without casts and the cast is left for the caller -- which is
        // the postfix level this prefix parse sits inside.
        if let Some(op) = unary_op_of(stream.current_kind()) {
            let start = stream.current_span();
            stream.advance();
            let operand = self.parse_postfix_without_cast(stream, ast)?;
            let span = start.to(ast.expr(operand).span);
            return Some(
                ast.builder()
                    .alloc_expr(ExprKind::Unary(UnaryExpr { op, operand }), span),
            );
        }

        // `this` is a keyword but names a value: the receiver of the enclosing
        // member function.
        if stream.at(TokenKind::Keyword(Keyword::This)) {
            let token = stream.advance();
            return Some(
                ast.builder()
                    .alloc_expr(ExprKind::Name("this".to_owned()), token.span),
            );
        }

        // Boolean literals are keywords, so they are handled before the
        // identifier case.
        if let TokenKind::Keyword(keyword) = stream.current_kind()
            && matches!(keyword, Keyword::True | Keyword::False)
        {
            let token = stream.advance();
            let value = keyword == Keyword::True;
            return Some(
                ast.builder()
                    .alloc_expr(ExprKind::BoolLiteral(value), token.span),
            );
        }

        // `nullptr` is a keyword literal, like `true` and `false`.
        if stream.at(TokenKind::Keyword(Keyword::Nullptr)) {
            let token = stream.advance();
            return Some(
                ast.builder()
                    .alloc_expr(ExprKind::NullPointer, token.span),
            );
        }

        // `Point{ .. }` is a designated initialiser with its type written out,
        // which is what makes it usable where nothing else supplies the type.
        // `.{ .. }` written alone takes its type from context instead.
        // A `<` after the name opens template arguments -- `Array<int>{}` --
        // which is what `f<int>(..)` uses too. The lookahead is the same: a
        // list of arguments is followed by something that continues the
        // expression, and `Array<int>{}` continues it with the brace.
        let has_arguments = stream.at(TokenKind::Ident)
            && stream.peek_kind(1) == TokenKind::Punct(Punctuation::Lt)
            && self.template_arguments_follow_from(stream, 1);
        // `Array<int>(..)` is a *call* on a type name, which the ordinary call
        // path handles -- a name followed by a parenthesised list is a call
        // whether the name is a function or a type. So `<` here means a braced
        // initialiser was written, and the arguments belong to the type it
        // initialises.
        if stream.at(TokenKind::Ident)
            && stream.peek_kind(1) == TokenKind::Punct(Punctuation::OpenBrace)
        {
            let name_token = stream.advance();
            let type_name = self.token_text(stream, &name_token);
            let type_arguments = if has_arguments {
                self.parse_template_arguments(stream, ast)?
            } else {
                Vec::new()
            };
            let mut init = self.parse_braced_init(stream, ast)?;
            let span = name_token.span.to(init.span);
            let id = ast.builder().alloc_expr(
                ExprKind::TypedInit(TypedInitExpr {
                    type_name,
                    type_arguments: type_arguments.clone(),
                    type_span: name_token.span,
                    init: init.clone(),
                }),
                span,
            );
            init.expr = id;
            ast.expr_mut(id).kind = ExprKind::TypedInit(TypedInitExpr {
                type_name: self.token_text(stream, &name_token),
                type_arguments,
                type_span: name_token.span,
                init,
            });
            return Some(id);
        }

        // `{1, 2, 3}` in an expression position is an array literal: the length
        // is how many elements there are. A statement position has already
        // taken `{` as a block, so a brace reaching here is a value.
        if stream.at(TokenKind::Punct(Punctuation::OpenBrace)) {
            return self.parse_array_literal(stream, ast);
        }

        // `.{ .. }` in an expression position is a nested initialiser. Its type
        // comes from the field it initialises, so it is not inferred here.
        if stream.at(TokenKind::Punct(Punctuation::DotBrace)) {
            let mut init = self.parse_designated_init(stream, ast)?;
            let span = init.span;
            let id = ast
                .builder()
                .alloc_expr(ExprKind::NestedInit(init.clone()), span);
            // The initialiser records its own expression id, so the checker can
            // attach the type it derived from the enclosing field.
            init.expr = id;
            ast.expr_mut(id).kind = ExprKind::NestedInit(init);
            return Some(id);
        }

        let token = match stream.current_kind() {
            TokenKind::Int
            | TokenKind::Float
            | TokenKind::Char
            | TokenKind::String
            | TokenKind::FString
            | TokenKind::ByteString
            | TokenKind::RawString
            | TokenKind::Ident => stream.advance(),
            TokenKind::Punct(Punctuation::OpenParen) => {
                // `(values + ...)` -- a fold over a parameter pack. Recognised
                // by shape rather than by parsing an expression and repairing
                // it: `name op ...` is not an expression, since `...` is not an
                // operand.
                let open = stream.current_span();
                if let Some(fold) = self.parse_fold(stream, ast)? {
                    let close = self.expect_punct(stream, Punctuation::CloseParen, "`)`")?;
                    return Some(
                        ast.builder()
                            .alloc_expr(ExprKind::Fold(fold), open.to(close.span)),
                    );
                }
                // `(init op ... op pack)`: the starting value comes first, so
                // the shape is only visible after reading it. Read as a *unary*
                // expression, because the operator that follows it is the fold's
                // own: `(a + b + ... x)` would take `+` for the fold and `a + b`
                // for the starting value, which is not what C++'s grammar says
                // and not what a reader expects.
                if let Some(fold) = self.parse_leading_fold(stream, ast)? {
                    let close = self.expect_punct(stream, Punctuation::CloseParen, "`)`")?;
                    return Some(
                        ast.builder()
                            .alloc_expr(ExprKind::Fold(fold), open.to(close.span)),
                    );
                }
                // `(pattern, ...)`: a comma fold, which repeats the pattern
                // for every element of the pack it names. The shape is only
                // visible after the pattern is read, so it is read on the side
                // and put back when it is an ordinary parenthesised
                // expression.
                if let Some(pattern) = self.speculate(stream, |parser, stream| {
                    stream.advance(); // `(`
                    let pattern = parser.parse_expr_no_comma(stream, ast)?;
                    if stream.current_kind() != TokenKind::Punct(Punctuation::Comma)
                        || stream.peek_kind(1) != TokenKind::Punct(Punctuation::DotDotDot)
                        || stream.peek_kind(2) != TokenKind::Punct(Punctuation::CloseParen)
                    {
                        return None;
                    }
                    stream.advance(); // `,`
                    stream.advance(); // `...`
                    let close_span = stream.current_span();
                    stream.advance(); // `)`
                    Some((pattern, close_span))
                }) {
                    let (pattern, close_span) = pattern;
                    return Some(
                        ast.builder().alloc_expr(
                            ExprKind::CommaFold { pattern },
                            open.to(close_span),
                        ),
                    );
                }
                // A parenthesised expression. Needed so that `(x as u8) as i32`
                // and similar nest correctly.
                stream.advance();
                // A parenthesised expression is an expression like any other,
                // so `->` inside it is member access again.
                let arrow_was_result = std::mem::replace(&mut self.result_arrow_next, false);
                let inner = self.parse_expr(stream, ast);
                self.result_arrow_next = arrow_was_result;
                let inner = inner?;
                let close = self.expect_punct(stream, Punctuation::CloseParen, "`)`")?;
                let _ = open;
                // The inner expression's span is widened to include the
                // parentheses so that diagnostics point at the whole group.
                let widened = ast.expr(inner).span.to(close.span);
                ast.expr_mut(inner).span = widened;
                return Some(inner);
            }
            _ => {
                self.report_unexpected(stream, "an expression");
                return None;
            }
        };
        let span = token.span;
        let text = self.token_text(stream, &token);
        let kind = match token.kind {
            // The suffix is split off here, so later stages need not
            // re-derive where the digits end. `5u8` is the text `5` with the
            // suffix `u8`.
            TokenKind::Int => {
                let (digits, suffix) = split_numeric_suffix(&text);
                ExprKind::IntLiteral(fallc_ast::IntLiteral {
                    digits: digits.to_owned(),
                    suffix: suffix.map(ToOwned::to_owned),
                })
            }
            TokenKind::Float => {
                let (digits, suffix) = split_numeric_suffix(&text);
                ExprKind::FloatLiteral(fallc_ast::FloatLiteral {
                    text: digits.to_owned(),
                    suffix: suffix.map(ToOwned::to_owned),
                })
            }
            TokenKind::Char => ExprKind::CharLiteral(fallc_ast::CharLiteral {
                text: text.clone(),
                // A width prefix is part of the literal's text, so it is read
                // back off the front: `u16'x'` is a `c16`.
                width: char_literal_width(&text),
            }),

            // `f"..."` becomes a `std.format` call: the text between the
            // quotes is the format, and each `{...}` is an argument, in the
            // order it appears. The literal carries its own `f`, so the body
            // is taken without it.
            TokenKind::FString => {
                let body = text.strip_prefix('f').unwrap_or(&text);
                return Some(self.expand_fstring(stream, ast, body, span));
            }
            TokenKind::String => ExprKind::StringLiteral(text),
            TokenKind::ByteString => ExprKind::ByteStringLiteral(text),
            TokenKind::RawString => ExprKind::RawStringLiteral(text),
            // A name is just a name. A dotted path such as `C.printf` is not
            // folded here: `p.show` and `C.printf` are written identically, and
            // only name resolution can tell a module from a value, so the dot
            // is left as member access and resolution decides.
            _ => ExprKind::Name(text),
        };
        Some(ast.builder().alloc_expr(kind, span))
    }

    /// Parses one argument of a call.
    ///
    /// Almost always an expression, but a *trait query*'s argument is a type,
    /// and a type that is a template instance -- `Iterable(Array<int>)` -- is
    /// not an expression at all. A name followed by `<` is tried with the type
    /// grammar first, and kept only if the type ends where an argument can end;
    /// `f(a < b, c)` is unchanged, because `a < b` is not a type.
    /// Parses a call's argument list, which is a prefix of positional
    /// arguments followed by any number of named ones.
    pub(super) fn parse_call_arguments(
        &mut self,
        stream: &mut TokenStream,
        ast: &mut Ast,
        args: &mut Vec<ExprId>,
        named: &mut Vec<NamedArgument>,
    ) -> Option<()> {
        // An empty argument list is legal, so this is guarded rather than
        // always entering the loop.
        if stream.at(TokenKind::Punct(Punctuation::CloseParen)) {
            return Some(());
        }
        loop {
            if self.at_named_argument(stream) {
                named.push(self.parse_named_argument(stream, ast)?);
            } else {
                if !named.is_empty() {
                    self.diagnostics.push(
                        Diagnostic::error(
                            "a positional argument cannot follow a named one",
                        )
                        .with_code(DiagnosticCode::parser(2))
                        .with_span(stream.current_span())
                        .with_note(
                            "write every positional argument first, then `.name = value` \
                             for the ones named",
                        ),
                    );
                }
                // A comma separates arguments, so it must not be read as the
                // comma operator.
                let argument = self.parse_call_argument(stream, ast)?;
                // `f(values...)` passes every element of a pack: the `...`
                // follows the one argument written for them all.
                let argument = match stream.eat(TokenKind::Punct(Punctuation::DotDotDot)) {
                    Some(dots) => {
                        let span = ast.expr(argument).span.to(dots.span);
                        ast.builder()
                            .alloc_expr(ExprKind::PackExpand(argument), span)
                    }
                    None => argument,
                };
                args.push(argument);
            }
            if stream.eat(TokenKind::Punct(Punctuation::Comma)).is_none() {
                break;
            }
            if stream.at(TokenKind::Punct(Punctuation::CloseParen)) {
                break;
            }
        }
        Some(())
    }

    /// Whether the next tokens are a named argument, `.name = value`.
    pub(super) fn at_named_argument(&self, stream: &TokenStream) -> bool {
        stream.at(TokenKind::Punct(Punctuation::Dot))
            && stream.peek_kind(1) == TokenKind::Ident
            && stream.peek_kind(2) == TokenKind::Punct(Punctuation::Eq)
    }

    /// Parses `.name = value`.
    pub(super) fn parse_named_argument(
        &mut self,
        stream: &mut TokenStream,
        ast: &mut Ast,
    ) -> Option<NamedArgument> {
        let dot = stream.advance();
        let name_token = self.parse_ident(stream, "an argument name")?;
        self.expect_punct(stream, Punctuation::Eq, "`=`")?;
        let value = self.parse_expr_no_comma(stream, ast)?;
        Some(NamedArgument {
            name: self.token_text(stream, &name_token),
            name_span: name_token.span,
            value,
            span: dot.span.to(ast.expr(value).span),
        })
    }

    pub(super) fn parse_call_argument(&mut self, stream: &mut TokenStream, ast: &mut Ast) -> Option<ExprId> {
        // A requirement's argument is the parameter's type, so the type grammar
        // is tried first: `it.add(*int)` and `it.each([2]int)` state a pointer
        // and an array, which are not expressions at all.
        //
        // A bare name is left to the ordinary reading, because in a requirement
        // it may name either a type (`it.push(int)`) or the subject itself
        // (`InputIterator(it)` asks about the type `it` has). Only the checker
        // knows which, and it already reads both.
        if self.in_requirement && !stream.at(TokenKind::Ident) {
            let written = self.speculate(stream, |parser, stream| {
                let start = stream.current_span();
                let ty = parser.parse_type(stream, ast)?;
                // `type(expr)` keeps its expression reading: the operand is an
                // expression, and its names are resolved as one. The checker
                // treats the argument as a type either way.
                if matches!(ty, fallc_ast::TypeSyntax::TypeOf(_)) {
                    return None;
                }
                let ends_argument = stream.at(TokenKind::Punct(Punctuation::Comma))
                    || stream.at(TokenKind::Punct(Punctuation::CloseParen));
                ends_argument.then(|| {
                    let span = start.to(ty.span());
                    ast.builder()
                        .alloc_expr(ExprKind::TypeLiteral(Box::new(ty)), span)
                })
            });
            if written.is_some() {
                return written;
            }
        }
        if stream.at(TokenKind::Ident)
            && stream.peek_kind(1) == TokenKind::Punct(Punctuation::Lt)
        {
            let written = self.speculate(stream, |parser, stream| {
                let start = stream.current_span();
                let ty = parser.parse_type(stream, ast)?;
                // `type(expr)` keeps its expression reading: the operand is an
                // expression, and its names are resolved as one. The checker
                // treats the argument as a type either way.
                if matches!(ty, fallc_ast::TypeSyntax::TypeOf(_)) {
                    return None;
                }
                let ends_argument = stream.at(TokenKind::Punct(Punctuation::Comma))
                    || stream.at(TokenKind::Punct(Punctuation::CloseParen));
                ends_argument.then(|| {
                    let span = start.to(ty.span());
                    ast.builder()
                        .alloc_expr(ExprKind::TypeLiteral(Box::new(ty)), span)
                })
            });
            if written.is_some() {
                return written;
            }
        }
        self.parse_expr_no_comma(stream, ast)
    }

    /// Parses postfix operators, applied left to right: `as Type`, postfix
    /// `++`/`--`, `.field` and `->field`, `[index]`, and a call, with or
    /// without explicit template arguments.
    ///
    /// They all bind tighter than any binary operator, so `a + b as u8` is
    /// `a + (b as u8)` and `x++ + 1` is `(x++) + 1`, and a chain is read in
    /// source order: `x as u8 as i32` converts twice.
    pub(super) fn parse_postfix(&mut self, stream: &mut TokenStream, ast: &mut Ast) -> Option<ExprId> {
        self.parse_postfix_inner(stream, ast, true)
    }

    /// Parses a postfix expression without consuming a trailing `as`.
    ///
    /// Used by the prefix operators, whose operand is a postfix expression --
    /// `&a[i]` addresses the element -- but which a following cast applies to as
    /// a whole: `&x as *byte` is `(&x) as *byte`.
    pub(super) fn parse_postfix_without_cast(
        &mut self,
        stream: &mut TokenStream,
        ast: &mut Ast,
    ) -> Option<ExprId> {
        self.parse_postfix_inner(stream, ast, false)
    }

    /// Parses a postfix expression, optionally consuming a trailing `as`.
    pub(super) fn parse_postfix_inner(
        &mut self,
        stream: &mut TokenStream,
        ast: &mut Ast,
        casts: bool,
    ) -> Option<ExprId> {
        let mut expr = self.parse_prefix(stream, ast)?;
        loop {
            // `as Type`
            if casts && stream.at(TokenKind::Keyword(Keyword::As)) {
                stream.advance();
                let ty = self.parse_type(stream, ast)?;
                let span = ast.expr(expr).span.to(ty.span());
                expr = ast
                    .builder()
                    .alloc_expr(ExprKind::Cast(CastExpr { operand: expr, ty }), span);
                continue;
            }
            // Postfix `++` and `--`. These bind tighter than any binary
            // operator, so `x++ + 1` is `(x++) + 1`.
            if let Some(op) = incdec_op_of(stream.current_kind()) {
                let end = stream.current_span();
                stream.advance();
                let span = ast.expr(expr).span.to(end);
                expr = ast.builder().alloc_expr(
                    ExprKind::IncDec(IncDecExpr {
                        op,
                        postfix: true,
                        operand: expr,
                    }),
                    span,
                );
                continue;
            }
            // A requirement's `->` says what the operation yields, and ends
            // the expression; see `result_arrow_next`.
            if self.result_arrow_next && stream.at(TokenKind::Punct(Punctuation::Arrow)) {
                break;
            }
            // `object.field` and `pointer->field`
            if stream.at(TokenKind::Punct(Punctuation::Dot))
                || stream.at(TokenKind::Punct(Punctuation::Arrow))
            {
                let arrow = stream.at(TokenKind::Punct(Punctuation::Arrow));
                stream.advance();
                let field_token = self.parse_ident(stream, "a field name")?;
                let span = ast.expr(expr).span.to(field_token.span);
                expr = ast.builder().alloc_expr(
                    ExprKind::Field(FieldExpr {
                        object: expr,
                        field: self.token_text(stream, &field_token),
                        through_pointer: arrow,
                    }),
                    span,
                );
                continue;
            }
            // `object[index]`. A comma inside would be the comma operator, so
            // the subscript is parsed without one.
            if stream.at(TokenKind::Punct(Punctuation::OpenBracket)) {
                stream.advance();
                let index = self.parse_expr_no_comma(stream, ast)?;
                let close = self.expect_punct(stream, Punctuation::CloseBracket, "`]`")?;
                let span = ast.expr(expr).span.to(close.span);
                expr = ast.builder().alloc_expr(
                    ExprKind::Index(IndexExpr {
                        object: expr,
                        index,
                    }),
                    span,
                );
                continue;
            }
            // `callee<T, N>(args)` -- explicit template arguments.
            //
            // `f<int>(x)` and `a < b > (c)` are written alike, and only what
            // follows the `>` tells them apart: a template argument list is
            // immediately followed by `(`, and a comparison is not. That is the
            // whole disambiguation, and it is enough because a call is the only
            // thing a template argument list can precede.
            if stream.at(TokenKind::Punct(Punctuation::Lt))
                && self.template_arguments_follow(stream)
            {
                let explicit = self.parse_template_arguments(stream, ast)?;
                let open = self.expect_punct(stream, Punctuation::OpenParen, "`(`")?;
                let mut args = Vec::new();
                let mut named = Vec::new();
                self.parse_call_arguments(stream, ast, &mut args, &mut named)?;
                let close = self.expect_punct(stream, Punctuation::CloseParen, "`)`")?;
                let span = ast.expr(expr).span.to(close.span);
                let _ = open;
                expr = ast.builder().alloc_expr(
                    ExprKind::Call(CallExpr {
                        callee: expr,
                        args,
                        named,
                        template_args: explicit,
                        explicit_template_arguments: true,
                    }),
                    span,
                );
                continue;
            }
            // `call()??` -- pass the call's exception on, and keep its value.
            //
            // Read in the postfix loop because it binds to the expression
            // before it, and because what it produces is a value: the call's
            // own, when there is no exception. Two question marks, because one
            // is the conditional: `a ? b : c` and `a??` cannot be confused.
            if stream.at(TokenKind::Punct(Punctuation::Question))
                && stream.peek_kind(1) == TokenKind::Punct(Punctuation::Question)
            {
                let start = ast.expr(expr).span;
                stream.advance();
                let end = stream.advance().span;
                expr = ast.builder().alloc_expr(
                    ExprKind::Propagate(fallc_ast::PropagateExpr { value: expr }),
                    start.to(end),
                );
                continue;
            }
            // `call()!!` -- the value, or the program stops. `!!` is two
            // bangs because one is `!=`'s first character and a single `!` is
            // the logical not; the pair is unambiguous in postfix position.
            if stream.at(TokenKind::Punct(Punctuation::Bang))
                && stream.peek_kind(1) == TokenKind::Punct(Punctuation::Bang)
            {
                let start = ast.expr(expr).span;
                stream.advance();
                let end = stream.advance().span;
                expr = ast.builder().alloc_expr(
                    ExprKind::Abort(fallc_ast::AbortExpr { value: expr }),
                    start.to(end),
                );
                continue;
            }
            // `callee(args)`
            if stream.at(TokenKind::Punct(Punctuation::OpenParen)) {
                stream.advance();
                let mut args = Vec::new();
                let mut named = Vec::new();
                self.parse_call_arguments(stream, ast, &mut args, &mut named)?;
                let close = self.expect_punct(stream, Punctuation::CloseParen, "`)`")?;
                let span = ast.expr(expr).span.to(close.span);
                expr = ast
                    .builder()
                    .alloc_expr(
                        ExprKind::Call(CallExpr {
                            callee: expr,
                            args,
                            named,
                            template_args: Vec::new(),
                            explicit_template_arguments: false,
                        }),
                        span,
                    );
                continue;
            }
            break;
        }
        Some(expr)
    }

    // ------------------------------------------------------------- recovery

}

impl Parser {
    /// Builds `std.format("...", args...)` from the body of an `f"..."`.
    ///
    /// The body is already one token, so the placeholders it names have to be
    /// read out of its text and then parsed as expressions. They are lexed
    /// again here rather than being kept as tokens: the lexer produces one
    /// interpolated-string token on purpose, so that what is inside a
    /// placeholder is the parser's business and the lexer stays context-free.
    ///
    /// `{{` and `}}` are the escapes for literal braces, as in every language
    /// that has this feature: a format string has to be able to say `{` without
    /// naming a value.
    fn expand_fstring(
        &mut self,
        stream: &mut TokenStream,
        ast: &mut Ast,
        body: &str,
        span: fallc_span::Span,
    ) -> fallc_ast::ExprId {
        let (format, placeholders) = split_fstring(body);
        // The format string is a literal of this tree, so its span is the
        // literal's: a diagnostic about the format points at the whole thing.
        let format_id = ast.builder().alloc_expr(
            fallc_ast::ExprKind::StringLiteral(format),
            span,
        );
        // The call is `std.format(...)`: the standard library's format, so an
        // f-string means the same thing whatever the file imports. A program
        // that writes an f-string needs `import std;`, exactly as one that
        // calls `std.println` does.
        // A span of its own, one byte in: two names must not share a span, or
        // resolution records one binding and reads it for both.
        let module_span = fallc_span::Span {
            file: span.file,
            lo: span.lo.saturating_add(1),
            hi: span.lo.saturating_add(2),
        };
        let module = ast
            .builder()
            .alloc_expr(fallc_ast::ExprKind::Name("std".to_owned()), module_span);
        let callee = ast.builder().alloc_expr(
            fallc_ast::ExprKind::Field(fallc_ast::FieldExpr {
                object: module,
                field: "format".to_owned(),
                through_pointer: false,
            }),
            span,
        );
        let mut args = vec![format_id];
        for (source, _spec) in placeholders {
            // Each placeholder is parsed with the same parser the rest of the
            // file uses, from its own token stream.
            let mut lexer = fallc_lexer::Lexer::new(fallc_lexer::LexerConfig {
                preserve_trivia: false,
                errors_are_warnings: false,
            });
            let lexed = lexer.tokenize(&source, fallc_span::FileId::SYNTHETIC);
            let tokens = lexed.tokens;
            let mut inner = TokenStream::new(tokens);
            match self.parse_expr(&mut inner, ast) {
                Some(expression) => args.push(expression),
                None => {
                    self.push_diagnostic(
                        fallc_diag::Diagnostic::error(
                            "this placeholder is not an expression",
                        )
                        .with_code(fallc_diag::DiagnosticCode::parser(1))
                        .with_span(span),
                    );
                }
            }
        }
        let _ = stream;
        let call = ast.builder().alloc_expr(
            fallc_ast::ExprKind::Call(fallc_ast::CallExpr {
                callee,
                args,
                named: Vec::new(),
                template_args: Vec::new(),
                explicit_template_arguments: false,
            }),
            span,
        );
        // No marker of its own: an f-string expands to a plain call, and what
        // happens when building the text fails is the caller's decision --
        // `f".."?` passes the exception on, `f".."!!` stops the program, and
        // writing neither asks the checker to report an unhandled raise.
        call
    }
}

/// Splits the body of an `f"..."` into the format text and its placeholders.
///
/// `{{` and `}}` become one brace in the format text. A `{expr}` yields the
/// expression, and `{expr:spec}` yields both: the spec is kept as the
/// placeholder's own, so `{n:#x}` becomes `{: #x}` in the format and `n` in the
/// arguments. The scan stops at the first `}` that closes the placeholder, so a
/// `:` inside it is the spec's unless it is inside brackets -- which is why the
/// nesting depth is counted.
fn split_fstring(body: &str) -> (String, Vec<(String, String)>) {
    let mut format = String::new();
    let mut placeholders = Vec::new();
    let mut characters = body.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            // `{{` and `}}` are a literal brace. They are kept doubled in the
            // format string: the placeholder scanner downstream treats a single
            // `{` as the start of a placeholder, so collapsing them here would
            // turn an escaped brace back into one.
            '{' if characters.peek() == Some(&'{') => {
                characters.next();
                format.push_str("{{");
            }
            '}' if characters.peek() == Some(&'}') => {
                characters.next();
                format.push_str("}}");
            }
            '{' => {
                let mut expression = String::new();
                let mut spec = String::new();
                let mut in_spec = false;
                let mut depth = 1usize;
                for inner in characters.by_ref() {
                    match inner {
                        '{' | '(' | '[' => {
                            depth += 1;
                            if in_spec {
                                spec.push(inner);
                            } else {
                                expression.push(inner);
                            }
                        }
                        '}' | ')' | ']' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                            if in_spec {
                                spec.push(inner);
                            } else {
                                expression.push(inner);
                            }
                        }
                        ':' if depth == 1 && !in_spec => {
                            in_spec = true;
                        }
                        other => {
                            if in_spec {
                                spec.push(other);
                            } else {
                                expression.push(other);
                            }
                        }
                    }
                }
                format.push('{');
                format.push(':');
                format.push_str(&spec);
                format.push('}');
                placeholders.push((expression, spec));
            }
            other => format.push(other),
        }
    }
    (format, placeholders)
}
