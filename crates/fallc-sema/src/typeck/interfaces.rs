//! Interfaces: structural satisfaction, and the values that erase a type.
//!
//! An interface states a set of restrictions about a type, exactly as a trait
//! does, and a restriction that says what a call yields is also an entry of the
//! table a boxed value carries. A type satisfies the interface by satisfying
//! every restriction -- there is no `implements` to write -- and a value of the
//! interface type is a heap copy of a concrete value beside a table of the
//! functions that act on it. The table is what makes a call through the
//! interface reach the type's own method.
//!
//! Whether a type satisfies an interface is decided in one pass after the items
//! are checked: it needs the resolution table, and the places that check a
//! value's type do not have it. Those places record the expectation instead,
//! and this pass answers it.

use super::*;

impl TypeChecker {
    /// The interface a type names, with its arguments, if it names one.
    pub(super) fn interface_of(&self, ty: &Type) -> Option<(String, Vec<Type>)> {
        let Type::Nominal(nominal) = ty.unqualified() else {
            return None;
        };
        let name = nominal.name.as_str();
        if self.interfaces.contains_key(name) {
            return Some((name.to_owned(), Vec::new()));
        }
        let (base, _) = name.split_once('<')?;
        if !self.interfaces.contains_key(base) {
            return None;
        }
        let (_, arguments) = self.type_template_arguments.get(name)?;
        let types = arguments
            .0
            .iter()
            .filter_map(|argument| match argument {
                crate::template::TemplateArgument::Type(ty) => Some(ty.clone()),
                _ => None,
            })
            .collect();
        Some((base.to_owned(), types))
    }

    /// Whether matching `found` to `expected` needs an interface box.
    ///
    /// A parameter typed as an interface accepts a value of any type by boxing
    /// it -- copying it onto the heap and pairing it with a method table. That
    /// is a conversion rather than a match, so overload and constructor
    /// selection prefers a candidate that takes the argument as it is: with
    /// `Writer(path)` and `Writer(buffer IBuffer)` declared, a path argument
    /// must reach the first.
    pub(super) fn is_interface_boxing(&self, expected: &Type, found: &Type) -> bool {
        let Some((base, arguments)) = self.interface_of(expected) else {
            return false;
        };
        self.interface_of(found) != Some((base, arguments))
    }

    /// Whether some restriction of the interface calls `name` on its subject.
    ///
    /// Such a restriction says the call has to compile, and is not a member of
    /// the table -- `it.begin() -> !void` asks for the operation without saying
    /// what it yields. A call through the box needs the second half, so the two
    /// cases are worth telling apart in a diagnostic.
    fn restriction_names(&self, base: &str, name: &str) -> bool {
        let Some(body) = self.trait_body_for(base) else {
            return false;
        };
        body.declaration.requirements.iter().any(|requirement| {
            let mut expr = requirement.expr;
            while let fallc_ast::ExprKind::Propagate(propagate) = &body.tree.expr(expr).kind {
                expr = propagate.value;
            }
            let fallc_ast::ExprKind::Call(call) = &body.tree.expr(expr).kind else {
                return false;
            };
            let fallc_ast::ExprKind::Field(field) = &body.tree.expr(call.callee).kind else {
                return false;
            };
            field.field == name
        })
    }

    /// The substitution an interface's own parameters take for a member.
    fn interface_substitution(
        &self,
        base: &str,
        arguments: &[Type],
    ) -> Option<crate::template::Substitution> {
        let interface = self.interfaces.get(base)?;
        crate::template::Substitution::new(
            &interface.template_params,
            &crate::template::Arguments(
                arguments
                    .iter()
                    .cloned()
                    .map(crate::template::TemplateArgument::Type)
                    .collect(),
            ),
        )
    }

    /// The symbol of the concrete method that satisfies each interface method.
    ///
    /// `None` means the type does not satisfy the interface, and the reason has
    /// been reported.
    fn interface_methods(
        &mut self,
        resolutions: &Resolutions,
        concrete: &Type,
        base: &str,
        arguments: &[Type],
        span: fallc_span::Span,
    ) -> Option<Vec<String>> {
        let interface = self.interfaces.get(base)?.clone();
        // What a box checks is the restrictions the declaration states, and
        // they are checked by the engine a trait query uses: the two are one
        // declaration used two ways, so a type that is boxed satisfies exactly
        // what a query would answer.
        //
        // The body is looked up rather than read off the declaration, because a
        // requirement is an expression: only the tree it was written in can be
        // checked, and the tree of an imported interface is not here. The
        // declaration's own list is empty in that case; the body is not.
        if let Some(body) = self.trait_body_for(&interface.name) {
            // The variables are the subject and the interface's own type
            // parameters, in the order [`InterfaceDecl::as_trait`] puts them:
            // the concrete type is what the subject stands for, and the
            // arguments are what `IValue<int>` supplies.
            let variables = body.declaration.type_variables();
            let mut bindings = Vec::with_capacity(variables.len());
            if let Some(subject) = variables.first() {
                bindings.push((subject.clone(), concrete.clone()));
            }
            for (variable, argument) in variables.iter().skip(1).zip(arguments) {
                bindings.push((variable.clone(), argument.clone()));
            }
            // The question may be about a type from another module: boxing a
            // program's own buffer into this module's `IBuffer` asks whether
            // the *caller's* `valid` exists, and the body's table knows only
            // this module's names. The caller's definitions are added for the
            // question -- as candidates, not as bindings -- so the subject's
            // type still picks which one answers.
            let mut merged = (*body.resolutions).clone();
            for def in &resolutions.defs {
                if !merged
                    .defs
                    .iter()
                    .any(|existing| {
                        existing.name == def.name && existing.name_span == def.name_span
                    })
                {
                    merged.defs.push(def.clone());
                }
            }
            let question = TraitBody {
                tree: body.tree.clone(),
                resolutions: std::rc::Rc::new(merged.clone()),
                declaration: body.declaration.clone(),
            };
            let outcome = self.trait_models_bound(&question, &bindings);
            if let crate::TraitOutcome::NotModelled { requirement, why } = &outcome {
                // The reason goes in the sentence, not only in a note: a
                // restriction that does not hold is nearly always "the method
                // is not there" or "it raises where the declaration does not",
                // and both are what the reader has to change.
                let mut message = format!(
                    "`{}` does not satisfy `{base}`: the restriction `{requirement}` does not hold",
                    concrete.spelling()
                );
                if let Some(why) = why {
                    message.push_str(&format!(" -- {why}"));
                }
                self.diagnostics.push(
                    Diagnostic::error(message)
                        .with_code(DiagnosticCode::sema(177))
                        .with_span(span)
                        .with_note(
                            "an interface states its restrictions the way a trait does; this \
                             type fails one of them, and the same question asked as a trait \
                             would answer no",
                        ),
                );
                return None;
            }
        }
        let substitution = self.interface_substitution(base, arguments)?;
        let mut symbols = Vec::with_capacity(interface.methods.len());
        for method in &interface.methods {
            let mut expected_params = Vec::with_capacity(method.params.len());
            for param in &method.params {
                expected_params.push(self.resolve_template_type(param, &substitution));
            }
            let expected_returns = self.resolve_template_type(&method.returns, &substitution);

            // The member's arguments, so that a candidate written against a
            // family's parameters -- `&Device<Mode>` for `Device<int>` -- means
            // the member and not the declaration: without them the parameter has
            // no value to put in it, and asking for one reported the family's
            // parameter as an argument of the wrong kind.
            let member_substitution = self.receiver_substitution(concrete);
            let previous_substitution = match &member_substitution {
                Some(substitution) => self.substitution.replace(substitution.clone()),
                None => self.substitution.clone(),
            };
            let mut chosen = None;
            for def in self.definitions_named_for_method(resolutions, &method.name) {
                let Some(signature) = def.signature.clone() else {
                    continue;
                };
                // A method of an interface is a receiver and its own
                // parameters, in that order.
                if signature.param_names.first().map(String::as_str) != Some("this") {
                    continue;
                }
                if signature.param_types.len() != expected_params.len() + 1 {
                    continue;
                }
                // Only the family a candidate is written against can own it,
                // and the name says which that is.
                if self.mentions_template_parameter_syntax(&signature.param_types[0])
                    && let Some(written) = Self::written_nominal_name(&signature.param_types[0])
                    && let Some(given) = concrete.referent().unwrap_or(concrete).nominal_name()
                    && super::methods::short_name(&written) != super::methods::short_name(given)
                {
                    continue;
                }
                let this_ty = self.resolve_type_syntax(&signature.param_types[0], true);
                if !self.receiver_fits(concrete, &this_ty) {
                    continue;
                }
                let mut fits = true;
                for (written, expected) in signature.param_types.iter().skip(1).zip(&expected_params)
                {
                    let param = self.resolve_type_syntax(written, true);
                    if !self.is_compatible(&param, expected) {
                        fits = false;
                        break;
                    }
                }
                if !fits {
                    continue;
                }
                // Whether the method may raise is part of its type: the result
                // travels through the exception ABI, and the call site builds
                // the tag from what the interface declares. A method that
                // disagrees cannot stand in for the one the table promises.
                if signature.throws != method.throws {
                    let (found, wanted) = if signature.throws {
                        ("may raise", "never raises")
                    } else {
                        ("never raises", "may raise")
                    };
                    self.substitution = previous_substitution.clone();
                    self.diagnostics.push(
                        Diagnostic::error(format!(
                            "`{}` cannot satisfy `{base}`: its method `{}` {found}, and `{base}` \
                             declares one that {wanted}",
                            concrete.spelling(),
                            method.name
                        ))
                        .with_code(DiagnosticCode::sema(178))
                        .with_span(span)
                        .with_note(
                            "a method that may raise returns its result through the exception \
                             ABI, so its calls and the interface's have to agree about it",
                        ),
                    );
                    return None;
                }
                let returns = self.signature_result_type(&signature);
                if !self.is_compatible(&returns, &expected_returns) {
                    continue;
                }
                let params: Vec<String> = signature
                    .param_types
                    .iter()
                    .map(|written| {
                        let ty = self.resolve_type_syntax(written, true);
                        crate::types::mangle_type(&ty)
                    })
                    .collect();
                chosen = Some(fallc_ast::special::symbol_for(
                    &signature.written_name,
                    &params,
                ));
                break;
            }
            self.substitution = previous_substitution.clone();
            let Some(symbol) = chosen else {
                let mut diagnostic = Diagnostic::error(format!(
                    "`{}` does not satisfy `{base}`: it has no method `{}` with this signature",
                    concrete.spelling(),
                    method.name
                ))
                .with_code(DiagnosticCode::sema(177))
                .with_span(span)
                .with_note(
                    "a type satisfies an interface by declaring its methods, with `this` \
                     as the receiver and compatible parameter and result types",
                );
                // A method of a type from another module has to be reachable
                // from here, because the table is built where the conversion is
                // written. A private method is not, and saying so beats "no
                // method with this signature" about a method the type has.
                if concrete
                    .nominal_name()
                    .is_some_and(|name| self.imported_type_names.contains(name))
                {
                    diagnostic = diagnostic.with_note(format!(
                        "`{}` is declared in another module, so its method has to be \
                         `export`ed to be reachable from here",
                        concrete.spelling()
                    ));
                }
                self.diagnostics.push(diagnostic);
                return None;
            };
            symbols.push(symbol);
        }
        Some(symbols)
    }

    /// Records the interface conversions the checked expected types asked for.
    pub(super) fn resolve_interface_conversions(
        &mut self,
        resolutions: &Resolutions,
        types: &TypeTable,
    ) {
        let expectations: Vec<(ExprId, (fallc_span::Span, Type))> =
            self.expected_types.drain().collect();
        for (expr, (span, expected)) in expectations {
            let Some((base, arguments)) = self.interface_of(&expected) else {
                continue;
            };
            let Some(found) = types.get(expr).cloned() else {
                continue;
            };
            // A value that is already an interface keeps its object and gets a
            // table of the target's methods, picked out of the source's: the
            // concrete type is erased, so the entries are what is left to copy.
            if let Some((source_base, source_arguments)) = self.interface_of(&found) {
                if let Some(conversion) = self.interface_to_interface(
                    &source_base,
                    &source_arguments,
                    &base,
                    &arguments,
                    &found,
                    &expected,
                    span,
                ) {
                    self.interface_conversions.insert(span, conversion);
                }
                continue;
            }
            let Some(methods) =
                self.interface_methods(resolutions, &found, &base, &arguments, span)
            else {
                continue;
            };
            self.interface_conversions.insert(
                span,
                InterfaceConversion {
                    concrete: found,
                    source: None,
                    interface_type: expected,
                    interface: base,
                    methods,
                },
            );
        }
    }

    /// Rebuilds one interface's value as another interface's.
    ///
    /// The target's methods must be among the source's -- by name, with the
    /// same parameters and result -- because they are the entries the source
    /// table holds. Nothing about the concrete type is needed.
    #[allow(clippy::too_many_arguments)]
    fn interface_to_interface(
        &mut self,
        source_base: &str,
        source_arguments: &[Type],
        target_base: &str,
        target_arguments: &[Type],
        source_type: &Type,
        target_type: &Type,
        span: fallc_span::Span,
    ) -> Option<InterfaceConversion> {
        let source = self.interfaces.get(source_base)?.clone();
        let target = self.interfaces.get(target_base)?.clone();
        let source_substitution = self.interface_substitution(source_base, source_arguments)?;
        let target_substitution = self.interface_substitution(target_base, target_arguments)?;
        let mut entries = Vec::with_capacity(target.methods.len());
        for method in &target.methods {
            let Some(index) = source
                .methods
                .iter()
                .position(|source_method| source_method.name == method.name)
            else {
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "`{target_base}` cannot be made from `{source_base}`: it declares no method `{}`",
                        method.name
                    ))
                    .with_code(DiagnosticCode::sema(178))
                    .with_span(span)
                    .with_note(
                        "an interface value can be used as a narrower interface, whose methods \
                         the wider one already declares",
                    ),
                );
                return None;
            };
            let source_method = &source.methods[index];
            let source_params: Vec<Type> = source_method
                .params
                .iter()
                .map(|param| self.resolve_template_type(param, &source_substitution))
                .collect();
            let target_params: Vec<Type> = method
                .params
                .iter()
                .map(|param| self.resolve_template_type(param, &target_substitution))
                .collect();
            let source_returns =
                self.resolve_template_type(&source_method.returns, &source_substitution);
            let target_returns =
                self.resolve_template_type(&method.returns, &target_substitution);
            if source_params != target_params
                || source_returns != target_returns
                || source_method.throws != method.throws
            {
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "`{target_base}` cannot be made from `{source_base}`: method `{}` has a different signature",
                        method.name
                    ))
                    .with_code(DiagnosticCode::sema(178))
                    .with_span(span)
                    .with_note(
                        "the parameters, the result, and whether the call may raise must be \
                         the same",
                    ),
                );
                return None;
            }
            entries.push(index);
        }
        Some(InterfaceConversion {
            concrete: source_type.clone(),
            source: Some(InterfaceSource {
                base: source_base.to_owned(),
                entries,
            }),
            interface_type: target_type.clone(),
            interface: target_base.to_owned(),
            methods: Vec::new(),
        })
    }

    /// Types a call of an interface method, which dispatches through the table.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn type_of_interface_call(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        field: &FieldExpr,
        node: &CallExpr,
        span: fallc_span::Span,
        base: &str,
        arguments: &[Type],
    ) -> Option<Type> {
        let interface = self.interfaces.get(base)?.clone();
        let Some(index) = interface
            .methods
            .iter()
            .position(|method| method.name == field.field)
        else {
            // A restriction that calls the method without saying what it
            // returns checks that the call compiles, and is not a member of the
            // table: a call through a box has to know what it gets back.
            // Saying which of the two the author wrote is the difference
            // between "no such method" and "add the result type".
            if self.restriction_names(base, &field.field) {
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "`{base}` states `{}` without a result, so it cannot be called through \
                         the box; write `it.{}(..) -> Type` in `{base}` to make it a method a \
                         box can call",
                        field.field, field.field
                    ))
                    .with_code(DiagnosticCode::sema(2))
                    .with_span(span)
                    .with_note(
                        "`-> !void` only says the call yields something, which is enough to \
                         check and not enough to call",
                    ),
                );
                return None;
            }
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "`{base}` has no method `{}`",
                    field.field
                ))
                .with_code(DiagnosticCode::sema(2))
                .with_span(span),
            );
            return None;
        };
        // A virtual call passes its arguments by position: the interface states
        // the types of the parameters, not names for them, so a named argument
        // has nowhere to go. Silently dropping it would call the method with
        // fewer arguments than the program wrote.
        if !node.named.is_empty() {
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "`{base}.{}` is called by position: an interface states its parameters' \
                     types, not their names",
                    field.field
                ))
                .with_code(DiagnosticCode::sema(2))
                .with_span(span)
                .with_note("pass the argument where it belongs in the list"),
            );
            return None;
        }
        let substitution = self.interface_substitution(base, arguments)?;
        let method = interface.methods[index].clone();
        let parameters: Vec<Type> = method
            .params
            .iter()
            .map(|param| self.resolve_template_type(param, &substitution))
            .collect();
        let returns = self.resolve_template_type(&method.returns, &substitution);
        if node.args.len() != parameters.len() {
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "`{}` takes {} argument(s), but {} were given",
                    method.name,
                    parameters.len(),
                    node.args.len()
                ))
                .with_code(DiagnosticCode::sema(36))
                .with_span(span),
            );
            return None;
        }
        for (argument, expected) in node.args.iter().zip(&parameters) {
            if self.is_default_int_literal(ast, *argument) && expected.is_integer() {
                self.adopted_types
                    .insert(ast.expr(*argument).span, expected.clone());
            }
            let previous = self.expected.replace(expected.clone());
            let found = self.infer_expr(ast, resolutions, types, *argument);
            self.expected = previous;
            if let Some(found) = found {
                self.note_indirect_argument(*argument, &found, expected, ast);
                self.types_must_match(expected, &found, ast.expr(*argument).span, "argument");
            }
        }
        // A method that may raise is a throwing call like any other: the
        // exception belongs to this call site, so a `try` here catches it, `?!`
        // passes it on, and otherwise the function has to say it may raise.
        if method.throws {
            self.note_raising_site(
                span,
                &format!(
                    "a call to `{base}.{}` may raise, and nothing here handles it",
                    method.name
                ),
            );
        }
        self.interface_calls.insert(
            span,
            InterfaceCall {
                interface: base.to_owned(),
                index,
                parameters,
                returns: returns.clone(),
                throws: method.throws,
                out_of_line: false,
            },
        );
        Some(returns)
    }
}
