//! Lambdas: anonymous functions, and the check that turns one into code.
//!
//! A `lambda(args) -> expr` is an expression, and a call of one is written with
//! parentheses around it: `(lambda(x int) -> x)(1)`. The parentheses are not
//! decoration -- without them the call would be part of the body, because
//! `lambda(x int) -> x(1)` says the lambda's result is `x(1)`.
//!
//! A parameter's type may be left out. Such a parameter takes the type of the
//! argument in its position, so the same lambda may be checked once per call --
//! exactly as a template is instantiated once per argument list. Each check
//! records its own types and resolutions, because the body's expressions are
//! the same expressions every time.
//!
//! A name in the body that belongs to the enclosing function is captured by
//! reference, which is what makes the lambda see the variable rather than a
//! copy of it.

use super::*;

impl TypeChecker {
    /// The types a lambda's parameters take, with untyped ones decided by a call.
    ///
    /// `node` is the call the lambda is being checked for. `None` means the
    /// lambda is being used as a value, where every parameter must already say
    /// what it is: nothing would decide an omitted type.
    fn lambda_parameter_types(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        lambda: &fallc_ast::LambdaExpr,
        node: Option<&CallExpr>,
        span: fallc_span::Span,
    ) -> Option<Vec<Type>> {
        if let Some(node) = node {
            let given = node.args.len() + node.named.len();
            if given != lambda.parameters.len() {
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "this lambda takes {} argument(s), but {given} were given",
                        lambda.parameters.len()
                    ))
                    .with_code(DiagnosticCode::sema(36))
                    .with_span(span),
                );
                return None;
            }
        }
        // The arguments, in parameter order, so an omitted type has something
        // to be deduced from. Named arguments are rare enough here that a
        // mismatch only makes the deduction fail, which is reported below.
        let positional: Vec<Option<Type>> = node
            .map(|node| {
                node.args
                    .iter()
                    .map(|argument| {
                        let previous = self.expected.take();
                        let found = self.infer_expr(ast, resolutions, types, *argument);
                        self.expected = previous;
                        found
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut parameter_types = Vec::with_capacity(lambda.parameters.len());
        for (index, parameter) in lambda.parameters.iter().enumerate() {
            match &parameter.ty {
                Some(written) => parameter_types.push(self.resolve_type_syntax(written, true)),
                None => {
                    let Some(deduced) = positional.get(index).cloned().flatten() else {
                        self.diagnostics.push(
                            Diagnostic::error(format!(
                                "cannot deduce the type of `{}`: the lambda does not write \
                                 one and the call gives nothing to take it from",
                                parameter.name
                            ))
                            .with_code(DiagnosticCode::sema(71))
                            .with_span(parameter.name_span)
                            .with_note(
                                "write the parameter's type, or call the lambda directly \
                                 with an argument that has one",
                            ),
                        );
                        return None;
                    };
                    parameter_types.push(deduced);
                }
            }
        }
        Some(parameter_types)
    }

    /// Collects the names a lambda captures by reference.
    ///
    /// A captured name is one the body uses that belongs to the enclosing
    /// function: a local or a parameter outside the lambda. A name declared
    /// inside the body is not a capture, and neither is a module-level object,
    /// which the emitted body can reach by itself.
    fn lambda_captures(
        &self,
        ast: &Ast,
        resolutions: &Resolutions,
        lambda: &fallc_ast::LambdaExpr,
    ) -> Vec<Capture> {
        use fallc_ast::visit::{Visitor, walk_expr};

        struct Names {
            found: Vec<(String, fallc_span::Span)>,
        }

        impl Visitor for Names {
            fn visit_expr(&mut self, ast: &Ast, id: fallc_ast::ExprId) {
                if let ExprKind::Name(name) = &ast.expr(id).kind {
                    self.found.push((name.clone(), ast.expr(id).span));
                }
            }
        }

        let mut names = Names { found: Vec::new() };
        match &lambda.body {
            fallc_ast::LambdaBody::Expression(value) => walk_expr(ast, *value, &mut names),
            fallc_ast::LambdaBody::Block(block) => fallc_ast::visit::walk_block(ast, *block, &mut names),
        }

        let parameters: std::collections::HashSet<&str> = lambda
            .parameters
            .iter()
            .map(|parameter| parameter.name.as_str())
            .collect();
        let mut captures: Vec<Capture> = Vec::new();
        for (name, span) in names.found {
            if parameters.contains(name.as_str()) {
                continue;
            }
            if captures.iter().any(|capture| capture.name == name) {
                continue;
            }
            let Some(def) = resolutions
                .resolve(span)
                .and_then(|id| resolutions.def(id))
            else {
                continue;
            };
            // Only a local or a parameter of the enclosing function is a
            // capture: a module-level object has a name of its own, and a
            // constant is a value.
            if !matches!(def.kind, crate::resolve::DefKind::Local | crate::resolve::DefKind::Param)
            {
                continue;
            }
            if def.scope.as_raw() == 0 {
                continue;
            }
            // A declaration inside the lambda's own body is not a capture. Its
            // name span says which it is, and that is true before the body has
            // been checked, which is when this runs.
            let inside_body = def.name_span.lo >= lambda.span.lo
                && def.name_span.hi <= lambda.span.hi;
            if inside_body {
                continue;
            }
            let Some(ty) = self.binding_types.get(def.name_span).cloned() else {
                continue;
            };
            captures.push(Capture {
                name,
                name_span: def.name_span,
                ty,
            });
        }
        captures
    }

    /// Checks a lambda body against one list of parameter types.
    ///
    /// The body is checked in a type table and a record set of its own, because
    /// the same body is checked again for every other list: its expressions are
    /// one set of expressions, and the types they have differ between calls.
    fn instantiate_lambda(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        lambda: &fallc_ast::LambdaExpr,
        parameter_types: Vec<Type>,
        captures: Vec<Capture>,
        span: fallc_span::Span,
    ) -> Option<LambdaInstance> {
        let mut key = format!("{}<", lambda.span.lo);
        for ty in &parameter_types {
            key.push_str(&crate::types::mangle_type(ty));
            key.push(',');
        }
        for capture in &captures {
            key.push_str(&crate::types::mangle_type(&capture.ty));
            key.push(',');
        }
        key.push('>');
        if let Some(instance) = self.lambda_instances.get(&key) {
            return Some(instance.clone());
        }

        let symbol = format!("__lambda.{}${key}", lambda.span.lo);
        let saved_records = self.take_records();
        // The body binds names of its own -- and a lambda written inside another
        // one still sees the enclosing body's -- so the table is copied rather
        // than emptied, and the copy replaces it while the body is checked.
        // What the check leaves behind is this instantiation's table: a local
        // declared in the body has a type per instantiation, exactly as a
        // template's do.
        let previous_bindings = std::mem::take(&mut self.binding_types);
        self.binding_types = previous_bindings.clone();
        // A captured name is a binding of the body too. Where the lambda is
        // written it is already bound, but a call from another function checks
        // the same body with that declaration out of scope, and the body's
        // uses still resolve to it -- so it is bound again here, from the
        // capture's own record.
        for capture in &captures {
            self.binding_types
                .insert(capture.name_span, capture.ty.clone());
        }
        // The parameters are bindings of the body, and their name spans are
        // what the body's uses resolve to.
        for (parameter, ty) in lambda.parameters.iter().zip(&parameter_types) {
            self.binding_types.insert(parameter.name_span, ty.clone());
        }
        let mut lambda_types = TypeTable::new();
        // A lambda body is a function body of its own: it may `raise`, and a
        // raise that no `try` in the same body catches leaves the lambda as a
        // function that raises. The call site then has to handle it.
        self.throws_allowed.push(true);
        self.lambda_raise_stack
            .push((self.protected_depth, false));
        let returns = match &lambda.body {
            fallc_ast::LambdaBody::Expression(value) => {
                let previous = self.expected.take();
                let found = self.infer_expr(ast, resolutions, &mut lambda_types, *value);
                self.expected = previous;
                found
            }
            fallc_ast::LambdaBody::Block(block) => {
                // A body written with braces returns with `return`, so it is
                // checked exactly as a function whose return type is deduced
                // from its first `return` is.
                let placeholder = Type::unspecified("lambda", span);
                self.return_types.push(("<lambda>".to_owned(), placeholder));
                self.pending_auto_returns.push(Some((span, false)));
                let previous = self.expected.take();
                self.check_block(ast, resolutions, &mut lambda_types, *block);
                self.expected = previous;
                self.pending_auto_returns.pop();
                self.return_types.pop().map(|(_, ty)| ty)
            }
        };
        // A raise at this level, or a `??` whose exception nothing here
        // catches, makes the lambda a throwing function.
        let raised = self
            .lambda_raise_stack
            .pop()
            .is_some_and(|(_, raised)| raised);
        let escaped = !self.propagating_calls.is_empty();
        self.throws_allowed.pop();
        let records = self.restore_records(saved_records);
        let bindings = std::mem::replace(&mut self.binding_types, previous_bindings);
        let returns = returns?;
        let instance = LambdaInstance {
            symbol,
            lambda: lambda.clone(),
            parameters: parameter_types,
            captures,
            returns,
            throws: raised || escaped,
            types: lambda_types,
            bindings,
            records,
        };
        self.lambda_instances.insert(key, instance.clone());
        Some(instance)
    }

    /// Types a lambda used as a value.
    ///
    /// A lambda that captures nothing and does not raise *is* a function: its
    /// type is a function type, and it decays to a pointer to itself. Anything
    /// else -- a capture, a raise, a parameter whose type a call decides -- is
    /// a **closure**: a value of an anonymous type that holds the captured
    /// addresses, and callable wherever the program can call it. That is the
    /// same distinction a written function has: a function is a `*fn`, and a
    /// function that raises or is generic is not.
    pub(super) fn type_of_lambda_value(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        lambda: &fallc_ast::LambdaExpr,
        expr: ExprId,
    ) -> Option<Type> {
        let captures = self.lambda_captures(ast, resolutions, lambda);
        let typed = lambda
            .parameters
            .iter()
            .all(|parameter| parameter.ty.is_some());
        if typed {
            let parameter_types = self.lambda_parameter_types(
                ast,
                resolutions,
                types,
                lambda,
                None,
                lambda.span,
            )?;
            let instance = self.instantiate_lambda(
                ast,
                resolutions,
                lambda,
                parameter_types.clone(),
                captures.clone(),
                lambda.span,
            )?;
            if captures.is_empty() && !instance.throws {
                let returns = instance.returns.clone();
                let _ = expr;
                self.lambda_values.insert(lambda.span, instance);
                return Some(Type::Function {
                    parameters: parameter_types,
                    returns: Box::new(returns),
                    abi: None,
                    variadic: false,
                    // A lambda whose body raises is a closure rather than a
                    // function value, so a function type here never raises.
                    throws: false,
                });
            }
        }
        // A closure: the captures are its fields, and a call of it reaches the
        // body through the record kept for its type.
        Some(self.closure_type(ast, lambda, captures))
    }

    /// The anonymous type of a capturing lambda's value.
    ///
    /// The fields are named after the captured variables, in order: the name is
    /// what the value's layout is read by, and it says which address is which.
    /// The type's own name carries the capture types, so one lambda inside a
    /// template is a different type for each instantiation.
    fn closure_type(
        &mut self,
        ast: &Ast,
        lambda: &fallc_ast::LambdaExpr,
        captures: Vec<Capture>,
    ) -> Type {
        let mut name = format!("__lambda.{}", lambda.span.lo);
        for capture in &captures {
            name.push('$');
            name.push_str(&crate::types::mangle_type(&capture.ty));
        }
        let fields: Vec<(String, Type)> = captures
            .iter()
            .map(|capture| {
                (
                    capture.name.clone(),
                    Type::Pointer {
                        pointee: Box::new(capture.ty.clone()),
                        pointee_is_const: false,
                        pointer_is_const: false,
                    },
                )
            })
            .collect();
        let nominal = self
            .nominal_types
            .entry(name.clone())
            .or_insert_with(|| crate::types::NominalType {
                name: name.clone(),
                fields,
                mutable_fields: Vec::new(),
                field_alignments: Vec::new(),
                is_union: false,
                is_incomplete: false,
                union_storage: None,
                template_arguments: None,
            })
            .clone();
        self.closures.insert(
            name.clone(),
            ClosureInfo {
                lambda: lambda.clone(),
                captures,
                name,
                tree: ast.clone(),
            },
        );
        Type::Nominal(nominal)
    }

    /// The closure type a value has, if it is one.
    pub(super) fn closure_name(&self, ty: &Type) -> Option<String> {
        let Type::Nominal(nominal) = ty.unqualified() else {
            return None;
        };
        self.closures
            .get(&nominal.name)
            .map(|info| info.name.clone())
    }

    /// Whether an expression is a value that could be called.
    pub(super) fn callee_is_a_value(
        &self,
        ast: &Ast,
        resolutions: &Resolutions,
        callee: ExprId,
    ) -> bool {
        match &ast.expr(callee).kind {
            ExprKind::Name(_) => resolutions
                .resolve(ast.expr(callee).span)
                .and_then(|id| resolutions.def(id))
                .is_some_and(|def| {
                    matches!(
                        def.kind,
                        crate::resolve::DefKind::Local | crate::resolve::DefKind::Param
                    )
                }),
            ExprKind::Lambda(_) | ExprKind::Field(_) => false,
            _ => true,
        }
    }

    /// Types a call of a closure value.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn type_of_closure_call(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        node: &CallExpr,
        callee: ExprId,
        name: &str,
        callee_ty: &Type,
        span: fallc_span::Span,
    ) -> Option<Type> {
        let info = self.closures.get(name)?.clone();
        let parameter_types = self.lambda_parameter_types(
            ast,
            resolutions,
            types,
            &info.lambda,
            Some(node),
            span,
        )?;
        let instance = self.instantiate_lambda(
            ast,
            resolutions,
            &info.lambda,
            parameter_types.clone(),
            info.captures.clone(),
            span,
        )?;
        self.check_lambda_arguments(ast, resolutions, types, node, &parameter_types)?;
        let returns = instance.returns.clone();
        if instance.throws {
            self.note_throwing_lambda_call(span);
        }
        self.lambda_calls.insert(span, instance.clone());
        self.closure_calls.insert(
            span,
            ClosureCall {
                value: callee,
                ty: callee_ty.clone(),
                instance,
                tree: info.tree.clone(),
            },
        );
        Some(returns)
    }

    /// Checks a lambda call's arguments against its parameters.
    fn check_lambda_arguments(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        node: &CallExpr,
        parameter_types: &[Type],
    ) -> Option<()> {
        // The arguments are checked against the parameters, with the same
        // rules a function call uses: an integer literal adopts the parameter's
        // type, and a reference parameter takes the argument's address.
        for (index, &argument) in node.args.iter().enumerate() {
            let Some(expected) = parameter_types.get(index) else {
                continue;
            };
            if self.is_default_int_literal(ast, argument) && expected.is_integer() {
                self.adopted_types
                    .insert(ast.expr(argument).span, expected.clone());
            }
            let previous = self.expected.replace(expected.clone());
            let found = self.infer_expr(ast, resolutions, types, argument);
            self.expected = previous;
            if let Some(found) = found {
                self.note_indirect_argument(argument, &found, expected, ast);
                let was_parameter = self.in_parameter_position;
                self.in_parameter_position = true;
                self.types_must_match(expected, &found, ast.expr(argument).span, "argument");
                self.in_parameter_position = was_parameter;
            }
        }
        Some(())
    }

    /// Types a call whose callee is a lambda expression.
    ///
    /// The call decides the types of any untyped parameters, and the body is
    /// checked against them; the symbol recorded here is what code generation
    /// emits and calls.
    pub(super) fn type_of_lambda_call(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        node: &CallExpr,
        lambda: &fallc_ast::LambdaExpr,
        span: fallc_span::Span,
    ) -> Option<Type> {
        let parameter_types =
            self.lambda_parameter_types(ast, resolutions, types, lambda, Some(node), span)?;
        let captures = self.lambda_captures(ast, resolutions, lambda);
        let instance = self.instantiate_lambda(
            ast,
            resolutions,
            lambda,
            parameter_types.clone(),
            captures,
            span,
        )?;
        self.check_lambda_arguments(ast, resolutions, types, node, &parameter_types)?;
        let returns = instance.returns.clone();
        // A lambda whose body raises is a throwing function, so the call is a
        // throwing call: it must be handled here, or passed on with `??`.
        if instance.throws {
            self.note_throwing_lambda_call(span);
        }
        self.lambda_calls.insert(span, instance);
        Some(returns)
    }

    /// Records that a call may raise, for a callee that has no signature.
    ///
    /// The rules are `note_throwing_call`'s: a `try` here catches it, `??`
    /// passes it on, and otherwise the call site has to say what it does.
    fn note_throwing_lambda_call(&mut self, span: fallc_span::Span) {
        self.note_raising_site(span, "this lambda may raise, and nothing here handles it");
    }
}
