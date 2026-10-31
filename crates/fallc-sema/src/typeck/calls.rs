//! Calls: free functions, constructors, module functions, and overload
//! resolution.
//!
//! A call is where the argument list meets a signature. The pairing of
//! arguments to parameters, the choice among overloads and the checks each
//! argument passes are all decided here.

use super::*;

impl TypeChecker {
    /// The result type a signature's call produces, or `None` when it returns
    /// nothing.
    ///
    /// A signature that crossed a module boundary carries the defining module's
    /// own resolution of its result -- `auto` is *deduced* there, and a member
    /// of a type family is one particular member. Resolving the written syntax
    /// again here could only guess, so an imported `auto` came back as the word
    /// `auto` and no caller could use it.
    pub(super) fn signature_result(&mut self, signature: &FunctionSignature) -> Option<Type> {
        if signature.return_type.as_name() == Some("") {
            return None;
        }
        Some(self.signature_result_type(signature))
    }

    /// The resolved result type of a signature, `void` included.
    ///
    /// Separate from [`Self::signature_result`] because some callers want the
    /// type itself -- an interface method with no result still has to compare
    /// equal to a method that returns nothing.
    pub(super) fn signature_result_type(&mut self, signature: &FunctionSignature) -> Type {
        match &signature.resolved {
            Some(resolved) => resolved.returns.clone(),
            None => self.resolve_type_syntax(&signature.return_type, false),
        }
    }

    /// The parameter types a signature's call is checked against.
    ///
    /// The defining module's resolution again when the signature came from one:
    /// the written types belong to a tree this file does not have.
    pub(super) fn signature_parameters(&mut self, signature: &FunctionSignature) -> Vec<Type> {
        match &signature.resolved {
            Some(resolved) => resolved.params.clone(),
            None => signature
                .param_types
                .iter()
                .map(|written| self.resolve_type_syntax(written, true))
                .collect(),
        }
    }

    /// Checks a call whose callee is a value of function type.
    ///
    /// `p(1)` where `p *fn(int) int` calls through the pointer: a function
    /// pointer is called the way a function is, so there is no dereference to
    /// write and nothing to look up. The parameters come from the type, which
    /// is the only place they exist -- there is no definition to ask.
    pub(super) fn type_of_indirect_call(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        node: &CallExpr,
        span: fallc_span::Span,
    ) -> Option<Type> {
        let ExprKind::Name(name) = &ast.expr(node.callee).kind else {
            return None;
        };
        // Only a value is called this way. A name that denotes a function has
        // already been through the overload search, and a type name constructs.
        let def = resolutions.defs.iter().find(|def| def.name == *name)?;
        if !matches!(
            def.kind,
            crate::resolve::DefKind::Local | crate::resolve::DefKind::Param
        ) {
            return None;
        }
        let callee_ty = self.binding_types.get(def.name_span)?.clone();
        // The callee is an expression like any other, and code generation needs
        // its type to know which convention the call uses. Inferring it here
        // records the type where the lowering reads it.
        let _ = self.infer_expr(ast, resolutions, types, node.callee);
        let function = match callee_ty.unqualified() {
            Type::Function { .. } => callee_ty.unqualified().clone(),
            // A function name used as a value decays to a pointer to it, so a
            // pointer is only callable when it points at a function.
            Type::Pointer { pointee, .. }
                if matches!(pointee.unqualified(), Type::Function { .. }) =>
            {
                pointee.unqualified().clone()
            }
            _ => return None,
        };
        self.check_indirect_call(ast, resolutions, types, node, span, &function, &callee_ty)
    }

    /// Checks the arguments of a call through a value of function type.
    ///
    /// The parameters come from the type, which is the only place they exist:
    /// there is no definition to ask, and no name to look up.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn check_indirect_call(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        node: &CallExpr,
        span: fallc_span::Span,
        function: &Type,
        callee_ty: &Type,
    ) -> Option<Type> {
        let Type::Function {
            parameters,
            returns,
            variadic,
            throws,
            ..
        } = function
        else {
            return None;
        };
        let parameters = parameters.clone();
        let returns = (**returns).clone();
        let variadic = *variadic;
        let throws = *throws;
        // A call through a pointer to a function that may raise is a throwing
        // call: the callee returns its result through the exception structure,
        // so the call site owes the same thing a named throwing call owes.
        let raising = throws;
        if (!variadic && node.args.len() != parameters.len())
            || (variadic && node.args.len() < parameters.len())
        {
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "`{}` is called with {} argument(s), but `{}` takes {}",
                    callee_ty.spelling(),
                    node.args.len(),
                    function.spelling(),
                    parameters.len()
                ))
                .with_code(DiagnosticCode::sema(36))
                .with_span(span),
            );
            return None;
        }
        for (index, arg) in node.args.iter().enumerate() {
            match parameters.get(index) {
                Some(expected) => {
                    let expected = expected.clone();
                    let _ = self.check_expr(ast, resolutions, types, *arg, &expected);
                }
                // The trailing arguments of a variadic function have no
                // declared type, so they are only inferred.
                None => {
                    let _ = self.infer_expr(ast, resolutions, types, *arg);
                }
            }
        }
        if raising {
            self.note_raising_site(
                span,
                &format!(
                    "a call through `{}` may raise, and nothing here handles it",
                    callee_ty.spelling()
                ),
            );
        }
        Some(returns)
    }

    /// Types a call.
    ///
    /// The callee decides the path: a member access is a method call or a
    /// module call, a value of function type is called through, and a name is
    /// matched against every function of that name. A candidate applies when
    /// the count fits -- a defaulted parameter is optional, and a variadic tail
    /// takes any number -- and every argument's type matches its parameter; see
    /// [`TypeChecker::resolve_overload`] for how one is chosen.
    pub(super) fn type_of_call(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        node: &CallExpr,
        span: fallc_span::Span,
    ) -> Option<Type> {
        // A lambda is called where it is written, and its parameters may be
        // decided by this call. Checked first: a lambda is not a name to look
        // up, and not a value whose type exists before the call.
        if let ExprKind::Lambda(lambda) = &ast.expr(node.callee).kind {
            return self.type_of_lambda_call(ast, resolutions, types, node, lambda, span);
        }

        // A call of a closure value: the value says which lambda it came from,
        // and the call decides whatever parameters it left untyped.
        if self.callee_is_a_value(ast, resolutions, node.callee) {
            let previous = self.expected.take();
            let callee_ty = self.infer_expr(ast, resolutions, types, node.callee);
            self.expected = previous;
            if let Some(name) = callee_ty.as_ref().and_then(|ty| self.closure_name(ty)) {
                let ty = callee_ty.clone()?;
                return self.type_of_closure_call(
                    ast,
                    resolutions,
                    types,
                    node,
                    node.callee,
                    &name,
                    &ty,
                    span,
                );
            }
        }

        // A call whose callee is a member access is a method call: `p.show()`
        // means `show(&p)`, where `show` is a function whose first parameter is
        // named `this` and is a pointer. The address is taken automatically,
        // so a method may be called on a value.
        if let ExprKind::Field(field) = &ast.expr(node.callee).kind {
            // A path that names a *type* constructs one: `std.Array<int>()` is
            // a member of that family, `pair.Pair(3, 4)` a type of the module
            // that declares it, `std.string.String("hi")` one whose module is
            // itself dotted. All three are written like a method call, and none
            // of them is one, so the whole written path is looked up as a type
            // before either module form below gets to read it as a function.
            if let Some(path) = written_path(ast, node.callee)
                && (self.type_templates.contains_key(&path)
                    || self.nominal_types.contains_key(&path))
            {
                return self.type_of_constructor_call(ast, resolutions, types, &path, node, span);
            }
            // `C.printf(..)` and `p.show()` are written identically, and only
            // name resolution can tell them apart: `C` is a module, `p` is a
            // value. That is the decision the grammar deliberately left open.
            //
            // A module's name may itself be dotted: `std.uuid.v4()` calls `v4`
            // in the module `std.uuid`, which the umbrella module publishes as
            // a name of its own. The object half is then not a `Name` but a
            // path, so the whole written path is split at its longest module
            // prefix rather than at its first dot.
            if let Some(path) = written_path(ast, field.object) {
                let mut module = path.as_str();
                let mut rest = field.field.as_str();
                let mut prefix = None;
                loop {
                    if self.is_module(module)
                        && resolutions.resolve(ast.expr(field.object).span).is_none()
                    {
                        prefix = Some(module.to_owned());
                        break;
                    }
                    let Some(sep) = module.rfind('.') else {
                        break;
                    };
                    rest = &path[sep + 1..];
                    module = &path[..sep];
                }
                if let Some(module) = prefix {
                    return self.type_of_module_call(
                        ast,
                        resolutions,
                        types,
                        &module,
                        rest,
                        node,
                        span,
                    );
                }
            }
            return self.type_of_method_call(ast, resolutions, types, field, node, span);
        }

        // A callee whose *type* is a function, or a pointer to one, is called
        // through: `pick()(1)` calls what a call returned, and the type is the
        // only description of it there is.
        if !matches!(&ast.expr(node.callee).kind, ExprKind::Name(_)) {
            let callee_ty = self.infer_expr(ast, resolutions, types, node.callee);
            if let Some(callee_ty) = callee_ty
                && let Some(function) = callee_ty.function_type().cloned()
            {
                return self.check_indirect_call(
                    ast,
                    resolutions,
                    types,
                    node,
                    span,
                    &function,
                    &callee_ty,
                );
            }
            // Otherwise a value of a type that defines `__call__` is callable,
            // and the object is the receiver: the arguments are checked against
            // the method's, which are its parameters after the receiver.
            return self.type_of_call_method(ast, resolutions, types, node, span);
        }

        let ExprKind::Name(name) = &ast.expr(node.callee).kind else {
            unreachable!("the callee is a name");
        };
        self.type_of_named_call(ast, resolutions, types, name, node, span)
    }

    /// The type a template's substitution binds `name` to, if it is a type.
    ///
    /// Used where a type parameter stands in expression position -- `T(..)` in
    /// a template body -- so that the construction reaches the member or type
    /// the instantiation substituted rather than looking for a declaration
    /// named after the parameter.
    fn substituted_template_type(&self, name: &str) -> Option<Type> {
        let substitution = self.substitution.as_ref()?;
        match substitution.get(name)? {
            crate::template::TemplateArgument::Type(ty) => Some(ty.clone()),
            _ => None,
        }
    }

    /// Checks a call to a function declared under `name`.
    ///
    /// A local name and a module-qualified name are the same question -- which
    /// declaration do these arguments call -- so they reach the same code. An
    /// imported module's declarations are the forward declarations, and its
    /// body is compiled with the program to provide the definitions.
    fn type_of_named_call(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        name: &String,
        node: &CallExpr,
        span: fallc_span::Span,
    ) -> Option<Type> {
        // A *template parameter* used as a constructor: inside a template body
        // `T(..)` is whatever the substitution says `T` is. Nothing in the
        // name namespace answers that -- `T` is not a declaration -- so without
        // this the construction was unresolved and a query like
        // `except(T(fake<&&T>()))` folded to "cannot raise" for every member.
        if node.template_args.is_empty()
            && let Some(target) = self.substituted_template_type(name)
            && target.is_nominal()
        {
            // A probe may build a type that has no constructor at all -- the
            // question is only whether the construction would raise -- so a
            // missing one answers "nothing to call" rather than an error. A
            // real `T(..)` over a parameter with no constructor is reported by
            // the ordinary path below, which names the parameter.
            if let Some(symbol) =
                self.constructor_call_symbol(&target, node, ast, resolutions, types, span)
            {
                self.constructor_calls.insert(span, symbol);
                self.constructor_types.insert(span, target.clone());
                return Some(target);
            }
        }

        // `Array<int>(..)` is a call on a *type*, which constructs a value.
        // Checked before the overload search, because a type is not a function
        // and the search would report the name as unresolvable.
        if !node.template_args.is_empty() {
            let result = self.type_of_constructor_call(ast, resolutions, types, name, node, span);
            if let Some(result) = result {
                return Some(result);
            }
        }

        // `str(x)` and `repr(x)` are the two ways a program asks for text.
        // They are handled here, before the ordinary overload search, because
        // what they do is not a property of `str` but of `x`: a type that
        // declares `__str__` is asked for it, and everything else is rendered
        // by the compiler.
        if name == "str" || name == "repr" {
            return self.type_of_text_call(ast, resolutions, types, node, name, span);
        }

        // A template is not a function, so it cannot be overloaded with one and
        // is looked for separately. Calling it means *instantiating* it: the
        // template arguments are deduced from the call or written out, and the
        // result is a function this call then reaches like any other.
        // A template that does not deduce is silent when an ordinary overload
        // under the same name may be the answer -- `std.reverse` is
        // `String.reverse` and the array's template -- so that diagnosis is
        // dropped before the overloads are looked at. When there is no ordinary
        // overload, the template's own answer is the whole answer: an
        // `@enable_if` that said no must say so, not disappear.
        let has_ordinary = resolutions.defs.iter().any(|definition| {
            definition.name == *name
                && definition.signature.as_ref().is_some_and(|signature| {
                    signature.template_params.is_empty() && signature.pack.is_none()
                })
        });
        let checkpoint = self.diagnostics.len();
        if let Some(instantiated) =
            self.instantiate_template(ast, resolutions, types, name, node, span)
        {
            return Some(instantiated);
        }
        if has_ordinary {
            self.diagnostics.truncate(checkpoint);
        }

        // Every function with this name is a candidate; which one applies is
        // decided by the arguments.
        let disabled = &self.disabled_functions;
        // A definition written here replaces an `@overridable` default, so the
        // defaults are dropped as soon as any other definition of the name
        // exists. This is what a library states a fallback with: the program
        // that wants its own writes a definition and says nothing else.
        let has_replacement = resolutions.defs.iter().any(|def| {
            def.name == *name
                && !disabled.contains(&def.name_span)
                && !def.is_overridable
                && def.signature.as_ref().is_some_and(|signature| {
                    signature.template_params.is_empty() && signature.pack.is_none()
                })
        });
        let candidates: Vec<FunctionSignature> = resolutions
            .defs
            .iter()
            .filter(|def| def.name == *name && !disabled.contains(&def.name_span))
            .filter(|def| !has_replacement || !def.is_overridable)
            // A module-qualified name names a free function: a member's `this`
            // is a receiver the call did not write.
            .filter(|def| {
                !name.contains('.')
                    || def.signature.as_ref().is_none_or(|signature| {
                        signature.param_names.first().map(String::as_str) != Some("this")
                    })
            })
            .filter(|def| {
                // A template is not a candidate: it has no parameter types a
                // call could fit, because its parameters name the very things
                // being deduced.
                def.signature.as_ref().is_none_or(|signature| {
                    signature.template_params.is_empty() && signature.pack.is_none()
                })
            })
            .filter_map(|def| def.signature.clone())
            .collect();
        if candidates.is_empty() {
            // A name whose only declarations `@enable_if` removed is not an
            // unknown name: the declaration exists, and the condition said it
            // does not apply to this program.
            if resolutions
                .defs
                .iter()
                .any(|def| def.name == *name && self.disabled_functions.contains(&def.name_span))
            {
                self.diagnostics.push(
                    Diagnostic::error(format!("`{name}` is disabled by `@enable_if`"))
                        .with_code(DiagnosticCode::sema(175))
                        .with_span(span),
                );
                return None;
            }
            // A name that is not a function may be a *value* whose type is a
            // function or a pointer to one: `p(1)` with `p *fn(int) int` is how
            // a function pointer is called, and there is no dereference to
            // write.
            if let Some(called) = self.type_of_indirect_call(ast, resolutions, types, node, span) {
                return Some(called);
            }
            // A name that is not a function may be a *type*, and calling one
            // constructs a value of it: `Array<int>()` is the empty array.
            // Checked before `__call__`, because a type name cannot be a value
            // and there is nothing for the two to be confused about.
            if let Some(result) =
                self.type_of_constructor_call(ast, resolutions, types, name, node, span)
            {
                return Some(result);
            }
            // A name that is not a function may still name a *value* whose type
            // defines `__call__`, so the callee is typed before giving up:
            // `c(5)` and `f(5)` are written identically and only the type says
            // which it is.
            if let Some(result) = self.type_of_call_method(ast, resolutions, types, node, span) {
                return Some(result);
            }
            // Nothing left to say. A name the resolver could not bind has
            // already been reported, and a name bound to something uncallable
            // was explained by `type_of_call_method` --
            // "`i32` cannot be called". A generic "is not a function" after
            // either of those turns one mistake into two sentences.
            return None;
        }

        let signature =
            self.resolve_overload(name, &candidates, node, ast, resolutions, types, span)?;
        if !self.record_omitted_defaults(name, &signature, node, resolutions, span) {
            return None;
        }

        // Using a declaration marked `@deprecated` is exactly what the
        // attribute exists to make visible, so every use says so.
        if let Some(message) = self.deprecated_functions.get(name.as_str()).cloned() {
            self.warn_deprecated(name, message, ast.expr(node.callee).span);
        }

        // Which definition this call reached is recorded, from the signature's
        // own parameter types. Code generation cannot work it out again: the
        // argument's recorded type is not always the parameter's -- a string
        // literal decays to a pointer, an unsuffixed literal adopts whatever
        // integer is wanted -- and rebuilding the symbol from the arguments
        // therefore chose the wrong overload, silently, whenever two candidates
        // differed only in that way.
        // A function implemented elsewhere keeps the name it was declared
        // with: the C library exports exactly that spelling, and a mangled one
        // names no symbol anybody defines.
        let symbol = if signature.abi.is_some() {
            signature.written_name.clone()
        } else {
            let params: Vec<String> = signature
                .param_types
                .iter()
                .map(|written| {
                    let ty = self.resolve_type_syntax(written, true);
                    crate::types::mangle_type(&ty)
                })
                .collect();
            fallc_ast::special::symbol_for(&signature.written_name, &params)
        };
        if self.must_use_functions.contains(&signature.written_name) {
            self.must_use_calls.insert(span);
        }
        self.call_symbols.insert(span, symbol);
        self.note_throwing_call(span, &signature, ast, Some(node));

        // The argument count and types were settled by the overload decision,
        // which already rejected a call that fits no candidate. Checking them
        // again here would reject a call that omits a defaulted parameter,
        // because this count does not know about defaults.

        // The trailing arguments of a variadic call are not type checked,
        // because nothing declares their types. C's `printf` relies on exactly
        // this.
        //
        // They are still *inferred*, though: the code generator reads each
        // argument's type from the table, so an argument that was never
        // inferred has no type to lower with. Skipping them entirely made
        // `C.printf(b"%f", 1.5f as double)` fail at code generation.
        for arg in node.args.iter().skip(signature.param_types.len()) {
            let previous = self.expected.take();
            let found = self.infer_expr(ast, resolutions, types, *arg);
            self.expected = previous;
            // A variadic slot has no declared type, but it is still a *value*
            // position: a reference argument is passed as what it names, not as
            // its address. Without this `printf("%d", r)` handed the callee a
            // pointer and printed it.
            if let Some(found) = found {
                let value_position = Type::unspecified("", ast.expr(*arg).span);
                self.note_reference_read(ast, *arg, &found, &value_position);
            }
        }

        let expected_params = self.signature_parameters(&signature);
        for (arg, expected_ty) in node.args.iter().zip(&expected_params) {
            let expected_ty = expected_ty.clone();
            // An unsuffixed integer literal adopts the parameter's type, so
            // that is what code generation has to pass. The literal's own type
            // stays the default `int`, which is what it is on its own.
            if self.is_default_int_literal(ast, *arg) && expected_ty.is_integer() {
                self.adopted_types
                    .insert(ast.expr(*arg).span, expected_ty.clone());
            }
            let previous = self.expected.replace(expected_ty.clone());
            let found = self.infer_expr(ast, resolutions, types, *arg);
            self.expected = previous;
            // Compatibility is decided by `types_must_match` alone. Comparing
            // here as well would skip it whenever the types differ textually,
            // which is exactly when the const-widening rule applies.
            if let Some(found) = found {
                self.note_indirect_argument(*arg, &found, &expected_ty, ast);
                let was_parameter = self.in_parameter_position;
                self.in_parameter_position = true;
                if !self.adopt_literal_argument(resolutions, ast, *arg, &expected_ty, &found) {
                    if super::methods::integer_promotes(&found, &expected_ty) {
                        self.adopted_types
                            .insert(ast.expr(*arg).span, expected_ty.clone());
                    } else if self.try_implicit_conversion(
                        resolutions,
                        ast,
                        types,
                        *arg,
                        &expected_ty,
                    ) {
                    } else {
                        self.types_must_match(
                            &expected_ty,
                            &found,
                            ast.expr(*arg).span,
                            "argument",
                        );
                    }
                }
                self.in_parameter_position = was_parameter;
            }
        }

        // A builtin that returns nothing has an empty return type. The call
        // still type checks; it just produces no value, which is what lets it
        // appear as a statement.
        self.signature_result(&signature)
    }

    /// Records the constant defaults a call leaves its parameters to.
    ///
    /// A default is an expression in the declaring module's tree, so a call from
    /// another module cannot evaluate it. A value can cross -- the exporter
    /// folded it where the expression lives -- and that is what lets
    /// `getline(line)` leave out `until = '\n'`. A default that is *not* a
    /// constant stays behind: the call has to give it, and the diagnostic says
    /// so rather than lowering an expression from the wrong tree.
    fn record_omitted_defaults(
        &mut self,
        name: &str,
        signature: &crate::FunctionSignature,
        node: &CallExpr,
        resolutions: &Resolutions,
        span: fallc_span::Span,
    ) -> bool {
        if !self.signature_is_imported(resolutions, signature) {
            return true;
        }
        let mut given = vec![false; signature.param_types.len()];
        let positional = node.args.len().min(given.len());
        for slot in given.iter_mut().take(positional) {
            *slot = true;
        }
        for argument in &node.named {
            if let Some(index) = signature
                .param_names
                .iter()
                .position(|parameter| *parameter == argument.name)
                && let Some(slot) = given.get_mut(index)
            {
                *slot = true;
            }
        }
        for (index, filled) in given.iter().enumerate() {
            if *filled || signature.defaults.get(index).copied().flatten().is_none() {
                continue;
            }
            if !self.record_omitted_default(name, signature, index, span) {
                return false;
            }
        }
        true
    }

    /// Records one parameter a call leaves to its default.
    ///
    /// A default that the declaring module folded to a value crosses as that
    /// value. One that is an expression is lowered at the call site with the
    /// declaring module's tree -- `b int = a + 1` is written against `a`, and
    /// the call site has `a`'s value in hand. A template's default is neither:
    /// it may name the template's parameters, and the caller's arguments for
    /// those are not what the expression is written against, so it is refused
    /// there.
    pub(super) fn record_omitted_default(
        &mut self,
        name: &str,
        signature: &crate::FunctionSignature,
        index: usize,
        span: fallc_span::Span,
    ) -> bool {
        if let Some(value) = signature.default_values.get(index).copied().flatten() {
            let ty = self.resolve_type_syntax(&signature.param_types[index], true);
            self.default_arguments
                .entry(span)
                .or_default()
                .push((index, value, ty));
            return true;
        }
        let Some(default) = signature.defaults.get(index).copied().flatten() else {
            return true;
        };
        let Some(module) = signature.declared_in.clone() else {
            return true;
        };
        if signature.template_params.is_empty() {
            let preceding = (0..index)
                .map(|parameter| {
                    let name = signature
                        .param_names
                        .get(parameter)
                        .cloned()
                        .unwrap_or_default();
                    let ty = self.resolve_type_syntax(&signature.param_types[parameter], true);
                    (name, ty)
                })
                .collect();
            self.default_expressions
                .entry(span)
                .or_default()
                .push((index, default, module, preceding));
            return true;
        }
        let parameter = signature
            .param_names
            .get(index)
            .cloned()
            .unwrap_or_else(|| format!("parameter {}", index + 1));
        self.diagnostics.push(
            Diagnostic::error(format!(
                "`{name}` omits `{parameter}`, whose default belongs to a template's parameter"
            ))
            .with_code(DiagnosticCode::sema(180))
            .with_span(span)
            .with_note(
                "a template's default may name the template's own parameters, which a call in \
                 another module cannot supply; give the argument",
            ),
        );
        false
    }

    /// Whether a signature came from another module's export table.
    ///
    /// A declaration in this tree carries the node its body was written at; an
    /// imported one has no node here, because the tree it names is another
    /// file's.
    pub(super) fn signature_is_imported(
        &self,
        resolutions: &Resolutions,
        signature: &crate::FunctionSignature,
    ) -> bool {
        resolutions.defs.iter().any(|def| {
            def.node.is_none()
                && !def.is_builtin
                && def.signature.as_ref().is_some_and(|candidate| {
                    candidate.bound_as == signature.bound_as
                        && candidate.param_types == signature.param_types
                })
        })
    }

    /// Types `T(args)`, which constructs a value of `T`.
    ///
    /// # The form
    ///
    /// `Array<int>()` is the empty array and `Array<int>(3, 7)` three sevens.
    /// The arguments are the constructor's, after the receiver -- the same call
    /// `T x = ...;` makes, spelled where no binding supplies the type.
    ///
    /// `None` when the name is not a type, which is the ordinary case: a call
    /// to a function never reaches here.
    pub(super) fn type_of_constructor_call(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        name: &str,
        node: &CallExpr,
        span: fallc_span::Span,
    ) -> Option<Type> {
        // A name that is not a type at all is not this.
        let known = self.nominal_types.contains_key(name)
            || self.type_templates.contains_key(name)
            // An alias is a type too: `String16` names `BasicString<u16>`, and
            // `String16()` constructs one exactly as the alias' target does.
            || self.aliases.contains_key(name)
            || resolutions
                .defs
                .iter()
                .any(|def| def.kind == crate::resolve::DefKind::Type && def.name == name);
        if !known {
            return None;
        }

        let target = self.resolve_type_syntax(
            &fallc_ast::TypeSyntax::Named(fallc_ast::NamedType {
                path: name.to_owned(),
                arguments: node.template_args.clone(),
                span,
                is_expansion: false,
            }),
            false,
        );
        if target.is_unspecified() {
            return None;
        }

        // The constructor is found by the arguments, exactly as an overloaded
        // function's is: a receiver parameter, then one per argument. A type
        // with no fitting constructor has nothing to call, and the caller
        // reports the name as not a function.
        let Some(symbol) =
            self.constructor_call_symbol(&target, node, ast, resolutions, types, span)
        else {
            self.report_missing_constructor(&target, node, span);
            return None;
        };
        // Code generation allocates the receiver and calls this, so the call is
        // recorded as a construction rather than as an ordinary call.
        self.constructor_calls.insert(span, symbol);
        self.constructor_types.insert(span, target.clone());
        Some(target)
    }

    /// Finds the constructor a declaration with no initialiser calls.
    ///
    /// `T x;` is not the bytes of zero: a type that defines `__init__(this &T)`
    /// is *constructed*, exactly as `T x = T();` would construct it. The two
    /// were different, so a type whose empty state is not all-zero bytes --
    /// `HashTable` allocates its buckets, and a device takes `-1` for "nothing
    /// pushed" -- started life in a state its own constructor never produces.
    ///
    /// A type with no such constructor is left alone; the binding is then the
    /// zero value, which is what a plain aggregate means.
    pub(super) fn default_constructor_symbol(
        &mut self,
        target: &Type,
        resolutions: &Resolutions,
    ) -> Option<String> {
        let receiver_name = target.nominal_name()?.to_owned();
        let base = receiver_name
            .split_once('<')
            .map_or(receiver_name.as_str(), |(base, _)| base);
        let base = base.rsplit_once('.').map_or(base, |(_, last)| last);
        let member_substitution = self.member_substitution(target.nominal_name());
        for definition in self.constructor_definitions(resolutions, base) {
            let Some(signature) = definition.signature.clone() else {
                continue;
            };
            // The receiver and nothing else: that is what "no arguments" means
            // for a constructor.
            if signature.param_types.len() != 1 || signature.required_params > 1 {
                continue;
            }
            let previous = match &member_substitution {
                Some(substitution) => self.substitution.replace(substitution.clone()),
                None => self.substitution.take(),
            };
            let params: Vec<String> = signature
                .param_types
                .iter()
                .map(|written| {
                    let ty = self.resolve_type_syntax(written, true);
                    crate::types::mangle_type(&ty)
                })
                .collect();
            self.substitution = previous;
            let symbol = fallc_ast::special::symbol_for(&signature.written_name, &params);
            if let Some(substitution) = &member_substitution {
                self.record_method_instantiation(&signature, substitution, &symbol, target);
            }
            return Some(symbol);
        }
        None
    }

    /// The constructor definitions a nominal type offers.
    ///
    /// Asking for `__init__` finds `__im_init__` as well -- declaring the
    /// implicit form declares the explicit one too -- so one lookup is enough.
    fn constructor_definitions(&self, resolutions: &Resolutions, base: &str) -> Vec<crate::Def> {
        self.definitions_named(resolutions, &format!("__init__{base}"))
    }

    /// Reports a `T(args)` call that no constructor of `T` accepts.
    ///
    /// A type with no fitting constructor has nothing to call, and saying so
    /// here is what keeps the mistake a type error rather than a failure in
    /// code generation. The brace form is the way to build a type that declares
    /// no `__init__`: the fields, in order.
    pub(super) fn report_missing_constructor(
        &mut self,
        target: &Type,
        node: &CallExpr,
        span: fallc_span::Span,
    ) {
        self.diagnostics.push(
            Diagnostic::error(format!(
                "`{}` has no constructor taking {} argument(s)",
                target.spelling(),
                node.args.len()
            ))
            .with_code(DiagnosticCode::sema(36))
            .with_span(span)
            .with_note(format!(
                "a type is built positionally with `{0}{{..}}`, which needs no `__init__`; \
                 the call form `{0}(..)` needs one",
                target.spelling()
            )),
        );
    }

    /// Finds the constructor a `T(args)` call reaches.
    ///
    /// The parameters after the receiver are matched against the arguments the
    /// way any overload is, and the symbol is mangled from the instantiated
    /// types -- so `Array<int>()` and `Array<int>(3, 7)` are two constructors
    /// and `Array<f64>()` is a third.
    pub(super) fn constructor_call_symbol(
        &mut self,
        target: &Type,
        node: &CallExpr,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        span: fallc_span::Span,
    ) -> Option<String> {
        let receiver_name = target.nominal_name()?.to_owned();
        // The family's *own* name, which is what its methods are bound under:
        // the member is `std.Array<i32>` and the definition is `__init__Array`.
        // Both the arguments and the module qualification are the member's
        // rather than the family's, so both are stripped.
        let base = receiver_name
            .split_once('<')
            .map_or(receiver_name.as_str(), |(base, _)| base);
        let base = base.rsplit_once('.').map_or(base, |(_, last)| last);
        let candidates: Vec<FunctionSignature> = self
            .constructor_definitions(resolutions, base)
            .into_iter()
            .filter_map(|def| def.signature.clone())
            .collect();

        let member_substitution = self.member_substitution(target.nominal_name());
        // Which parameter each argument fills. A constructor takes its receiver
        // first, so the positional prefix starts at one; a named argument goes
        // to the parameter with its name, wherever that is.
        let positional = node.args.len();
        let given = positional + node.named.len();
        let mut best: Option<ConstructorChoice> = None;
        for signature in candidates {
            // The receiver, then one parameter per argument.
            if signature.param_types.len() != given + 1 {
                continue;
            }
            let mut paired: Vec<(usize, fallc_ast::ExprId)> = node
                .args
                .iter()
                .enumerate()
                .map(|(index, argument)| (index + 1, *argument))
                .collect();
            let mut filled = vec![false; signature.param_types.len()];
            for slot in filled.iter_mut().take(positional + 1).skip(1) {
                *slot = true;
            }
            let mut named_fits = true;
            for argument in &node.named {
                let Some(index) = signature
                    .param_names
                    .iter()
                    .position(|name| name == &argument.name)
                else {
                    named_fits = false;
                    break;
                };
                if index == 0 || filled.get(index).copied().unwrap_or(true) {
                    named_fits = false;
                    break;
                }
                filled[index] = true;
                paired.push((index, argument.value));
            }
            if !named_fits {
                continue;
            }
            let previous = match &member_substitution {
                Some(substitution) => self.substitution.replace(substitution.clone()),
                None => self.substitution.take(),
            };
            let expected_receiver = self.resolve_type_syntax(&signature.param_types[0], true);
            let expected: Vec<Type> = signature.param_types[1..]
                .iter()
                .map(|written| self.resolve_type_syntax(written, true))
                .collect();
            self.substitution = previous;
            let _ = &expected_receiver;

            let mut fits = true;
            // How much of the call is a match rather than a conversion. An
            // interface parameter accepts any value by boxing it, so a
            // constructor that needs one loses to one that takes the argument
            // as it is: `ODevice("a.txt")` opens the file.
            let mut score = 0usize;
            // What each argument was checked against. Testing a candidate
            // writes the expectation into the shared table, and the next
            // candidate overwrites it -- the last one tested won, which was a
            // path constructor after an interface one, so the box the interface
            // parameter needs was never recorded. The chosen candidate's
            // expectations are written again after selection.
            let mut expectations: Vec<(fallc_ast::ExprId, Type)> = Vec::new();
            for &(index, argument) in &paired {
                let Some(wanted) = expected.get(index - 1) else {
                    fits = false;
                    break;
                };
                expectations.push((argument, wanted.clone()));
                let previous = self.expected.replace(wanted.clone());
                let found = self.infer_expr(ast, resolutions, types, argument);
                self.expected = previous;
                let Some(found) = found else {
                    fits = false;
                    break;
                };
                // A parameter that is still a template parameter takes the
                // argument's type: `Array<int>(3, 7)` reaches `value T` with
                // `T` already bound to `int`, so this only decides the ones the
                // member did not.
                if wanted.is_unspecified() {
                    continue;
                }
                // An argument that is a value may fit a parameter that is a
                // reference or a pointer to it: `BasicString<u16>(values)` is
                // the constructor taking `&const Array<u16>`, with the value's
                // address supplied by the call site. This is the same rule an
                // ordinary function's arguments follow; a constructor is a
                // function.
                let mut compatible = self.is_compatible(&found, wanted);
                if !compatible && let Some(inner) = wanted.referent().or_else(|| wanted.pointee()) {
                    compatible = self.is_compatible(&found, inner);
                }
                if !compatible {
                    fits = false;
                    break;
                }
                if !self.is_interface_boxing(wanted, &found) {
                    score += 1;
                }
                // A value that is dying -- a temporary, or an expression like
                // `std.move(x)` that says so -- prefers a parameter taking it
                // by rvalue reference. That is what `$MOVES(T)`'s query asks:
                // `except(T(fake<&&T>()))` must reach `T`'s move constructor,
                // not its copy constructor, or the answer is the wrong one.
                let dying = Self::argument_is_dying(ast, argument)
                    || matches!(
                        &found,
                        Type::Reference {
                            is_rvalue: true,
                            ..
                        }
                    );
                let rvalue_parameter = signature.param_types.get(index).is_some_and(|written| {
                    matches!(
                        written,
                        fallc_ast::TypeSyntax::RvalueReference(_)
                            | fallc_ast::TypeSyntax::ConstRvalueReference(_)
                    )
                });
                if dying && rvalue_parameter {
                    score += 2;
                }
            }
            if !fits {
                continue;
            }

            let previous = match &member_substitution {
                Some(substitution) => self.substitution.replace(substitution.clone()),
                None => self.substitution.take(),
            };
            let params: Vec<String> = signature
                .param_types
                .iter()
                .map(|written| {
                    let ty = self.resolve_type_syntax(written, true);
                    crate::types::mangle_type(&ty)
                })
                .collect();
            self.substitution = previous;
            let symbol = fallc_ast::special::symbol_for(&signature.written_name, &params);
            let choice = ConstructorChoice {
                score,
                symbol,
                signature,
                expectations,
            };
            match &mut best {
                None => best = Some(choice),
                // A better-fitting constructor replaces the earlier one. An
                // equal fit keeps the first; a dying argument gives the
                // rvalue-reference form a higher score, so declaration order is
                // no longer what tells copy from move.
                Some(current) if choice.score > current.score => *current = choice,
                Some(_) => {}
            }
        }
        let chosen = best?;
        // The call-site decisions are recorded for the chosen candidate only.
        // A trial records "this argument is passed by address" and "this value
        // is read through a reference", and those are facts about the call, not
        // about a candidate: recording them while testing a rejected overload
        // left the chosen one emitting a pointer where its signature takes the
        // value -- `Rc<int>(7)` did exactly that.
        for (argument, wanted) in &chosen.expectations {
            let span = ast.expr(*argument).span;
            self.expected_types
                .insert(*argument, (span, wanted.clone()));
            let previous = self.expected.replace(wanted.clone());
            let found = self.infer_expr(ast, resolutions, types, *argument);
            self.expected = previous;
            if let Some(found) = found {
                let expected = if wanted.is_unspecified() {
                    found.clone()
                } else {
                    wanted.clone()
                };
                self.note_indirect_argument(*argument, &found, &expected, ast);
            }
        }
        if let Some(substitution) = &member_substitution {
            let receiver_ty = target.clone();
            self.record_method_instantiation(
                &chosen.signature,
                substitution,
                &chosen.symbol,
                &receiver_ty,
            );
        }
        // A constructor may raise -- `String s = make();` allocates -- and the
        // call is at `span`, so that is where the duty to handle it is judged.
        self.note_throwing_call(span, &chosen.signature, ast, None);
        Some(chosen.symbol)
    }

    /// Types a call to a function in an imported module: `C.printf(..)`.
    ///
    /// The module's name is not a value, so the object half is not inferred;
    /// the function is looked up through the qualified name the resolver bound.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn type_of_module_call(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        module: &str,
        member: &str,
        node: &CallExpr,
        span: fallc_span::Span,
    ) -> Option<Type> {
        let _ = ast;
        let qualified = format!("{module}.{member}");
        // A function the module publishes is called exactly like a local one:
        // the import is its declaration and its module's body is its
        // definition, so overloads, templates and defaults are decided by the
        // same code. There is no second rule for module calls.
        let publishes_function = resolutions.defs.iter().any(|def| {
            def.name == qualified
                && def.signature.as_ref().is_some_and(|signature| {
                    signature.param_names.first().map(String::as_str) != Some("this")
                })
        });
        if publishes_function {
            return self.type_of_named_call(ast, resolutions, types, &qualified, node, span);
        }
        // Not a function: a member value called through its type -- `std.out(..)`
        // is the file's `__call__` -- or a type's constructor.
        if self.imported_variables.contains_key(&qualified)
            && let Some(result) = self.type_of_call_method(ast, resolutions, types, node, span)
        {
            return Some(result);
        }
        if let Some(result) =
            self.type_of_constructor_call(ast, resolutions, types, &qualified, node, span)
        {
            return Some(result);
        }
        let defined = resolutions.defs.iter().any(|def| def.name == qualified)
            || self.imported_variables.contains_key(&qualified);
        if defined {
            self.diagnostics.push(
                Diagnostic::error(format!("`{qualified}` is not a function"))
                    .with_code(DiagnosticCode::sema(35))
                    .with_span(span),
            );
        } else {
            self.diagnostics.push(
                Diagnostic::error(format!("module `{module}` does not export `{member}`"))
                    .with_code(DiagnosticCode::sema(4))
                    .with_span(span)
                    .with_note("a module's declarations are private unless marked `export`"),
            );
        }
        None
    }

    /// Returns `true` if `name` is an imported module.
    /// Whether the name at `object` means a module here.
    ///
    /// A module name is in a namespace of its own, and a local value of the
    /// same name shadows it: `node->value` where a local is called `node` reads
    /// a field, and `node.value()` where `node` is a module calls it. Only the
    /// scope says which, so the module table alone is not the answer.
    pub(super) fn names_a_module(
        &self,
        resolutions: &Resolutions,
        ast: &Ast,
        object: fallc_ast::ExprId,
    ) -> bool {
        let ExprKind::Name(name) = &ast.expr(object).kind else {
            return false;
        };
        if !self.is_module(name) {
            return false;
        }
        resolutions.resolve(ast.expr(object).span).is_none()
    }

    pub(super) fn is_module(&self, name: &str) -> bool {
        // A name is a module if it is one here, or in any module whose body is
        // being checked: an instantiation writes `util.__ft_moves<T>()` from
        // the module that imported `util`, which this file may never have
        // heard of.
        self.modules.contains(name)
            || self
                .type_environments
                .iter()
                .any(|environment| environment.modules.contains(name))
    }

    /// Chooses which overload of `name` a call refers to.
    ///
    /// # The rule
    ///
    /// A candidate applies when the argument count fits — a parameter with a
    /// default is optional — and every argument's type **exactly** matches its
    /// parameter. There are no implicit conversions, so "exactly" is the only
    /// matching rule there is.
    ///
    /// When several candidates apply, one is preferred if the call had to
    /// match it more precisely. The only precision the language has left is a
    /// literal's default type: an unsuffixed integer literal prefers `int`, so
    /// `f(int)` beats `f(u8)` for `f(5)`. If no candidate is more precise than
    /// the others, the call is ambiguous and is reported rather than resolved
    /// by declaration order, which would silently change meaning when a
    /// function is added.
    ///
    /// A pointer or array argument does not prefer one overload: an argument
    /// that is compatible — an array decaying to a pointer, a pointer gaining
    /// `const` — scores the same as an exact match, so two candidates that both
    /// accept it are ambiguous. `bool` takes no special part either, because
    /// there are no conversion sequences to order: the only preference is an
    /// unsuffixed integer literal's default `int`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn resolve_overload(
        &mut self,
        name: &str,
        candidates: &[FunctionSignature],
        node: &CallExpr,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        span: fallc_span::Span,
    ) -> Option<FunctionSignature> {
        // Argument types are inferred once, with no expectation, so that the
        // choice does not depend on the candidate being tested. Only needed to
        // *choose*, so a single candidate skips it: inferring `.{1, 2}` with no
        // expectation reported it as having no type, when the parameter it
        // fills is right there. The arguments are checked against it below.
        let mut argument_types = Vec::with_capacity(node.args.len());
        let mut named_types: Vec<(String, Option<Type>, fallc_ast::ExprId)> = Vec::new();
        if candidates.len() > 1 {
            for &arg in &node.args {
                let previous = self.expected.take();
                let inferred = self.infer_expr(ast, resolutions, types, arg);
                self.expected = previous;
                argument_types.push(inferred);
            }
            for argument in &node.named {
                let previous = self.expected.take();
                let inferred = self.infer_expr(ast, resolutions, types, argument.value);
                self.expected = previous;
                named_types.push((argument.name.clone(), inferred, argument.value));
            }
        }

        // A name with one candidate is not an overload, so the arguments are
        // checked against it directly and reported in the usual terms. Running
        // the overload selection here instead would report every ordinary
        // argument mistake as "no overload matches".
        if candidates.len() == 1 {
            let signature = candidates[0].clone();

            // Only the count decides whether the call is even shaped right. The
            // types are checked per argument below, so that a mismatch names
            // the parameter it is about rather than saying the arguments do not
            // match in general.
            //
            // A call that names its arguments is not counted: `f(.b = 2)` gives
            // one argument for two parameters, which is only wrong if `a` has no
            // default. The pairing below decides that, and says which parameter
            // is missing.
            let declared = signature.param_types.len();
            if node.named.is_empty() {
                let given = node.args.len();
                let count_fits =
                    given >= signature.required_params && (signature.variadic || given <= declared);
                if !count_fits {
                    self.report_no_candidate(name, &signature, node, span);
                    return None;
                }
            }

            // A named argument may leave a hole in the middle of the list --
            // `f(1, .c = 3)` fills the first and third parameters -- so the
            // names are matched before anything is checked. A call that leaves
            // a parameter without a value and without a default is reported
            // here rather than passed on with a hole in it.
            let arguments = self.match_named_arguments(&signature, node, 0, name, span)?;
            self.check_call_arguments(&signature, node, &arguments, 0, ast, resolutions, types);
            if signature.return_type.as_name() == Some("") {
                return None;
            }
            return Some(signature);
        }

        let mut applicable: Vec<(usize, usize)> = Vec::new();
        for (index, candidate) in candidates.iter().enumerate() {
            // `@enable_if` is `std::enable_if`: a candidate whose condition is
            // false does not take part in this call at all. It is decided before
            // the arguments are scored, exactly as substitution failure removes
            // a C++ candidate, so two overloads that are otherwise identical are
            // told apart by their conditions alone. Leaving it to the scorer
            // made both of them applicable, and the first one won whichever
            // condition it carried.
            let enabled = self.signature_enabled(candidate, &None);
            if !enabled {
                continue;
            }
            if let Some(score) =
                self.candidate_score(candidate, &argument_types, &named_types, node, ast)
            {
                applicable.push((index, score));
            }
        }

        match applicable.len() {
            0 => {
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "no overload of `{name}` matches {} argument(s)",
                        node.args.len()
                    ))
                    .with_code(DiagnosticCode::sema(36))
                    .with_span(span)
                    .with_note(format!(
                        "`{name}` has {} overload(s); check the argument types",
                        candidates.len()
                    )),
                );
                None
            }
            1 => Some(candidates[applicable[0].0].clone()),
            _ => {
                // Several apply. The best is the one that needed the least
                // literal adaptation; a tie is ambiguous.
                let best = applicable.iter().map(|(_, score)| *score).max()?;
                let winners: Vec<usize> = applicable
                    .iter()
                    .filter(|(_, score)| *score == best)
                    .map(|(index, _)| *index)
                    .collect();
                if winners.len() == 1 {
                    return Some(candidates[winners[0]].clone());
                }
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "the call to `{name}` is ambiguous: {} overloads match",
                        winners.len()
                    ))
                    .with_code(DiagnosticCode::sema(37))
                    .with_span(span)
                    .with_note(
                        "add a suffix to a literal or a cast to select one, since there are \
                         no implicit conversions to rank them by",
                    ),
                );
                None
            }
        }
    }

    /// Pairs a call's arguments with the parameters they fill.
    ///
    /// `first` is the index of the first parameter the written arguments fill:
    /// zero for a free call, one for a method call, whose receiver is written
    /// before the dot rather than in the list. The returned pairs are the
    /// parameter index and the value written for it, positional ones first and
    /// named ones after them, in the order the call wrote them.
    ///
    /// The names decide everything here -- an unknown one, one given twice,
    /// and a parameter the call leaves without a value are all reported -- so
    /// the caller is left with types to check and nothing else. A call with no
    /// named arguments is paired by position alone, which cannot be wrong:
    /// its shape is the count.
    pub(super) fn match_named_arguments(
        &mut self,
        signature: &FunctionSignature,
        node: &CallExpr,
        first: usize,
        callee: &str,
        span: fallc_span::Span,
    ) -> Option<Vec<(usize, fallc_ast::ExprId)>> {
        let declared = signature.param_types.len();
        let mut matched: Vec<(usize, fallc_ast::ExprId)> = node
            .args
            .iter()
            .enumerate()
            .map(|(index, argument)| (index + first, *argument))
            .collect();
        if node.named.is_empty() {
            return Some(matched);
        }
        let given = node.args.len() + node.named.len();
        if given > declared && !signature.variadic {
            self.report_no_candidate(callee, signature, node, span);
            return None;
        }
        // What the positional prefix already fills.
        let mut filled: Vec<bool> = vec![false; declared];
        for index in node.args.iter().enumerate().map(|(index, _)| index + first) {
            if let Some(slot) = filled.get_mut(index) {
                *slot = true;
            }
        }

        for argument in &node.named {
            let Some(index) = signature
                .param_names
                .iter()
                .position(|name| name == &argument.name)
            else {
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "`{callee}` has no parameter named `{}`",
                        argument.name
                    ))
                    .with_code(DiagnosticCode::sema(170))
                    .with_span(argument.name_span)
                    .with_note(format!(
                        "its parameters are {}",
                        Self::parameter_list(&signature.param_names)
                    )),
                );
                return None;
            };
            if index < first {
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "`{callee}` cannot be given its receiver by name, because it is \
                         written before the dot"
                    ))
                    .with_code(DiagnosticCode::sema(170))
                    .with_span(argument.name_span),
                );
                return None;
            }
            if filled.get(index).copied().unwrap_or(false) {
                self.diagnostics.push(
                    Diagnostic::error(format!("`{callee}` is given `{}` twice", argument.name))
                        .with_code(DiagnosticCode::sema(171))
                        .with_span(argument.name_span)
                        .with_note(
                            "a parameter takes one argument; write it once, either by position \
                         or by name",
                        ),
                );
                return None;
            }
            if let Some(slot) = filled.get_mut(index) {
                *slot = true;
            }
            matched.push((index, argument.value));
        }

        // Every parameter written after the positional prefix must be
        // supplied, by name or by a default: the language has no other way to
        // leave one out.
        for index in first..declared {
            if filled.get(index).copied().unwrap_or(false) {
                continue;
            }
            if signature.defaults.get(index).is_some_and(Option::is_some) {
                continue;
            }
            let name = signature
                .param_names
                .get(index)
                .map_or_else(|| (index + 1).to_string(), Clone::clone);
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "`{callee}` needs `{name}`, which the call leaves out"
                ))
                .with_code(DiagnosticCode::sema(172))
                .with_span(span)
                .with_note("write it, or give the parameter a default"),
            );
            return None;
        }
        Some(matched)
    }

    /// Renders a parameter list for a diagnostic: `` `a`, `b` and `c` ``.
    pub(super) fn parameter_list(names: &[String]) -> String {
        match names {
            [] => "none".to_owned(),
            [only] => format!("`{only}`"),
            [front, back] => format!("`{front}` and `{back}`"),
            many => {
                let mut text = String::new();
                for (index, name) in many.iter().enumerate() {
                    if index > 0 {
                        text.push_str(", ");
                    }
                    text.push('`');
                    text.push_str(name);
                    text.push('`');
                }
                text
            }
        }
    }

    /// Checks a call's arguments against one signature, reporting each
    /// mismatch against its own parameter.
    ///
    /// Used when there is no choice to make, so the diagnostics can name the
    /// parameter rather than saying no overload matched.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn check_call_arguments(
        &mut self,
        signature: &FunctionSignature,
        node: &CallExpr,
        arguments: &[(usize, fallc_ast::ExprId)],
        first: usize,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
    ) {
        // Only the declared parameters are checked; a variadic tail is not,
        // because nothing declares its types.
        //
        // A parameter is checked against whatever fills it, whether it was
        // written by position or by name: the two are the same argument as far
        // as the signature is concerned.
        for &(index, argument) in arguments {
            let Some(expected_syntax) = signature.param_types.get(index) else {
                continue;
            };
            let expected_ty = self.resolve_type_syntax(expected_syntax, true);
            // An unsuffixed integer literal adopts the parameter's type, exactly
            // as an argument to a free function does. Without this the literal
            // stayed an `int` while the parameter was a `usize`, so
            // `a.resize(4, 7)` matched no overload -- passing the width through
            // a variable of the right type worked, which is what gave it away.
            if self.is_default_int_literal(ast, argument) && expected_ty.is_integer() {
                self.adopted_types
                    .insert(ast.expr(argument).span, expected_ty.clone());
            }
            let previous = self.expected.replace(expected_ty.clone());
            let found = self.infer_expr(ast, resolutions, types, argument);
            self.expected = previous;
            if let Some(found) = found {
                self.note_indirect_argument(argument, &found, &expected_ty, ast);
                let was_parameter = self.in_parameter_position;
                self.in_parameter_position = true;
                if !self.adopt_literal_argument(resolutions, ast, argument, &expected_ty, &found) {
                    if super::methods::integer_promotes(&found, &expected_ty) {
                        self.adopted_types
                            .insert(ast.expr(argument).span, expected_ty.clone());
                    } else if self.try_implicit_conversion(
                        resolutions,
                        ast,
                        types,
                        argument,
                        &expected_ty,
                    ) {
                    } else {
                        self.types_must_match(
                            &expected_ty,
                            &found,
                            ast.expr(argument).span,
                            "argument",
                        );
                    }
                }
                self.in_parameter_position = was_parameter;
            }
        }
        // A variadic tail has no declared type, but it is still a *value*
        // position: a reference argument is passed as what it names, not as
        // its address. Without this `C.printf("%d", r)` printed the address,
        // which is how a reference loop variable looked broken.
        for (index, argument) in node.args.iter().enumerate() {
            if index + first < signature.param_types.len() {
                continue;
            }
            let previous = self.expected.take();
            let found = self.infer_expr(ast, resolutions, types, *argument);
            self.expected = previous;
            if let Some(found) = found {
                let value_position = Type::unspecified("", ast.expr(*argument).span);
                self.note_reference_read(ast, *argument, &found, &value_position);
            }
        }
    }

    /// Records that an argument is passed as its address.
    ///
    /// A reference or pointer parameter takes the address of a value that fits
    /// it, the way a method takes its receiver's address. Only the checker is
    /// looking at the parameter, so only it can say which arguments those are.
    pub(super) fn note_indirect_argument(
        &mut self,
        argument: fallc_ast::ExprId,
        found: &Type,
        expected: &Type,
        ast: &Ast,
    ) {
        // A reference passed where a *value* is wanted is read through: the
        // call gets the value behind it, not the address. `take(r)` with `r
        // &int` and `fn take(v int)` passed the address, and the IR verifier
        // refused the call -- and `(&slot)->__init__(std.move(x))` wrote the
        // address where the value belonged.
        if let Some(referent) = found.referent()
            && !expected.is_indirection()
        {
            self.deref_reads
                .insert(ast.expr(argument).span, referent.clone());
            return;
        }
        // A pointer or a reference is already an address and is passed as it
        // stands. Taking the address of a reference would hand over a pointer
        // to a pointer: `std.move(b)` returns `&&Box<int>`, which *is* the
        // address of `b`, and the constructor's `&&Box<int>` parameter wants
        // exactly that.
        //
        // That holds for an expression whose *lowering* yields the address: a
        // name, or a call. A dereference does not -- `*it` over a container's
        // iterator names storage, so its value is the element and its type is
        // `&T`; handing the value to a constructor that takes `&const T` gave
        // LLVM an aggregate where a pointer was declared. Such an expression is
        // passed as the place it names, which is the address the parameter
        // wants.
        if found.is_pointer() || found.is_reference() {
            let names_a_place = matches!(
                &ast.expr(argument).kind,
                ExprKind::Unary(unary) if unary.op == fallc_ast::UnaryOp::Deref
            ) || matches!(&ast.expr(argument).kind, ExprKind::Index(_));
            if names_a_place {
                self.by_address_arguments.insert(ast.expr(argument).span);
            }
            return;
        }
        // Everything else that reaches an indirect parameter is recorded, which
        // tells code generation to pass a *place* rather than a value. That
        // covers both a plain `T` -- whose address is taken -- and a reference,
        // whose own address is what the parameter gets.
        //
        // A dereference `*p` is included deliberately: it names storage, and
        // `lower_lvalue` already resolves it to the pointer's value, which *is*
        // that address. Leaving it out passed the loaded value instead, and
        // `result String = *this;` inside `__add__` handed LLVM a struct where
        // the copy constructor takes `&const String`.
        if self.operand_fits_indirect(found, expected).is_some() {
            self.by_address_arguments.insert(ast.expr(argument).span);
        }
    }

    /// Reports a call that fits no candidate, describing the one it was
    /// checked against when there was only one.
    pub(super) fn report_no_candidate(
        &mut self,
        name: &str,
        signature: &FunctionSignature,
        node: &CallExpr,
        span: fallc_span::Span,
    ) {
        let required = signature.required_params;
        let declared = signature.param_types.len();
        let given = node.args.len() + node.named.len();

        // "1 argument" rather than "1 argument(s)", because a diagnostic that
        // reads badly is a diagnostic people stop reading.
        let plural = |count: usize| if count == 1 { "" } else { "s" };
        let was = |count: usize| if count == 1 { "was" } else { "were" };

        let message = if given < required {
            // "At least" only for a variadic function, where more is allowed;
            // otherwise the count is exact and saying "at least" would be
            // misleading about what the function accepts.
            if signature.variadic {
                format!(
                    "`{name}` takes at least {required} argument{}, but {given} {} given",
                    plural(required),
                    was(given)
                )
            } else {
                format!(
                    "`{name}` takes {declared} argument{}, but {given} {} given",
                    plural(declared),
                    was(given)
                )
            }
        } else if given > declared && !signature.variadic {
            format!(
                "`{name}` takes {declared} argument{}, but {given} {} given",
                plural(declared),
                was(given)
            )
        } else {
            format!("the arguments to `{name}` do not match its parameters")
        };

        self.diagnostics.push(
            Diagnostic::error(message)
                .with_code(DiagnosticCode::sema(36))
                .with_span(span),
        );
    }

    /// Scores how well a candidate matches, or `None` if it does not apply.
    ///
    /// A higher score is a better match. The score counts arguments that
    /// matched without the literal default having been changed, so a candidate
    /// needing no adaptation beats one that does.
    pub(super) fn candidate_score(
        &mut self,
        candidate: &FunctionSignature,
        argument_types: &[Option<Type>],
        named_types: &[(String, Option<Type>, fallc_ast::ExprId)],
        node: &CallExpr,
        ast: &Ast,
    ) -> Option<usize> {
        // Which parameter each written argument fills. A positional argument
        // fills the parameter in its position; a named one fills the parameter
        // with its name, which is what makes the order of the named list
        // irrelevant.
        let declared = candidate.param_types.len();
        let required = if candidate.required_params <= declared {
            candidate.required_params
        } else {
            declared
        };
        let mut slots: Vec<Option<(&Type, fallc_ast::ExprId)>> = vec![None; declared];
        for (index, argument) in argument_types.iter().enumerate() {
            // Arguments beyond the declared parameters are the variadic tail
            // and are not checked, as in C.
            if index >= declared {
                if !candidate.variadic {
                    return None;
                }
                continue;
            }
            if let Some(found) = argument {
                slots[index] = Some((found, node.args[index]));
            }
        }
        for (name, argument, value) in named_types {
            let index = candidate
                .param_names
                .iter()
                .position(|parameter| parameter == name)?;
            // A parameter filled twice is a call no candidate can accept; the
            // checker reports that separately once one is chosen.
            if index >= declared || slots[index].is_some() {
                return None;
            }
            if let Some(found) = argument {
                slots[index] = Some((found, *value));
            }
        }

        let mut score = 0;
        for (index, slot) in slots.iter().enumerate() {
            let Some((found, argument)) = slot else {
                // A parameter with a default may be left out; one without
                // cannot, which is a count that does not fit.
                if index < required {
                    return None;
                }
                continue;
            };
            let expected = self.resolve_type_syntax(&candidate.param_types[index], true);
            // An rvalue reference takes only an rvalue: an lvalue is not about
            // to disappear.
            if matches!(
                expected,
                Type::Reference {
                    is_rvalue: true,
                    ..
                }
            ) && Self::value_is_lvalue(ast, *argument, found)
            {
                return None;
            }
            // An rvalue reference is the promise "this argument is about to
            // disappear", and an lvalue is not about to disappear. Without this
            // both `f(&int)` and `f(&&int)` matched every call, and forwarding
            // could not hand on the value category it was given.
            if matches!(
                expected,
                Type::Reference {
                    is_rvalue: true,
                    ..
                }
            ) && Self::value_is_lvalue(ast, *argument, found)
            {
                return None;
            }
            // The general compatibility rule applies, so an argument that is
            // compatible — a pointer gaining `const`, say — matches. Scoring
            // exactly here would reject a call the language allows.
            if self.is_compatible(found, &expected) {
                // An interface parameter accepts any value, by boxing it. That
                // is a conversion, not a match, so a candidate that needs one
                // scores lower than a candidate that takes the argument as it
                // is: `ODevice("a.txt")` must open the file, not box the text
                // as a buffer.
                if !self.is_interface_boxing(&expected, found) {
                    score += 1;
                    // An rvalue reference is the more specific answer for an
                    // rvalue: `f(&&T)` is chosen over `f(&T)` for a temporary,
                    // which is how a move overload wins over a copy one.
                    if matches!(
                        expected,
                        Type::Reference {
                            is_rvalue: true,
                            ..
                        }
                    ) && !Self::value_is_lvalue(ast, *argument, found)
                    {
                        score += 2;
                    }
                }
                continue;
            }
            // An unsuffixed integer literal may match any integer parameter,
            // because it takes its type from context. Matching `int` scores
            // higher, which is what makes `f(int)` win over `f(u8)` for
            // `f(5)` — the one precision the language has.
            //
            // Any *other* parameter type is not a match at all: there are no
            // implicit conversions, and a literal does not become a `bool` or
            // a pointer by being written.
            if self.is_default_int_literal(ast, *argument) && expected.is_integer() {
                if expected == Type::INT {
                    score += 1;
                }
                continue;
            }
            // A suffixed literal's type is fixed, so a mismatch is a mismatch.
            if self.is_int_literal(ast, *argument) && expected.is_integer() {
                continue;
            }
            return None;
        }
        Some(score)
    }

    /// Returns `true` if this expression is an integer literal, suffixed or not.
    ///
    /// Read from the tree, not from the constant table: the table is indexed by
    /// expression id, and an id names different expressions in different
    /// translation units. A literal in a template body from another module was
    /// looked up in the importing file's table, was not a literal there, and so
    /// stopped adopting its parameter's type.
    pub(super) fn is_int_literal(&self, ast: &Ast, expr: ExprId) -> bool {
        matches!(&ast.expr(expr).kind, ExprKind::IntLiteral(_))
    }

    /// Returns `true` if this expression is an integer literal with no suffix.
    pub(super) fn is_default_int_literal(&self, ast: &Ast, expr: ExprId) -> bool {
        matches!(
            &ast.expr(expr).kind,
            ExprKind::IntLiteral(literal) if literal.suffix.is_none()
        )
    }
}

/// The dotted path an expression writes, when it is a chain of names and
/// fields: `Array`, `std.Array`, `std.string.String`.
///
/// A type may be named through the module that declares it, and the module may
/// itself be dotted, so the whole chain is the name to look up.
pub(super) fn written_path(ast: &Ast, expr: ExprId) -> Option<String> {
    match &ast.expr(expr).kind {
        ExprKind::Name(name) => Some(name.clone()),
        ExprKind::Field(field) => {
            let object = written_path(ast, field.object)?;
            Some(format!("{object}.{}", field.field))
        }
        _ => None,
    }
}

/// A constructor that fits a call, with how well it fits.
///
/// Used by `constructor_call_symbol`: the best score wins, and two equal scores
/// are an ambiguity rather than a first-declared-wins choice.
struct ConstructorChoice {
    /// One per argument that needed no interface boxing.
    score: usize,
    /// The symbol the chosen constructor was declared under.
    symbol: String,
    /// The declaration, for recording and for the raising judgement.
    signature: FunctionSignature,
    /// What each argument of the call was checked against, so that the chosen
    /// candidate's expectations are the ones settled -- testing the others
    /// overwrote them in the shared table.
    expectations: Vec<(fallc_ast::ExprId, Type)>,
}
