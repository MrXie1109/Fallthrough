//! Items: the top level of a file.
//!
//! `parse_file` reads items until the end; each declaration has a function of
//! its own here. The template machinery lives with them because a template
//! header is part of the declaration it introduces.

use super::*;

impl Parser {
    /// Creates a parser with the given configuration.
    #[must_use]
    pub fn new(config: ParserConfig) -> Self {
        Parser {
            config,
            diagnostics: DiagnosticBag::new(),
            source: None,
            depth: 0,
            poisoned: false,
            result_arrow_next: false,
            in_requirement: false,
            in_template_header: 0,
        }
    }

    /// Returns this parser's configuration.
    #[must_use]
    pub fn config(&self) -> ParserConfig {
        self.config
    }

    /// Supplies the source text, so the parser can recover identifier
    /// spelling. Must be called before [`Parser::parse_file`] for names to be
    /// captured.
    pub fn set_source(&mut self, source: impl Into<String>) {
        self.source = Some(source.into());
    }

    /// Parses a whole token stream into an [`Ast`].
    pub fn parse_file(&mut self, stream: &mut TokenStream) -> ParseResult {
        let mut ast = Ast::new();
        self.parse_items(stream, &mut ast);
        ParseResult {
            ast,
            diagnostics: std::mem::take(&mut self.diagnostics).into_vec(),
        }
    }

    // ---------------------------------------------------------------- items

    /// Parses top-level items until EOF.
    ///
    /// A production that cannot parse its item reports and returns `None`, and
    /// recovery skips to the start of the next item, consuming at least one
    /// token so the loop cannot spin.
    pub(super) fn parse_items(&mut self, stream: &mut TokenStream, ast: &mut Ast) {
        while !stream.at_eof() {
            let before = stream.position();
            let reported = self.diagnostics.len();
            match self.parse_item(stream, ast) {
                Some(id) => {
                    ast.builder().push_item(id);
                }
                None => {
                    // A production that failed has already said why; one that
                    // found nothing it could start with has not. Without this
                    // the token was skipped in silence -- `static fn f()` at
                    // the top level compiled, with the `static` dropped on the
                    // floor and no suggestion that it meant nothing there.
                    if self.diagnostics.len() == reported {
                        self.report_unexpected(stream, "a declaration");
                    }
                    // Recovery must consume something, or this loop spins.
                    self.recover_to_item(stream);
                }
            }
            // Belt and braces: if a production returned without consuming,
            // force progress. An infinite loop in the parser is a hang, which
            // is far worse than a spurious diagnostic.
            if stream.position() == before && !stream.at_eof() {
                let span = stream.current_span();
                self.report_unexpected(stream, "a declaration");
                let _ = span;
                stream.advance();
            }
        }
    }

    /// Parses one top-level item.
    pub fn parse_item(&mut self, stream: &mut TokenStream, ast: &mut Ast) -> Option<ItemId> {
        // Attributes come before the declaration they describe, and a
        // declaration may carry several. They are read here rather than in each
        // production because which attributes are allowed where is a question
        // for the checker, not for the grammar.
        let mut attrs = self.parse_attributes(stream, ast)?;
        // `export` is a keyword that introduces a declaration, not an
        // attribute: `export fn f()`, `export from M import *;`,
        // `export define NAME ..`. The attribute spelling is refused with the
        // rule, so a reader is not left wondering which one the language wants.
        if let Some(attribute) = attrs.iter().find(|attr| attr.name == "export") {
            self.push_diagnostic(
                Diagnostic::error("`export` is a keyword, not an attribute")
                    .with_code(DiagnosticCode::parser(2))
                    .with_span(attribute.span)
                    .with_note("write `export` before the declaration: `export fn f() ..;`"),
            );
            attrs.retain(|attr| attr.name != "export");
        }
        let exported = if stream.at(TokenKind::Keyword(Keyword::Export)) {
            stream.advance();
            true
        } else {
            false
        };
        // `export from M import *;` republishes a module's names; the word
        // after `export` decides which production this is.
        let id = if exported && stream.at(TokenKind::Keyword(Keyword::From)) {
            self.parse_from_import(stream, ast, true)?
        } else if exported
            && (stream.at(TokenKind::Keyword(Keyword::Import))
                || stream.at(TokenKind::Keyword(Keyword::Link))
                || stream.at(TokenKind::Keyword(Keyword::StaticAssert)))
        {
            self.report_unexpected(stream, "an exportable declaration");
            return None;
        } else {
            self.parse_item_kind(stream, ast, exported)?
        };
        // The attributes that replace an old keyword are folded into the
        // fields it used to set, so the rest of the compiler keeps working
        // while both spellings exist. Attributes with new semantics stay on the
        // item for the checker.
        if !attrs.is_empty() {
            self.apply_declaration_attributes(ast, id, &attrs);
            ast.item_mut(id).attrs = attrs;
        }
        Some(id)
    }

    /// Folds declaration attributes that replace old keywords into the fields
    /// the parser used to set from those keywords.
    ///
    /// `@export` is the old `export` for every entity that could carry it;
    /// `@constexpr` and `@extern_C` are the old `constexpr` and `extern "C"`
    /// for functions. Everything else is left for the checker: `@must_use`,
    /// `@unused`, `@deprecated`, `@enable_if`, `@likely` and so on have
    /// semantics of their own rather than a flag already in the tree.
    fn apply_declaration_attributes(
        &mut self,
        ast: &mut Ast,
        id: ItemId,
        attrs: &[Attr],
    ) {
        let exported = attrs.iter().any(|attr| attr.name == "export");
        let constexpr = attrs.iter().any(|attr| attr.name == "constexpr");
        let extern_c = attrs.iter().any(|attr| attr.name == "extern_C");
        match &mut ast.item_mut(id).kind {
            ItemKind::Function(function) => {
                function.exported |= exported;
                function.is_constexpr |= constexpr;
                if extern_c && function.abi.is_none() {
                    function.abi = Some("C".to_owned());
                }
            }
            ItemKind::TypeDeclaration(declaration) => declaration.exported |= exported,
            ItemKind::EnumDeclaration(declaration) => declaration.exported |= exported,
            ItemKind::Trait(declaration) => declaration.exported |= exported,
            ItemKind::Interface(declaration) => declaration.exported |= exported,
            ItemKind::Const(constant) => constant.exported |= exported,
            ItemKind::Variable(binding) => binding.exported |= exported,
            ItemKind::ExternVariable(variable) => {
                variable.exported |= exported;
                if extern_c {
                    variable.abi = "C".to_owned();
                }
            }
            _ => {}
        }
    }

    /// Parses the run of `@name` / `@name(args)` before a declaration.
    pub(super) fn parse_attributes(&mut self, stream: &mut TokenStream, ast: &mut Ast) -> Option<Vec<Attr>> {
        let mut attrs = Vec::new();
        while stream.at(TokenKind::Punct(Punctuation::At)) {
            let at = stream.advance();
            // An attribute name is a word. While the old keywords still exist,
            // `@static`, `@export` and `@constexpr` arrive as keyword tokens;
            // once they are ordinary identifiers the same branch still reads
            // them. Taking any keyword here is deliberate: the whitelist is a
            // question for the checker, not for the lexer.
            let name_token = match stream.current_kind() {
                TokenKind::Ident => stream.advance(),
                TokenKind::Keyword(_) => stream.advance(),
                _ => {
                    self.diagnostics.push(
                        Diagnostic::error("expected an attribute name after `@`")
                            .with_code(DiagnosticCode::parser(2))
                            .with_span(stream.current_span()),
                    );
                    return None;
                }
            };
            let name = self.token_text(stream, &name_token);
            let mut arguments = Vec::new();
            let mut end = name_token.span;
            if stream.at(TokenKind::Punct(Punctuation::OpenParen)) {
                stream.advance();
                while !stream.at(TokenKind::Punct(Punctuation::CloseParen)) {
                    arguments.push(self.parse_expr(stream, ast)?);
                    if stream.at(TokenKind::Punct(Punctuation::Comma)) {
                        stream.advance();
                        continue;
                    }
                    break;
                }
                end = self
                    .expect_punct(stream, Punctuation::CloseParen, "`)`")?
                    .span;
            }
            attrs.push(Attr::new(name, name_token.span, arguments, at.span.to(end)));
        }
        Some(attrs)
    }

    /// Parses one declaration, whose attributes have already been read.
    pub(super) fn parse_item_kind(
        &mut self,
        stream: &mut TokenStream,
        ast: &mut Ast,
        exported: bool,
    ) -> Option<ItemId> {
        // `import Name;`, `import Name as Alias;`
        if stream.at(TokenKind::Keyword(Keyword::Import)) {
            return self.parse_import(stream, ast);
        }

        // `from Name import a, b;` and `from Name import *;`
        if stream.at(TokenKind::Keyword(Keyword::From)) {
            return self.parse_from_import(stream, ast, false);
        }

        // `link m, dynamic, default;`
        if stream.at(TokenKind::Keyword(Keyword::Link)) {
            return self.parse_link(stream, ast);
        }

        // `__setup__ { .. }`: the block a module runs before `main`.
        //
        // The name is an ordinary identifier rather than a keyword, and it is
        // recognised by position -- `__setup__` followed by a brace at the top
        // level -- because a keyword would collide with the name a program may
        // have used before this form existed.
        if stream.at(TokenKind::Ident)
            && self.token_text(stream, stream.peek(0)) == "__setup__"
            && stream.peek_kind(1) == TokenKind::Punct(Punctuation::OpenBrace)
        {
            stream.advance();
            let block = self.parse_block(stream, ast)?;
            let span = ast.block(block).span;
            return Some(ast.builder().alloc_item(ItemKind::Setup(block), span));
        }

        // `static_assert(cond);` at module level: a claim checked where it is
        // written.
        if stream.at(TokenKind::Keyword(Keyword::StaticAssert)) {
            let node = self.parse_static_assert(stream, ast)?;
            let span = node.span;
            return Some(
                ast.builder()
                    .alloc_item(ItemKind::StaticAssert(node), span),
            );
        }

        // `template <..>` prefixes a declaration, so it is read first and then
        // handed to whichever declaration follows. A template header on its own
        // declares nothing, which is reported rather than silently ignored.
        let start = stream.current_span();
        let template_params = if stream.at(TokenKind::Keyword(Keyword::Template)) {
            self.parse_template_header(stream, ast)?
        } else {
            Vec::new()
        };

        // The rest of the declaration prefix is attributes: `@constexpr` and
        // `@extern_C` were read before this point and are applied to the item
        // after it is parsed. Only `export` and `template <..>` are keywords
        // in prefix position.
        let abi: Option<String> = None;
        let is_constexpr = false;

        if stream.at(TokenKind::Keyword(Keyword::Fn)) {
            return self.parse_function(
                stream,
                ast,
                start,
                template_params,
                DeclarationPrefix {
                    exported,
                    abi,
                    is_constexpr,
                },
            );
        }

        // The three forms are one declaration with three layouts, so they are
        // built once and told apart by the keyword that led here. The prefix is
        // gathered here rather than earlier because the checks above read its
        // parts directly.
        let prefix = DeclarationPrefix {
            exported,
            abi,
            is_constexpr,
        };
        // `traits Name(it Iter) { .. }`. A prefix that says nothing about the
        // requirements is refused rather than ignored: `extern traits` and
        // `constexpr traits` would both be meaningless, and silently dropping
        // them would make a program read differently from what it says.
        if stream.at(TokenKind::Keyword(Keyword::Traits)) {
            if prefix.abi.is_some() || prefix.is_constexpr {
                self.report_unexpected(stream, "a trait declaration");
                return None;
            }
            if !template_params.is_empty() {
                self.report_unexpected(stream, "a trait declaration");
                return None;
            }
            return self.parse_trait(stream, ast, prefix.exported, start);
        }
        // `interface Name(it T) { it.method() -> Type, .. }`. A prefix that says
        // nothing about the signatures is refused for the same reason a trait's
        // is: silently dropping it would make the program read differently from
        // what it says.
        if stream.at(TokenKind::Keyword(Keyword::Interface)) {
            if prefix.abi.is_some() || prefix.is_constexpr {
                self.report_unexpected(stream, "an interface declaration");
                return None;
            }
            return self.parse_interface(
                stream,
                ast,
                prefix.exported,
                start,
                template_params,
            );
        }
        if stream.at(TokenKind::Keyword(Keyword::Type)) {
            return self.parse_type_declaration(
                stream,
                ast,
                prefix,
                start,
                template_params,
                false,
            );
        }
        if stream.at(TokenKind::Keyword(Keyword::Union)) {
            return self.parse_type_declaration(
                stream,
                ast,
                prefix,
                start,
                template_params,
                true,
            );
        }
        if stream.at(TokenKind::Keyword(Keyword::Enum)) {
            return self.parse_enum(stream, ast, prefix, start, template_params);
        }

        // `NAME const Type = value;` -- a constant is a binding whose type is
        // `const`-qualified, so it is written the way every other declaration
        // is: the name first, then what it is.
        if stream.at(TokenKind::Ident)
            && stream.peek_kind(1) == TokenKind::Keyword(Keyword::Const)
        {
            if !template_params.is_empty() {
                self.report_unexpected(stream, "a templated declaration");
                return None;
            }
            return self.parse_const(stream, ast, exported, start);
        }
        // `name Type = expr;` -- a variable that lives as long as the program.
        // A constant is the same shape with `const` in the type, so this branch
        // takes every other binding: `seed u64 = 1;`.
        if stream.at(TokenKind::Ident) {
            if !template_params.is_empty() || prefix.is_constexpr {
                self.report_unexpected(stream, "a variable declaration");
                return None;
            }
            let Some(StmtKind::Bind(mut bind)) = self.parse_bind_inner(stream, ast, true) else {
                return None;
            };
            // The one place a binding may be exported: a module-level variable
            // is one object for the whole program, so another module can name
            // it. `export` on a local is refused where the local is parsed,
            // because there the keyword would silently do nothing.
            bind.exported |= exported;
            let span = start.to(bind.name_span);
            // `stdin *c8 @extern_C;` declares a variable whose storage belongs
            // to another library: the old `extern "C" NAME Type;` written with
            // the attribute in its variable position.
            if bind.attrs.iter().any(|attr| attr.name == "extern_C") {
                if bind.init.is_some() || bind.is_static || bind.is_thread_local {
                    self.push_diagnostic(
                        Diagnostic::error(
                            "an `@extern_C` variable has no storage of its own, so it cannot \
                             have an initialiser or a storage class",
                        )
                        .with_code(DiagnosticCode::parser(2))
                        .with_span(bind.name_span),
                    );
                    return None;
                }
                let fallc_ast::BindingType::Written(ty) = bind.ty else {
                    self.push_diagnostic(
                        Diagnostic::error(
                            "an `@extern_C` variable needs a written type: `auto` would have \
                             nothing to deduce from",
                        )
                        .with_code(DiagnosticCode::parser(2))
                        .with_span(bind.name_span),
                    );
                    return None;
                };
                let attrs = bind.attrs;
                return Some(ast.builder().alloc_item(
                    ItemKind::ExternVariable(fallc_ast::ExternVariableItem {
                        name: bind.name,
                        name_span: bind.name_span,
                        abi: "C".to_owned(),
                        ty,
                        exported: bind.exported,
                        attrs,
                        span,
                    }),
                    span,
                ));
            }
            return Some(
                ast.builder()
                    .alloc_item(ItemKind::Variable(bind), span),
            );
        }
        // The old spelling, refused with the rule rather than a vague error.
        if stream.at(TokenKind::Keyword(Keyword::Const)) {
            let span = stream.current_span();
            self.push_diagnostic(
                Diagnostic::error("a constant is declared `NAME const Type = value;`")
                    .with_code(DiagnosticCode::parser(2))
                    .with_span(span)
                    .with_note("the name comes first, as in every other declaration"),
            );
            return None;
        }

        if !template_params.is_empty() {
            self.report_unexpected(stream, "a function or type declaration after `template`");
            return None;
        }

        if prefix.exported || prefix.abi.is_some() {
            // A prefix on something that cannot carry it is worth saying, or
            // the keyword would silently do nothing.
            self.report_unexpected(stream, "a function declaration");
            return None;
        }

        // Every declaration form has been taken above, so what is left here is
        // not a declaration at all. The caller reports the token and recovers.
        None
    }

    /// Returns `true` if the `<` at the cursor opens a template argument list.
    ///
    /// Scans to the matching `>` and checks that a `(` follows it, which is what
    /// a call has and a comparison does not. The scan is over *tokens*, so a
    /// `>` inside a nested type argument is matched rather than taken for the
    /// end: `f<Box<int>>(x)` has two.
    pub(super) fn template_arguments_follow(&self, stream: &TokenStream) -> bool {
        self.template_arguments_follow_from(stream, 0)
    }

    /// The same, starting `from` tokens ahead.
    pub(super) fn template_arguments_follow_from(&self, stream: &TokenStream, from: usize) -> bool {
        let mut depth = 0usize;
        // Parentheses are skipped as *balanced* pairs, because a `constexpr`
        // call is a legal template argument: `f<sz()>(x)` has a `)` between the
        // angle brackets, and treating that as the end of an enclosing group
        // made the whole thing parse as `(f < sz()) > (x)` -- a comparison.
        let mut parens = 0usize;
        let mut index = from;
        loop {
            let kind = stream.peek_kind(index);
            match kind {
                TokenKind::Punct(Punctuation::OpenParen) => parens += 1,
                TokenKind::Punct(Punctuation::CloseParen) if parens > 0 => parens -= 1,
                TokenKind::Punct(Punctuation::Lt) => depth += 1,
                TokenKind::Punct(Punctuation::Gt) => {
                    depth = match depth.checked_sub(1) {
                        Some(depth) => depth,
                        None => return false,
                    };
                    if depth == 0 {
                        // The list ended here. A call or a braced initialiser
                        // is what continues an expression; anything else means
                        // this `<` was a comparison.
                        let next = stream.peek_kind(index + 1);
                        return next == TokenKind::Punct(Punctuation::OpenParen)
                            || next == TokenKind::Punct(Punctuation::OpenBrace);
                    }
                }
                // `>>` closes two levels at once when it ends a nested list,
                // but the lexer reads it as one token. It counts as two `>`
                // only when there are two levels open.
                TokenKind::Punct(Punctuation::Shr) => {
                    if depth >= 2 {
                        depth -= 2;
                        // `TSet<int, Greater<int>>(x)` closes both lists with
                        // one token, and the call that follows is what says
                        // these were argument lists rather than comparisons.
                        if depth == 0 {
                            let next = stream.peek_kind(index + 1);
                            return next == TokenKind::Punct(Punctuation::OpenParen)
                                || next == TokenKind::Punct(Punctuation::OpenBrace);
                        }
                    } else if depth == 1 {
                        return stream.peek_kind(index + 1)
                            == TokenKind::Punct(Punctuation::OpenParen);
                    } else {
                        return false;
                    }
                }
                // Anything that cannot appear inside a template argument list
                // means this `<` was a comparison. A `)` does not, while it is
                // closing a call that is itself an argument.
                TokenKind::Punct(Punctuation::Semi)
                | TokenKind::Punct(Punctuation::OpenBrace)
                | TokenKind::Punct(Punctuation::CloseBrace)
                | TokenKind::Eof
                    if parens == 0 =>
                {
                    return false;
                }
                TokenKind::Punct(Punctuation::CloseParen) | TokenKind::Eof
                    if parens > 0 =>
                {
                    return false;
                }
                _ => {}
            }
            index += 1;
        }
    }

    /// Parses `<int, 3, "text">`.
    ///
    /// # Grammar
    ///
    /// ```text
    /// args := '<' arg (',' arg)* '>'
    /// arg  := T | integer-literal | string-literal
    /// ```
    ///
    /// Which of the three an argument is cannot be decided here: `N` is a type
    /// if the parameter it fills is a type and a value if the parameter is a
    /// value, and the parameters are the checker's to look up. So a name is
    /// recorded as a type and an expression as a value, and the checker reports
    /// a mismatch. A bare integer or string is unambiguously a value.
    pub(super) fn parse_template_arguments(
        &mut self,
        stream: &mut TokenStream,
        ast: &mut Ast,
    ) -> Option<Vec<TemplateArgumentSyntax>> {
        self.expect_punct(stream, Punctuation::Lt, "`<`")?;
        let mut args = Vec::new();
        while !stream.at_gt() && !stream.at_eof() {
            let argument = match stream.current_kind() {
                // A literal is a value: there is no type it could name.
                //
                // Parsed as a *primary*, not as an expression: `f<4, "ab">()`
                // has a `>` after the argument, and an expression parser would
                // read it as a comparison and swallow the list's own closing
                // bracket. A template argument is one literal, so there is
                // nothing an expression parse could add.
                TokenKind::Int | TokenKind::String | TokenKind::Char => {
                    TemplateArgumentSyntax::Value(self.parse_prefix(stream, ast)?)
                }
                // A `constexpr` call is a value too: `f<sz()>()` names a
                // constant the compiler computes, which is the whole point of
                // marking the function. Only a *call* is admitted here, because
                // it is the one expression form that ends at its own closing
                // parenthesis -- so the `>` after it is unambiguously the end of
                // the argument list and not a comparison.
                TokenKind::Ident
                    if stream.peek_kind(1) == TokenKind::Punct(Punctuation::OpenParen) =>
                {
                    let callee = self.parse_ident(stream, "a function name")?;
                    let name = self.token_text(stream, &callee);
                    self.expect_punct(stream, Punctuation::OpenParen, "`(`")?;
                    let mut arguments = Vec::new();
                    while !stream.at(TokenKind::Punct(Punctuation::CloseParen))
                        && !stream.at_eof()
                    {
                        let argument = match stream.current_kind() {
                            TokenKind::Int | TokenKind::String | TokenKind::Char => {
                                self.parse_prefix(stream, ast)?
                            }
                            _ => {
                                self.report_unexpected(
                                    stream,
                                    "a literal argument in a constant call",
                                );
                                return None;
                            }
                        };
                        arguments.push(argument);
                        if stream.eat(TokenKind::Punct(Punctuation::Comma)).is_none() {
                            break;
                        }
                    }
                    self.expect_punct(stream, Punctuation::CloseParen, "`)`")?;
                    let span = callee.span;
                    let callee_expr = ast.builder().alloc_expr(
                        fallc_ast::ExprKind::Name(name),
                        span,
                    );
                    let call = fallc_ast::CallExpr {
                        callee: callee_expr,
                        args: arguments,
                        named: Vec::new(),
                        template_args: Vec::new(),
                        explicit_template_arguments: false,
                    };
                    TemplateArgumentSyntax::Value(
                        ast.builder().alloc_expr(fallc_ast::ExprKind::Call(call), span),
                    )
                }
                // `OpenMode.READ` may name a constant or a type, and only the
                // parameter says which: `Box<std.String>` names a type, and
                // `Device<OpenMode.READ>` names the constant `READ`. The path is
                // kept as written, and the checker reads it by the parameter's
                // kind.
                TokenKind::Ident
                    if stream.peek_kind(1) == TokenKind::Punct(Punctuation::Dot) =>
                {
                    let start = stream.current_span();
                    let first = self.parse_ident(stream, "a name")?;
                    let mut path = self.token_text(stream, &first);
                    while stream.eat(TokenKind::Punct(Punctuation::Dot)).is_some() {
                        let segment = self.parse_ident(stream, "a name")?;
                        path.push('.');
                        path.push_str(&self.token_text(stream, &segment));
                    }
                    TemplateArgumentSyntax::Named {
                        path,
                        span: start.to(stream.current_span()),
                    }
                }
                _ => {
                    let ty = self.parse_type(stream, ast)?;
                    TemplateArgumentSyntax::Type(ty)
                }
            };
            args.push(argument);
            if stream.eat(TokenKind::Punct(Punctuation::Comma)).is_none() {
                break;
            }
        }
        self.expect_gt(stream)?;
        Some(args)
    }

    /// Parses `template <P, Q>`.
    ///
    /// # Grammar
    ///
    /// ```text
    /// template := 'template' '<' param (',' param)* '>'
    /// param    := NAME 'type'          -- a type the caller chooses
    ///           | NAME T               -- a value the caller chooses
    /// ```
    ///
    /// The two forms are told apart by the word after the name: `type` is the
    /// keyword, so anything else is a type and the parameter is a *value* of
    /// it. `template <index usize, string []char>` is one of each.
    ///
    /// A value parameter's type is restricted to an integer or a character
    /// sequence, which is checked where the template is checked rather than
    /// here: the parser's business is the shape of the list.
    pub(super) fn parse_template_header(
        &mut self,
        stream: &mut TokenStream,
        ast: &mut Ast,
    ) -> Option<Vec<TemplateParam>> {
        stream.advance(); // `template`
        self.expect_punct(stream, Punctuation::Lt, "`<`")?;
        let mut params = Vec::new();
        let mut template_default_seen = false;
        while !stream.at_gt() && !stream.at_eof() {
            let start = stream.current_span();
            let name_token = self.parse_ident(stream, "a template parameter name")?;
            // `args ...Args` marks a pack. The `...` goes *before* the kind so
            // that `values ...int` reads as "values, each an `int`" rather than
            // as a type whose name begins with `...`.
            let is_pack = if stream.at(TokenKind::Punct(Punctuation::DotDotDot)) {
                stream.advance();
                true
            } else {
                false
            };
            let kind = if stream.at(TokenKind::Keyword(Keyword::Type)) {
                let keyword = stream.advance();
                let _ = keyword;
                TemplateParamKind::Type
            } else {
                // A value parameter: the type it takes, written as any type is.
                let ty = self.parse_type(stream, ast)?;
                TemplateParamKind::Value(ty)
            };
            // `= default`: a type for a type parameter, an expression for a
            // value parameter.
            let default = if stream.eat(TokenKind::Punct(Punctuation::Eq)).is_some() {
                if is_pack {
                    self.diagnostics.push(
                        Diagnostic::error("a template parameter pack cannot have a default")
                            .with_code(DiagnosticCode::parser(2))
                            .with_span(stream.current_span())
                            .with_note("a pack stands for any number of arguments, so there is                                         nothing for a default to fill in"),
                    );
                    return None;
                }
                match &kind {
                    TemplateParamKind::Type => {
                        Some(fallc_ast::TemplateDefault::Type(self.parse_type(stream, ast)?))
                    }
                    TemplateParamKind::Value(_) => {
                        let was_in_header = self.in_template_header;
                        self.in_template_header += 1;
                        let value = self.parse_expr_no_comma(stream, ast);
                        self.in_template_header = was_in_header;
                        Some(fallc_ast::TemplateDefault::Value(value?))
                    }
                }
            } else {
                None
            };
            // A parameter with a default cannot be followed by one without:
            // a caller that leaves an argument out can only leave out the tail.
            let defaults_must_be_last = template_default_seen && default.is_none();
            if default.is_some() {
                template_default_seen = true;
            }
            if defaults_must_be_last {
                self.diagnostics.push(
                    Diagnostic::error(
                        "a template parameter after a defaulted one must also have a default",
                    )
                    .with_code(DiagnosticCode::parser(2))
                    .with_span(name_token.span)
                    .with_note("a caller leaves arguments out from the right, so a parameter                                 without a default cannot follow one that has one"),
                );
            }
            // Only the last parameter may be a pack: nothing would say where
            // its arguments stopped and the next parameter's began. Checked
            // after the default, so that a pack written with one is reported as
            // the mistake it is rather than as a missing `>`.
            if is_pack && !stream.at_gt() {
                let span = stream.current_span();
                self.push_diagnostic(
                    Diagnostic::error("a template parameter pack must be the last parameter")
                        .with_code(DiagnosticCode::parser(2))
                        .with_span(span)
                        .with_note(
                            "nothing would say where this pack's arguments ended and the next \
                             parameter's began",
                        ),
                );
                return None;
            }
            let end = stream.current_span();
            params.push(TemplateParam {
                name: self.token_text(stream, &name_token),
                name_span: name_token.span,
                kind,
                is_pack,
                default,
                span: start.to(end),
            });
            if stream.at(TokenKind::Punct(Punctuation::Comma)) {
                stream.advance();
                continue;
            }
            break;
        }
        self.expect_gt(stream)?;
        Some(params)
    }

    /// Parses `NAME const Type = value;`.
    ///
    /// A constant is a binding that cannot be assigned, so it is written as a
    /// binding is: the name, then the type, then the value. `/`const/` is the
    /// qualifier on the *type*, exactly as it is in `x const int`, and the
    /// difference from a local binding is only where the declaration stands.
    pub(super) fn parse_const(
        &mut self,
        stream: &mut TokenStream,
        ast: &mut Ast,
        exported: bool,
        start: Span,
    ) -> Option<ItemId> {
        let name_token = self.parse_ident(stream, "a constant's name")?;
        let name = self.token_text(stream, &name_token);
        if !stream.at(TokenKind::Keyword(Keyword::Const)) {
            self.report_unexpected(stream, "`const`");
            return None;
        }
        stream.advance(); // `const`
        let ty = self.parse_type(stream, ast)?;
        // `LIMIT const usize = 8 @export;`
        let attrs = self.parse_attributes(stream, ast)?;
        let exported = exported || attrs.iter().any(|attr| attr.name == "export");
        self.expect_punct(stream, Punctuation::Eq, "`=`")?;
        let value = self.parse_expr_no_comma(stream, ast)?;
        let semi = self.expect_punct(stream, Punctuation::Semi, "`;`")?;
        Some(ast.builder().alloc_item(
            ItemKind::Const(ConstItem {
                exported,
                name,
                name_span: name_token.span,
                ty,
                value,
                span: start.to(semi.span),
                attrs,
            }),
            start.to(semi.span),
        ))
    }

    /// Parses `type Name { .. }`, `type Name = Other;`, or `union Name { .. }`.
    ///
    /// # One form, three meanings
    ///
    /// `type` and `union` declare the same thing with a different layout: the
    /// fields side by side, or all at one address. `type Name = Other;` is an
    /// alias, which introduces no new type at all -- `Name` *is* `Other` under
    /// another spelling.
    ///
    /// The three are one function because they share a name and a brace, and
    /// which one it is is decided by one token: `=` means an alias, and the
    /// keyword that led here says whether the braces describe a struct or a
    /// union.
    pub(super) fn parse_type_declaration(
        &mut self,
        stream: &mut TokenStream,
        ast: &mut Ast,
        prefix: DeclarationPrefix,
        start: Span,
        template_params: Vec<TemplateParam>,
        is_union: bool,
    ) -> Option<ItemId> {
        stream.advance(); // `type` or `union`
        let name_token = self.parse_ident(stream, "a type name")?;
        let name = self.token_text(stream, &name_token);

        // A *specialisation* writes the family's argument pattern after the
        // name: `type Box<int>` or `type Box<*T>`. An ordinary declaration
        // writes nothing there, so the `<` is unambiguous.
        let pattern = if stream.at(TokenKind::Punct(Punctuation::Lt)) {
            self.parse_type_pattern(stream, ast)?
        } else {
            Vec::new()
        };

        // `type Name;` / `union Name;` -- a forward declaration. Only the name
        // is known here; the fields come with the definition, and until then the
        // type is incomplete: a pointer or a reference to it, and nothing else.
        if stream.at(TokenKind::Punct(Punctuation::Semi)) {
            let semi = stream.advance();
            return Some(ast.builder().alloc_item(
                ItemKind::TypeDeclaration(TypeDecl {
                    template_params,
                    name,
                    name_span: name_token.span,
                    exported: prefix.exported,
                    fields: Vec::new(),
                    kind: if is_union {
                        TypeDeclKind::Union
                    } else {
                        TypeDeclKind::Struct
                    },
                    nested: Vec::new(),
                    pattern: pattern.clone(),
                    is_forward: true,
                    constants: Vec::new(),
                }),
                start.to(semi.span),
            ));
        }

        // `type Id = usize;` -- an alias names another type and stops there.
        if stream.eat(TokenKind::Punct(Punctuation::Eq)).is_some() {
            let target = self.parse_type(stream, ast)?;
            let semi = self.expect_punct(stream, Punctuation::Semi, "`;`")?;
            return Some(ast.builder().alloc_item(
                ItemKind::TypeDeclaration(TypeDecl {
                    template_params,
                    name,
                    name_span: name_token.span,
                    exported: prefix.exported,
                    fields: Vec::new(),
                    kind: TypeDeclKind::Alias(target),
                    nested: Vec::new(),
                    pattern: pattern.clone(),
                    is_forward: false,
                    constants: Vec::new(),
                }),
                start.to(semi.span),
            ));
        }

        let open = self.expect_punct(stream, Punctuation::OpenBrace, "`{` or `=`")?;
        let _ = open;
        let mut fields = Vec::new();
        let mut nested = Vec::new();
        let mut constants = Vec::new();
        while !stream.at(TokenKind::Punct(Punctuation::CloseBrace)) && !stream.at_eof() {
            let field_start = stream.current_span();
            // `NAME const Type = value;` -- a constant member. It is not a
            // field: it takes no storage, and every use folds to its value.
            // Inside a body an identifier followed by `const` can only start
            // this form, because a field writes its *type* in that position and
            // `const` is a keyword rather than a type name.
            if stream.at(TokenKind::Ident)
                && stream.peek_kind(1) == TokenKind::Keyword(Keyword::Const)
            {
                let member_token = self.parse_ident(stream, "a constant member's name")?;
                stream.advance(); // `const`
                let ty = self.parse_type(stream, ast)?;
                self.expect_punct(stream, Punctuation::Eq, "`=`")?;
                let value = self.parse_expr_no_comma(stream, ast)?;
                let semi = self.expect_punct(stream, Punctuation::Semi, "`;`")?;
                constants.push(ConstantMember {
                    name: self.token_text(stream, &member_token),
                    name_span: member_token.span,
                    ty,
                    value,
                    // A member has no export syntax of its own: the type's
                    // export decides whether the family and its answers travel.
                    exported: false,
                });
                let _ = field_start;
                let _ = semi;
                continue;
            }
            // `type Name = Other;` inside a body declares a type that belongs
            // to this one: `Array<int>.Iterator`. It is not a field and takes
            // no storage. Only the alias form is accepted for now; a nested
            // struct would need its own construction rules, which are not
            // specified.
            if stream.at(TokenKind::Keyword(Keyword::Type)) {
                let nested_start = stream.advance().span;
                let nested_token = self.parse_ident(stream, "a nested type name")?;
                if !stream.at(TokenKind::Punct(Punctuation::Eq)) {
                    self.report_unexpected(stream, "`=` (only a nested alias is specified)");
                    return None;
                }
                stream.advance();
                let target = self.parse_type(stream, ast)?;
                let semi = self.expect_punct(stream, Punctuation::Semi, "`;`")?;
                nested.push(TypeDecl {
                    template_params: Vec::new(),
                    name: self.token_text(stream, &nested_token),
                    name_span: nested_token.span,
                    // A nested type is as visible as its owner: the owner's
                    // export decides.
                    exported: false,
                    fields: Vec::new(),
                    kind: TypeDeclKind::Alias(target),
                    nested: Vec::new(),
                    pattern: Vec::new(),
                    is_forward: false,
                    constants: Vec::new(),
                });
                let _ = nested_start;
                let _ = semi;
                continue;
            }
            let field_token = self.parse_ident(stream, "a field name")?;
            let ty = self.parse_type(stream, ast)?;
            // `mutable` and `alignas(..)` are declaration modifiers, so they
            // come *after* the type, in either order: they say what the field's
            // storage is like, and the type says what it holds.
            let mut mutable = false;
            let mut align = None;
            loop {
                if stream.at(TokenKind::Keyword(Keyword::Mutable)) {
                    stream.advance();
                    mutable = true;
                } else {
                    break;
                }
            }
            // The attribute spelling: `a int @align(16);` in a type body. It
            // means what `alignas(16)` meant, written after the declaration.
            for attr in self.parse_attributes(stream, ast)? {
                match attr.name.as_str() {
                    "align" => match attr.arguments.first() {
                        Some(&alignment) => {
                            align = Some(fallc_ast::AlignSpec {
                                alignment,
                                span: attr.span,
                            });
                        }
                        None => self.diagnostics.push(
                            Diagnostic::error("`@align` needs an argument: `@align(16)`")
                                .with_code(DiagnosticCode::parser(2))
                                .with_span(attr.span),
                        ),
                    },
                    other => self.diagnostics.push(
                        Diagnostic::error(format!(
                            "`@{other}` is not an attribute of a field"
                        ))
                        .with_code(DiagnosticCode::parser(2))
                        .with_span(attr.span)
                        .with_note("a field takes `@align(N)` or `mutable`"),
                    ),
                }
            }
            let semi = self.expect_punct(stream, Punctuation::Semi, "`;`")?;
            fields.push(FieldDecl {
                name: self.token_text(stream, &field_token),
                name_span: field_token.span,
                ty,
                mutable,
                align,
                span: field_start.to(semi.span),
            });
        }
        let close = self.expect_punct(stream, Punctuation::CloseBrace, "`}`")?;

        Some(ast.builder().alloc_item(
            ItemKind::TypeDeclaration(TypeDecl {
                template_params,
                name,
                name_span: name_token.span,
                exported: prefix.exported,
                fields,
                nested,
                kind: if is_union {
                    TypeDeclKind::Union
                } else {
                    TypeDeclKind::Struct
                },
                pattern,
                is_forward: false,
                constants,
            }),
            start.to(close.span),
        ))
    }

    /// Parses the argument pattern of a specialisation, `<int>` or `<*T>`.
    ///
    /// Each entry is a *type*, because that is what the family's parameters
    /// are: `Box<*T>` says the argument is a pointer to whatever `T` becomes.
    fn parse_type_pattern(
        &mut self,
        stream: &mut TokenStream,
        ast: &mut Ast,
    ) -> Option<Vec<fallc_ast::TypeSyntax>> {
        self.expect_punct(stream, Punctuation::Lt, "`<`")?;
        let mut pattern = Vec::new();
        while !stream.at_gt() && !stream.at_eof() {
            pattern.push(self.parse_type(stream, ast)?);
            if stream.eat(TokenKind::Punct(Punctuation::Comma)).is_none() {
                break;
            }
        }
        self.expect_gt(stream)?;
        Some(pattern)
    }

    /// Parses `interface Name(it T) { it.method() -> Type, .. }`.
    ///
    /// The body is a trait's body: restrictions about the subject, separated by
    /// commas. An interface says which operations a type must have, not what
    /// they do -- and the ones whose result type is stated are also the methods
    /// a box can dispatch.
    pub(super) fn parse_interface(
        &mut self,
        stream: &mut TokenStream,
        ast: &mut Ast,
        exported: bool,
        start: Span,
        template_params: Vec<fallc_ast::TemplateParam>,
    ) -> Option<ItemId> {
        stream.advance(); // `interface`
        let name_token = self.parse_ident(stream, "an interface name")?;
        let name = self.token_text(stream, &name_token);

        // The parameter list is written the way a trait's is: `(it T)`. It is
        // the same declaration surface for the same idea -- a contract stated
        // about a type -- so a reader does not have to learn two.
        //
        // Each entry `name Type` declares what its *type* says, exactly as
        // `template <..>` reads it: a bare name is a type parameter (`it T`),
        // and anything else would be a value parameter, which an interface has
        // no use for -- the receiver is the value the interface holds.
        //
        // `interface IIterator<T>` is still accepted, because the angle form is
        // how a type family is written and an interface is a type. The
        // parenthesised form is the one the reference documents.
        // The parameter list as a trait writes it, kept when the body turns out
        // to be requirements: the same declaration then becomes a trait item,
        // because that is what an interface is -- one construct, one spelling.
        let mut written_params: Option<Vec<fallc_ast::TraitParam>> = None;
        let template_params = if stream.at(TokenKind::Punct(Punctuation::OpenParen)) {
            stream.advance();
            let mut params = Vec::new();
            let mut trait_params = Vec::new();
            let mut first = true;
            while !stream.at(TokenKind::Punct(Punctuation::CloseParen)) && !stream.at_eof() {
                let param_token = self.parse_ident(stream, "an interface parameter")?;
                let ty = self.parse_type(stream, ast)?;
                trait_params.push(fallc_ast::TraitParam {
                    name: self.token_text(stream, &param_token),
                    name_span: param_token.span,
                    ty: ty.clone(),
                });
                if !first {
                    // Every parameter after the first is a type parameter, named
                    // by the type written for its value: `interface IValue(it S,
                    // value T)` is `IValue<T>` at a use, exactly as
                    // `template <T type>` would be.
                    if let fallc_ast::TypeSyntax::Named(named) = &ty
                        && named.arguments.is_empty()
                    {
                        params.push(fallc_ast::TemplateParam {
                            name: named.path.clone(),
                            name_span: param_token.span,
                            kind: fallc_ast::TemplateParamKind::Type,
                            is_pack: false,
                            default: None,
                            span: param_token.span.to(named.span),
                        });
                    } else {
                        self.push_diagnostic(
                            Diagnostic::error(
                                "an interface parameter is a type parameter; write `it Name`",
                            )
                            .with_code(DiagnosticCode::parser(2))
                            .with_span(param_token.span),
                        );
                    }
                }
                first = false;
                if stream.eat(TokenKind::Punct(Punctuation::Comma)).is_none() {
                    break;
                }
            }
            self.expect_punct(stream, Punctuation::CloseParen, "`)`")?;
            written_params = Some(trait_params);
            params
        } else if stream.at(TokenKind::Punct(Punctuation::Lt)) {
            stream.advance(); // `<`
            let mut params = Vec::new();
            while !stream.at_gt() && !stream.at_eof() {
                let param_start = stream.current_span();
                let param_token = self.parse_ident(stream, "a template parameter name")?;
                let is_pack = if stream.at(TokenKind::Punct(Punctuation::DotDotDot)) {
                    stream.advance();
                    true
                } else {
                    false
                };
                // `T type` and a bare `T` both declare a type parameter; any
                // other type after the name is a value parameter's type.
                let kind = if stream.at(TokenKind::Keyword(Keyword::Type)) {
                    stream.advance();
                    fallc_ast::TemplateParamKind::Type
                } else if stream.at(TokenKind::Punct(Punctuation::Comma))
                    || stream.at_gt()
                {
                    fallc_ast::TemplateParamKind::Type
                } else {
                    fallc_ast::TemplateParamKind::Value(self.parse_type(stream, ast)?)
                };
                params.push(fallc_ast::TemplateParam {
                    name: self.token_text(stream, &param_token),
                    name_span: param_token.span,
                    kind,
                    is_pack,
                    default: None,
                    span: param_start.to(param_token.span),
                });
                if stream.eat(TokenKind::Punct(Punctuation::Comma)).is_none() {
                    break;
                }
            }
            self.expect_gt(stream)?;
            params
        } else {
            template_params
        };

        // `interface Name;` -- the name and nothing else. It may be named
        // behind a pointer or a reference until a definition arrives.
        if stream.at(TokenKind::Punct(Punctuation::Semi)) {
            let semi = stream.advance();
            let span = start.to(semi.span);
            return Some(ast.builder().alloc_item(
                ItemKind::Interface(fallc_ast::InterfaceDecl {
                    template_params,
                    name,
                    name_span: name_token.span,
                    subject: written_params.and_then(|params| params.into_iter().next()),
                    methods: Vec::new(),
                    requirements: Vec::new(),
                    exported,
                    is_forward: true,
                    span,
                }),
                span,
            ));
        }

        self.expect_punct(stream, Punctuation::OpenBrace, "`{`")?;
        // The body of an interface is the body of a trait: a list of
        // restrictions about the subject, written `it.next() -> int`. A
        // restriction that says what the call yields is also a method the box
        // can dispatch, and the table is derived from it below; one that does
        // not -- `it.begin() -> !void` -- is a check and nothing more, because
        // a table entry needs a result type to call with.
        //
        // A signature is not written here: `fn next() int;` and an interface
        // would then state the same thing twice, and the two statements could
        // disagree. The call states both -- that the method must exist, and
        // what it returns.
        let mut requirements = Vec::new();
        while !stream.at(TokenKind::Punct(Punctuation::CloseBrace)) && !stream.at_eof() {
            if written_params.is_none() {
                self.push_diagnostic(
                    Diagnostic::error(
                        "an interface writes its parameters as a trait does: write \
                         `interface Name(it T) { .. }`",
                    )
                    .with_code(DiagnosticCode::parser(2))
                    .with_span(stream.current_span())
                    .with_note(
                        "the parameter list introduces the subject's type variable, which the \
                         restrictions are written against",
                    ),
                );
                return None;
            }
            if stream.at(TokenKind::Keyword(Keyword::Fn)) {
                let fn_span = stream.current_span();
                self.push_diagnostic(
                    Diagnostic::error(
                        "an interface states a method by calling it: write `it.next() -> int`",
                    )
                    .with_code(DiagnosticCode::parser(2))
                    .with_span(fn_span)
                    .with_note(
                        "the call says the method has to exist, and `-> int` says what it \
                         returns; a separate signature could disagree with it",
                    ),
                );
                // Recover at the member's own edge, so the rest of the body is
                // still read as restrictions rather than as the wreckage of a
                // signature.
                while !stream.at_eof()
                    && !stream.at(TokenKind::Punct(Punctuation::Comma))
                    && !stream.at(TokenKind::Punct(Punctuation::CloseBrace))
                {
                    stream.advance();
                }
                if stream.eat(TokenKind::Punct(Punctuation::Comma)).is_none() {
                    break;
                }
                continue;
            }
            requirements.push(self.parse_requirement(stream, ast)?);
            if stream.eat(TokenKind::Punct(Punctuation::Comma)).is_none() {
                break;
            }
        }
        let close = self.expect_punct(stream, Punctuation::CloseBrace, "`}`")?;
        let span = start.to(close.span);
        let methods = self.interface_methods(ast, &requirements, written_params.as_deref());
        Some(ast.builder().alloc_item(
            ItemKind::Interface(fallc_ast::InterfaceDecl {
                template_params,
                name,
                name_span: name_token.span,
                subject: written_params.and_then(|params| params.into_iter().next()),
                methods,
                requirements,
                exported,
                is_forward: false,
                span,
            }),
            span,
        ))
    }

    /// The methods a box dispatches, derived from the restrictions that state
    /// one.
    ///
    /// A restriction is a call on the subject -- `it.next() -> int` -- and the
    /// table needs three things from it: the name, the parameter types, and the
    /// result. The name is the field the call selects; the parameters are the
    /// types its arguments name, because a restriction states a type where a
    /// call site passes a value; and the result is what `-> Type` says.
    ///
    /// `?!` marks a method that may raise, as it marks a call that passes an
    /// exception on: `it.save(String)?! -> void`. The annotation belongs to the
    /// method's type, so a concrete method that disagrees cannot stand in for
    /// it.
    ///
    /// A restriction that says no result type -- `it.begin() -> !void` -- is
    /// not a table entry. It is still worth checking, which is what boxing
    /// does; it is simply not callable through the box, because a call has to
    /// know what it gets back.
    fn interface_methods(
        &mut self,
        ast: &Ast,
        requirements: &[fallc_ast::TraitRequirement],
        subject: Option<&[fallc_ast::TraitParam]>,
    ) -> Vec<fallc_ast::InterfaceMethod> {
        let Some(subject) = subject.and_then(|params| params.first()) else {
            return Vec::new();
        };
        let mut methods: Vec<fallc_ast::InterfaceMethod> = Vec::new();
        // The source text of each entry's restriction, without whitespace. Two
        // spellings that mean one signature differ in their spans, so the text
        // is what says whether a repeated statement is the same one.
        let mut stated: Vec<String> = Vec::new();
        for requirement in requirements {
            let mut expr = requirement.expr;
            // `it.save(..)?!` -- the call passes an exception on, which is how
            // the restriction says the method may raise one.
            let mut throws = false;
            while let fallc_ast::ExprKind::Propagate(propagate) = &ast.expr(expr).kind {
                throws = true;
                expr = propagate.value;
            }
            let fallc_ast::ExprKind::Call(call) = &ast.expr(expr).kind else {
                continue;
            };
            // Only a call on the subject is a method of the type: `it != it`
            // and `InputIterator(type(it.begin()))` are statements about the
            // type, not members of it.
            let fallc_ast::ExprKind::Field(field) = &ast.expr(call.callee).kind else {
                continue;
            };
            let fallc_ast::ExprKind::Name(object) = &ast.expr(field.object).kind else {
                continue;
            };
            if object != &subject.name {
                continue;
            }
            let Some(fallc_ast::TraitResult::Exact(returns)) = &requirement.result else {
                continue;
            };
            // The parameters' types, which is what makes the call an entry of
            // the table. A value where a type belongs is reported here -- and
            // only here, because a restriction that states no result is a check
            // and needs no signature.
            let mut params = Vec::with_capacity(call.args.len());
            let mut written = true;
            for &argument in &call.args {
                match self.written_argument_type(ast, argument) {
                    Some(ty) => params.push(ty),
                    None => {
                        written = false;
                        break;
                    }
                }
            }
            if !written {
                continue;
            }
            // One entry per name: a second entry would put a second copy in the
            // table, and the two could disagree about what the call is. Stating
            // the same signature twice is harmless repetition; stating two
            // signatures for one method is a contradiction, and it is reported.
            if let Some(index) = methods.iter().position(|method| method.name == field.field) {
                let text = requirement.text.split_whitespace().collect::<String>();
                let same = stated.get(index).is_some_and(|first| *first == text);
                if !same {
                    self.push_diagnostic(
                        Diagnostic::error(format!(
                            "`{0}` is stated twice in one method table, with different \
                             signatures",
                            field.field
                        ))
                        .with_code(DiagnosticCode::parser(2))
                        .with_span(requirement.span)
                        .with_note(
                            "an interface states each method once: the call, its parameter \
                             types, and `-> Type` are the whole signature",
                        ),
                    );
                }
                continue;
            }
            methods.push(fallc_ast::InterfaceMethod {
                name: field.field.clone(),
                name_span: ast.expr(call.callee).span,
                params,
                returns: returns.clone(),
                throws,
                span: requirement.span,
            });
            stated.push(requirement.text.split_whitespace().collect::<String>());
        }
        methods
    }

    /// The type a restriction's argument names, which is a parameter's type.
    ///
    /// A restriction states types, so every spelling is a type: a name
    /// (`it.push(int)`), a module-qualified name, a written instance
    /// (`it.push(Array<int>)`), and `type(expr)`, which asks for the type of
    /// what it names. Anything else is reported: a value here would leave the
    /// method's signature unsaid.
    fn written_argument_type(
        &mut self,
        ast: &Ast,
        argument: fallc_ast::ExprId,
    ) -> Option<fallc_ast::TypeSyntax> {
        let node = ast.expr(argument);
        match &node.kind {
            // A type written where one belongs: `int`, `*int`, `[2]int`,
            // `fn(int) int`.
            fallc_ast::ExprKind::TypeLiteral(written) => Some((**written).clone()),
            fallc_ast::ExprKind::TypeOf(inner) => {
                self.written_argument_type(ast, *inner)
            }
            fallc_ast::ExprKind::Name(name) => Some(named_type(name, node.span)),
            fallc_ast::ExprKind::Field(field) => {
                // `std.String`: a type named through its module.
                let fallc_ast::ExprKind::Name(module) = &ast.expr(field.object).kind else {
                    self.report_value_argument(node.span);
                    return None;
                };
                Some(named_type(&format!("{module}.{}", field.field), node.span))
            }
            _ => {
                self.report_value_argument(node.span);
                None
            }
        }
    }

    /// Reports a restriction argument that is a value where a type belongs.
    fn report_value_argument(&mut self, span: Span) {
        self.push_diagnostic(
            Diagnostic::error("a method's parameter is written as its type")
                .with_code(DiagnosticCode::parser(2))
                .with_span(span)
                .with_note(
                    "the restriction states the signature a box calls, so \
                     `it.push(int) -> void` says the call takes an `int`",
                ),
        );
    }

    /// Parses `enum Name Base { A, B = 3, C }`.
    ///
    /// The base type is optional and defaults to `int`. A trailing comma is
    /// allowed, because a list written one member per line is easier to edit
    /// that way and a language that forbade it would be punishing the layout.
    pub(super) fn parse_enum(
        &mut self,
        stream: &mut TokenStream,
        ast: &mut Ast,
        prefix: DeclarationPrefix,
        start: Span,
        template_params: Vec<TemplateParam>,
    ) -> Option<ItemId> {
        stream.advance(); // `enum`
        let name_token = self.parse_ident(stream, "an enum name")?;
        let name = self.token_text(stream, &name_token);
        // The base type is whatever is not a brace. `enum Color { .. }` has
        // none, and `enum Color int { .. }` names `int`.
        let base = if stream.at(TokenKind::Punct(Punctuation::OpenBrace)) {
            None
        } else {
            Some(self.parse_type(stream, ast)?)
        };
        // `enum Name;` / `enum Name Base;` -- a forward declaration. The base
        // type is known, so values of the enum work immediately; the members
        // arrive with the definition.
        if stream.at(TokenKind::Punct(Punctuation::Semi)) {
            let semi = stream.advance();
            return Some(ast.builder().alloc_item(
                ItemKind::EnumDeclaration(EnumDecl {
                    template_params,
                    name,
                    name_span: name_token.span,
                    exported: prefix.exported,
                    base,
                    members: Vec::new(),
                    is_forward: true,
                    span: start.to(semi.span),
                }),
                start.to(semi.span),
            ));
        }
        self.expect_punct(stream, Punctuation::OpenBrace, "`{`")?;
        let mut members = Vec::new();
        while !stream.at(TokenKind::Punct(Punctuation::CloseBrace)) && !stream.at_eof() {
            let member_start = stream.current_span();
            let member_token = self.parse_ident(stream, "an enumerator name")?;
            // `GREEN = 5` says the value; without it the enumerator is one more
            // than the previous one, starting at zero.
            // Parsed *above* the comma operator: `,` separates the members
            // here, so it cannot also mean "evaluate and discard" -- reading a
            // value as a full expression swallowed the following member and
            // left the `}` with nothing to close.
            let value = if stream.eat(TokenKind::Punct(Punctuation::Eq)).is_some() {
                Some(self.parse_assignment(stream, ast)?)
            } else {
                None
            };
            let end = stream.current_span();
            members.push(EnumMember {
                name: self.token_text(stream, &member_token),
                name_span: member_token.span,
                value,
                span: member_start.to(end),
            });
            if stream.eat(TokenKind::Punct(Punctuation::Comma)).is_none() {
                break;
            }
        }
        let close = self.expect_punct(stream, Punctuation::CloseBrace, "`}`")?;
        Some(ast.builder().alloc_item(
            ItemKind::EnumDeclaration(EnumDecl {
                template_params,
                name,
                name_span: name_token.span,
                exported: prefix.exported,
                base,
                members,
                is_forward: false,
                span: start.to(close.span),
            }),
            start.to(close.span),
        ))
    }

    /// Parses one requirement: an operation, and optionally what its result has
    /// to be.
    ///
    /// Shared by a trait's body and an interface's, because the two declarations
    /// are one construct: `interface X(it T) { ++it, }` states what
    /// `traits X(it T) { ++it, }` states, and an interface is what a program
    /// writes now.
    pub(super) fn parse_requirement(
        &mut self,
        stream: &mut TokenStream,
        ast: &mut Ast,
    ) -> Option<fallc_ast::TraitRequirement> {
        let mut requirements = Vec::new();
        let requirement_start = stream.current_span();
        let arrow_was_result = std::mem::replace(&mut self.result_arrow_next, true);
        let requirement_was = std::mem::replace(&mut self.in_requirement, true);
        let expr = self.parse_expr_no_comma(stream, ast);
        self.in_requirement = requirement_was;
        self.result_arrow_next = arrow_was_result;
        let expr = expr?;
        let mut end = ast.expr(expr).span;
        // `-> !void` says the operation must yield something; `-> T` says
        // it must yield exactly `T`. `!` here negates the type, which is
        // why it is read before the type itself.
        let result = if stream.eat(TokenKind::Punct(Punctuation::Arrow)).is_some() {
            if let Some(bang) = stream.eat(TokenKind::Punct(Punctuation::Bang)) {
                let void_token = self.parse_ident(stream, "`void`")?;
                end = bang.span.to(void_token.span);
                Some(fallc_ast::TraitResult::NotVoid(bang.span.to(void_token.span)))
            } else if stream.at(TokenKind::Keyword(Keyword::True))
                || stream.at(TokenKind::Keyword(Keyword::False))
            {
                // `-> true`: the expression must *have* this value, which is
                // how a requirement that is itself a question is written.
                let token = stream.advance();
                end = token.span;
                Some(fallc_ast::TraitResult::Value(fallc_ast::TraitValue::Bool(
                    token.kind == TokenKind::Keyword(Keyword::True),
                )))
            } else if stream.at(TokenKind::Int) {
                let token = stream.advance();
                end = token.span;
                let text = self.token_text(stream, &token);
                let (digits, _) = split_numeric_suffix(&text);
                let value: i64 = integer_literal_value(digits)?;
                Some(fallc_ast::TraitResult::Value(fallc_ast::TraitValue::Int(value)))
            } else {
                let written = self.parse_type(stream, ast)?;
                end = written.span();
                Some(fallc_ast::TraitResult::Exact(written))
            }
        } else {
            None
        };
        let span = requirement_start.to(end);
        requirements.push(fallc_ast::TraitRequirement {
            expr,
            result,
            text: self.source_slice(stream, span),
            span,
        });

        requirements.into_iter().next()
    }

    /// Parses `traits Name(it Iter) : Base(Iter) { requirement, .. }`.
    ///
    /// The parameters are written like every other declaration: the value's
    /// name first, then its type. Their types name the trait's type variables,
    /// so `(it Iter)` both introduces `Iter` and gives `it` that type.
    ///
    /// A requirement is an *expression* -- the operation the type must have --
    /// with an optional `-> result` saying what that operation must yield.
    /// Requirements are separated by commas because they are a list rather than
    /// statements: nothing runs, and nothing here is executed in order.
    pub(super) fn parse_trait(
        &mut self,
        stream: &mut TokenStream,
        ast: &mut Ast,
        exported: bool,
        start: Span,
    ) -> Option<ItemId> {
        stream.advance(); // `traits`
        let name_token = self.parse_ident(stream, "a trait name")?;
        let name = self.token_text(stream, &name_token);
        self.expect_punct(stream, Punctuation::OpenParen, "`(`")?;
        let mut params = Vec::new();
        while !stream.at(TokenKind::Punct(Punctuation::CloseParen)) && !stream.at_eof() {
            let param_token = self.parse_ident(stream, "a trait parameter")?;
            let ty = self.parse_type(stream, ast)?;
            params.push(fallc_ast::TraitParam {
                name: self.token_text(stream, &param_token),
                name_span: param_token.span,
                ty,
            });
            if stream.eat(TokenKind::Punct(Punctuation::Comma)).is_none() {
                break;
            }
        }
        self.expect_punct(stream, Punctuation::CloseParen, "`)`")?;

        // `traits Name(it Iter);` -- a forward declaration: the name and its
        // parameters are known, the requirements arrive with the definition.
        // Asking a query of it before then is an incomplete trait.
        if stream.at(TokenKind::Punct(Punctuation::Semi)) {
            let semi = stream.advance();
            return Some(ast.builder().alloc_item(
                ItemKind::Trait(fallc_ast::TraitDecl {
                    exported,
                    name,
                    name_span: name_token.span,
                    params,
                    requirements: Vec::new(),
                    is_forward: true,
                    span: start.to(semi.span),
                }),
                start.to(semi.span),
            ));
        }
        self.expect_punct(stream, Punctuation::OpenBrace, "`{`")?;
        let mut requirements = Vec::new();
        while !stream.at(TokenKind::Punct(Punctuation::CloseBrace)) && !stream.at_eof() {
            requirements.push(self.parse_requirement(stream, ast)?);
            if stream.eat(TokenKind::Punct(Punctuation::Comma)).is_none() {
                break;
            }
        }
        let close = self.expect_punct(stream, Punctuation::CloseBrace, "`}`")?;
        Some(ast.builder().alloc_item(
            ItemKind::Trait(fallc_ast::TraitDecl {
                exported,
                name,
                name_span: name_token.span,
                params,
                requirements,
                is_forward: false,
                span: start.to(close.span),
            }),
            start.to(close.span),
        ))
    }

    /// Returns the source text a span covers, for a node that quotes itself.
    ///
    /// Trimmed, because a requirement's span starts at whatever whitespace the
    /// parser had not yet consumed, and a diagnostic that quotes a leading
    /// newline is unreadable.
    pub(super) fn source_slice(&self, stream: &TokenStream, span: Span) -> String {
        let _ = stream;
        self.source
            .as_ref()
            .and_then(|text| text.get(span.lo as usize..span.hi as usize))
            .map(|text| text.trim().to_owned())
            .unwrap_or_default()
    }

    /// Parses `import Name;`.
    ///
    /// The name is a module's name, which is its file's stem.
    /// Parses a name that may be written as an identifier or as a string.
    ///
    /// The two are equivalent, and the quoted form exists only so that a name
    /// which is not a legal identifier can still be written: a C library is
    /// `libgcc_s.so.1` as often as it is `m`, and a module file may be named
    /// for something the identifier rules exclude.
    ///
    /// Returns the name and the span it was written at.
    pub(super) fn parse_name_or_string(&mut self, stream: &mut TokenStream, what: &str) -> Option<(String, Span)> {
        if stream.at(TokenKind::String) {
            let token = stream.advance();
            let text = self.token_text(stream, &token);
            let name = text
                .strip_prefix('"')
                .and_then(|rest| rest.strip_suffix('"'))
                .unwrap_or(&text)
                .to_owned();
            return Some((name, token.span));
        }
        let token = self.parse_ident(stream, what)?;
        let mut name = self.token_text(stream, &token);
        let mut span = token.span;
        // A module may live in a directory, written as a dotted path:
        // `std.Panic` is the module `Panic` under `std`. The dots are kept, and
        // the loader turns them into directory separators.
        while stream.at(TokenKind::Punct(Punctuation::Dot)) {
            stream.advance();
            let part = self.parse_ident(stream, "a module name")?;
            name.push('.');
            name.push_str(&self.token_text(stream, &part));
            span = span.to(part.span);
        }
        Some((name, span))
    }

    pub(super) fn parse_import(&mut self, stream: &mut TokenStream, ast: &mut Ast) -> Option<ItemId> {
        let start = stream.current_span();
        stream.advance(); // `import`
        let (module, name_span) = self.parse_name_or_string(stream, "a module name")?;

        // `import C as Libc;` names the module something else here. The module
        // is still loaded under its own name; this is only the local spelling.
        let alias = if stream.at(TokenKind::Keyword(Keyword::As)) {
            stream.advance();
            let alias_token = self.parse_ident(stream, "a name for the module")?;
            Some(ImportedName {
                name: self.token_text(stream, &alias_token),
                span: alias_token.span,
            })
        } else {
            None
        };

        let semi = self.expect_punct(stream, Punctuation::Semi, "`;`")?;
        Some(ast.builder().alloc_item(
            ItemKind::Import(ImportItem {
                module,
                name_span,
                alias,
                exported: false,
                members: None,
            }),
            start.to(semi.span),
        ))
    }

    /// Parses `from Module import a, b;` or `from Module import *;`.
    ///
    /// The members are collected before the semicolon, so a trailing comma is
    /// accepted but a missing member is not: `from C import ;` has no meaning
    /// to fall back on.
    pub(super) fn parse_from_import(
        &mut self,
        stream: &mut TokenStream,
        ast: &mut Ast,
        exported: bool,
    ) -> Option<ItemId> {
        let start = stream.current_span();
        stream.advance(); // `from`
        let (module, name_span) = self.parse_name_or_string(stream, "a module name")?;

        if !stream.at(TokenKind::Keyword(Keyword::Import)) {
            self.report_unexpected(stream, "`import`");
            return None;
        }
        stream.advance(); // `import`

        // `*` brings in every exported name.
        let members = if stream.at_punct(Punctuation::Star) {
            stream.advance();
            ImportedMembers::All
        } else {
            let mut names = Vec::new();
            loop {
                let member_token = self.parse_ident(stream, "an exported name")?;
                names.push(ImportedName {
                    name: self.token_text(stream, &member_token),
                    span: member_token.span,
                });
                if !stream.at_punct(Punctuation::Comma) {
                    break;
                }
                stream.advance();
            }
            ImportedMembers::Named(names)
        };

        let semi = self.expect_punct(stream, Punctuation::Semi, "`;`")?;
        Some(ast.builder().alloc_item(
            ItemKind::Import(ImportItem {
                module,
                name_span,
                alias: None,
                exported,
                members: Some(members),
            }),
            start.to(semi.span),
        ))
    }

    /// Parses `link name dynamic from default;`.
    ///
    /// Three parts, all required: the library's name, whether it is shared or
    /// an archive, and where to find it. The last is the `from` clause that
    /// `from C import printf` also uses, meaning where a thing comes from; here
    /// it is either the `default` keyword or a directory in quotes.
    ///
    /// Nothing is optional because both omissions would be consequential: a
    /// library linked the wrong way is a bug that shows up at run time, not at
    /// compile time.
    pub(super) fn parse_link(&mut self, stream: &mut TokenStream, ast: &mut Ast) -> Option<ItemId> {
        let start = stream.current_span();
        stream.advance(); // `link`

        let name_quoted = stream.at(TokenKind::String);
        let (library, name_span) = self.parse_name_or_string(stream, "a library name")?;

        // `dynamic` is not a reserved word: the position after a library name
        // is the only place it can mean anything, so an identifier that says
        // `dynamic` there is the link kind, and a program may still name
        // something `dynamic` anywhere else.
        let kind = if stream.at(TokenKind::Keyword(Keyword::Dynamic)) {
            stream.advance();
            LinkKind::Dynamic
        } else if stream.at(TokenKind::Ident) && self.token_text(stream, stream.peek(0)) == "static"
        {
            // `static` is no longer a reserved word; in the link-kind position
            // it is read as the word it is, exactly as `dynamic` was before it
            // became reserved.
            stream.advance();
            LinkKind::Static
        } else {
            self.report_unexpected(stream, "`dynamic` or `static`");
            return None;
        };

        if !stream.at(TokenKind::Keyword(Keyword::From)) {
            self.report_unexpected(stream, "`from`");
            return None;
        }
        stream.advance();

        // `default` says to use the linker's own search path, which is where a
        // system library lives. A string names a directory instead.
        let location = if stream.at(TokenKind::Keyword(Keyword::Default)) {
            stream.advance();
            LinkLocation::Default
        } else if stream.at(TokenKind::String) {
            let token = stream.advance();
            let text = self.token_text(stream, &token);
            LinkLocation::Directory(
                text.strip_prefix('"')
                    .and_then(|rest| rest.strip_suffix('"'))
                    .unwrap_or(&text)
                    .to_owned(),
            )
        } else {
            self.report_unexpected(stream, "`default` or a directory in quotes");
            return None;
        };

        let semi = self.expect_punct(stream, Punctuation::Semi, "`;`")?;
        Some(ast.builder().alloc_item(
            ItemKind::Link(LinkItem {
                library,
                name_span,
                name_quoted,
                kind,
                location,
                span: start.to(semi.span),
            }),
            start.to(semi.span),
        ))
    }

    /// Returns `true` if the identifier at the cursor begins a type.
    ///
    /// A parameter is written `name Type`, so when the token after an
    /// identifier also begins a type the identifier was the name and this is
    /// the type. When it does not, the identifier *is* the type and the name
    /// was omitted.
    pub(super) fn ident_starts_a_type(&self, stream: &TokenStream) -> bool {
        // The parameter grammar is `name Type`, so an identifier is the *type*
        // exactly when nothing that could begin a type follows it: `double` on
        // its own, or `double,` with another parameter after. When something
        // that begins a type does follow, the identifier was the name, as in
        // `n double` or `fmt *const c8`.
        !matches!(
            stream.peek_kind(1),
            TokenKind::Ident
                | TokenKind::Keyword(Keyword::Auto)
                | TokenKind::Keyword(Keyword::Const)
                | TokenKind::Punct(Punctuation::Star)
                | TokenKind::Punct(Punctuation::StarStar)
                | TokenKind::Punct(Punctuation::Amp)
                // `&&T` is an rvalue reference, so it begins a type just as
                // `&T` does. Without this, `x &&T` read `x` as the *type* and
                // then failed on the `&&`.
                | TokenKind::Punct(Punctuation::AmpAmp)
                | TokenKind::Punct(Punctuation::OpenBracket)
                // `values ...Ts`: the dots start the type part, so the
                // identifier before them was the parameter's name.
                | TokenKind::Punct(Punctuation::DotDotDot)
                // `b type(a)` -- the type is an expression's, so the
                // identifier was the parameter's name.
                | TokenKind::Keyword(Keyword::Type)
                // `x ?int` -- an optional type begins with `?`.
                | TokenKind::Punct(Punctuation::Question)
        )
    }

}

/// A written type that is nothing but a name.
///
/// Used where a type is taken from an expression's text -- a restriction's
/// argument, `it.push(int)` -- because what was written is a name and a
/// parameter's type is what the name means.
fn named_type(path: &str, span: Span) -> fallc_ast::TypeSyntax {
    fallc_ast::TypeSyntax::Named(fallc_ast::NamedType {
        path: path.to_owned(),
        arguments: Vec::new(),
        is_expansion: false,
        span,
    })
}
