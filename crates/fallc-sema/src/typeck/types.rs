//! Written types, constants, and the folds and packs that need both.
//!
//! `resolve_type_syntax` turns a `TypeSyntax` into a `Type`; the constant
//! evaluator folds the expressions the language requires to be constant;
//! packs and folds are typed here because their lengths and element types are
//! the same kind of question.

use super::*;

impl TypeChecker {
    /// Resolves a written type annotation to a type.
    ///
    /// Unknown names stay [`Type::Unspecified`], so an unrecognised type is
    /// carried rather than rejected: user-defined types are not specified yet,
    /// so reporting them would be wrong.
    pub(super) fn resolve_written_type(&mut self, ty: &fallc_ast::TypeSyntax) -> Type {
        self.resolve_type_syntax(ty, false)
    }

    /// Resolves every `type(expr)` inside a written type.
    ///
    /// A type is resolved in places that have no expression tree to ask -- the
    /// machinery for nested and substituted types is deliberately ignorant of
    /// statements -- so the questions are answered here, where the tree is in
    /// hand, and remembered by span. `resolve_type_syntax` then only reads the
    /// answer.
    ///
    /// The walk covers the whole written type, which is what makes a nested
    /// use work: `Array<type(a)>`, `*type((a))`, `&const type(b)`.
    pub(super) fn prepare_type_of(
        &mut self,
        ty: &fallc_ast::TypeSyntax,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
    ) {
        use fallc_ast::TypeSyntax;
        match ty {
            TypeSyntax::TypeOf(node) => {
                // Answered once: the same written form is reached again when
                // the enclosing type is resolved.
                if self.type_of_types.contains_key(&node.span) {
                    return;
                }
                let declared = !node.parenthesized
                    && matches!(
                        &ast.expr(node.operand).kind,
                        ExprKind::Name(_) | ExprKind::Field(_)
                    );
                let found =
                    self.type_of_expression(ast, resolutions, types, node.operand, declared);
                let resolved = found.unwrap_or_else(|| Type::unspecified("type(..)", node.span));
                self.type_of_types.insert(node.span, resolved);
            }
            TypeSyntax::Array(array) => {
                self.prepare_type_of(&array.element, ast, resolutions, types);
            }
            TypeSyntax::UnsizedArray(inner)
            | TypeSyntax::Pointer(inner)
            | TypeSyntax::PointerToConst(inner)
            | TypeSyntax::ConstPointer(inner)
            | TypeSyntax::Reference(inner)
            | TypeSyntax::ConstReference(inner)
            | TypeSyntax::RvalueReference(inner)
            | TypeSyntax::ConstRvalueReference(inner)
            | TypeSyntax::Volatile(inner)
            | TypeSyntax::Const(inner) => {
                self.prepare_type_of(inner, ast, resolutions, types);
            }
            TypeSyntax::Named(named) => {
                for argument in &named.arguments {
                    if let fallc_ast::TemplateArgumentSyntax::Type(inner) = argument {
                        self.prepare_type_of(inner, ast, resolutions, types);
                    }
                }
            }
            // The parts of a function type are types, so a `type(expr)` inside
            // one is answered here like any other.
            TypeSyntax::Function(function) => {
                for parameter in &function.parameters {
                    self.prepare_type_of(parameter, ast, resolutions, types);
                }
                self.prepare_type_of(&function.returns, ast, resolutions, types);
            }
            // `auto` and `type(auto)` stand for a type deduced from the
            // initialiser, which the binding supplies; there is nothing to
            // prepare here.
            TypeSyntax::Auto(_) | TypeSyntax::DecltypeAuto(_) => {}
            TypeSyntax::Member(member) => {
                self.prepare_type_of(&member.outer, ast, resolutions, types);
            }
            TypeSyntax::Unresolved(_) => {}
        }
    }

    /// The type `type(expr)` names.
    ///
    /// With the operand written bare, an id-expression or a member access asks
    /// what that *entity* is, which is its declared type. Everything else asks
    /// what the *expression* is, and that includes how it may be used: an
    /// expression that names storage is a place, which the type system spells
    /// `&T`, and one that does not is a value.
    ///
    /// The operand is inferred exactly as it would be in the code, so a typo in
    /// it is reported once, where it was written.
    pub(super) fn type_of_expression(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        operand: fallc_ast::ExprId,
        declared: bool,
    ) -> Option<Type> {
        let previous = self.expected.take();
        let inferred = self.infer_expr(ast, resolutions, types, operand);
        self.expected = previous;
        let inferred = inferred?;
        if declared {
            return Some(inferred);
        }
        // A reference already *is* the type of the expression: `r` of type
        // `&T` asked as `type((r))` is `&T`, because folding another `&` onto
        // it would name a place that holds a reference.
        if inferred.is_reference() {
            return Some(inferred);
        }
        if self.expression_names_storage(ast, operand) {
            return Some(self.reference_to(inferred, false));
        }
        Some(inferred)
    }

    /// Whether an expression denotes storage rather than a value.
    ///
    /// A name, an element, a member and a dereference all do; a call and a
    /// construction do not. This is the same distinction `&` uses, asked
    /// without reporting anything, because a question about a type is allowed
    /// to be asked about an expression that has no address -- it then just
    /// answers with the value's type.
    pub(super) fn expression_names_storage(&self, ast: &Ast, expr: fallc_ast::ExprId) -> bool {
        match &ast.expr(expr).kind {
            ExprKind::Name(_) | ExprKind::Index(_) | ExprKind::Field(_) => true,
            ExprKind::Unary(unary) => unary.op == UnaryOp::Deref,
            ExprKind::Propagate(inner) => self.expression_names_storage(ast, inner.value),
            _ => false,
        }
    }

    /// The reference type that `&ty` would be, with reference collapsing.
    ///
    /// Folding a reference onto a reference is the reference it already was:
    /// `&` and `&&` may not stack, because a reference is an address and an
    /// address of an address is what a pointer is for. The rvalue kind survives
    /// only when both are rvalue references.
    pub(super) fn reference_to(&self, ty: Type, is_rvalue: bool) -> Type {
        match ty {
            Type::Reference {
                referent,
                referent_is_const,
                is_rvalue: existing,
            } => Type::Reference {
                referent,
                referent_is_const,
                is_rvalue: is_rvalue && existing,
            },
            other => Type::Reference {
                referent: Box::new(other),
                referent_is_const: false,
                is_rvalue,
            },
        }
    }

    /// Resolves a type that is nested inside another type.
    ///
    /// A `const` here qualifies the value rather than a binding, so it is kept
    /// as part of the type.
    pub(super) fn resolve_nested(&mut self, ty: &fallc_ast::TypeSyntax) -> Type {
        let previous = self.resolving_nested_type;
        self.resolving_nested_type = true;
        let resolved = self.resolve_type_syntax(ty, false);
        self.resolving_nested_type = previous;
        resolved
    }

    /// Resolves a written type to a checked type.
    ///
    /// `in_parameter` is set when resolving a function parameter, where `[]T`
    /// means `*T` rather than an array of unknown length.
    ///
    /// A `const` in a value position — the `byte` in `[N]const byte` — is part
    /// of the type, because it describes the value. A `const` on a binding is
    /// the binding's own immutability, recorded separately, so it resolves to
    /// the inner type.
    pub(super) fn resolve_type_syntax(
        &mut self,
        ty: &fallc_ast::TypeSyntax,
        in_parameter: bool,
    ) -> Type {
        let resolved = self.resolve_type_syntax_inner(ty, in_parameter);
        // Every written type is remembered under its own span, because code
        // generation resolves written types too -- it builds a symbol's name and
        // its LLVM type from them -- and a type whose shape depends on a folded
        // constant, like `[N][N]double`, cannot be re-derived there. This is the
        // one key the two stages share, so the checker publishes its answer.
        //
        // `auto` is left out: its answer comes from the body, and is filed
        // under the same span once the body has been checked.
        if !matches!(ty, fallc_ast::TypeSyntax::Auto(_)) {
            self.type_of_types.insert(ty.span(), resolved.clone());
        }
        resolved
    }

    /// Applies reference collapsing to a `&`/`&&` that was written.
    ///
    /// `T&&` where `T` is itself a reference is that reference -- `&` wins over
    /// `&&` -- and `&& &&` stays an rvalue reference. This is what makes a
    /// forwarding parameter mean "whatever the caller passed": an lvalue deduces
    /// `T` as `&U`, and `T&&` then collapses to `&U`.
    fn collapse_reference(&self, referent: Type, referent_is_const: bool, is_rvalue: bool) -> Type {
        match referent {
            Type::Reference {
                referent,
                referent_is_const: inner_const,
                is_rvalue: inner_rvalue,
            } => Type::Reference {
                referent,
                referent_is_const: inner_const || referent_is_const,
                is_rvalue: inner_rvalue && is_rvalue,
            },
            // `&const [N]T`: an array has no layer for a qualifier to sit on,
            // so a read-only reference to one says its *elements* are read-only.
            // Left on the reference, the `const` had nothing to constrain -- the
            // element type stayed writable, and `&const [4]T` did not match the
            // `[4]const T` an argument actually has.
            Type::Array { element, len } if referent_is_const => Type::Reference {
                referent: Box::new(Type::Array {
                    element: Box::new(match *element {
                        Type::Const(inner) => Type::Const(inner),
                        other => Type::Const(Box::new(other)),
                    }),
                    len,
                }),
                referent_is_const: false,
                is_rvalue,
            },
            other => Type::Reference {
                referent: Box::new(other),
                referent_is_const,
                is_rvalue,
            },
        }
    }

    pub(super) fn resolve_type_syntax_inner(
        &mut self,
        ty: &fallc_ast::TypeSyntax,
        in_parameter: bool,
    ) -> Type {
        use fallc_ast::TypeSyntax;
        match ty {
            TypeSyntax::Named(named) => {
                // An expansion is a *list* of types, not one, so it never
                // yields a single type. It is consumed where the list is wanted
                // -- the arguments of another template, which
                // `expand_type_arguments` gathers before anything is resolved.
                // Reaching here means a pack was used where one type belongs.
                if named.is_expansion {
                    self.diagnostics.push(
                        Diagnostic::error(format!(
                            "`{}...` expands to several types, so it cannot stand where one \
                             type is wanted",
                            named.path
                        ))
                        .with_code(DiagnosticCode::sema(71))
                        .with_span(named.span)
                        .with_note(
                            "an expansion belongs in the argument list of another template, as \
                             in `Box<Ts...>`",
                        ),
                    );
                    return Type::unspecified(named.path.clone(), named.span);
                }
                // A template parameter, if one is in scope. Checked first,
                // because it is not a declared type and never will be: `T`
                // outside an instantiation names nothing at all.
                if let Some(substitution) = &self.substitution
                    && let Some(ty) = substitution.type_of(&named.path)
                {
                    return ty.clone();
                }
                // `void` is written where a function returns nothing. It is not
                // a type — nothing can have it — so it becomes the same "no
                // value" the omitted form produces.
                if named.path == "void" {
                    return Type::unspecified(String::new(), named.span);
                }
                // A template body's names mean what its own module meant by
                // them, so its module's declarations are looked up first, and
                // only while a body from another module is being checked.
                if let Some(environment) = self.type_environments.last().cloned()
                    && let Some(found) = self.environment_type(&environment, named)
                {
                    return found;
                }
                // A declared type is looked up first, so that a program may
                // name a type after a built-in one. The placeholder `String`
                // is what made this necessary to get right: `type String` was
                // unreachable, because the built-in was found first and the
                // declaration's fields then did not exist.
                // `Array<int>` names a member of a family, which is built the
                // first time it is named. Checked before the plain table,
                // because a templated name has no entry there -- it declares
                // no type, only a way of making them.
                // A family named without arguments is a member too, when its
                // parameters all have defaults: `Box` beside `Box<int>`.
                // A family an import brought in steps aside for a type this
                // file declares under the same name: `type Box` in a program
                // that imported `std` is that program's `Box`, and the imported
                // family of the same name is not what the name means here.
                let shadowed = self.local_type_names.contains(&named.path)
                    && !self.local_families.contains(&named.path);
                if !shadowed
                    && (!named.arguments.is_empty()
                        || self.type_templates.contains_key(&named.path))
                    && let Some(member) = self.instantiate_type_template(named)
                {
                    return member;
                }
                // An alias is the type it names, so the lookup ends here
                // rather than producing a nominal type of its own.
                if let Some(target) = self.aliases.get(&named.path) {
                    return target.clone();
                }
                if let Some(nominal) = self.nominal_types.get(&named.path) {
                    return Type::Nominal(nominal.clone());
                }
                if let Some(primitive) = Type::from_name(&named.path) {
                    return primitive;
                }
                // A name that is neither declared nor primitive names no type.
                // It is reported rather than silently becoming an unspecified
                // type, because the unspecified type is compatible with
                // nothing -- so the program was rejected anyway, with a message
                // about the value rather than about the type name that was
                // never defined.
                //
                // Only when checking source, not while resolving a builtin's
                // signature: those are prose, and a probe for a type that does
                // not exist yet is how a forward reference is handled.
                // An empty name is how "no return type" is spelled, not a
                // missing type.
                // A name that a *type template* declared as a parameter is not
                // an unknown type: it is a parameter, whose meaning is the
                // member's arguments. It resolves to itself -- an unspecified
                // type carrying the name -- and the caller that knows the
                // arguments substitutes it.
                if self.type_template_parameter(&named.path) {
                    return Type::unspecified(named.path.clone(), named.span);
                }
                // A bare name that no declaration in scope introduces, used in
                // a template's own signature, is that template's parameter:
                // `template <T type> fn f(this &const T)` has no `T` to look up
                // until a call supplies one. The signature is a *pattern*, and
                // a pattern's variables are not looked up -- they are bound.
                if self.template_parameter_names.contains(&named.path) {
                    return Type::unspecified(named.path.clone(), named.span);
                }
                if !self.resolving_probe && !named.path.is_empty() && named.arguments.is_empty() {
                    self.diagnostics.push(
                        Diagnostic::error(format!("cannot find type `{}`", named.path))
                            .with_code(DiagnosticCode::sema(2))
                            .with_span(named.span)
                            .with_note(
                                "a type must be declared with `type` or imported before it \
                                 can be named",
                            ),
                    );
                }
                Type::unspecified(named.path.clone(), named.span)
            }
            TypeSyntax::Const(inner) => {
                // `const` has two jobs, and which one applies depends on where
                // it appears.
                //
                //   * On a *binding* — `x const int` — it says the binding
                //     cannot be reassigned. That is a property of the binding,
                //     recorded separately, so the type is the inner one.
                //   * On a *value type* — the `byte` in `[N]const byte` — it is
                //     part of the type, because it describes the value: those
                //     bytes cannot be written through, whoever holds them.
                //
                // The second case is recognised by context: when a const type
                // is the element of an array or the pointee of a pointer, it is
                // a value qualifier. When it is the whole declared type of a
                // binding, it is binding immutability.
                let inner_ty = self.resolve_type_syntax(inner, in_parameter);
                if !(in_parameter || self.resolving_nested_type) {
                    return inner_ty;
                }
                // An array has no layer for a qualifier to sit on: its elements
                // are the array. `const [4]int` therefore says the same thing
                // as `[4]const int`, and the two are one type. Pushing the
                // qualifier onto the element is what makes that true here
                // rather than in every comparison that would otherwise have to
                // know about it.
                match inner_ty.unqualified() {
                    Type::Array { element, len } => Type::Array {
                        // Already read-only stays read-only: `const [N]const T`
                        // and `[N]const T` are one type, and wrapping twice
                        // would spell it `const const`.
                        element: Box::new(match **element {
                            Type::Const(_) => (**element).clone(),
                            _ => Type::Const(element.clone()),
                        }),
                        len: *len,
                    },
                    _ => Type::Const(Box::new(inner_ty)),
                }
            }
            // `fn(int) int` is a function type. A parameter list is a parameter
            // list: `[]T` there means `*T`, exactly as it does in a
            // declaration, because the two describe the same calling
            // convention.
            TypeSyntax::Function(function) => Type::Function {
                parameters: function
                    .parameters
                    .iter()
                    .map(|parameter| self.resolve_type_syntax(parameter, true))
                    .collect(),
                returns: Box::new(self.resolve_type_syntax(&function.returns, false)),
                abi: function.abi.clone(),
                variadic: function.variadic,
                // `except` in the written type is part of it: `fn(int) int
                // except` and `fn(int) int` are two types, and a pointer to one
                // is not a pointer to the other.
                throws: function.throws,
            },
            TypeSyntax::Pointer(inner) => Type::Pointer {
                pointee: Box::new(self.resolve_nested(inner)),
                pointee_is_const: false,
                pointer_is_const: false,
            },
            TypeSyntax::PointerToConst(inner) => Type::Pointer {
                pointee: Box::new(self.resolve_type_syntax(inner, false)),
                pointee_is_const: true,
                pointer_is_const: false,
            },
            TypeSyntax::ConstPointer(inner) => Type::Pointer {
                pointee: Box::new(self.resolve_type_syntax(inner, false)),
                pointee_is_const: false,
                pointer_is_const: true,
            },
            TypeSyntax::Reference(inner) => {
                let referent = self.resolve_type_syntax(inner, false);
                self.collapse_reference(referent, false, false)
            }
            TypeSyntax::ConstReference(inner) => {
                let referent = self.resolve_type_syntax(inner, false);
                self.collapse_reference(referent, true, false)
            }
            // `&&T` and `&&const T`. An rvalue reference is a reference that
            // binds to a temporary, so it is a reference with one more fact
            // recorded rather than a different kind of indirection.
            TypeSyntax::RvalueReference(inner) => {
                let referent = self.resolve_type_syntax(inner, false);
                self.collapse_reference(referent, false, true)
            }
            TypeSyntax::ConstRvalueReference(inner) => {
                let referent = self.resolve_type_syntax(inner, false);
                self.collapse_reference(referent, true, true)
            }
            // `volatile T` is part of the type: which accesses are real
            // accesses is a property of the storage, and it has to survive
            // being passed around.
            TypeSyntax::Volatile(inner) => {
                Type::Volatile(Box::new(self.resolve_type_syntax(inner, in_parameter)))
            }
            TypeSyntax::UnsizedArray(inner) => {
                if in_parameter {
                    // `[]T` in a parameter list is `*T`, by definition -- and
                    // `[]const T` is `*const T`: the qualifier belongs to the
                    // pointee, exactly as it does in `*const T`. Resolving the
                    // element as a plain type dropped the `const`, so the
                    // callee could write through a pointer the caller had said
                    // was read-only.
                    let pointee_is_const = matches!(inner.as_ref(), TypeSyntax::Const(_));
                    let element = match inner.as_ref() {
                        TypeSyntax::Const(element) => self.resolve_type_syntax(element, false),
                        other => self.resolve_type_syntax(other, false),
                    };
                    Type::Pointer {
                        pointee: Box::new(element),
                        pointee_is_const,
                        pointer_is_const: false,
                    }
                } else {
                    Type::UnsizedArray(Box::new(self.resolve_nested(inner)))
                }
            }
            TypeSyntax::Array(array) => {
                let element = self.resolve_nested(&array.element);
                let len = self.const_eval_array_len(array.len);
                match len {
                    Some(len) => Type::Array {
                        element: Box::new(element),
                        len,
                    },
                    None => Type::unspecified("<invalid array length>", array.element.span()),
                }
            }
            // `type(expr)` was resolved where the expression was in hand and
            // recorded by span, because a written type is resolved in places
            // that have no tree to ask.
            TypeSyntax::TypeOf(node) => match self.type_of_types.get(&node.span) {
                Some(ty) => ty.clone(),
                None => {
                    self.diagnostics.push(
                        Diagnostic::error("`type(expr)` cannot be used here")
                            .with_code(DiagnosticCode::sema(84))
                            .with_span(node.span)
                            .with_note(
                                "the expression it names is checked where it is written; \
                                 write the type out, or use `type(expr)` in a local \
                                 declaration",
                            ),
                    );
                    Type::unspecified("type(..)".to_owned(), node.span)
                }
            },
            // `Outer<args>.Name`: the outer is resolved to a type, and the
            // member is looked up under the name its instantiation was
            // registered with. The *canonical* name is used, so the member of
            // `Array<int>` is found however the outer was spelled -- bare
            // here, `std.Array<int>` in a file that imported the umbrella.
            TypeSyntax::Member(member) => {
                let outer = self.resolve_type_syntax(&member.outer, in_parameter);
                let Some(name) = outer.nominal_name().map(str::to_owned) else {
                    self.diagnostics.push(
                        Diagnostic::error(format!(
                            "`{}` is not a declared type, so it has no members",
                            outer.spelling()
                        ))
                        .with_code(DiagnosticCode::sema(136))
                        .with_span(member.span),
                    );
                    return Type::unspecified(&member.name, member.span);
                };
                let qualified = format!("{name}.{}", member.name);
                if let Some(alias) = self.aliases.get(&qualified) {
                    return alias.clone();
                }
                if let Some(nominal) = self.nominal_types.get(&qualified) {
                    return Type::Nominal(nominal.clone());
                }
                self.diagnostics.push(
                    Diagnostic::error(format!("`{name}` has no type named `{}`", member.name))
                        .with_code(DiagnosticCode::sema(136))
                        .with_span(member.span)
                        .with_note("a nested type is declared inside the type that owns it"),
                );
                Type::unspecified(&member.name, member.span)
            }
            // `auto` resolves to whatever the binding, or the function's
            // `return`, deduced for it. The deduction is filed under the
            // written form's span, which is what lets a *call* to an
            // `auto`-return function know its type: this arm is reached from
            // `signature.return_type` at every call site.
            TypeSyntax::Auto(span) | TypeSyntax::DecltypeAuto(span) => self
                .type_of_types
                .get(span)
                .cloned()
                .or_else(|| self.auto_base.clone())
                .unwrap_or_else(|| Type::unspecified("auto", *span)),
            TypeSyntax::Unresolved(u) => Type::unspecified(u.text.clone(), u.span),
        }
    }

    /// Evaluates an array length expression at compile time.
    ///
    /// The length has to be one of the language's constants: an integer
    /// literal, a named constant, arithmetic on those, a `sizeof`/`alignof`, or
    /// a call to a `constexpr` function. Anything else is reported rather than
    /// folded, so the same source means the same thing under every compiler.
    pub(super) fn const_eval_array_len(&mut self, len: ExprId) -> Option<u64> {
        // The expression is not type checked here: it has no context, and a
        // length is a compile-time quantity rather than a runtime value.
        //
        // It *is* held to the language's rule for constants, though, which is
        // stricter than "the compiler can work this out": a call here has to be
        // to a `constexpr` function. Folding a plain function's call would make
        // the array's size depend on an optimisation the program never asked
        // for, and the same source would then mean different things under
        // different compilers.
        //
        // A bare name is the exception, because a template's `[N]T` writes one:
        // `N` is the instantiation's number, not a constant of the module, so it
        // is decided by the substitution rather than looked up. Before any
        // substitution exists -- a method is matched against its receiver before
        // its call is known -- the length is *undecided*, which is not the same
        // as wrong: the declaration says what a template's array looks like, and
        // the call is where a number arrives.
        let is_a_name = matches!(
            self.const_exprs.get(len.as_raw() as usize),
            Some(ConstExpr::Named { .. })
        );
        if is_a_name {
            return self
                .const_eval_int(len)
                .and_then(|value| u64::try_from(value).ok());
        }
        // A length this checker cannot see the shape of -- a module's own
        // template, whose expressions were indexed against another tree -- is
        // undecided rather than wrong while a template is what is being read.
        // The instantiation resolves it, and a length that is really not a
        // constant is reported then.
        if !self.template_parameter_names.is_empty() && self.const_eval_int(len).is_none() {
            return None;
        }
        // A length whose shape this checker cannot see -- a module's own
        // template, whose expressions were indexed against another tree -- is
        // undecided rather than wrong while a template is what is being read.
        // The instantiation resolves it, and a length that is really not a
        // constant is reported then.
        if self.const_eval_int(len).is_none() && !self.template_parameter_names.is_empty() {
            return None;
        }
        // An expression id outside this index belongs to *another* tree: an
        // exported template's signature is read again by an importer, and the
        // ids in it were numbered in the module that wrote it. The length is
        // undecided here and the instantiation decides it, which is the same
        // answer a bare name gets.
        if (len.as_raw() as usize) >= self.const_exprs.len() {
            return None;
        }
        if !self.require_constant(len, self.const_span(len), "an array length", 40) {
            return None;
        }
        match self.const_eval_int(len) {
            Some(value) if value >= 0 => Some(value as u64),
            Some(_) => {
                self.diagnostics.push(
                    Diagnostic::error("an array length cannot be negative")
                        .with_code(DiagnosticCode::sema(40))
                        .with_span(self.const_span(len)),
                );
                None
            }
            None => {
                self.diagnostics.push(
                    Diagnostic::error("an array length must be a constant integer")
                        .with_code(DiagnosticCode::sema(40))
                        .with_span(self.const_span(len)),
                );
                None
            }
        }
    }

    /// Returns the span of an expression, for a diagnostic raised before
    /// normal checking reaches it.
    pub(super) fn const_span(&self, expr: ExprId) -> fallc_span::Span {
        self.const_expr_spans
            .get(expr.as_raw() as usize)
            .copied()
            .unwrap_or(fallc_span::Span::SYNTHETIC)
    }

    /// Folds a constant integer expression.
    /// Why an expression may not be used where a constant is required, if it
    /// may not be.
    ///
    /// `None` means it is a constant. This is the *strict* question the language
    /// asks for an array length, a template argument, or the condition of an
    /// `if constexpr` -- and it differs from "can the compiler work this out":
    /// a function is a constant here only when it is declared `constexpr`, so
    /// that what a program may rely on is written down rather than inferred
    /// from how clever the compiler happens to be.
    pub(super) fn constant_expression_problem(&self, expr: ExprId) -> Option<ConstantProblem> {
        // A form already folded to a number is a constant: `sizeof`, an
        // `except(..)` query, and a call to a type family's predicate -- the
        // answer is known while compiling, which is what the position asks.
        //
        // A *function* call is different only in that the language asks for the
        // `constexpr` promise, so the folded value alone does not settle it and
        // the declaration is consulted below. That is why the table is
        // consulted first but not trusted for a call.
        let folded = self
            .const_expr_spans
            .get(expr.as_raw() as usize)
            .is_some_and(|span| self.constant_values.contains_key(span));
        let node = self.const_exprs.get(expr.as_raw() as usize)?;
        match node {
            ConstExpr::Call { callee, args } => {
                // A call is constant through a `constexpr` function, or through
                // a type family's predicate, whose answer the fold already has.
                let Some(function) = self.evaluable_functions.get(callee) else {
                    return if self.constants_by_name.contains_key(callee) || folded {
                        None
                    } else {
                        Some(ConstantProblem::NotAFunction(callee.clone()))
                    };
                };
                if !function.is_constexpr {
                    return Some(ConstantProblem::NotConstexpr(callee.clone()));
                }
                // Every argument must itself be constant, by this same rule:
                // `constexpr int f(int)` called with a runtime value is not a
                // constant, and the diagnostic says which argument was the
                // problem.
                for (index, arg) in args.iter().enumerate() {
                    if let Some(problem) = self.constant_expression_problem(*arg) {
                        return Some(ConstantProblem::Argument {
                            index,
                            callee: callee.clone(),
                            inner: Box::new(problem),
                        });
                    }
                }
                None
            }
            ConstExpr::Named { name, .. } => {
                // A template's value parameter is a constant by declaration:
                // `template <N usize>` says the caller writes a number for it,
                // which is what makes `[N]T` a length and not a variable. Its
                // number is decided per instantiation, so the check is about
                // what the name *is*, not about what it currently stands for.
                if self.constants_by_name.contains_key(name)
                    || self.template_value_types.contains_key(name)
                {
                    None
                } else {
                    Some(ConstantProblem::NotAConstantName(name.clone()))
                }
            }
            // Anything with a value the compiler can fold is constant.
            _ => {
                if self.const_eval_int(expr).is_some() {
                    None
                } else {
                    Some(ConstantProblem::NotFoldable)
                }
            }
        }
    }

    /// Reports an expression that may not stand where a constant is required.
    ///
    /// Returns `true` when it is a constant. Every such position goes through
    /// this, so the rule -- and the message explaining it -- is stated once.
    pub(super) fn require_constant(
        &mut self,
        expr: ExprId,
        span: fallc_span::Span,
        what: &str,
        code: u16,
    ) -> bool {
        let Some(problem) = self.constant_expression_problem(expr) else {
            return true;
        };
        let (message, note) = match &problem {
            ConstantProblem::NotConstexpr(name) => (
                format!("`{name}` is not `constexpr`, so it cannot be {what}"),
                format!(
                    "declare it `constexpr fn {name}(..)` to promise the value is known while \
                     compiling, or compute it into a `const` first",
                ),
            ),
            ConstantProblem::Argument {
                index,
                callee,
                inner,
            } => {
                let why = match &**inner {
                    ConstantProblem::NotConstexpr(name) => {
                        format!("`{name}` is not `constexpr`")
                    }
                    ConstantProblem::NotFoldable => "it is not a constant".to_owned(),
                    ConstantProblem::NotAConstantName(name) => {
                        format!("`{name}` is not a constant")
                    }
                    _ => "it is not a constant".to_owned(),
                };
                (
                    format!(
                        "argument {} of `{callee}` is not a constant: {why}, so it cannot be \
                         {what}",
                        index + 1,
                    ),
                    "every argument of a `constexpr` call has to be a constant too".to_owned(),
                )
            }
            ConstantProblem::NotAFunction(name) => (
                format!("`{name}` is not a function this compiler can evaluate"),
                "a constant expression may only call functions whose bodies it can run".to_owned(),
            ),
            ConstantProblem::NotAConstantName(name) => (
                format!("`{name}` is not a constant, so it cannot be {what}"),
                "a constant expression is known before the program runs".to_owned(),
            ),
            ConstantProblem::NotFoldable => (
                format!("{what} must be a constant"),
                "a constant expression is known before the program runs".to_owned(),
            ),
        };
        self.diagnostics.push(
            Diagnostic::error(message)
                .with_code(DiagnosticCode::sema(code))
                .with_span(span)
                .with_note(note),
        );
        false
    }

    /// Whether an initialiser can be part of the program image.
    ///
    /// A literal is one, and so is anything the constant folder can work out:
    /// a named constant, arithmetic on those, a `sizeof`, a fold over a pack, a
    /// call the compiler can evaluate. A designated initialiser is one when
    /// every element is one, and so is a nested one.
    pub(super) fn initializer_is_constant(
        &mut self,
        ast: &Ast,
        initializer: &fallc_ast::Initializer,
    ) -> bool {
        match initializer {
            fallc_ast::Initializer::Expr(expr) => self.expr_is_constant(ast, *expr),
            fallc_ast::Initializer::Designated(init) => init
                .elements
                .iter()
                .all(|element| self.expr_is_constant(ast, *element)),
        }
    }

    /// Whether one expression can be computed before the program runs.
    pub(super) fn expr_is_constant(&mut self, ast: &Ast, expr: ExprId) -> bool {
        use fallc_ast::ExprKind;
        let node = ast.expr(expr);
        match &node.kind {
            // A literal is its value.
            ExprKind::IntLiteral(_)
            | ExprKind::BoolLiteral(_)
            | ExprKind::CharLiteral(_)
            | ExprKind::FloatLiteral(_)
            | ExprKind::StringLiteral(_)
            | ExprKind::ByteStringLiteral(_)
            | ExprKind::Sizeof(_)
            | ExprKind::Alignof(_)
            | ExprKind::SizeofPack(_) => true,
            // A coroutine's suspension point is a run-time fact about the
            // program, never a constant.
            ExprKind::Yield(_) => false,
            // The sign of a literal is not a run-time operation.
            ExprKind::Unary(unary)
                if matches!(unary.op, fallc_ast::UnaryOp::Neg | fallc_ast::UnaryOp::Plus) =>
            {
                self.expr_is_constant(ast, unary.operand)
            }
            ExprKind::NestedInit(init) => init
                .elements
                .iter()
                .all(|element| self.expr_is_constant(ast, *element)),
            _ => {
                // Everything else follows the rule that governs every position
                // where a constant is required: a named constant, arithmetic on
                // constants, a pack fold, or a call through a `constexpr`
                // function. A call the compiler merely *could* work out is not
                // enough, because the initialiser has to be emitted as part of
                // the program image rather than evaluated when the program runs.
                if self.constant_expression_problem(expr).is_some() {
                    return false;
                }
                // The value is recorded where code generation reads it.
                if let Some(value) = self.const_eval_int(expr) {
                    self.constant_values.insert(node.span, value);
                }
                true
            }
        }
    }

    pub(super) fn const_eval_int(&self, expr: ExprId) -> Option<i64> {
        // A value the check already folded for this very expression -- a
        // `constexpr` template's call is the case: the body was folded while it
        // was instantiated, and the number is recorded at the call's span.
        if let Some(span) = self.const_expr_spans.get(expr.as_raw() as usize)
            && let Some(value) = self.constant_values.get(span)
        {
            return Some(*value);
        }
        let node = self.const_exprs.get(expr.as_raw() as usize)?.clone();
        match node {
            ConstExpr::Int(value) => Some(value),
            ConstExpr::Neg(inner) => self.const_eval_int(inner).map(|v| -v),
            ConstExpr::Not(inner) => self.const_eval_int(inner).map(|v| !v),
            ConstExpr::LogicalNot(inner) => self.const_eval_int(inner).map(|v| i64::from(v == 0)),
            ConstExpr::Binary { op, lhs, rhs } => {
                let lhs = self.const_eval_int(lhs)?;
                let rhs = self.const_eval_int(rhs)?;
                match op {
                    BinaryOp::Add => lhs.checked_add(rhs),
                    BinaryOp::Sub => lhs.checked_sub(rhs),
                    BinaryOp::Mul => lhs.checked_mul(rhs),
                    BinaryOp::Div => lhs.checked_div(rhs),
                    BinaryOp::Rem => lhs.checked_rem(rhs),
                    // A shift by a negative or oversized amount is not a
                    // constant this compiler will fold.
                    BinaryOp::Shl => u32::try_from(rhs).ok().and_then(|s| lhs.checked_shl(s)),
                    BinaryOp::Shr => u32::try_from(rhs).ok().and_then(|s| lhs.checked_shr(s)),
                    BinaryOp::BitAnd => Some(lhs & rhs),
                    BinaryOp::BitOr => Some(lhs | rhs),
                    BinaryOp::BitXor => Some(lhs ^ rhs),
                    _ => None,
                }
            }
            // Only the selected branch is folded, which is the whole point of
            // the operator: the other branch may not even be a constant.
            ConstExpr::Conditional {
                condition,
                then_value,
                else_value,
            } => {
                if self.const_eval_int(condition)? != 0 {
                    self.const_eval_int(then_value)
                } else {
                    self.const_eval_int(else_value)
                }
            }
            // Short-circuiting, for the same reason the conditional folds one
            // branch: the operand the answer does not need may divide by zero.
            ConstExpr::Logical { is_and, lhs, rhs } => {
                let lhs = self.const_eval_int(lhs)?;
                if is_and {
                    if lhs == 0 {
                        Some(0)
                    } else {
                        Some(i64::from(self.const_eval_int(rhs)? != 0))
                    }
                } else if lhs != 0 {
                    Some(1)
                } else {
                    Some(i64::from(self.const_eval_int(rhs)? != 0))
                }
            }
            ConstExpr::Compare { op, lhs, rhs } => {
                let lhs = self.const_eval_int(lhs)?;
                let rhs = self.const_eval_int(rhs)?;
                let result = match op {
                    BinaryOp::Eq => lhs == rhs,
                    BinaryOp::Ne => lhs != rhs,
                    BinaryOp::Lt => lhs < rhs,
                    BinaryOp::Le => lhs <= rhs,
                    BinaryOp::Gt => lhs > rhs,
                    BinaryOp::Ge => lhs >= rhs,
                    _ => return None,
                };
                // 0 or 1, as any condition is.
                Some(i64::from(result))
            }
            ConstExpr::Sizeof(ty) => {
                let ty = self.resolve_const_type(&ty);
                self.size_of(&ty)
            }
            ConstExpr::Alignof(ty) => {
                let ty = self.resolve_const_type(&ty);
                self.align_of(&ty)
            }
            // A pack is a list of compile-time values, so counting or
            // combining one is a constant like any other arithmetic.
            ConstExpr::SizeofPack(pack) => Some(self.pack_elements_quiet(&pack.pack)?.len() as i64),
            ConstExpr::Fold(fold) => self.const_fold(&fold),
            ConstExpr::Named { name, .. } => {
                // A template value parameter in scope is a constant: `n * 3`
                // inside a default argument folds with the `n` the caller gave,
                // which is what lets one default use the parameters before it.
                if let Some(crate::template::TemplateArgument::Int(value)) = self
                    .substitution
                    .as_ref()
                    .and_then(|substitution| substitution.get(&name))
                {
                    return Some(*value);
                }
                let expr = self.constants.get(&name).copied()?;
                self.const_eval_int(expr)
            }
            ConstExpr::Call { callee, args } => {
                // *Folding* is an optimisation, and it does not ask whether the
                // function is marked `constexpr`: a call the compiler can work
                // out is worth working out whatever the declaration says. What
                // the marker controls is whether the call may stand where the
                // language *requires* a constant, which
                // `constant_expression_problem` decides.
                let mut values = Vec::with_capacity(args.len());
                for arg in args {
                    values.push(self.const_eval_int(arg)?);
                }
                let callee = self.evaluable_functions.get(&callee)?;
                crate::interp::eval_call(
                    &callee.ast,
                    callee.body,
                    &callee.params,
                    &callee.defaults,
                    &values,
                    &self.evaluable_functions,
                    &self.constants_by_name,
                )
            }
            ConstExpr::Unknown => None,
        }
    }

    /// Folds a parameter pack, in the form written.
    ///
    /// The two unary forms differ only in which end the combination starts
    /// from, and the two binary ones also take a starting value. An empty pack
    /// has no elements to combine, so a unary fold needs the operator's
    /// identity: `&&` is true, `||` is false, and `,` is void -- zero here,
    /// because a fold this evaluator produces is an integer. Any other
    /// operator is ill-formed, because the language states no identity for it.
    pub(super) fn const_fold(&self, fold: &fallc_ast::FoldExpr) -> Option<i64> {
        let elements = self.pack_elements_quiet(&fold.pack.pack)?;
        let mut values: Vec<i64> = Vec::with_capacity(elements.len());
        for element in &elements {
            let crate::template::TemplateArgument::Int(value) = element else {
                return None;
            };
            values.push(*value);
        }
        let init = match fold.init {
            Some(expr) => Some(self.const_eval_int(expr)?),
            None => None,
        };
        use fallc_ast::FoldKind;
        if values.is_empty() {
            return match fold.kind {
                FoldKind::UnaryRight | FoldKind::UnaryLeft => fold_identity(fold.op),
                FoldKind::BinaryRight | FoldKind::BinaryLeft => init,
            };
        }
        match fold.kind {
            FoldKind::UnaryRight => {
                let mut total = *values.last()?;
                for value in values.iter().rev().skip(1) {
                    total = apply_binary_op(fold.op, *value, total)?;
                }
                Some(total)
            }
            FoldKind::UnaryLeft => {
                let mut total = values[0];
                for value in &values[1..] {
                    total = apply_binary_op(fold.op, total, *value)?;
                }
                Some(total)
            }
            FoldKind::BinaryRight => {
                let mut total = init?;
                for value in values.iter().rev() {
                    total = apply_binary_op(fold.op, *value, total)?;
                }
                Some(total)
            }
            FoldKind::BinaryLeft => {
                let mut total = init?;
                for value in &values {
                    total = apply_binary_op(fold.op, total, *value)?;
                }
                Some(total)
            }
        }
    }

    /// Returns `true` if a written type mentions a parameter of the family it
    /// belongs to.
    ///
    /// A method of a type family is written against the *family's* parameters
    /// -- `fn push(this &Array<T>, value T)` -- and its receiver is the only
    /// sign that it belongs to a family. The parameter has to belong to the
    /// family the receiver *names*: `&A` is a reference to the declared type
    /// `A`, even when some unrelated family in the program happens to have a
    /// parameter called `A`. Asking every template in the program made a
    /// function with such a receiver look like a family method, so its
    /// definition was never emitted and calls to it failed in code generation.
    /// Whether a written type has an array whose length is a name.
    ///
    /// A name there is either a value parameter of the enclosing template or a
    /// named constant; only the first can be undecidable while a method is
    /// being matched, and the caller treats "undecidable" as "the family
    /// matches, the instantiation will confirm the shape".
    /// Whether a written type names a template's type parameter.
    ///
    /// Such a name is a *pattern variable*: `template <T type> fn f(this
    /// &const T)` says the receiver may be any type, and the call supplies it.
    /// The name is read from the program's own list of parameters, because a
    /// signature resolved before its instantiation exists has no other way to
    /// tell a pattern variable from a misspelt type.
    pub(super) fn written_names_a_template_parameter(&self, ty: &fallc_ast::TypeSyntax) -> bool {
        use fallc_ast::TypeSyntax;
        match ty {
            TypeSyntax::Named(named) => self.template_parameter_names.contains(&named.path),
            // `[N]T` has a length that may be a value parameter, which is one
            // of the same template's parameters under another kind.
            TypeSyntax::Array(array) => {
                // A length this checker cannot read belongs to another tree --
                // an imported template's signature is written in the module that
                // declared it, and its ids were numbered there. Such a length is
                // a parameter by construction: a fixed one would have been
                // folded before the signature was published.
                let unknown_length = (array.len.as_raw() as usize) >= self.const_exprs.len();
                (unknown_length
                    || matches!(
                        self.const_exprs.get(array.len.as_raw() as usize),
                        Some(ConstExpr::Named { name, .. })
                            if self.template_parameter_names.contains(name)
                                || self.template_value_types.contains_key(name)
                    ))
                    || self.written_names_a_template_parameter(&array.element)
            }
            TypeSyntax::UnsizedArray(inner)
            | TypeSyntax::Pointer(inner)
            | TypeSyntax::PointerToConst(inner)
            | TypeSyntax::ConstPointer(inner)
            | TypeSyntax::Reference(inner)
            | TypeSyntax::ConstReference(inner)
            | TypeSyntax::RvalueReference(inner)
            | TypeSyntax::ConstRvalueReference(inner)
            | TypeSyntax::Volatile(inner)
            | TypeSyntax::Const(inner) => self.written_names_a_template_parameter(inner),
            _ => false,
        }
    }

    pub(super) fn written_mentions_a_value_parameter(&self, ty: &fallc_ast::TypeSyntax) -> bool {
        use fallc_ast::TypeSyntax;
        match ty {
            TypeSyntax::Array(array) => {
                matches!(
                    self.const_exprs.get(array.len.as_raw() as usize),
                    Some(ConstExpr::Named { .. })
                ) || self.written_mentions_a_value_parameter(&array.element)
            }
            TypeSyntax::UnsizedArray(inner)
            | TypeSyntax::Pointer(inner)
            | TypeSyntax::PointerToConst(inner)
            | TypeSyntax::ConstPointer(inner)
            | TypeSyntax::Reference(inner)
            | TypeSyntax::ConstReference(inner)
            | TypeSyntax::RvalueReference(inner)
            | TypeSyntax::ConstRvalueReference(inner)
            | TypeSyntax::Volatile(inner)
            | TypeSyntax::Const(inner) => self.written_mentions_a_value_parameter(inner),
            _ => false,
        }
    }

    pub(super) fn mentions_template_parameter_syntax(&self, ty: &fallc_ast::TypeSyntax) -> bool {
        use fallc_ast::TypeSyntax;
        match ty {
            TypeSyntax::Named(named) => {
                // The family this type names, if it names one. Only *its*
                // parameters can be in scope in a declaration written against
                // it.
                //
                // A family is registered under the name its own module gave it,
                // and named from elsewhere by that name qualified with the
                // module: `Device` here is `libdev.Device` there. Both spellings
                // name one family, so the last segment is tried when the whole
                // path is not a key.
                let family = self.type_templates.get(&named.path).or_else(|| {
                    let last = named.path.rsplit('.').next().unwrap_or(&named.path);
                    self.type_templates
                        .values()
                        .find(|family| family.name == last)
                });
                let names_family_parameter = |argument: &TypeSyntax| {
                    let TypeSyntax::Named(inner) = argument else {
                        return false;
                    };
                    // A parameter is a parameter whether it was declared with a
                    // type or with a value: `Device<Mode>` writes a *value*
                    // parameter's name inside a type, and the declaration of the
                    // family is the only place that says which kind it is. The
                    // checker decides the kind when it substitutes.
                    family.is_some_and(|family| {
                        family
                            .template_params
                            .iter()
                            .any(|param| param.name == inner.path)
                    })
                };
                if named.arguments.iter().any(|argument| match argument {
                    fallc_ast::TemplateArgumentSyntax::Type(inner) => names_family_parameter(inner),
                    fallc_ast::TemplateArgumentSyntax::Value(_) => false,
                    fallc_ast::TemplateArgumentSyntax::Named { .. } => false,
                }) {
                    return true;
                }
                named.arguments.iter().any(|argument| match argument {
                    fallc_ast::TemplateArgumentSyntax::Type(inner) => {
                        self.mentions_template_parameter_syntax(inner)
                    }
                    fallc_ast::TemplateArgumentSyntax::Value(_) => false,
                    fallc_ast::TemplateArgumentSyntax::Named { .. } => false,
                })
            }
            TypeSyntax::Array(array) => self.mentions_template_parameter_syntax(&array.element),
            TypeSyntax::Function(function) => {
                function
                    .parameters
                    .iter()
                    .any(|parameter| self.mentions_template_parameter_syntax(parameter))
                    || self.mentions_template_parameter_syntax(&function.returns)
            }
            TypeSyntax::UnsizedArray(inner)
            | TypeSyntax::Pointer(inner)
            | TypeSyntax::PointerToConst(inner)
            | TypeSyntax::ConstPointer(inner)
            | TypeSyntax::Reference(inner)
            | TypeSyntax::ConstReference(inner)
            | TypeSyntax::RvalueReference(inner)
            | TypeSyntax::ConstRvalueReference(inner)
            | TypeSyntax::Volatile(inner)
            | TypeSyntax::Const(inner) => self.mentions_template_parameter_syntax(inner),
            TypeSyntax::Member(member) => self.mentions_template_parameter_syntax(&member.outer),
            TypeSyntax::TypeOf(_)
            | TypeSyntax::DecltypeAuto(_)
            | TypeSyntax::Auto(_)
            | TypeSyntax::Unresolved(_) => false,
        }
    }

    /// Returns `true` if a type mentions one whose name did not resolve.
    ///
    /// A template parameter is the case: it resolves to an unspecified type
    /// carrying its name, and it is nested inside the pointers and references
    /// around it -- `&Array<T>` is a reference to one.
    pub(super) fn mentions_unspecified(&self, ty: &Type) -> bool {
        match ty {
            // `void` is the empty spelling of "no value", not a name waiting to
            // be resolved: `Coroutine<void, void, void>` is a settled type, and
            // treating it as unresolved skipped its member instantiation -- so
            // a coroutine had no destructor and leaked its frame.
            Type::Unspecified(unspecified) => !unspecified.text.is_empty(),
            Type::Pointer { pointee, .. }
            | Type::Reference {
                referent: pointee, ..
            }
            | Type::Volatile(pointee)
            | Type::Const(pointee)
            | Type::Array {
                element: pointee, ..
            }
            | Type::UnsizedArray(pointee) => self.mentions_unspecified(pointee),
            _ => false,
        }
    }

    /// Whether a template argument is still a parameter rather than a type.
    ///
    /// A member built from such an argument is a mention inside the family's
    /// own body, not an instantiation, and nothing can be emitted for it.
    pub(super) fn template_argument_is_unresolved(
        &self,
        argument: &crate::template::TemplateArgument,
    ) -> bool {
        match argument {
            crate::template::TemplateArgument::Type(ty) => self.mentions_unspecified(ty),
            crate::template::TemplateArgument::Pack(elements) => elements
                .iter()
                .any(|element| self.template_argument_is_unresolved(element)),
            _ => false,
        }
    }

    /// Returns the elements of the named pack, reporting if the name is not
    /// bound to one.
    ///
    /// A pack is the only thing in the language that holds a variable number
    /// of compile-time items, and it lives in the substitution: an
    /// instantiation is where `values` is a list, and everywhere else it is a
    /// name nothing binds.
    pub(super) fn pack_elements(
        &mut self,
        pack: &fallc_ast::PackRef,
        wanted: &str,
    ) -> Option<Vec<crate::template::TemplateArgument>> {
        match self.pack_elements_quiet(&pack.pack) {
            Some(elements) => Some(elements),
            None => {
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "`{}` is not a template parameter pack, so it cannot be {wanted}",
                        pack.pack
                    ))
                    .with_code(DiagnosticCode::sema(80))
                    .with_span(pack.pack_span)
                    .with_note(
                        "a pack is a list of compile-time values, as in \
                         `template <values ...int>`",
                    ),
                );
                None
            }
        }
    }

    /// Returns the elements of the named pack, or `None` without reporting.
    ///
    /// Used where a value is wanted with no diagnostic to raise -- the
    /// constant folder, which answers "not a constant" and lets the caller
    /// decide what that means.
    pub(super) fn pack_elements_quiet(
        &self,
        name: &str,
    ) -> Option<Vec<crate::template::TemplateArgument>> {
        match self
            .substitution
            .as_ref()
            .and_then(|substitution| substitution.get(name))
        {
            Some(crate::template::TemplateArgument::Pack(elements)) => Some(elements.clone()),
            _ => None,
        }
    }

    /// Reads one element of a pack as a number.
    ///
    /// Only values can be combined: a type has no number in it, and saying so
    /// here is what keeps `(Ts + ...)` from becoming a mystery.
    pub(super) fn pack_element_value(
        &mut self,
        pack: &fallc_ast::PackRef,
        element: &crate::template::TemplateArgument,
        wanted: &str,
    ) -> Option<i64> {
        match element {
            crate::template::TemplateArgument::Int(value) => Some(*value),
            crate::template::TemplateArgument::Type(ty) => {
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "`{}` must be {wanted}, but its elements are types (here `{}`)",
                        pack.pack,
                        ty.spelling()
                    ))
                    .with_code(DiagnosticCode::sema(80))
                    .with_span(pack.pack_span)
                    .with_note("a pack of values is written `template <values ...int>`"),
                );
                None
            }
            crate::template::TemplateArgument::Text(_) => {
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "`{}` must be {wanted}, but its elements are text",
                        pack.pack
                    ))
                    .with_code(DiagnosticCode::sema(80))
                    .with_span(pack.pack_span),
                );
                None
            }
            crate::template::TemplateArgument::Pack(_) => None,
        }
    }

    /// Folds a parameter pack with an operator: `(values + ...)`.
    ///
    /// The elements are combined left to right, and the result is a constant
    /// because every element is: a pack's elements are values known before the
    /// program runs, so a fold over them is too. That is what makes it usable
    /// where a constant is required, and what lets `constexpr fn sum()` be a
    /// single expression.
    pub(super) fn type_of_fold(
        &mut self,
        node: &fallc_ast::FoldExpr,
        span: fallc_span::Span,
    ) -> Option<Type> {
        let elements = self.pack_elements(&node.pack, "folded")?;
        // Every element is checked as a value before it is combined, so an
        // element that is not a constant is reported where it stands.
        for element in &elements {
            let _ = self.pack_element_value(&node.pack, element, "values")?;
        }
        match self.const_fold(node) {
            Some(total) => {
                self.constant_values.insert(span, total);
                // A logical fold is a condition, not a number: `(flags && ...)`
                // has the type `bool`, whatever the elements are.
                Some(match node.op {
                    BinaryOp::LogicalAnd | BinaryOp::LogicalOr => Type::BOOL,
                    _ => Type::INT,
                })
            }
            None => {
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "`{}` is an empty pack, so folding it has no value",
                        node.pack.pack
                    ))
                    .with_code(DiagnosticCode::sema(80))
                    .with_span(span)
                    .with_note(
                        "an empty fold has a value only for `&&` (true), `||` (false) and `,` \
                         (void), or when a starting value is given",
                    ),
                );
                None
            }
        }
    }

    /// Counts a parameter pack: `sizeof...(values)`.
    ///
    /// The count is a `usize`, like every other size the language reports, and
    /// it is known before the program runs. An empty pack counts zero rather
    /// than being an error, because counting needs no identity.
    pub(super) fn type_of_sizeof_pack(
        &mut self,
        node: &fallc_ast::PackRef,
        span: fallc_span::Span,
    ) -> Option<Type> {
        let elements = self.pack_elements(node, "counted")?;
        self.constant_values.insert(span, elements.len() as i64);
        Some(Type::USIZE)
    }

    /// Types `cond ? a : b`.
    ///
    /// # The rule
    ///
    /// The condition is tested for truth, as any condition is, and the two
    /// branches must have a type in common -- either the same type, or one that
    /// the other converts to by a rule the language already has. Which branch
    /// is taken is decided at run time, so only one is evaluated; that is why
    /// this is an operator rather than a call to some function.
    pub(super) fn type_of_conditional(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        conditional: &fallc_ast::ConditionalExpr,
        span: fallc_span::Span,
    ) -> Option<Type> {
        // The condition is checked as any condition is, which is what lets a
        // pointer or a number stand where a `bool` would.
        self.check_condition(ast, resolutions, types, conditional.condition);

        // Each branch is inferred with no expectation from the other, so that
        // neither adopts the other's type before the two are compared.
        let previous = self.expected.take();
        let then_ty = self.infer_expr(ast, resolutions, types, conditional.then_value);
        let else_ty = self.infer_expr(ast, resolutions, types, conditional.else_value);
        self.expected = previous;
        let (Some(then_ty), Some(else_ty)) = (then_ty, else_ty) else {
            return None;
        };

        // A branch that is a reference is read as its referent: `lhs < rhs ?
        // lhs : rhs` yields a *value*, because what a reader wants is the
        // value the condition selected -- and the result of the whole
        // expression is a value unless an address is asked for.
        //
        // Recorded so that code generation reads through the reference, which
        // it does only where the checker said to.
        let wanted = self.expected.clone();
        let address_wanted = wanted.as_ref().is_some_and(|ty| ty.is_indirection());
        let read_branch = |this: &mut Self, branch: fallc_ast::ExprId, ty: &Type| {
            if !address_wanted && let Some(referent) = ty.referent().cloned() {
                this.deref_reads
                    .insert(ast.expr(branch).span, referent.clone());
                return referent;
            }
            ty.clone()
        };
        let then_ty = read_branch(self, conditional.then_value, &then_ty);
        let else_ty = read_branch(self, conditional.else_value, &else_ty);

        // The result is whichever branch's type both fit. When one fits the
        // other, that one is the result; when neither does, the branches have
        // no type in common and the expression has none either.
        if self.is_compatible(&else_ty, &then_ty) {
            return Some(then_ty);
        }
        if self.is_compatible(&then_ty, &else_ty) {
            return Some(else_ty);
        }
        self.diagnostics.push(
            Diagnostic::error(format!(
                "the branches of `?:` have different types: `{}` and `{}`",
                then_ty.spelling(),
                else_ty.spelling()
            ))
            .with_code(DiagnosticCode::sema(70))
            .with_span(span)
            .with_note(
                "both branches must have a type in common, because either may be the \
                 result",
            ),
        );
        None
    }

    /// Returns `true` if this object's named field is marked `mutable`.
    ///
    /// The field is looked up through the pointee, because that is where the
    /// declaration is: an object of type `*const Cache` has `Cache`'s fields,
    /// and one of them may be mutable.
    pub(super) fn field_is_mutable(&self, object_ty: &Type, field: &str) -> bool {
        let pointee = object_ty.pointee().unwrap_or(object_ty);
        let pointee = pointee.unqualified();
        let Some(nominal) = pointee.nominal() else {
            return false;
        };
        nominal.mutable_fields.iter().any(|name| name == field)
    }
}
