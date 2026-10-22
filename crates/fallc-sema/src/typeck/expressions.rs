//! Expressions: literals, operators, initialisers and the conversions that
//! turn one into another.
//!
//! `infer_expr` is the entry point; each kind of expression has a function of
//! its own below it.

use super::*;

impl TypeChecker {
    /// Infers the type of an expression, recording it in `types`.
    ///
    /// Returns `None` when the expression's type cannot be determined; a
    /// diagnostic is recorded in that case.
    pub fn infer_expr(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        expr: ExprId,
    ) -> Option<Type> {
        let node = ast.expr(expr);
        // The expectation is recorded so that the interface pass can answer
        // whether the value satisfies an interface -- a question that needs
        // the resolution table, which this does not have.
        if let Some(expected) = self.expected.clone() {
            self.expected_types.insert(expr, (node.span, expected));
        }
        let inferred = match &node.kind {
            // These two exist only inside a function that takes a parameter
            // pack; the expansion replaces them before the body is checked, so
            // meeting one here means it was written where no pack is in scope.
            fallc_ast::ExprKind::PackExpand(_) => {
                self.diagnostics.push(
                    Diagnostic::error("`...` after an argument needs a parameter pack to expand")
                        .with_code(DiagnosticCode::sema(71))
                        .with_span(node.span)
                        .with_note(
                            "only a function that declares `name ...Ts` has a pack, and \
                             only there do its elements exist",
                        ),
                );
                None
            }
            fallc_ast::ExprKind::CommaFold { .. } => {
                self.diagnostics.push(
                    Diagnostic::error("a comma fold needs a parameter pack to repeat")
                        .with_code(DiagnosticCode::sema(71))
                        .with_span(node.span)
                        .with_note("only a function that declares `name ...Ts` has a pack"),
                );
                None
            }
            // An integer literal's type comes from its suffix, or from its
            // context when there is none.
            //
            // A suffix is the literal's type, **not** a conversion: `5u32` is a
            // `u32`, so `x u8 = 5u32;` is a mismatch rather than a silent
            // truncation. Without a suffix the literal takes the type its
            // context requires, which is what makes `return 7;` work in a `u8`
            // function; with no context at all it is an `int`.
            fallc_ast::ExprKind::IntLiteral(literal) => self.type_of_int_literal(literal, expr),
            // A float's type comes from its suffix: `f` means `float`, and
            // nothing means `double`. It does not adopt its context's type, as
            // an integer literal does, because `1.5` in a `float` position
            // would then silently lose precision.
            fallc_ast::ExprKind::FloatLiteral(literal) => Some(match literal.suffix.as_deref() {
                Some("f") => Type::from_name("float").expect("`float` is a primitive"),
                _ => Type::from_name("double").expect("`double` is a primitive"),
            }),
            fallc_ast::ExprKind::BoolLiteral(_) => Some(Type::BOOL),
            // `nullptr` has its own type until it is used, at which point it
            // becomes the null value of whatever pointer is wanted. Adopting
            // the expected type here is what makes `p *T = nullptr;` work
            // without a cast, and what lets `nullptr` be compared with a
            // pointer of any type.
            fallc_ast::ExprKind::NullPointer => Some(match self.expected.clone() {
                Some(ty) if ty.is_indirection() => ty,
                _ => Type::NullPointer,
            }),
            // A character literal's type comes from its width prefix, and is
            // a `c8` without one.
            fallc_ast::ExprKind::CharLiteral(literal) => {
                let ty = match literal.width {
                    Some(16) => Type::C16,
                    Some(32) => Type::C32,
                    _ => Type::C8,
                };
                self.check_char_literal_fits(literal, &ty, expr);
                Some(ty)
            }
            // A string literal is an array of characters whose width comes
            // from its prefix, with the terminating NUL counted in the length:
            // `"ab"` is `[3]const c8`. The element is `const` because the
            // characters live in read-only storage. Where a `std.String` is
            // required the literal constructs one, and where a pointer is
            // wanted the array decays to a pointer.
            fallc_ast::ExprKind::StringLiteral(text) => Some(Type::Array {
                element: Box::new(Type::Const(Box::new(string_element_type(text)))),
                len: string_literal_length(text),
            }),
            // A byte string is raw memory, not text: the same array shape
            // without the terminating NUL, and with `byte` rather than `c8` as
            // the element. The two prefixes therefore differ in both the
            // element type and the terminator, which is what makes `b"ab"` a
            // buffer rather than a C string.
            fallc_ast::ExprKind::ByteStringLiteral(text) => Some(Type::Array {
                element: Box::new(Type::Const(Box::new(Type::BYTE))),
                len: byte_string_length(text),
            }),
            // A raw string is a string: the same `[N]const c8` with a
            // terminator, only its characters are taken literally. Length is
            // its bytes plus the zero, because it may be handed to C like any
            // other string.
            fallc_ast::ExprKind::RawStringLiteral(text) => Some(Type::Array {
                element: Box::new(Type::Const(Box::new(Type::C8))),
                len: raw_string_length(text),
            }),
            fallc_ast::ExprKind::Name(name) => {
                // A value template parameter is not a binding, so it is looked
                // up here: its type comes from the template's declaration and
                // its *value* from the instantiation.
                if let Some(ty) = self.template_value_types.get(name).cloned() {
                    return Some(ty);
                }
                self.type_of_name(ast, resolutions, name, node.span)
            }
            fallc_ast::ExprKind::Comma(node) => {
                // The left side is evaluated for its effects only, so its type
                // is not constrained; the whole expression has the right
                // side's type.
                let previous = self.expected.take();
                let _ = self.infer_expr(ast, resolutions, types, node.lhs);
                self.expected = previous;
                self.infer_expr(ast, resolutions, types, node.rhs)
            }
            fallc_ast::ExprKind::Binary(node) => {
                self.type_of_binary(ast, resolutions, types, node, node_span(ast, expr))
            }
            fallc_ast::ExprKind::Unary(node) => {
                let previous = self.expected.take();
                // `&x` expects a pointer, and what it takes the address of is
                // that pointer's pointee -- so the expectation is passed down
                // one level. That is what lets `p *fn(int) int = &f;` name the
                // overload of `f` whose type the binding asks for.
                self.expected = if node.op == UnaryOp::AddressOf {
                    previous.as_ref().and_then(|ty| match ty.unqualified() {
                        Type::Pointer { pointee, .. } => Some(pointee.as_ref().clone()),
                        _ => None,
                    })
                } else {
                    None
                };
                let operand = self.infer_expr(ast, resolutions, types, node.operand);
                self.expected = previous;
                match (node.op, operand) {
                    // Address-of produces a pointer to the operand's type, and
                    // needs an lvalue to take the address of.
                    (UnaryOp::AddressOf, Some(ty)) => {
                        // The operand must denote storage, but taking its
                        // address is not a write, so a `*const` field is
                        // refused only for its *mutability*, not for existing.
                        self.check_addressable(ast, resolutions, types, node.operand)?;
                        // A reference already names storage, so `&a[i]` -- where
                        // `a[i]` is a reference -- is the address of the
                        // element, not the address of a reference value. One
                        // level is dropped, exactly as reading through it would.
                        let ty = ty.referent().cloned().unwrap_or(ty);
                        // The pointee's constness follows the storage it names:
                        // `&this->v` through a `*const` receiver is a `*const
                        // int`, because handing out a mutable pointer to
                        // something the program promised to read would step
                        // around the whole point of `const`.
                        let pointee_is_const = self.storage_is_const(ast, types, node.operand);
                        Some(Type::Pointer {
                            pointee: Box::new(ty),
                            pointee_is_const,
                            pointer_is_const: false,
                        })
                    }
                    // Dereference produces a *reference* to the pointee: `*p`
                    // is `&T` when `p` is `*T`. The reference reads and writes
                    // as the value does, so `*p = 1` and `*p + 1` both work
                    // unchanged; what the reference adds is that it names the
                    // storage rather than a copy of it.
                    (UnaryOp::Deref, Some(ty)) => match ty.pointer_pointee() {
                        Some(pointee) => {
                            // A reference is read as a value unless the context
                            // wants an address, and the type checker is the
                            // only part that knows which. Recording it here
                            // means code generation reads the storage exactly
                            // when a value is wanted.
                            if !self
                                .expected
                                .as_ref()
                                .is_some_and(|wanted| wanted.is_indirection())
                            {
                                self.deref_reads
                                    .insert(node_span(ast, expr), pointee.clone());
                            }
                            // `*p` yields a reference to what `p` points at,
                            // and it names storage that already exists -- so it
                            // is an lvalue reference, not an rvalue one.
                            Some(Type::Reference {
                                referent: Box::new(pointee.clone()),
                                referent_is_const: ty.pointee_is_const(),
                                is_rvalue: false,
                            })
                        }
                        None if ty.is_reference() => {
                            // A reference is *already* the thing it names, so
                            // there is nothing to take: `*this` where `this` is
                            // `&T` asks to reach through a reference, and that
                            // is not an operation the language has. `&x` is what
                            // makes a *pointer*, and `*p` is what follows one.
                            self.diagnostics.push(
                                Diagnostic::error(format!(
                                    "`*` cannot be applied to `{}`, which is a reference",
                                    ty.spelling()
                                ))
                                .with_code(DiagnosticCode::sema(43))
                                .with_span(node_span(ast, expr))
                                .with_note(
                                    "a reference already names the value it stands for, so \
                                     write it as it is; `*` follows a *pointer*, which `&x` \
                                     produces",
                                ),
                            );
                            None
                        }
                        None => {
                            // A type takes part in `*` by defining `__deref__`,
                            // which is what makes a smart pointer or an
                            // iterator dereference like a pointer does.
                            if let Some(method) =
                                fallc_ast::special::SpecialMethod::for_unary(node.op)
                                && let Some(result) = self.resolve_unary_result(
                                    resolutions,
                                    &ty,
                                    method,
                                    node_span(ast, expr),
                                    ast,
                                )
                            {
                                // A reference is read as a value unless an
                                // address is what the context wants, which the
                                // expectation says. Recording it here keeps the
                                // decision in one place instead of making code
                                // generation infer it from the types again.
                                if !self
                                    .expected
                                    .as_ref()
                                    .is_some_and(|wanted| wanted.is_indirection())
                                    && let Some(referent) = result.referent().cloned()
                                {
                                    self.deref_reads.insert(node_span(ast, expr), referent);
                                }
                                return Some(result);
                            }
                            self.diagnostics.push(
                                Diagnostic::error(format!(
                                    "cannot dereference `{}`, which is not a pointer",
                                    ty.spelling()
                                ))
                                .with_code(DiagnosticCode::sema(43))
                                .with_span(node_span(ast, expr))
                                .with_note(
                                    "a type is dereferenced by defining `__deref__`",
                                ),
                            );
                            None
                        }
                    },
                    // The arithmetic operators need a number.
                    (_, Some(ty)) if ty.is_numeric() => Some(ty),
                    // `!` is built into `bool`. A truth value is not something a
                    // type gets to redefine the negation of, because `bool` is
                    // already the type every condition has -- and without this
                    // `!(a == b)` looked for a `__not__` on `bool` and found
                    // none, which made the most ordinary condition unwritable.
                    (UnaryOp::Not, Some(ty)) if ty.is_bool() => Some(Type::BOOL),
                    // `!x` truth-tests its operand, so anything a condition may
                    // test can be negated: a pointer (including a function
                    // pointer), a reference, an array, a number.
                    (UnaryOp::Not, Some(ty)) if ty.is_truth_testable() => Some(Type::BOOL),
                    (_, Some(ty)) => {
                        // A type takes part in a unary operator by defining the
                        // method for it: `-v` is `__neg__`, `~v` is `__not__`.
                        if let Some(method) =
                            fallc_ast::special::SpecialMethod::for_unary(node.op)
                            && let Some(symbol) = self.resolve_nullary_method(
                                resolutions,
                                &ty,
                                method,
                            )
                        {
                            let found = resolutions
                                .defs
                                .iter()
                                .filter(|def| {
                                    def.name == format!("{}{}", method.name(), ty.nominal_name().unwrap_or(""))
                                })
                                .find_map(|def| def.signature.clone());
                            let returns = found
                                .map(|signature| self.signature_result_type(&signature));
                            self.unary_methods.insert(node_span(ast, expr), symbol);
                            return returns;
                        }
                        self.diagnostics.push(
                            Diagnostic::error(format!(
                                "`{}` cannot be applied to `{}`",
                                node.op.spelling(),
                                ty.spelling()
                            ))
                            .with_code(DiagnosticCode::sema(31))
                            .with_span(node_span(ast, expr))
                            .with_note(
                                "a type takes part in an operator by defining the special \
                                 method for it, such as `__neg__`",
                            ),
                        );
                        None
                    }
                    (_, None) => None,
                }
            }
            fallc_ast::ExprKind::IncDec(node) => {
                // The operand must be an lvalue, and the result has its type.
                //
                // A pointer is advanced by one element, which is pointer
                // arithmetic with an offset of one: `++p` is `p = p + 1`, and
                // the scaling is the same. No method is involved, because a
                // pointer has none.
                let operand_ty = self.infer_lvalue(ast, resolutions, types, node.operand)?;
                if operand_ty.is_pointer() {
                    Some(operand_ty)
                } else if !operand_ty.is_numeric() {
                    // One method per direction, not one per position: the
                    // method modifies the receiver, and whether the expression
                    // yields the old or the new value is the compiler's
                    // business rather than the type's.
                    let method = if node.op == fallc_ast::IncDecOp::Increment {
                        fallc_ast::special::SpecialMethod::Inc
                    } else {
                        fallc_ast::special::SpecialMethod::Dec
                    };
                    if let Some(symbol) =
                        self.resolve_nullary_method(resolutions, &operand_ty, method)
                    {
                        self.unary_methods.insert(node_span(ast, expr), symbol);
                        // The result falls through to the table below rather
                        // than returning here: the type checker's own table is
                        // what code generation reads, and an early `return`
                        // left `(c++).value()` with no type for its receiver.
                        Some(operand_ty)
                    } else {
                    self.diagnostics.push(
                        Diagnostic::error(format!(
                            "`{}` cannot be applied to `{}`",
                            node.op.spelling(),
                            operand_ty.spelling()
                        ))
                        .with_code(DiagnosticCode::sema(31))
                        .with_span(node_span(ast, expr)),
                    );
                    None
                    }
                } else {
                    Some(operand_ty)
                }
            }
            fallc_ast::ExprKind::Assign(node) => {
                self.type_of_assign(ast, resolutions, types, node, node_span(ast, expr))
            }
            // A typed initialiser names its type, so it needs no context: this
            // is what makes `p auto = Point{42, 37}` deducible while
            // `p auto = .{42, 37}` is not.
            fallc_ast::ExprKind::TypedInit(node) => {
                let target = self.resolve_type_syntax(
                    &fallc_ast::TypeSyntax::Named(fallc_ast::NamedType {
                        path: node.type_name.clone(),
                        arguments: node.type_arguments.clone(),
                        span: node.type_span,
                is_expansion: false,
            }),
                    false,
                );
                if target.is_unspecified() {
                    self.diagnostics.push(
                        Diagnostic::error(format!("cannot find type `{}`", node.type_name))
                            .with_code(DiagnosticCode::sema(2))
                            .with_span(node.type_span),
                    );
                    return None;
                }
                self.check_designated_init(ast, resolutions, types, &node.init, &target);
                if let Some(id) = Some(expr) {
                    types.insert(id, target.clone());
                }
                Some(target)
            }
            // A nested initialiser takes its type from the field it fills, so
            // its elements are checked against *its* expectation, which the
            // enclosing check installed.
            fallc_ast::ExprKind::NestedInit(init) => {
                // A reference is transparent here: `a[i] = .{3, 4}` initialises
                // the element the reference names, so the expectation is what
                // it refers to.
                let expected = self
                    .expected
                    .clone()
                    .map(|target| target.referent().cloned().unwrap_or(target));
                match expected {
                    Some(target) => {
                        self.check_designated_init(ast, resolutions, types, init, &target);
                        Some(target)
                    }
                    None => {
                        self.diagnostics.push(
                            Diagnostic::error("a nested `.{ .. }` needs a type from its context")
                                .with_code(DiagnosticCode::sema(64))
                                .with_span(node.span),
                        );
                        None
                    }
                }
            }
            fallc_ast::ExprKind::Field(node) => {
                self.type_of_field(ast, resolutions, types, node, node_span(ast, expr))
            }
            fallc_ast::ExprKind::Index(node) => {
                self.type_of_index(ast, resolutions, types, node, node_span(ast, expr))
            }
            fallc_ast::ExprKind::Call(node) => {
                // A call whose callee names a *trait* is a question about types
                // rather than a call: `InputIterator(IntIter)` is a
                // compile-time `bool`, and its arguments are types.
                if let Some(answer) =
                    self.trait_query(ast, resolutions, types, node, node_span(ast, expr))
                {
                    // The table records every expression's type, and a
                    // condition is read from it: a query is an ordinary `bool`
                    // whose value is known before the program runs, so it may
                    // stand in an `if` as well as in an `if constexpr`.
                    types.insert(expr, answer.clone());
                    return Some(answer);
                }
                let result = self.type_of_call(ast, resolutions, types, node, node_span(ast, expr));
                // A call whose value is known before the program runs is folded
                // to that value, exactly as `sizeof` is. Recording it here is
                // what makes `x int = square(3);` emit `3 * 3` rather than a
                // call: the checker is the stage that can evaluate it, and code
                // generation reads the answer rather than repeating the work.
                //
                // The call is still type checked above, so a call that does not
                // type check is reported; only the *value* is folded away.
                if result.is_some()
                    && let Some(value) = self.const_eval_int(expr)
                {
                    self.constant_values.insert(node_span(ast, expr), value);
                }
                result
            }
            fallc_ast::ExprKind::Cast(cast) => {
                self.prepare_type_of(&cast.ty, ast, resolutions, types);
                let target = self.resolve_written_type(&cast.ty);
                // The operand is inferred with no expectation: a cast does not
                // ask its operand to already have the target type.
                let previous = self.expected.take();
                let source = self.infer_expr(ast, resolutions, types, cast.operand);
                self.expected = previous;

                // A reference converts as the value it refers to does: `r as
                // int` where `r` is `&const int` is a conversion of an `int`,
                // because that is what reading the reference gives.
                let source = source?;
                let source = match source.referent() {
                    Some(referent) => referent.clone(),
                    None => source,
                };
                // A type may define its own conversions by declaring `__as__`
                // with a selector parameter naming the target type. That is
                // asked first, because it is the type's own answer to "how do
                // I become that", and a built-in rule cannot know it.
                if let Some(method) = self.find_conversion_method(resolutions, &source, &target)
                {
                    // The method's return type is what the cast produces, which
                    // lets a conversion return something other than the type
                    // its selector named.
                    let returns = method.returns.clone();
                    self.note_throwing_call(node.span, &method.signature, ast, None);
                    self.conversion_methods.insert(node.span, method);
                    return Some(returns);
                }
                if !source.is_castable_to(&target) {
                    self.diagnostics.push(
                        Diagnostic::error(format!(
                            "cannot convert `{}` to `{}`",
                            source.spelling(),
                            target.spelling()
                        ))
                        .with_code(DiagnosticCode::sema(30))
                        .with_span(node.span)
                        .with_note(format!(
                            "`{}` may define `__as__` with a selector for `{}`",
                            source.spelling(),
                            target.spelling()
                        )),
                    );
                    return None;
                }
                Some(target)
            }
            // `sizeof(T)` and `alignof(T)` are compile-time constants. They are
            // folded here so that the expression has a value like any other,
            // and so that a type whose layout is not fixed is reported rather
            // than given an invented number.
            fallc_ast::ExprKind::Fold(node) => self.type_of_fold(node, node_span(ast, expr)),
            fallc_ast::ExprKind::SizeofPack(node) => {
                self.type_of_sizeof_pack(node, node_span(ast, expr))
            }
            // `call()?!` -- the call's value, unless it raises, in which case
            // the exception leaves this function.
            // `?!(expr)` -- analyse the expression and answer whether it may
            // raise. The answer is a constant, so this stands wherever one is
            // required.
            // A type is not a value. `type(expr)` is meaningful only where a
            // type is wanted -- a trait query's argument, or a requirement's
            // method call -- and both handle it before this point, so reaching
            // here is a mistake in the program.
            //
            // A requirement is the place where a written type stands for a
            // value of it: `it.push(type(Iter))` states the parameter, exactly
            // as `it.push(Iter)` does, and a restriction that cannot state its
            // parameters is not a restriction. Everywhere else the type is
            // reported, because there is nothing to emit for it.
            fallc_ast::ExprKind::TypeOf(inner) => {
                if self.in_trait_requirement {
                    return self.infer_expr(ast, resolutions, types, *inner);
                }
                self.diagnostics.push(
                    Diagnostic::error("a type is not a value")
                        .with_code(DiagnosticCode::sema(135))
                        .with_span(node.span)
                        .with_note(
                            "`type(expr)`, and a written type such as `Array<int>`, name a type; \
                             they can be used where a type is wanted, such as a trait query's \
                             argument or an interface's method",
                        ),
                );
                None
            }
            fallc_ast::ExprKind::TypeLiteral(written) => {
                if self.in_trait_requirement {
                    let substitution = self.substitution.clone().unwrap_or_default();
                    return Some(self.resolve_template_type(written, &substitution));
                }
                self.diagnostics.push(
                    Diagnostic::error("a type is not a value")
                        .with_code(DiagnosticCode::sema(135))
                        .with_span(node.span)
                        .with_note(
                            "`type(expr)`, and a written type such as `Array<int>`, name a type; \
                             they can be used where a type is wanted, such as a trait query's \
                             argument or an interface's method",
                        ),
                );
                None
            }
            fallc_ast::ExprKind::ThrowQuery(inner) => {
                let previous = self.expected.take();
                self.probe_depth += 1;
                self.probe_saw_throw = false;
                let _ = self.infer_expr(ast, resolutions, types, *inner);
                let may_raise = self.probe_saw_throw;
                self.probe_depth -= 1;
                self.expected = previous;
                let value = i64::from(may_raise);
                self.constant_values.insert(node_span(ast, expr), value);
                Some(Type::BOOL)
            }
            fallc_ast::ExprKind::Propagate(node) => {
                let span = node_span(ast, expr);
                // A restriction writes `?!` to say that the *method* may raise,
                // not to pass an exception on: there is no function here whose
                // exceptions could leave, and the call is never emitted.
                if !self.in_trait_requirement {
                    self.require_throw_allowed(span, "`?!`");
                }
                self.propagating_depth += 1;
                let found = self.infer_expr(ast, resolutions, types, node.value);
                self.propagating_depth -= 1;
                // A `?!` on something that cannot raise does nothing. In a
                // program that is worth a warning -- inside a template the same
                // `?!` may be about a call that raises for some instantiations
                // and not for others, and a rule that made it an error would
                // forbid the generic spelling the marker exists for.
                //
                // A restriction is the other way round: `?!` is how the
                // declaration says the method may raise, so a method that never
                // raises does not satisfy it.
                let raises = self.throwing_calls.contains(&ast.expr(node.value).span);
                if self.in_trait_requirement && found.is_some() && !raises {
                    self.diagnostics.push(
                        Diagnostic::error(
                            "this call cannot raise, and the restriction marks it with `?!`",
                        )
                        .with_code(DiagnosticCode::sema(178))
                        .with_span(span)
                        .with_note(
                            "`?!` in a restriction says the method returns through the \
                             exception path; drop it if the method never raises",
                        ),
                    );
                } else if found.is_some() && !raises {
                    self.diagnostics.push(
                        Diagnostic::warning("this call cannot raise, so `?!` does nothing")
                            .with_code(DiagnosticCode::sema(83))
                            .with_warning("useless-raise-mark")
                            .with_span(span)
                            .with_note(
                                "`?!` passes on the exception a call may raise, and this one \
                                 raises nothing: drop the mark, or silence this line with \
                                 `// no warning` if the call is generic elsewhere",
                            ),
                    );
                }
                found
            }
            fallc_ast::ExprKind::Sizeof(ty) | fallc_ast::ExprKind::Alignof(ty) => {
                let is_size = matches!(&node.kind, fallc_ast::ExprKind::Sizeof(_));
                self.prepare_type_of(ty, ast, resolutions, types);
                let resolved = self.resolve_written_type(ty);
                // An incomplete type has no size to give: saying so is more
                // useful than "not a constant", which would sound like a fold
                // that failed rather than a type that is not finished.
                if let Some(nominal) = self.incomplete_nominal(&resolved) {
                    self.diagnostics.push(
                        Diagnostic::error(format!(
                            "`{}` of `{}` is not known: the type is incomplete",
                            if is_size { "sizeof" } else { "alignof" },
                            nominal
                        ))
                        .with_code(DiagnosticCode::sema(166))
                        .with_span(node.span)
                        .with_note("only a pointer or a reference to it may be written until its definition is seen"),
                    );
                    return Some(Type::USIZE);
                }
                let value = if is_size {
                    self.size_of(&resolved)
                } else {
                    self.align_of(&resolved)
                };
                match value {
                    // The result is `usize`, the type a size is: it is what an
                    // array length and an index already are, so
                    // `[sizeof(int)]u8` and `a[sizeof(T)]` need no cast.
                    Some(value) => {
                        self.constant_values.insert(node.span, value);
                        Some(Type::USIZE)
                    }
                    None => {
                        self.diagnostics.push(
                            Diagnostic::error(format!(
                                "`{}` of `{}` is not a constant",
                                if is_size { "sizeof" } else { "alignof" },
                                resolved.spelling()
                            ))
                            .with_code(DiagnosticCode::sema(69))
                            .with_span(node.span)
                            .with_note(
                                "a type's size is known when its layout is: a pointer's width \
                                 depends on the target, and a declared type is its fields",
                            ),
                        );
                        None
                    }
                }
            }
            fallc_ast::ExprKind::Conditional(conditional) => {
                self.type_of_conditional(ast, resolutions, types, conditional, node.span)
            }
            fallc_ast::ExprKind::Lambda(lambda) => {
                self.type_of_lambda_value(ast, resolutions, types, lambda, expr)
            }
            fallc_ast::ExprKind::Unresolved(_) => None,
        };
        if let Some(ty) = &inferred {
            types.insert(expr, ty.clone());
        }
        inferred
    }

    /// Reports a character literal whose code point does not fit its type.
    ///
    /// `'中'` is U+4E2D and needs a `c16` or `c32`; as a `c8` it would have to
    /// be truncated, which silently changes the character. Requiring the
    /// prefix is what makes the width a property of the literal rather than
    /// something inferred by luck.
    pub(super) fn check_char_literal_fits(
        &mut self,
        literal: &fallc_ast::CharLiteral,
        ty: &Type,
        expr: ExprId,
    ) {
        let Some(bits) = ty.bits() else {
            return;
        };
        let Some(value) = fallc_ast::literal::decode_char_code_point(&literal.text) else {
            // An empty or multi-character literal; the code generator reports
            // it with the same message it would use for any other bad literal.
            return;
        };
        let limit = 1u64 << bits;
        if u64::from(value) >= limit {
            let needed = if value <= 0xFF {
                "u8"
            } else if value <= 0xFFFF {
                "u16"
            } else {
                "u32"
            };
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "the character `{}` does not fit in `{}`; write it as `{needed}{}`",
                    literal.text, ty.spelling(), literal.text
                ))
                .with_code(DiagnosticCode::sema(51))
                .with_span(self.const_span(expr)),
            );
        }
    }

    /// Determines an integer literal's type.
    ///
    /// A suffix names the type outright. Without one the literal adopts its
    /// context's type if the context wants an integer, and is an `int`
    /// otherwise.
    pub(super) fn type_of_int_literal(
        &mut self,
        literal: &fallc_ast::IntLiteral,
        expr: ExprId,
    ) -> Option<Type> {
        if let Some(suffix) = &literal.suffix {
            let named = match suffix.as_str() {
                // The two shorthands, which the type system defines as aliases.
                "u" => "uint",
                "i" => "int",
                other => other,
            };
            let Some(ty) = Type::from_name(named) else {
                self.diagnostics.push(
                    Diagnostic::error(format!("`{suffix}` is not a type"))
                        .with_code(DiagnosticCode::sema(50))
                        .with_span(self.const_span(expr)),
                );
                return None;
            };
            if !ty.is_integer() {
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "`{suffix}` cannot be the type of an integer literal"
                    ))
                    .with_code(DiagnosticCode::sema(50))
                    .with_span(self.const_span(expr)),
                );
                return None;
            }
            return Some(ty);
        }

        // No suffix: take the context's type if it is one an integer literal
        // can have, so that `return 7;` in a `u8` function is a `u8` and
        // `x byte = 1;` is a `byte`.
        //
        // `byte` counts even though it takes no part in arithmetic: it is
        // based on `u8`, and a literal is the only way to produce one without
        // a cast.
        match self.expected.clone() {
            Some(ty) if ty.is_integer() || ty.is_byte() => Some(ty),
            _ => Some(Type::INT),
        }
    }

    /// Checks a `.{ ... }` initialiser against the type it initialises.
    ///
    /// Elements are assigned by position, then by name for the ones written
    /// `.field = value`. Every field must be given exactly once, so the names
    /// only decide *where* a value goes, not which fields are optional.
    pub(super) fn check_designated_init(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        init: &fallc_ast::DesignatedInit,
        target: &Type,
    ) {
        if !self.require_complete(target, init.span, "an initialiser") {
            return;
        }
        let Some(nominal) = target.as_nominal() else {
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "`{{ .. }}` initialises a declared type, but `{}` is not one",
                    target.spelling()
                ))
                .with_code(DiagnosticCode::sema(64))
                .with_span(init.span),
            );
            return;
        };
        let fields = nominal.fields.clone();
        let owner = nominal.name.clone();

        // Which field each value fills. The positional prefix fills from the
        // first field; a named element fills the field with its name, wherever
        // that is in the declaration.
        let mut filled = vec![false; fields.len()];
        for slot in filled.iter_mut().take(init.elements.len()) {
            *slot = true;
        }
        let mut named: Vec<(usize, &fallc_ast::NamedArgument)> = Vec::new();
        for argument in &init.named {
            let Some(index) = fields.iter().position(|(name, _)| name == &argument.name) else {
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "`{owner}` has no field named `{}`",
                        argument.name
                    ))
                    .with_code(DiagnosticCode::sema(170))
                    .with_span(argument.name_span)
                    .with_note(format!(
                        "its fields are {}",
                        Self::parameter_list(
                            &fields
                                .iter()
                                .map(|(name, _)| name.clone())
                                .collect::<Vec<_>>()
                        )
                    )),
                );
                return;
            };
            if filled[index] {
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "field `{}` of `{owner}` is given twice",
                        argument.name
                    ))
                    .with_code(DiagnosticCode::sema(171))
                    .with_span(argument.name_span)
                    .with_note(
                        "give each field one value, either by position or by name",
                    ),
                );
                return;
            }
            filled[index] = true;
            named.push((index, argument));
        }

        if init.elements.len() > fields.len() {
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "`{owner}` has {} field(s), but {} value(s) were given",
                    fields.len(),
                    init.elements.len() + init.named.len()
                ))
                .with_code(DiagnosticCode::sema(65))
                .with_span(init.span),
            );
            return;
        }
        if let Some(missing) = filled.iter().position(|filled| !filled) {
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "`{owner}` needs field `{}`, which the initialiser leaves out",
                    fields[missing].0
                ))
                .with_code(DiagnosticCode::sema(172))
                .with_span(init.span)
                .with_note("every field of a `.{ .. }` initialiser must be given"),
            );
            return;
        }

        for (element, (field_name, field_ty)) in init.elements.iter().zip(&fields) {
            self.check_field_value(ast, resolutions, types, &owner, field_name, field_ty, *element);
        }
        for (index, argument) in named {
            let (field_name, field_ty) = &fields[index];
            self.check_field_value(
                ast,
                resolutions,
                types,
                &owner,
                field_name,
                field_ty,
                argument.value,
            );
        }
    }

    /// Checks one value of an initialiser against the field it fills.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn check_field_value(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        owner: &str,
        field_name: &str,
        field_ty: &Type,
        element: fallc_ast::ExprId,
    ) {
        let previous = self.expected.replace(field_ty.clone());
        let found = self.infer_expr(ast, resolutions, types, element);
        self.expected = previous;
        // The general compatibility rule, not equality: a field declared
        // `volatile int` takes an `int`, and one declared `*const byte`
        // takes a string literal that decays to it. Comparing the types
        // directly rejected both.
        if let Some(found) = found
            && !self.is_compatible(&found, field_ty)
        {
            let mut converted = super::methods::integer_promotes(&found, field_ty);
            if converted {
                self.adopted_types
                    .insert(ast.expr(element).span, field_ty.clone());
            }
            if !converted {
                // The inferred type is what the conversion is looked up from,
                // and a bare name's type is not guaranteed to be in the table
                // at this point: record it before asking.
                types.insert(element, found.clone());
                converted = self.try_implicit_conversion(
                    resolutions, ast, types, element, field_ty,
                );
            }
            if !converted {
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "field `{field_name}` of `{owner}` is `{}`, but `{}` was given",
                    field_ty.spelling(),
                    found.spelling()
                ))
                .with_code(DiagnosticCode::sema(65))
                .with_span(ast.expr(element).span)
                .with_note("there are no implicit conversions; use `as`"),
            );
            }
        }
    }

    /// Types a binary operation.
    ///
    /// # Conversion rules
    ///
    /// There are no implicit conversions, so both operands must already have
    /// the *same* type, with one exception: an integer literal takes the type
    /// its context requires, so `x + 1` works when `x` is any integer type.
    /// A shift's right operand is the same type as its left for that same
    /// reason, and a signed value is never mixed with an unsigned one.
    ///
    /// Comparison and logical operators produce `bool`.
    pub(super) fn type_of_binary(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        node: &BinaryExpr,
        span: fallc_span::Span,
    ) -> Option<Type> {
        // A reference reads as the value it refers to, so an operation on one
        // is an operation on the referent. `*p + 1` is the same kind of
        // expression as `x + 1`; what the reference adds is that it names the
        // storage, which matters for assignment rather than for arithmetic.
        // The left is inferred first and with no expectation, so it does not
        // adopt a type from the right.
        let lhs = self.read_through_reference(ast, resolutions, types, node.lhs);
        let lhs = lhs?;
        // The right *does* adopt the left's type, which is what lets an integer
        // literal sit on either side: `x + 1` and `1 + x`. Without it `n + 1`
        // on a `u8` is a `u8` and an `int`, which is a mismatch.
        //
        // A pointer's offset is always a `usize`, however the pointer is
        // qualified, so that is what the right operand is inferred under. The
        // arithmetic itself is typed below, where both operands are known:
        // `p - q` is not an offset but a difference, and its right operand is a
        // pointer like its left.
        let offset_context = lhs.is_pointer()
            && matches!(
                node.op,
                fallc_ast::BinaryOp::Add | fallc_ast::BinaryOp::Sub
            );
        let previous = self
            .expected
            .replace(if offset_context { Type::USIZE } else { lhs.clone() });
        let rhs = self.infer_expr(ast, resolutions, types, node.rhs);
        self.expected = previous;
        let rhs = rhs?;
        let rhs = match rhs.referent() {
            Some(referent) => {
                // The right operand is read through exactly as the left one is:
                // using a reference as an operand is using the value behind it.
                // Without this record the code generator compared an `int` with
                // the address it was stored at, which is not a comparison it can
                // emit -- `a < b` on two reference parameters of a templated
                // method failed there.
                self.deref_reads
                    .insert(ast.expr(node.rhs).span, referent.clone());
                referent.clone()
            }
            None => rhs,
        };
        self.type_of_binary_operands(ast, resolutions, types, node, lhs, rhs, span)
    }

    /// Infers an operand, yielding the referent when it is a reference.
    ///
    /// The reference itself is left alone: reading through it is a property of
    /// *using* the value, and the operand's own type is still what assignment
    /// and field access need.
    pub(super) fn read_through_reference(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        expr: ExprId,
    ) -> Option<Type> {
        let previous = self.expected.take();
        let inferred = self.infer_expr(ast, resolutions, types, expr);
        self.expected = previous;
        let ty = inferred?;
        Some(match ty.referent().cloned() {
            Some(referent) => {
                // Recorded, because the expression's own type is still the
                // reference: code generation loads the address and has to be
                // told that this use wants the value behind it.
                self.deref_reads.insert(ast.expr(expr).span, referent.clone());
                referent
            }
            None => ty,
        })
    }

    /// Types a binary operation on operands whose references are already read.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn type_of_binary_operands(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        node: &BinaryExpr,
        lhs: Type,
        rhs: Type,
        span: fallc_span::Span,
    ) -> Option<Type> {

        if node.op.is_short_circuit() {
            // Both sides must be usable as conditions. The result is always
            // `bool`, and short-circuiting means the operands are tested, not
            // converted.
            return Some(Type::BOOL);
        }

        // A value compared with itself has one answer, and the expression that
        // produced it may have been evaluated twice. `x != x` on a float is the
        // established way to ask "is this NaN", so that one is left alone --
        // and a trait requirement, `it != it`, is how the requirement says "two
        // positions can be compared" rather than a comparison anybody runs.
        if !self.in_trait_requirement
            && matches!(
                node.op,
                fallc_ast::BinaryOp::Eq
                    | fallc_ast::BinaryOp::Lt
                    | fallc_ast::BinaryOp::Le
                    | fallc_ast::BinaryOp::Gt
                    | fallc_ast::BinaryOp::Ge
                    | fallc_ast::BinaryOp::Cmp
            )
            && same_place(ast, node.lhs, node.rhs)
        {
            self.diagnostics.push(
                Diagnostic::warning(format!("`{}` compares a value with itself", node.op.spelling()))
                    .with_code(DiagnosticCode::sema(146))
                    .with_warning("self-comparison")
                    .with_span(span)
                    .with_note(
                        "both sides are the same place, so the answer is fixed: one of the two \
                         was meant to be a different value",
                    ),
            );
        }

        // A divisor that is a constant zero is a mistake in the program, not a
        // value that happens to be zero: the division cannot succeed, and in a
        // release build it is undefined rather than an error.
        if matches!(node.op, fallc_ast::BinaryOp::Div | fallc_ast::BinaryOp::Rem)
            && self.const_eval_int(node.rhs) == Some(0)
        {
            self.diagnostics.push(
                Diagnostic::warning(format!("`{}` divides by a constant zero", node.op.spelling()))
                    .with_code(DiagnosticCode::sema(147))
                    .with_warning("division-by-zero")
                    .with_span(span)
                    .with_note(
                        "the division cannot have a value: a debug build stops the program \
                         here, and a release build leaves the result undefined",
                    ),
            );
        }

        // ---- pointer arithmetic ----
        //
        // A pointer is an address, and the language's only address arithmetic
        // is the one C has: an offset in *elements*, scaled by the pointee's
        // size. The offset is a `usize` for the same reason an index is --
        // negative offsets are written with `-`, which is a different
        // operation -- and the difference of two pointers is an `isize`,
        // because it may be negative. Nothing here is checked, exactly as
        // indexing a pointer is not.
        if matches!(
            node.op,
            fallc_ast::BinaryOp::Add | fallc_ast::BinaryOp::Sub
        ) && (lhs.is_pointer() || rhs.is_pointer())
        {
            if lhs.is_pointer() {
                if rhs.is_pointer() {
                    if node.op == fallc_ast::BinaryOp::Sub {
                        // A difference is measured in elements of the type
                        // both pointers point at. Two pointers of different
                        // pointee types have no common unit, so the question
                        // has no answer rather than an approximate one.
                        if lhs.pointee() == rhs.pointee() {
                            return Some(Type::ISIZE);
                        }
                        self.diagnostics.push(
                            Diagnostic::error(format!(
                                "`-` cannot take the difference of `{}` and `{}`",
                                lhs.spelling(),
                                rhs.spelling()
                            ))
                            .with_code(DiagnosticCode::sema(32))
                            .with_span(span)
                            .with_note(
                                "a pointer difference is measured in elements, so both \
                                 pointers must point at the same type",
                            ),
                        );
                        return None;
                    }
                    self.diagnostics.push(
                        Diagnostic::error("`+` cannot add two pointers".to_owned())
                            .with_code(DiagnosticCode::sema(32))
                            .with_span(span)
                            .with_note(
                                "two pointers have a distance, written `p - q`, and no sum",
                            ),
                    );
                    return None;
                }
                if rhs.is_integer() {
                    if rhs == Type::USIZE {
                        return Some(lhs);
                    }
                    self.diagnostics.push(
                        Diagnostic::error(format!(
                            "`{}` is offset by `usize`, but `{}` was given",
                            lhs.spelling(),
                            rhs.spelling()
                        ))
                        .with_code(DiagnosticCode::sema(32))
                        .with_span(span)
                        .with_note("there are no implicit conversions; write `as usize`"),
                    );
                    return None;
                }
            } else if node.op == fallc_ast::BinaryOp::Add
                && rhs.is_pointer()
                && lhs.is_integer()
            {
                // `1 + p` is the same operation as `p + 1`. The left literal
                // was inferred before the right, so it is recorded as the
                // `usize` the offset needs -- the type it adopts in every other
                // offset position.
                if lhs == Type::USIZE {
                    return Some(rhs);
                }
                if self.is_default_int_literal(ast, node.lhs) {
                    types.insert(node.lhs, Type::USIZE);
                    return Some(rhs);
                }
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "`{}` is offset by `usize`, but `{}` was given",
                        rhs.spelling(),
                        lhs.spelling()
                    ))
                    .with_code(DiagnosticCode::sema(32))
                    .with_span(span)
                    .with_note("there are no implicit conversions; write `as usize`"),
                );
                return None;
            }
        }

        // Comparison is the one operation that may mix arithmetic types. It
        // compares by mathematical value, so `-1i8 < 1u32` is true rather than
        // the nonsense a bit-width reinterpretation would give.
        //
        // Everything else requires matching types, because there are no
        // implicit conversions between numeric types.
        // The three-way comparison yields a sign rather than a truth value, so
        // it is not a comparison in the sense above. Two arithmetic operands
        // give `int`; anything else is the type's `__cmp__`.
        if node.op == fallc_ast::BinaryOp::Cmp {
            if lhs.is_arithmetic_for_comparison() && rhs.is_arithmetic_for_comparison() {
                return Some(Type::INT);
            }
            if let Some(result) =
                self.type_of_operator_method(ast, resolutions, types, node, &lhs, &rhs, span)
            {
                return Some(result);
            }
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "`<=>` cannot compare `{}` with `{}`",
                    lhs.spelling(),
                    rhs.spelling()
                ))
                .with_code(DiagnosticCode::sema(32))
                .with_span(span)
                .with_note("a type takes part in `<=>` by defining `__cmp__`"),
            );
            return None;
        }

        if node.op.is_comparison() {
            // Two pointers compare as the addresses they are, and that comes
            // before anything the pointee defines: `it != end` over an
            // `Array<String>` walks `*String`s, and asking `String`'s `__ne__`
            // compared two elements -- or an element with the end of the
            // buffer -- instead of the positions. A loop then stopped wherever
            // the *values* happened to compare equal.
            if lhs.is_pointer()
                && rhs.is_pointer()
                && lhs.pointee().zip(rhs.pointee()).is_some_and(|(a, b)| a == b)
            {
                return Some(Type::BOOL);
            }
            // A type that is not arithmetic may still define the comparison,
            // which is the only way a declared type takes part in `<`. Asking
            // the operands comes *before* the built-in rule, not after it: two
            // values of a declared type are equal as types, so the built-in rule
            // claims them and a type that defines `__lt__` never gets asked.
            if !(lhs.is_arithmetic_for_comparison() && rhs.is_arithmetic_for_comparison())
                && let Some(result) =
                    self.type_of_operator_method(ast, resolutions, types, node, &lhs, &rhs, span)
            {
                return Some(result);
            }
            if lhs.is_comparable_with(&rhs) {
                return Some(Type::BOOL);
            }
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "`{}` cannot compare `{}` with `{}`",
                    node.op.spelling(),
                    lhs.spelling(),
                    rhs.spelling()
                ))
                .with_code(DiagnosticCode::sema(32))
                .with_span(span)
                .with_note(
                    "arithmetic types compare with each other, and a type compares with \
                     itself by defining `__lt__` and its siblings",
                ),
            );
            return None;
        }

        if lhs != rhs {
            // Operands of different types may still work: one of them may
            // define the operation with the other's type as its argument, which
            // is what makes `1 + box` and `box + 1` both expressible. That is
            // tried before refusing, because a type that defines `__radd__(int)`
            // has answered this question already.
            if let Some(result) =
                self.type_of_operator_method(ast, resolutions, types, node, &lhs, &rhs, span)
            {
                return Some(result);
            }
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "`{}` needs operands of the same type, but the left is `{}` and the right is `{}`",
                    node.op.spelling(),
                    lhs.spelling(),
                    rhs.spelling()
                ))
                .with_code(DiagnosticCode::sema(32))
                .with_span(span)
                .with_note(
                    "there are no implicit conversions between numeric types; use `as`",
                ),
            );
            return None;
        }

        // A `+` or `-` between two values of a declared type asks that type
        // first, before the numeric rule below: an iterator's difference is
        // defined by the iterator (`r - q` is an `isize`), and both operands
        // have the same type, so nothing about them says the answer is that
        // type.
        if matches!(
            node.op,
            fallc_ast::BinaryOp::Add | fallc_ast::BinaryOp::Sub
        ) && !lhs.is_numeric()
            && let Some(result) =
                self.type_of_operator_method(ast, resolutions, types, node, &lhs, &rhs, span)
        {
            return Some(result);
        }

        if !lhs.is_numeric() {
            // Neither operand is a number, so the operation is whatever the
            // operands' types define it to be. This is the only way a
            // user-defined type takes part in `+`.
            if let Some(result) = self.type_of_operator_method(
                ast, resolutions, types, node, &lhs, &rhs, span,
            ) {
                return Some(result);
            }
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "`{}` cannot be applied to `{}`",
                    node.op.spelling(),
                    lhs.spelling()
                ))
                .with_code(DiagnosticCode::sema(33))
                .with_span(span)
                .with_note(
                    "a type takes part in an operator by defining the special method \
                     for it, such as `__add__`",
                ),
            );
            return None;
        }

        Some(lhs)
    }

    /// Types `a op b` by looking for an operator method on either operand.
    ///
    /// # The rule
    ///
    /// The left operand's method is tried first: `a + b` asks `a`'s type for
    /// `__add__`. Only when that finds nothing is the right operand's
    /// *reflected* method tried, with the operands swapped: `a + b` then asks
    /// `b`'s type for `__radd__` and passes `a` as the argument.
    ///
    /// Trying the right operand first would let it override the left, so
    /// `1 + x` and `x + 1` could mean different things for reasons the reader
    /// cannot see.
    ///
    /// The receiver is the method's `this`, so it is passed as a pointer; the
    /// other operand is the argument. That makes `a + b` exactly a call to
    /// `a.__add__(b)` with `a`'s address taken.
    ///
    /// Returns the method's return type, or `None` when neither operand
    /// defines the operation.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn type_of_operator_method(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        node: &BinaryExpr,
        lhs: &Type,
        rhs: &Type,
        span: fallc_span::Span,
    ) -> Option<Type> {
        // A string literal on either side is constructed into a declared type
        // that can perform the operation, which is what makes `"ab" * 3` work:
        // a literal is an array of characters and has no methods of its own.
        //
        // The declared types that define this operator are tried, and the one
        // whose method accepts the other operand is the answer -- the same
        // question `resolve_operator_method` asks, with the literal-constructed
        // receiver standing in for the literal.
        for (receiver_expr, argument_expr, argument, reflected) in [
            (node.lhs, node.rhs, rhs, false),
            (node.rhs, node.lhs, lhs, true),
        ] {
            if !matches!(
                &ast.expr(receiver_expr).kind,
                fallc_ast::ExprKind::StringLiteral(_)
            ) {
                continue;
            }
            let Some(method) =
                fallc_ast::special::SpecialMethod::for_binary(node.op, reflected)
            else {
                continue;
            };
            let Some(name) = (if reflected {
                method.reflected_name()
            } else {
                Some(method.name())
            }) else {
                continue;
            };
            // Every declared type is a candidate receiver, because the literal
            // does not say which one it should become. The first whose method
            // accepts the other operand wins; declaring two is an ambiguity the
            // language has no rule for.
            let receivers: Vec<Type> = self
                .nominal_types
                .values()
                .map(|nominal| Type::Nominal(nominal.clone()))
                .collect();
            for receiver in receivers {
                let Some(result) = self.resolve_operator_method(
                    ast,
                    resolutions,
                    types,
                    name,
                    &receiver,
                    argument,
                    argument_expr,
                    span,
                    reflected,
                ) else {
                    continue;
                };
                // The literal is constructed into the receiver's type, and that
                // construction is recorded under the literal's own span, which
                // is where code generation emits it.
                self.construct_from_literal(
                    resolutions,
                    ast,
                    &receiver,
                    receiver_expr,
                    ast.expr(receiver_expr).span,
                );
                return Some(result);
            }
        }

        for reflected in [false, true] {
            let Some(method) = fallc_ast::special::SpecialMethod::for_binary(node.op, reflected)
            else {
                continue;
            };
            // The *written* name, which for a reflected form is not the base
            // method's: `a + b` asks `b` for `__radd__`, not for `__add__`. The
            // base method is what the operation means; the name looked up
            // depends on which operand is the receiver.
            let Some(name) = (if reflected {
                method.reflected_name()
            } else {
                Some(method.name())
            }) else {
                continue;
            };
            // For `a + b` the receiver is `a`; for the reflected form it is
            // `b`, with `a` passed as the argument. Swapping these the wrong
            // way meant `b + 1` looked for `__radd__` on `int` and never found
            // the one on `Box`, which is the whole reason the reflected form
            // exists.
            let (receiver, argument) = if reflected { (rhs, lhs) } else { (lhs, rhs) };
            // The expression that produced `argument`, so that a literal can be
            // recognised as one and allowed to adopt the parameter's type.
            let argument_expr = if reflected { node.lhs } else { node.rhs };
            if let Some(result) = self.resolve_operator_method(
                ast, resolutions, types, name, receiver, argument, argument_expr, span, reflected,
            ) {
                return Some(result);
            }
        }
        let _ = span;
        None
    }

    /// Finds an operator method on `receiver` and checks `argument` against it.
    ///
    /// Returns its return type when the method exists and the argument fits.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn resolve_operator_method(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        name: &str,
        receiver: &Type,
        argument: &Type,
        argument_expr: fallc_ast::ExprId,
        span: fallc_span::Span,
        reflected: bool,
    ) -> Option<Type> {
        let _ = (types, argument);
        // Only a declared type can define a method, so a primitive has nothing
        // to look up and the built-in rules already applied.
        receiver.nominal_name()?;
        // The method takes the other operand as its second parameter, after
        // `this`. A method whose parameter does not accept it is not an
        // implementation of this operation, so the search continues rather
        // than accepting a call that cannot be checked -- which is also what
        // makes an operator with two overloads work: `r - 1` subtracts an
        // offset and `r - q` another position.
        let calls = self.special_method_calls(resolutions, receiver, name, 2);
        for call in calls {
            if let Some(result) = self.fit_operator_candidate(
                ast,
                resolutions,
                call,
                argument,
                argument_expr,
                span,
                reflected,
            ) {
                return Some(result);
            }
        }
        None
    }

    /// Checks one overload of an operator method against the other operand.
    ///
    /// `None` means "this overload does not take this argument", so the caller
    /// tries the next one: an iterator subtracts an offset and another
    /// iterator, and which one is meant is the argument's type.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn fit_operator_candidate(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        call: SpecialMethodCall,
        argument: &Type,
        argument_expr: fallc_ast::ExprId,
        span: fallc_span::Span,
        reflected: bool,
    ) -> Option<Type> {
        let signature = call.signature.clone();
        let parameter_type = call.param_types.get(1)?.clone();
        // The operand is a value and the parameter is usually a pointer to one,
        // because an operator method takes its argument the way a method takes
        // its receiver: by address, so a large value is not copied to be read.
        // Comparing the value against the pointer rejected
        // `fn __add__(this *N, other *const N)` for `a + b`, which is the
        // obvious way to write it and the way the documentation shows it.
        // Whether the operand is handed over as a value or by address is what
        // the parameter says, and only the checker is looking at the parameter.
        // Whether the operand is handed over by address is a fact about the
        // *parameter*, not a side effect of the comparison: it is by address
        // when the parameter is an indirect type the value fits, and the value
        // is not already an address itself.
        //
        // This used to be inferred as `!is_compatible(..) && operand_fits(..)`,
        // which read "it did not compare equal, but it would fit if taken by
        // address" -- a proxy that happened to hold while a value never
        // compared equal to a reference. Once a `T` correctly satisfies a `&T`,
        // the first half is false and the address was never recorded, so
        // `a == b` passed a `String` where `*const String` was declared.
        let by_address = !argument.is_indirection()
            && self.operand_fits(argument, &parameter_type);
        // An unsuffixed integer literal takes the parameter's type, which is
        // how `s * 3` reaches `__mul__(.., usize)`: the literal is an `int`
        // until something tells it otherwise, and the parameter is that
        // something. It is the rule a literal already follows when it is passed
        // to an overloaded function.
        let literal_fits =
            self.is_default_int_literal(ast, argument_expr) && parameter_type.is_integer();
        // A string literal is an array of characters, and a declared type that
        // can be built from one is what makes `s + "text"` and `"text" + s`
        // work. The construction is recorded under the *operand's* span, so
        // code generation emits it where the literal stands.
        let constructed = !literal_fits
            && !by_address
            && !self.is_compatible(argument, &parameter_type)
            && parameter_type.pointee().is_some_and(|pointee| {
                self.construct_from_literal(
                    resolutions,
                    ast,
                    pointee,
                    argument_expr,
                    ast.expr(argument_expr).span,
                )
            });
        if !self.is_compatible(argument, &parameter_type) && !by_address && !literal_fits
            && !constructed
        {
            return None;
        }
        // A literal that fitted by adopting the parameter's type has to be
        // *converted* to it: the expression's own type is still the default
        // `int`, and code generation lowers what the table says.
        if literal_fits {
            self.adopted_types
                .insert(ast.expr(argument_expr).span, parameter_type.clone());
        }
        // The symbol follows the same rule as every function's: the written
        // name and the mangled parameters, which `special_method_call` already
        // built under the member's own arguments.
        self.operator_methods.insert(
            span,
            OperatorCall {
                symbol: call.symbol.clone(),
                argument_by_address: by_address,
                reflected,
            },
        );
        self.note_throwing_call(span, &signature, ast, None);
        // The method's own return type is what the expression has.
        Some(call.returns)
    }

    /// Returns `true` if a value operand may be passed to an indirect parameter.
    ///
    /// This is the same widening a method call performs on its receiver: a
    /// value fits a pointer or a reference to it, because the call takes its
    /// address. `operand_fits_indirect` reports which, so the caller can record
    /// that the address is what to pass.
    pub(super) fn operand_fits(&self, argument: &Type, parameter: &Type) -> bool {
        self.operand_fits_indirect(argument, parameter).is_some()
    }

    /// Returns the referent a value operand fits, if it fits one.
    ///
    /// `Some` when the argument is a value and the parameter is a pointer or
    /// reference to something it *is*: `String` fits `&const String` and
    /// `*const String` alike, and the call passes the value's address. The
    /// indirection itself may not be `const` -- a parameter that is cannot be
    /// reassigned, and there is no lvalue to reassign -- but the referent may
    /// be.
    pub(super) fn operand_fits_indirect(&self, argument: &Type, parameter: &Type) -> Option<Type> {
        let (referent, indirect_is_const) = match parameter {
            Type::Pointer {
                pointee,
                pointer_is_const,
                ..
            } => (pointee.as_ref(), *pointer_is_const),
            Type::Reference {
                referent,
                is_rvalue,
                ..
            } => {
                // An rvalue reference binds to a temporary, and taking the
                // address of a value is exactly that.
                let _ = is_rvalue;
                (referent.as_ref(), false)
            }
            _ => return None,
        };
        if indirect_is_const {
            return None;
        }
        if self.is_compatible(argument, referent) {
            return Some(referent.clone());
        }
        None
    }

    /// The constructor that turns a string literal into `target`, if any.
    ///
    /// Side-effect free: `construct_from_literal` records the call, while
    /// deciding which overload an argument fits must only ask the question.
    pub(super) fn literal_constructor(
        &mut self,
        resolutions: &Resolutions,
        _ast: &Ast,
        target: &Type,
        _argument_expr: fallc_ast::ExprId,
    ) -> Option<crate::FunctionSignature> {
        let target = target
            .referent()
            .or_else(|| target.pointee())
            .unwrap_or(target);
        let receiver = target.nominal_name().map(str::to_owned)?;
        let bare = receiver
            .rsplit_once('.')
            .map_or(receiver.as_str(), |(_, last)| last);
        // A literal is adopted only by a constructor that says it may be
        // implicit. `__init__` alone is explicit: `String("text")` calls it,
        // but `String s = "text";` must not.
        let qualified = format!("__im_init__{bare}");
        // The constructor must take a C string. A candidate taking anything
        // else -- a copy constructor, say -- cannot be given a literal.
        self.definitions_named(resolutions, &qualified)
            .into_iter()
            .filter_map(|def| def.signature.clone())
            .find(|candidate| {
                candidate.param_types.len() == 2
                    && candidate.param_types.get(1).is_some_and(|parameter| {
                        let ty = self.resolve_type_syntax(parameter, true);
                        crate::types::is_c_byte_pointer(&ty)
                    })
            })
    }

    /// Turns a string-literal argument into the declared type it is passed as.
    ///
    /// The rule is the initialiser's: `String s = "text";` constructs, and
    /// `f("text")` with `fn f(s String)` is the same conversion reached from an
    /// argument. Returns whether the argument was converted; when it was, the
    /// call-site check is done.
    ///
    /// # Arguments
    ///
    /// `argument` is the expression the caller wrote; `expected` the parameter's
    /// type and `found` the type the checker inferred for the expression.
    pub(super) fn adopt_literal_argument(
        &mut self,
        resolutions: &Resolutions,
        ast: &Ast,
        argument: fallc_ast::ExprId,
        expected: &Type,
        found: &Type,
    ) -> bool {
        if self.is_compatible(found, expected) {
            return false;
        }
        if !self.construct_from_literal(
            resolutions,
            ast,
            expected,
            argument,
            ast.expr(argument).span,
        ) {
            return false;
        }
        // A parameter that takes a reference receives the constructed value's
        // address, exactly as it would for any other place.
        if expected.is_indirection() {
            self.by_address_arguments.insert(ast.expr(argument).span);
        }
        true
    }

    /// Finds the constructor that turns a string literal into `target`.
    ///
    /// # The rule
    ///
    /// `"text"` is an array of characters, so it is not a `String`. Where a
    /// `String` is wanted -- `s + "text"`, or a literal passed to a function
    /// taking one -- it has to be *constructed*, which is a call to an `__init__`
    /// whose parameter is a C string. That is the very constructor
    /// `String s = "text";` uses, so this is not a new rule: it is the same one,
    /// reached from an operand rather than from a binding.
    ///
    /// A string literal and a declared type only. A value of any other type is
    /// never silently constructed into a `String`, because that would be an
    /// implicit conversion, which the language does not have.
    pub(super) fn construct_from_literal(
        &mut self,
        resolutions: &Resolutions,
        ast: &Ast,
        target: &Type,
        argument_expr: fallc_ast::ExprId,
        span: fallc_span::Span,
    ) -> bool {
        if !matches!(
            &ast.expr(argument_expr).kind,
            fallc_ast::ExprKind::StringLiteral(_)
        ) {
            return false;
        }
        // `f("text")` with `fn f(s &const String)` constructs a `String`, not a
        // reference to one: the value type is what the constructor builds.
        let target = target
            .referent()
            .or_else(|| target.pointee())
            .unwrap_or(target)
            .clone();
        let Some(signature) = self.literal_constructor(resolutions, ast, &target, argument_expr)
        else {
            return false;
        };
        let params: Vec<String> = signature
            .param_types
            .iter()
            .map(|written| {
                let ty = self.resolve_type_syntax(written, true);
                crate::types::mangle_type(&ty)
            })
            .collect();
        self.literal_constructions.insert(
            span,
            fallc_ast::special::symbol_for(&signature.written_name, &params),
        );
        // Constructing from a literal may raise as any constructor may, and it
        // happens at the literal's span.
        self.note_throwing_call(span, &signature, ast, None);
        self.literal_construction_types.insert(span, target.clone());
        true
    }

    /// Finds an `__as__` method that converts `source` to `target`.
    ///
    /// # The form
    ///
    /// ```text
    /// fn __as__(this *const Fraction, *double) double {
    ///     return this->num as double / this->den as double;
    /// }
    /// ```
    ///
    /// The second parameter is a **selector**: it names the target type and is
    /// never used, which is why its name may be omitted. A conversion cannot be
    /// selected by its return type -- two `__as__` overloads differing only
    /// there would be the same signature twice -- so the target is named by a
    /// parameter instead.
    ///
    /// The selector is a pointer to the target, so `as double` matches
    /// `*double`. Matching is by the type the selector points at, ignoring
    /// `const`, because a conversion asks what to become rather than whether
    /// the result may be written.
    pub(super) fn find_conversion_method(
        &mut self,
        resolutions: &Resolutions,
        source: &Type,
        target: &Type,
    ) -> Option<ConversionMethod> {
        self.find_conversion_method_named(
            resolutions,
            source,
            target,
            fallc_ast::special::SpecialMethod::As.name(),
        )
    }

    /// Finds an `__im_as__` converting `source` to `target`.
    ///
    /// The implicit spelling only: `__as__` alone is explicit and does not make
    /// `Target x = source;` legal. The method's own result type must be
    /// compatible with the target it was selected for, or it converts to
    /// something the position cannot hold.
    pub(super) fn find_implicit_conversion(
        &mut self,
        resolutions: &Resolutions,
        source: &Type,
        target: &Type,
    ) -> Option<ConversionMethod> {
        let method = self.find_conversion_method_named(
            resolutions,
            source,
            target,
            "__im_as__",
        )?;
        if !self.is_compatible(&method.returns, target) {
            return None;
        }
        Some(method)
    }

    /// Records an implicit conversion of `expr` to `target`, when one exists.
    ///
    /// Returns whether it did. The decision is keyed by the source expression's
    /// span, which is where code generation performs the call.
    pub(super) fn try_implicit_conversion(
        &mut self,
        resolutions: &Resolutions,
        ast: &Ast,
        types: &TypeTable,
        expr: fallc_ast::ExprId,
        target: &Type,
    ) -> bool {
        let Some(source) = types.get(expr).cloned() else {
            return false;
        };
        let Some(method) = self.find_implicit_conversion(resolutions, &source, target)
        else {
            return false;
        };
        self.note_throwing_call(ast.expr(expr).span, &method.signature, ast, None);
        self.implicit_conversions
            .insert(ast.expr(expr).span, method);
        true
    }

    fn find_conversion_method_named(
        &mut self,
        resolutions: &Resolutions,
        source: &Type,
        target: &Type,
        written_name: &str,
    ) -> Option<ConversionMethod> {
        let receiver_name = source.nominal_name()?.to_owned();
        let qualified = Self::special_method_qualified(&receiver_name, written_name);
        // A type may define several conversions -- one per target -- so every
        // `__as__` for this receiver is a candidate and the selector picks
        // between them. Looking only at the first found gave "cannot convert"
        // for the second target, however many were defined.
        //
        // Matched on the last name segment, so an imported conversion --
        // bound as `std.__as__String` -- is found.
        for def in self.definitions_named(resolutions, &qualified) {
            let Some(signature) = def.signature.clone() else {
                continue;
            };
            // The selector is the second parameter. A `__as__` without one is
            // not a conversion, and is left to the ordinary argument checking
            // to report.
            let Some(selector) = signature.param_types.get(1) else {
                continue;
            };
            let selector_type = self.resolve_type_syntax(selector, true);
            // The selector names the target by pointing at it: `*double`
            // selects `double`. `const` is not what is being asked about, so it
            // is ignored on both sides.
            let selected = selector_type.pointee().cloned().unwrap_or(selector_type);
            if selected.unqualified() == target.unqualified() {
                // The symbol is built here, from the parameter types this
                // signature actually has. Nothing is looked up by a name that
                // would have to be reproduced elsewhere: `symbol_for` appends
                // the mangled parameters, and every one of them is resolved
                // right here.
                let params: Vec<String> = signature
                    .param_types
                    .iter()
                    .map(|written| {
                        let ty = self.resolve_type_syntax(written, true);
                        crate::types::mangle_type(&ty)
                    })
                    .collect();
                // The *written* name, not the bound one. A special method is
                // bound as `__as__Fraction` so the checker can find it, but it
                // is emitted under `__as__` plus its parameter types, like
                // every other function.
                let symbol = fallc_ast::special::symbol_for(&signature.written_name, &params);
                let returns = self.signature_result_type(&signature);
                return Some(ConversionMethod {
                    symbol,
                    returns,
                    signature,
                    // `__as__` names its target through a parameter, so the
                    // call supplies one.
                    takes_selector: true,
                });
            }
        }
        None
    }

    /// Resolves one `__pointer__` step and records the call under `span`.
    ///
    /// The step keeps its own span because a chain has one call per link, and
    /// code generation walks the links by the symbols it recorded.
    pub(super) fn resolve_pointer_step(
        &mut self,
        resolutions: &Resolutions,
        receiver: &Type,
        span: fallc_span::Span,
        ast: &Ast,
    ) -> Option<Type> {
        self.resolve_unary_result(
            resolutions,
            receiver,
            fallc_ast::special::SpecialMethod::Pointer,
            span,
            ast,
        )
    }

}
