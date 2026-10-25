//! Templates: deciding arguments, instantiating a body, and the records an
//! instantiation produces.
//!
//! A template is not a function: a call decides which member of the family it
//! means, and that member is checked here against its own tree.

use super::*;

impl TypeChecker {
    /// Instantiates a template for this call, if the callee is one.
    ///
    /// # The steps
    ///
    ///   1. Find the template, which is a definition whose signature has
    ///      template parameters. There is at most one: a template cannot be
    ///      overloaded, because its parameters name the very things a call
    ///      would have to match.
    ///   2. Decide the arguments -- written out, or deduced from the call's
    ///      own arguments.
    ///   3. Substitute them into the parameter types, the return type, and the
    ///      body, and check the result like any other function.
    ///   4. Record the instantiation so that code generation emits it, and so
    ///      that a second call with the same arguments reuses it.
    ///
    /// Returns the call's type, or `None` when the callee is not a template --
    /// which is not an error, and is the common case.
    pub(super) fn instantiate_template(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        name: &str,
        node: &CallExpr,
        span: fallc_span::Span,
    ) -> Option<Type> {
        // Every declaration under the name is a candidate. A name may carry
        // several template overloads -- `sort(first, last)` and `sort(values)`
        // are one algorithm in two spellings -- and their parameters cannot be
        // matched against the arguments, because the parameters name what is
        // being deduced. Their *number* can: it is what tells the range from the
        // container, so a fitting count wins and the first stays the answer when
        // none has it.
        let templates: Vec<&crate::resolve::Def> = resolutions
            .defs
            .iter()
            .filter(|def| {
                def.name == name
                    && !self.disabled_functions.contains(&def.name_span)
                    // `module.name(..)` names a free function: a member is
                    // reached through a value, and its `this` is not an
                    // argument the call wrote. Without this, the member
                    // `File.print(..)` was a candidate for `std.print(..)` and
                    // took the first written argument as its receiver.
                    && (!name.contains('.')
                        || def.signature.as_ref().is_none_or(|signature| {
                            signature.param_names.first().map(String::as_str) != Some("this")
                        }))
                    && def
                        .signature
                        .as_ref()
                        .is_some_and(|signature| {
                            !signature.template_params.is_empty()
                                || signature.pack.is_some()
                        })
            })
            .collect();
        let given = node.args.len() + node.named.len();
        // Candidates with the fitting arity are tried in turn. Two overloads
        // that differ only in a type parameter -- the C++ `enable_if` shape --
        // have the same parameters at this point; the first whose `@enable_if`
        // holds and whose arguments fit is the call, and the others are not
        // this call's business.
        let mut ordered: Vec<&crate::resolve::Def> = templates
            .iter()
            .copied()
            .filter(|def| {
                def.signature.as_ref().is_some_and(|signature| {
                    signature.pack.is_some() || signature.param_types.len() == given
                })
            })
            .collect();
        if ordered.is_empty() {
            ordered = templates.to_vec();
        }
        // When nothing fits, the first candidate's diagnostics are the error
        // the caller sees: a pack whose element type does not take the argument
        // must say so, not fall through to "this call is undefined".
        let mut first_failure: Option<Vec<fallc_diag::Diagnostic>> = None;
        for template in ordered {
            let checkpoint = self.diagnostics.len();
            if let Some(result) =
                self.instantiate_one(ast, resolutions, types, name, node, span, template)
            {
                return Some(result);
            }
            if first_failure.is_none() {
                first_failure = Some(self.diagnostics.iter().skip(checkpoint).cloned().collect());
            }
            self.diagnostics.truncate(checkpoint);
        }
        if let Some(diagnostics) = first_failure {
            self.diagnostics.extend(diagnostics);
        }
        None
    }

    /// Instantiates the template the call selected.
    #[allow(clippy::too_many_arguments)]
    fn instantiate_one(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        name: &str,
        node: &CallExpr,
        span: fallc_span::Span,
        template: &crate::resolve::Def,
    ) -> Option<Type> {
        let template = template.clone();
        let signature = template.signature.clone()?;
        // Where the definition is.
        //
        // A template declared in this file is found by node id. One declared in
        // another module travels as its *body*, registered under the symbol its
        // declaration mangles to, because its tree is not this one -- node ids
        // are indices into a single tree, and the same number names a different
        // function there.
        let local_item = template.node.and_then(|node| {
            ast.items
                .iter()
                .copied()
                .find(|candidate| ast.item(*candidate).node == node)
        });
        let symbol = template_symbol_of(&signature.written_name, &signature.param_types);
        let imported_body = if local_item.is_none() {
            let body = self
                .template_body_for(&symbol)
                .or_else(|| self.template_body_for(&signature.bound_as))?;
            Some(body)
        } else {
            None
        };
        // The body's own module's declarations are what its names mean, for as
        // long as it is being checked. This is what lets a template use a type
        // its module kept private, and keeps `Box<T>` in `std` from becoming a
        // `Box` the instantiating file happens to declare.
        if let Some(body) = &imported_body
            && let Some(environment) = body.environment.clone()
        {
            self.type_environments.push(environment);
        }
        let environment_pushed = imported_body
            .as_ref()
            .is_some_and(|body| body.environment.is_some());
        // The declaration's attributes travel with it: `@enable_if` is checked
        // per instantiation, because its condition may mention a parameter.
        // The condition is an expression of the *declaration's* tree, so it is
        // read there.
        let (definition_tree, definition_resolutions): (&Ast, &Resolutions) =
            match (&local_item, &imported_body) {
                (Some(_), _) => (ast, resolutions),
                (None, Some(body)) => (&body.tree, &body.resolutions),
                (None, None) => return None,
            };
        let attrs = match &local_item {
            Some(item) => ast.item(*item).attrs.clone(),
            None => imported_body.as_ref().map(|body| body.attrs.clone())?,
        };

        // The arguments: written out when the call has them, deduced otherwise.
        // The call's own arguments are paired with the parameters they fill at
        // the same time, because whether one is passed by address is a fact
        // about the call site either way.
        let (arguments, written) =
            if node.template_args.is_empty() && !node.explicit_template_arguments {
                self.deduce_template_arguments(ast, resolutions, types, &signature, node, span)?
            } else {
                let arguments = self.written_template_arguments(
                    ast,
                    resolutions,
                    types,
                    &signature,
                    node,
                    span,
                )?;
                let written = self.match_named_arguments(&signature, node, 0, name, span)?;
                (arguments, written)
            };
        // Whatever the caller left out comes from the parameter's default.
        let arguments = self.complete_template_arguments(
            &signature.template_params,
            arguments,
            span,
        )?;
        let substitution =
            crate::template::Substitution::new(&signature.template_params, &arguments)?;

        // `@enable_if(cond)` decides whether this member of the family exists.
        // The condition is checked with the arguments substituted, exactly as
        // the body will be.
        if attrs.iter().any(|attr| attr.name == "enable_if") {
            let previous = self.substitution.replace(substitution.clone());
            let allowed = self.enable_if(
                definition_tree,
                definition_resolutions,
                types,
                &attrs,
                span,
            );
            self.substitution = previous;
            if allowed == Some(false) {
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "`{}` is disabled for these arguments by `@enable_if`",
                        signature.written_name
                    ))
                    .with_code(DiagnosticCode::sema(175))
                    .with_span(span),
                );
                return None;
            }
        }

        // One instantiation per argument list: the same call twice is the same
        // function, and making a second copy would emit it twice. A pack whose
        // element type is fixed -- `values ...int` -- has the same template
        // arguments at every arity, so the number of arguments is part of the
        // key.
        let key = if signature.pack.is_some() {
            format!("{name}<{}>#{}", arguments.key(), written.len())
        } else {
            format!("{name}<{}>", arguments.key())
        };
        if let Some(existing) = self.instantiations.get(&key) {
            let existing = existing.clone();
            self.call_symbols.insert(span, existing);
            // The arguments are checked for *this* call as well, even though
            // the instantiation already exists: whether they are passed by
            // address is a fact about the call site, not about the function.
            // Skipping this made a second call to the same template pass values
            // where the first passed addresses.
            // The parameters of the instantiation that already exists, which
            // for a pack is the expanded list -- the written signature has one
            // parameter where the call passes many, and resolving the pack's
            // element type by name would find nothing.
            let recorded = self
                .instantiation_list
                .iter()
                .find(|instantiation| instantiation.key == key)
                .map(|instantiation| instantiation.resolved_params.clone())
                .unwrap_or_default();
            for &(index, argument) in &written {
                let Some(expected) = recorded.get(index).cloned() else {
                    continue;
                };
                let previous = self.expected.replace(expected.clone());
                let found = self.infer_expr(ast, resolutions, types, argument);
                self.expected = previous;
                if let Some(found) = found {
                    self.note_indirect_argument(argument, &found, &expected, ast);
                }
            }
            // A call of a template or a pack obeys the same rule as any other
            // call: a callee declared `except` may raise, and the call site is
            // where that is either caught, passed on with `??`, or reported.
            // This used to be skipped here, so a template that raises
            // unconditionally could be called with nothing handling it.
            if signature.throws {
                self.note_raising_site(
                    span,
                    &format!(
                        "a call to `{}` may raise, and nothing here handles it",
                        signature.written_name
                    ),
                );
            }
            return self
                .instantiated_returns
                .get(&key)
                .cloned()
                .or(Some(Type::INT));
        }

        // The definition records the *node*, and the item is what carries the
        // body; it was found above, with its attributes. A body from another
        // module is the one registered with it.
        let function = match &local_item {
            Some(item) => {
                let fallc_ast::ItemKind::Function(function) = &ast.item(*item).kind else {
                    return None;
                };
                function.clone()
            }
            None => imported_body.as_ref()?.function.clone(),
        };
        // The declaration's constants travel with the body: its tree's
        // expression ids mean nothing here, so the values its body folds are
        // what it uses.
        if let Some(body) = &imported_body {
            self.instantiation_constants = body.constants.clone();
            self.constant_types_by_name
                .extend(body.constant_types.clone());
        }
        // The body's ids belong to its own tree, so its own constant index is
        // what folding reads while it is checked.
        let previous_index = self.adopt_const_index(definition_tree);
        let result = self.check_instantiation(
            definition_tree,
            definition_resolutions,
            types,
            &function,
            &signature,
            &substitution,
            &key,
            &written,
            span,
            (ast, resolutions),
            imported_body.as_ref().map(|body| &body.bindings),
        );
        self.restore_const_index(previous_index);
        if environment_pushed {
            self.type_environments.pop();
        }
        // A candidate that does not take these arguments is not this call's
        // overload. The fit is judged from the types the deduction pass already
        // recorded, so no second inference and no call-site record is made: the
        // next candidate gets a clean slate. Without this, `std.print("a", 1)`
        // was `File.print(..)` with `"a"` as the receiver, only because the
        // member overload was declared before the free one.
        let fits = result.is_some()
            && self.instantiated_arguments_fit(&signature, &substitution, &written, types);
        if result.is_some() && !fits {
            // The attempt recorded what it had reached before the arguments
            // were judged. A later call with the same key would reuse that
            // record without judging again, so the rejected instantiation is
            // taken back.
            self.instantiations.remove(&key);
            self.call_symbols.remove(&span);
            return None;
        }
        // The duty to handle a call that may raise is the call site's, and it
        // is judged here for every call: the declaration says `except`, so the
        // caller either catches it, writes `??` to pass it on, or hears about
        // it. Without this a template's `except` was invisible at every call.
        if result.is_some() && signature.throws {
            self.note_raising_site(
                span,
                &format!(
                    "a call to `{}` may raise, and nothing here handles it",
                    signature.written_name
                ),
            );
        }
        result
    }

    /// Decides a template's arguments from the call's own arguments.
    ///
    /// # The rule
    ///
    /// Each parameter is matched against the argument in its position, and what
    /// the parameter names is what the argument's type says it is:
    ///
    /// * a parameter written `T` takes the argument's type;
    /// * `&T`, `&const T`, `*T` and the rest take what the argument points or
    ///   refers to, so that `min(a, b)` on two `int`s deduces `T` from an
    ///   argument that is passed by reference;
    /// * a parameter that mentions no template parameter -- `N usize` -- says
    ///   nothing, and is left to the explicit form.
    ///
    /// Every deduction for one parameter must agree: deducing `T` as `int` from
    /// one argument and `bool` from another is an ambiguity, and it is reported
    /// rather than resolved by picking one.
    pub(super) fn deduce_template_arguments(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        signature: &FunctionSignature,
        node: &CallExpr,
        span: fallc_span::Span,
    ) -> Option<(
        crate::template::Arguments,
        Vec<(usize, fallc_ast::ExprId)>,
    )> {
        // A parameter with a default may be left out, so the count is a range:
        // at least the required ones, at most all of them. A parameter *pack*
        // takes the rest, however many there are, so with one there is no upper
        // bound and the fixed parameters are the required ones.
        let pack = signature.pack;
        let required = pack.unwrap_or(signature.required_params);
        let given = node.args.len() + node.named.len();
        if pack.is_some() && !node.named.is_empty() {
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "`{}` fills a parameter pack, so its arguments cannot be named",
                    signature.written_name
                ))
                .with_code(DiagnosticCode::sema(170))
                .with_span(span)
                .with_note("a pack's elements are positional: one argument per element"),
            );
            return None;
        }
        if given < required || (pack.is_none() && given > signature.param_types.len()) {
            let expected = if pack.is_some() {
                format!("at least {required}")
            } else if required == signature.param_types.len() {
                signature.param_types.len().to_string()
            } else {
                format!("{} to {}", required, signature.param_types.len())
            };
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "`{}` takes {expected} argument(s), but {given} were given",
                    signature.written_name,
                ))
                .with_code(DiagnosticCode::sema(36))
                .with_span(span),
            );
            return None;
        }
        // Which parameter each argument fills, by position or by name.
        let arguments_written =
            self.match_named_arguments(signature, node, 0, &signature.written_name, span)?;

        let mut deduced: std::collections::HashMap<String, Type> =
            std::collections::HashMap::new();
        // The types of a pack parameter's elements, gathered as the arguments
        // are read: one argument is one element, so the list *is* the pack.
        let mut deduced_packs: std::collections::HashMap<String, Vec<Type>> =
            std::collections::HashMap::new();
        let pack_written = pack.and_then(|index| signature.param_types.get(index));
        // When the pack's element type is itself a type pack -- `values
        // ...Ts` -- the arguments decide `Ts`, one element each. When it is a
        // single type, every element has that type and the ordinary deduction
        // does the work.
        let element_parameter = pack_written
            .and_then(|written| pack_element_parameter(written, &signature.template_params));
        for &(index, argument) in &arguments_written {
            let is_element = pack.is_some_and(|pack| index >= pack);
            let parameter = if is_element {
                pack_written
            } else {
                signature.param_types.get(index)
            };
            let Some(parameter) = parameter else {
                continue;
            };
            let previous = self.expected.take();
            let found = self.infer_expr(ast, resolutions, types, argument);
            self.expected = previous;
            let Some(found) = found else {
                continue;
            };
            if is_element
                && let Some(name) = element_parameter
            {
                deduced_packs
                    .entry(name.to_owned())
                    .or_default()
                    .push(found);
                continue;
            }
            // A `&&T` whose `T` is a template parameter is a forwarding
            // reference: an lvalue deduces `T` as `&U`, an rvalue deduces the
            // value type. That is what lets `forward<T>` give the callee back
            // the value category it was handed.
            if let Some(name) =
                Self::universal_reference_parameter(parameter, &signature.template_params)
            {
                if Self::expression_is_lvalue(ast, argument) {
                    let value = found.referent().cloned().unwrap_or_else(|| found.clone());
                    deduced.insert(
                        name.to_owned(),
                        Type::Reference {
                            referent: Box::new(value),
                            referent_is_const: false,
                            is_rvalue: false,
                        },
                    );
                    continue;
                }
            }
            self.deduce_from_type(
                parameter,
                &found,
                &signature.template_params,
                &mut deduced,
            );
        }

        // A pack with no elements is still a pack: `f()` is a call of a
        // function whose only parameter is a pack, and the pack is empty.
        if let Some(name) = element_parameter {
            deduced_packs.entry(name.to_owned()).or_default();
        }

        // Every parameter must be decided. One that is not is a call that
        // cannot say what it means, and the message names the parameter rather
        // than saying the arguments do not match.
        let mut arguments = Vec::with_capacity(signature.template_params.len());
        for param in &signature.template_params {
            if let Some(types) = deduced_packs.get(&param.name) {
                arguments.push(crate::template::TemplateArgument::Pack(
                    types
                        .iter()
                        .map(|ty| crate::template::TemplateArgument::Type(ty.clone()))
                        .collect(),
                ));
                continue;
            }
            if let Some(ty) = deduced.get(&param.name) {
                arguments.push(crate::template::TemplateArgument::Type(ty.clone()));
                continue;
            }
            // A parameter that could not be deduced may still have a default.
            // Defaults come last, so everything from here on is defaulted, and
            // the filler below supplies them.
            if param.default.is_some() {
                break;
            }
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "cannot deduce `{}` for `{}`",
                    param.name, signature.written_name
                ))
                .with_code(DiagnosticCode::sema(71))
                .with_span(span)
                .with_note("write it out: `f<int>(..)`, or name it in a parameter"),
            );
            return None;
        }
        Some((
            crate::template::Arguments(arguments),
            arguments_written,
        ))
    }

    /// Fills in the template arguments a caller left out.
    ///
    /// Defaults are applied from the left, and one may mention the parameters
    /// before it -- `template <T type, n int = sizeof(T)>` -- so each is
    /// resolved against the arguments gathered so far rather than against the
    /// whole list. A missing argument whose parameter has no default is the
    /// caller's mistake, reported where the arity is checked.
    pub(super) fn complete_template_arguments(
        &mut self,
        params: &[fallc_ast::TemplateParam],
        arguments: crate::template::Arguments,
        span: fallc_span::Span,
    ) -> Option<crate::template::Arguments> {
        use fallc_ast::{TemplateDefault, TemplateParamKind};
        let fixed = params.iter().take_while(|param| !param.is_pack).count();
        let mut collected = arguments.0;
        if collected.len() >= fixed {
            return Some(crate::template::Arguments(collected));
        }
        while collected.len() < fixed {
            let index = collected.len();
            let param = &params[index];
            let Some(default) = param.default.clone() else {
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "template parameter `{}` has no value",
                        param.name
                    ))
                    .with_code(DiagnosticCode::sema(72))
                    .with_span(span)
                    .with_note("write it out, or give it a default: `template <{} type = ...>`"),
                );
                return None;
            };
            // The parameters before this one are all that its default may use.
            let so_far = crate::template::Substitution::new(
                &params[..index],
                &crate::template::Arguments(collected.clone()),
            )?;
            let argument = match default {
                TemplateDefault::Type(written) => {
                    crate::template::TemplateArgument::Type(
                        self.resolve_template_type(&written, &so_far),
                    )
                }
                TemplateDefault::Value(expr) => {
                    // The default is a constant expression, which may mention
                    // the parameters so far -- `sizeof(T)` and `n * 3` are the
                    // usual ones. The type parameters travel in the
                    // substitution; the value parameters have to be visible to
                    // the folder as constants, which is what makes `n` in
                    // `m int = n * 3` fold.
                    let mut saved_values = Vec::new();
                    for (earlier, argument) in params[..index].iter().zip(&collected) {
                        if let crate::template::TemplateArgument::Int(value) = argument {
                            let previous = self
                                .instantiation_constants
                                .insert(earlier.name.clone(), *value);
                            saved_values.push((earlier.name.clone(), previous));
                        }
                    }
                    let previous = self.substitution.replace(so_far);
                    let value = self.const_eval_int(expr);
                    self.substitution = previous;
                    for (name, previous) in saved_values {
                        match previous {
                            Some(value) => {
                                self.instantiation_constants.insert(name, value);
                            }
                            None => {
                                self.instantiation_constants.remove(&name);
                            }
                        }
                    }
                    let Some(value) = value else {
                        self.diagnostics.push(
                            Diagnostic::error(format!(
                                "the default for template parameter `{}` is not a constant",
                                param.name
                            ))
                            .with_code(DiagnosticCode::sema(73))
                            .with_span(span)
                            .with_note(
                                "a template argument has to be known before the program runs",
                            ),
                        );
                        return None;
                    };
                    let _ = TemplateParamKind::Type;
                    crate::template::TemplateArgument::Int(value)
                }
            };
            collected.push(argument);
        }
        Some(crate::template::Arguments(collected))
    }

    /// Records what one parameter says about the template parameters.
    ///
    /// The parameter is *syntax*, so this walks it rather than the resolved
    /// type: a name in it is a template parameter exactly when it is one of the
    /// names the template declared, and the argument's type is what that
    /// parameter becomes.
    pub(super) fn deduce_from_type(
        &self,
        parameter: &fallc_ast::TypeSyntax,
        argument: &Type,
        params: &[fallc_ast::TemplateParam],
        deduced: &mut std::collections::HashMap<String, Type>,
    ) {
        use fallc_ast::TypeSyntax;
        match parameter {
            TypeSyntax::Named(named) => {
                let names_a_parameter = params
                    .iter()
                    .any(|param| param.name == named.path
                        && param.kind == fallc_ast::TemplateParamKind::Type);
                if names_a_parameter {
                    deduced
                        .entry(named.path.clone())
                        .or_insert_with(|| argument.clone());
                    return;
                }
                // `Crate<T>` against the member `Crate<int>`: the family and
                // the arguments it was built with are recorded under the
                // member's own name, so the written patterns are matched
                // against them one position at a time. Without this a
                // parameter written by value or by reference -- `fn first(c
                // &const Crate<T>)` -- deduced nothing, and every call had to
                // write `first<int>(c)`.
                if named.arguments.is_empty() {
                    return;
                }
                let Some(receiver) = argument.nominal_name() else {
                    return;
                };
                let Some((family, actual)) = self.type_template_arguments.get(receiver) else {
                    return;
                };
                // The same family, spelled either way: `Crate` in the module
                // that declares it, `std.Crate` elsewhere.
                let last = |name: &str| {
                    name.rsplit_once('.').map_or(name, |(_, last)| last).to_owned()
                };
                if last(family) != last(&named.path) {
                    return;
                }
                for (written, actual) in named.arguments.iter().zip(actual.0.iter()) {
                    let fallc_ast::TemplateArgumentSyntax::Type(written) = written else {
                        continue;
                    };
                    let crate::template::TemplateArgument::Type(actual) = actual else {
                        continue;
                    };
                    self.deduce_from_type(written, actual, params, deduced);
                }
            }
            // A pointer is looked through only when the argument *is* one: a
            // `*T` parameter takes a pointer, so an iterator passed to
            // `sort(first, last)` of `*T` deduces nothing and the next candidate
            // gets its turn.
            TypeSyntax::Pointer(inner)
            | TypeSyntax::PointerToConst(inner)
            | TypeSyntax::ConstPointer(inner) => {
                let Some(pointee) = argument.pointee().cloned() else {
                    return;
                };
                let pointee = pointee.unqualified().clone();
                self.deduce_from_type(inner, &pointee, params, deduced);
            }
            // A reference binds to a value: `&const T` says `T` is what the
            // argument refers to, which is how `min(a, b)` deduces from
            // arguments passed by reference.
            TypeSyntax::Reference(inner)
            | TypeSyntax::ConstReference(inner)
            | TypeSyntax::RvalueReference(inner)
            | TypeSyntax::ConstRvalueReference(inner)
            | TypeSyntax::Volatile(inner)
            | TypeSyntax::Const(inner) => {
                let pointee = argument.pointee().cloned().unwrap_or_else(|| argument.clone());
                let pointee = pointee.unqualified().clone();
                self.deduce_from_type(inner, &pointee, params, deduced);
            }
            // A value parameter, an array, or anything else says nothing about
            // a type parameter: `N usize` is a value the caller writes out.
            _ => {}
        }
    }

    /// Substitutes, checks, and records one instantiation.
    ///
    /// # Why the body is not rewritten
    ///
    /// The substitution is installed while the body is checked, and applied as
    /// types are resolved and names are looked up. Rewriting the tree instead
    /// would mean a *second* AST -- one per instantiation -- and every later
    /// pass would have to know which one it was looking at.
    ///
    /// The check is a real check: the body is checked against the substituted
    /// types, so `lhs < rhs` is resolved for the type `T` actually became, and a
    /// template used with a type that has no `<` is reported at the line inside
    /// the template.
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_arguments)]
    pub(super) fn check_instantiation(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        function: &fallc_ast::FunctionItem,
        signature: &FunctionSignature,
        substitution: &crate::template::Substitution,
        key: &str,
        call_arguments: &[(usize, fallc_ast::ExprId)],
        span: fallc_span::Span,
        caller: (&Ast, &Resolutions),
        module_bindings: Option<&crate::types::BindingTypes>,
    ) -> Option<Type> {
        let (caller_ast, caller_resolutions) = caller;
        // A function that declares a parameter pack is expanded for this call:
        // the elements become ordinary parameters, so the symbol, the body
        // check and code generation all see a function of fixed arity. The
        // expansion lives in local storage because the references below point
        // into it.
        let expanded: Option<(packs::Expanded, Resolutions)>;
        let (function, ast, resolutions, param_types) = match signature.pack {
            Some(pack) => {
                // The fixed parameters, then the elements. The elements' types
                // are what the arguments have, unless the pack's written
                // element type is itself a type pack -- `values ...Ts` -- in
                // which case `Ts` was bound to exactly those types.
                let mut param_types: Vec<Type> = signature.param_types[..pack]
                    .iter()
                    .map(|parameter| self.resolve_template_type(parameter, substitution))
                    .collect();
                let element_count = call_arguments.len().saturating_sub(pack);
                let written = &signature.param_types[pack];
                let element_types = match bound_pack_types(written, substitution) {
                    Some(types) => types,
                    None => {
                        let ty = self.resolve_template_type(written, substitution);
                        vec![ty; element_count]
                    }
                };
                if element_types.len() != element_count {
                    self.diagnostics.push(
                        Diagnostic::error(format!(
                            "`{}` is given {} element(s), but `{}` supplies {}",
                            signature.written_name,
                            element_count,
                            written_name_of(written),
                            element_types.len()
                        ))
                        .with_code(DiagnosticCode::sema(36))
                        .with_span(span),
                    );
                    return None;
                }
                param_types.extend(element_types.iter().cloned());
                let expansion = match packs::expand(ast, function, pack, &param_types) {
                    Ok(expansion) => expansion,
                    Err(problem) => {
                        self.diagnostics.push(
                            Diagnostic::error(problem.message())
                                .with_code(DiagnosticCode::sema(80))
                                .with_span(function.name_span),
                        );
                        return None;
                    }
                };
                // The elements are parameters of the expanded function, so the
                // body's uses of them need definitions: one per element, named
                // as the expansion named them.
                let mut extended = resolutions.clone();
                for (name, name_span) in packs::element_bindings(&expansion, pack) {
                    let id = crate::resolve::DefId::from_raw(
                        u32::try_from(extended.defs.len()).expect("definitions fit in u32"),
                    );
                    extended.defs.push(crate::resolve::Def {
                        id,
                        kind: crate::resolve::DefKind::Param,
                        name,
                        node: None,
                        name_span,
                        span: name_span,
                        scope: crate::resolve::ScopeId::from_raw(0),
                        signature: None,
                        emitted_as: None,
                        imported_as: None,
                        imported_via: None,
                        is_builtin: false,
                    });
                }
                // A copied name has a span of its own; what it means is what
                // the name it copied meant.
                for (new_span, old_span) in &expansion.name_spans {
                    if let Some(id) = resolutions.resolve(*old_span) {
                        extended.bind(*new_span, id);
                    }
                }
                let param_types = expansion.param_types.clone();
                expanded = Some((expansion, extended));
                let (expansion, extended) = expanded.as_ref().expect("just stored");
                (&expansion.function, &expansion.tree, extended, param_types)
            }
            None => {
                let param_types: Vec<Type> = signature
                    .param_types
                    .iter()
                    .map(|parameter| self.resolve_template_type(parameter, substitution))
                    .collect();
                (function, ast, resolutions, param_types)
            }
        };
        // The parameters of this instantiation may be by-value family members
        // (`fn take(v Array<T>)`); each copy at the callee's entry calls the
        // member's copy constructor, so it is requested for exactly those types.
        for parameter in &param_types {
            self.request_parameter_copy_constructor(resolutions, parameter);
        }
        let returns = self.resolve_template_type(&signature.return_type, substitution);

        // The symbol follows the same rule as every function's -- the written
        // name, then the mangled parameters -- with the instantiation's
        // parameters standing in for the template's. Two instantiations of one
        // template therefore have different symbols, which is what makes them
        // two functions rather than one.
        let mangled: Vec<String> = param_types.iter().map(crate::types::mangle_type).collect();
        let mut symbol = fallc_ast::special::symbol_for(&signature.written_name, &mangled);
        // An instantiation whose parameters do not mention `T` would otherwise
        // share the template's own name -- and two instantiations of it would
        // share each other's. `template <index usize, string []char> fn
        // get_pos()` has no parameters at all, so the arguments are what tells
        // its instantiations apart, and they go into the symbol for the same
        // reason a parameter type does: two functions may not share a name.
        // Whether the *written* parameters mention a type parameter is what
        // decides it: the resolved ones never do, because substitution has
        // already replaced them.
        let distinguished = signature.param_types.iter().any(|written| {
            mentions_type_parameter_syntax(written, &signature.template_params)
        });
        // A *value* parameter never appears in the mangled type of a
        // parameter, so two instantiations that differ only in one would share
        // a symbol -- `scaled<int, 3>` and `scaled<int, 4>` both mangling to
        // `scaled_i32`. The argument list is what distinguishes them, so it is
        // appended for any template that declares a value parameter.
        let has_value_parameter = signature
            .template_params
            .iter()
            .any(|param| matches!(param.kind, fallc_ast::TemplateParamKind::Value(_)));
        // A concrete pack -- `values ...int` -- mangles to the same string
        // whatever the number of elements, so the call's key keeps two
        // instantiations of different arity apart. A type-pack argument
        // already shows in the mangled types.
        if !distinguished || has_value_parameter || signature.pack.is_some() {
            symbol = format!("{symbol}${}", key);
        }

        // The parameters are recorded under their name spans, exactly as a
        // function's are: a reference to `lhs` inside the body resolves the
        // same way, and it has to, because the *substituted* types are what
        // its uses are checked against.
        for (parameter, ty) in function.params.iter().zip(&param_types) {
            self.binding_types.insert(parameter.name_span, ty.clone());
        }

        // The body is checked with the substitution in place. Anything the
        // check resolves -- a call, an operator, an index -- is recorded under
        // its span exactly as it would be for an ordinary function, and code
        // generation reads those records the same way.
        let previous = self.substitution.take();
        let previous_expected = self.expected.take();
        let previous_values = std::mem::take(&mut self.template_value_types);
        self.substitution = Some(substitution.clone());
        self.expected = Some(returns.clone());
        // The value parameters' declared types, so that a use of one inside the
        // body -- `string[index]` -- has a type to be checked against.
        for param in &signature.template_params {
            if let fallc_ast::TemplateParamKind::Value(ty) = &param.kind {
                let ty = self.resolve_template_type(ty, substitution);
                self.template_value_types.insert(param.name.clone(), ty);
            }
        }
        // The return type is published the way a function's is, so that a
        // `return` inside the body is checked against it -- and so that an
        // `auto` binding inside the template picks up the instantiation's
        // types rather than the template's names.
        self.return_types
            .push((signature.written_name.clone(), returns.clone()));
        // A table of this instantiation's own, so that a later instantiation
        // checking the same expressions does not overwrite these. The same is
        // true of every record the check makes, because they are all keyed by
        // span and the spans are shared.
        //
        // The swap happens *here*, after the call's own arguments have been
        // recorded: those belong to the caller, and taking them with the
        // instantiation's left the call passing values where the instantiation
        // declares references.
        let mut instantiation_types = TypeTable::new();
        // The body's own bindings, carrying only what the check below records.
        // The parameters are re-registered afterwards, because the map has to
        // be cleared for the body and the parameter bindings were put in it
        // just above -- taking it here discarded them, so every use of a
        // parameter inside an instantiation had no type at all. Nothing that
        // uses a parameter could then be checked: `a < b ? a : b` reported the
        // condition as untyped at code generation.
        let parameters: Vec<(fallc_span::Span, Type)> = function
            .params
            .iter()
            .zip(&param_types)
            .map(|(parameter, ty)| (parameter.name_span, ty.clone()))
            .collect();
        let previous_bindings = std::mem::take(&mut self.binding_types);
        for (span, ty) in &parameters {
            self.binding_types.insert(*span, ty.clone());
        }
        // The body's module's variables are named by their bare names, so their
        // types come with the body: a global's type is not in this file's
        // tables, and without it the member call on one was never resolved.
        if let Some(bindings) = module_bindings {
            for (span, ty) in bindings.iter() {
                self.binding_types.insert(span, ty.clone());
            }
        }
        let saved_records = self.take_records();
        // The body's expressions have ids of their own, which mean something
        // else in the file being compiled: nothing it records may be filed with
        // the main tree's expectations.
        let saved_expectations = std::mem::take(&mut self.expected_types);
        // An expanded body is a *different* tree: its expression ids are not
        // the declaration's, so the constant index has to be rebuilt for it.
        // Without this, folding read whatever the original tree had at the
        // same number -- a call became the constant of an unrelated
        // expression.
        let expanded_index = if signature.pack.is_some() {
            self.adopt_const_index(ast)
        } else {
            None
        };
        // `raise` and `??` are allowed exactly where the function says it may
        // raise; a free template's instantiation is a function, so its `except`
        // is what the body is checked under. Without this push every `raise`
        // inside a free template was reported as S0082, however the template
        // was declared.
        self.throws_allowed.push(function.throws.is_some());
        if let Some(body) = function.body {
            self.check_block(ast, resolutions, &mut instantiation_types, body);
        }
        self.throws_allowed.pop();
        self.restore_const_index(expanded_index);
        // An interface used inside the body is settled against this
        // instantiation's own types, and recorded with its records.
        self.resolve_interface_conversions(resolutions, &instantiation_types);
        self.expected_types = saved_expectations;
        let instantiation_records = self.restore_records(saved_records);
        let instantiation_bindings = std::mem::replace(&mut self.binding_types, previous_bindings);
        // A `constexpr` template whose body is a single `return <constant>` is
        // itself a constant, and the caller has to see it: the value was folded
        // while the body was checked, and recording it at the *call's* span is
        // what lets `a [sum<1, 2, 3>()]int;` stand where a constant is
        // required. The general rule -- any body the interpreter can run -- is
        // a larger question; this is the shape a constant-returning template
        // actually has.
        let returned = function.body.and_then(|body| {
            let statements = ast.block(body).statements.clone();
            match statements.as_slice() {
                [stmt] => match &ast.stmt(*stmt).kind {
                    fallc_ast::StmtKind::Return(ret) => ret.value,
                    _ => None,
                },
                _ => None,
            }
        });
        let returned_value = returned
            .and_then(|expr| instantiation_records.constant_values.get(&ast.expr(expr).span).copied());
        if let Some(value) = returned_value {
            self.constant_values.insert(span, value);
        }

        // The call's own arguments are checked against the *instantiated*
        // parameters, which is where a value is found to fit a reference and
        // recorded as one to pass by address. This belongs to the *caller*, so
        // it happens after the instantiation's own records have been taken
        // away -- otherwise the record travelled into the instantiation and the
        // call passed values where the instantiation declares references.
        //
        // The constant index has to be the caller's for the same reason: these
        // are the caller's expressions, and their ids only mean something in
        // the caller's tree. Reading them through the body's index folded an
        // unrelated expression -- `data.length()` became `-1`, the negation of
        // whatever the body's tree had at that number.
        let caller_index = self.adopt_const_index(caller_ast);
        for &(index, argument) in call_arguments {
            let Some(expected) = param_types.get(index) else {
                continue;
            };
            let previous = self.expected.replace(expected.clone());
            let found = self.infer_expr(caller_ast, caller_resolutions, types, argument);
            self.expected = previous;
            if let Some(found) = found {
                self.note_indirect_argument(argument, &found, expected, caller_ast);
                // The argument has to fit the *instantiated* parameter, which
                // is the only place a pack's element type is known: a concrete
                // `values ...int` must reject a word of text.
                let was_parameter = self.in_parameter_position;
                self.in_parameter_position = true;
                self.types_must_match(
                    expected,
                    &found,
                    caller_ast.expr(argument).span,
                    "argument",
                );
                self.in_parameter_position = was_parameter;
            }
        }
        self.restore_const_index(caller_index);
        self.return_types.pop();
        self.template_value_types = previous_values;
        self.expected = previous_expected;
        self.substitution = previous;

        // Recorded so that a second call with the same arguments reuses this
        // instantiation, and so that code generation knows what to emit.
        // The call reaches the instantiation, so its symbol is what code
        // generation emits -- recorded here for the same reason every other
        // call's is: only the checker knows which function the call selected.
        self.call_symbols.insert(span, symbol.clone());
        self.instantiations.insert(key.to_owned(), symbol.clone());
        self.instantiated_returns.insert(key.to_owned(), returns.clone());
        self.instantiation_list.push(Instantiation {
            key: key.to_owned(),
            symbol: symbol.clone(),
            tree: ast.clone(),
            function: function.clone(),
            arguments: crate::template::Arguments(
                signature
                    .template_params
                    .iter()
                    .map(|param| {
                        substitution
                            .get(&param.name)
                            .cloned()
                            .unwrap_or(crate::template::TemplateArgument::Int(0))
                    })
                    .collect(),
            ),
            params: signature.template_params.clone(),
            types: instantiation_types,
            bindings: instantiation_bindings,
            records: instantiation_records,
            resolved_params: param_types.clone(),
            resolved_returns: returns.clone(),
        });
        let _ = span;
        Some(returns)
    }

    /// Whether the instantiated parameters take the call's arguments.
    ///
    /// Judged from the types recorded while the arguments were matched, so a
    /// candidate can be rejected without side effects on the call site.
    fn instantiated_arguments_fit(
        &mut self,
        signature: &FunctionSignature,
        substitution: &crate::template::Substitution,
        written: &[(usize, fallc_ast::ExprId)],
        types: &TypeTable,
    ) -> bool {
        // A signature whose own parameters are not all bound is a pack being
        // expanded, not a candidate whose arguments can be judged here.
        if signature.pack.is_some()
            || signature
                .template_params
                .iter()
                .any(|param| substitution.type_of(&param.name).is_none()
                    && substitution.value_of(&param.name).is_none())
        {
            return true;
        }
        for &(index, argument) in written {
            let Some(parameter) = signature.param_types.get(index) else {
                continue;
            };
            let expected = self.resolve_template_type(parameter, substitution);
            let Some(found) = types.get(argument).cloned() else {
                // Not recorded: the argument is not this candidate's to judge.
                continue;
            };
            if self.is_compatible(&found, &expected) {
                continue;
            }
            // An unsuffixed integer literal adopts an integer parameter.
            if found == Type::INT && expected.is_integer() {
                continue;
            }
            // A value fits a reference or pointer to it.
            if expected
                .referent()
                .or_else(|| expected.pointee())
                .is_some_and(|inner| self.is_compatible(&found, inner))
            {
                continue;
            }
            return false;
        }
        true
    }

/// The template parameter a forwarding reference deduces, if this is one.
///
/// `&&T`, with `T` a type parameter, is the shape: an lvalue argument binds it
/// as `T = &U` and an rvalue as `T = U`.
fn universal_reference_parameter<'a>(
    parameter: &fallc_ast::TypeSyntax,
    params: &'a [fallc_ast::TemplateParam],
) -> Option<&'a str> {
    use fallc_ast::TypeSyntax;
    let inner = match parameter {
        TypeSyntax::RvalueReference(inner) | TypeSyntax::ConstRvalueReference(inner) => inner,
        _ => return None,
    };
    let TypeSyntax::Named(named) = inner.as_ref() else {
        return None;
    };
    if !named.arguments.is_empty() {
        return None;
    }
    params
        .iter()
        .find(|param| {
            param.name == named.path && param.kind == fallc_ast::TemplateParamKind::Type
        })
        .map(|param| param.name.as_str())
}

/// Whether an expression names a place rather than producing a value.
///
/// Names, fields, indexed elements and dereferences are places: passing one to
/// a forwarding reference is passing an lvalue. A literal, a call and an
/// arithmetic expression are values.
pub(super) fn value_is_lvalue(ast: &Ast, expression: fallc_ast::ExprId, ty: &Type) -> bool {
    // A reference says which category it stands for: `&&U` is a value about to
    // disappear, `&U` is one that will still be there. Only when the type is not
    // a reference does the shape of the expression decide.
    match ty {
        Type::Reference { is_rvalue, .. } => !is_rvalue,
        _ => Self::expression_is_lvalue(ast, expression),
    }
}

pub(super) fn expression_is_lvalue(ast: &Ast, expression: fallc_ast::ExprId) -> bool {
    use fallc_ast::ExprKind;
    match &ast.expr(expression).kind {
        ExprKind::Name(_) | ExprKind::Field(_) | ExprKind::Index(_) => true,
        ExprKind::Unary(unary) if unary.op == fallc_ast::UnaryOp::Deref => true,
        _ => false,
    }
}

    /// Resolves a written type with the current substitution applied.
    ///
    /// A name that a template parameter stands for becomes what it stands for;
    /// everything else resolves as usual. This is the seam that makes an
    /// instantiation's `T` a real type without the tree ever being rewritten.
    pub(super) fn resolve_template_type(
        &mut self,
        ty: &fallc_ast::TypeSyntax,
        substitution: &crate::template::Substitution,
    ) -> Type {
        // A parameter that stands for a type is replaced by that type *here*,
        // rather than by a name the substitution would have to invent: the
        // types are in hand at this point, and a name would have to be looked
        // up again to get back to them.
        if let fallc_ast::TypeSyntax::Named(named) = ty
            && let Some(ty) = substitution.type_of(&named.path)
        {
            return ty.clone();
        }
        // A written type whose *arguments* contain an expansion -- `Box<Args...>`
        // -- has its arguments spliced before it is resolved, because the
        // substitution is what says how many there are. `substitute_written_type`
        // cannot do this: a pack is not a type but a list of them, and only a
        // written argument list has somewhere to put the list.
        //
        // The expansion yields *resolved* types, which is what
        // `instantiate_type_template_from` takes; going back through written
        // syntax would mean inventing names for types that have none.
        if let fallc_ast::TypeSyntax::Named(named) = ty
            && named
                .arguments
                .iter()
                .any(fallc_ast::TemplateArgumentSyntax::is_expansion)
            && self.type_templates.contains_key(&named.path)
        {
            let arguments = self.expand_written_type_arguments(&named.arguments, substitution);
            let previous = self.substitution.replace(substitution.clone());
            let template = self.type_templates.get(&named.path).cloned();
            let resolved = template.and_then(|template| {
                self.build_type_member(&template, arguments, named.span)
            });
            self.substitution = previous;
            return resolved.unwrap_or_else(|| {
                Type::unspecified(named.path.clone(), named.span)
            });
        }
        // A parameter *inside* another type -- the `T` in `&const T` -- is
        // replaced as the written type is resolved, by the substitution the
        // checker is holding. Installing it here rather than making the caller
        // do so means every nested form is covered by the one rule in
        // `resolve_type_syntax`.
        let substituted = substitution.substitute_written_type(ty);
        let previous = self.substitution.replace(substitution.clone());
        let resolved = self.resolve_type_syntax(&substituted, true);
        self.substitution = previous;
        resolved
    }

    /// Reads the template arguments a call wrote out.
    pub(super) fn written_template_arguments(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        signature: &FunctionSignature,
        node: &CallExpr,
        span: fallc_span::Span,
    ) -> Option<crate::template::Arguments> {
        let params = &signature.template_params;
        let fixed = params.iter().take_while(|param| !param.is_pack).count();
        let has_pack = fixed < params.len();
        let given = node.template_args.len();
        // A trailing pack accepts any number of arguments, so only the fixed
        // part has to match exactly. Without a pack the two counts are the same
        // number, which is the check this replaces.
        // A caller leaves arguments out from the right, so a short list is
        // fine when every parameter it omits has a default.
        let defaulted_tail = params
            .iter()
            .skip(given)
            .take(fixed.saturating_sub(given))
            .all(|param| param.default.is_some());
        let arity_ok = if has_pack {
            given >= fixed
        } else {
            given == fixed || given < fixed && defaulted_tail
        };
        if !arity_ok {
            let expected = if has_pack {
                format!("at least {fixed}")
            } else {
                fixed.to_string()
            };
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "`{}` takes {expected} template argument(s), but {given} were given",
                    signature.written_name,
                ))
                .with_code(DiagnosticCode::sema(72))
                .with_span(span),
            );
            return None;
        }
        // Only the *fixed* part: everything past it belongs to the pack, which
        // is gathered below. Zipping the whole list against the whole parameter
        // list put the pack's first argument in twice -- once on its own and
        // once inside the pack -- so `Box<Args...>` saw one argument too many.
        let mut arguments = Vec::with_capacity(given);
        for (written, param) in node.template_args.iter().zip(params).take(fixed) {
            match (written, &param.kind) {
                (fallc_ast::TemplateArgumentSyntax::Type(ty), fallc_ast::TemplateParamKind::Type) => {
                    let resolved = self.resolve_written_type(ty);
                    arguments.push(crate::template::TemplateArgument::Type(resolved));
                }
                (
                    fallc_ast::TemplateArgumentSyntax::Value(expr),
                    fallc_ast::TemplateParamKind::Value(_),
                ) => {
                    arguments.push(self.template_value_argument(
                        ast,
                        resolutions,
                        types,
                        *expr,
                        param,
                        span,
                    )?);
                }
                // A type was written where a value belongs, or the reverse. The
                // message says which the parameter wanted, because that is the
                // part the caller got wrong.
                (_, fallc_ast::TemplateParamKind::Type) => {
                    self.diagnostics.push(
                        Diagnostic::error(format!(
                            "template parameter `{}` is a type, but a value was given",
                            param.name
                        ))
                        .with_code(DiagnosticCode::sema(73))
                        .with_span(span),
                    );
                    return None;
                }
                (_, fallc_ast::TemplateParamKind::Value(_)) => {
                    self.diagnostics.push(
                        Diagnostic::error(format!(
                            "template parameter `{}` is a value, but a type was given",
                            param.name
                        ))
                        .with_code(DiagnosticCode::sema(73))
                        .with_span(span),
                    );
                    return None;
                }
            }
        }
        // The pack collects whatever is left, which is what makes
        // `count<int, f64, char>()` reach a single `Args ...type` parameter.
        if has_pack {
            let mut elements = Vec::with_capacity(given - fixed);
            for (written, param) in node.template_args[fixed..].iter().zip(std::iter::repeat(&params[fixed])) {
                match (written, &param.kind) {
                    (
                        fallc_ast::TemplateArgumentSyntax::Type(ty),
                        fallc_ast::TemplateParamKind::Type,
                    ) => {
                        let resolved = self.resolve_written_type(ty);
                        elements.push(crate::template::TemplateArgument::Type(resolved));
                    }
                    (
                        fallc_ast::TemplateArgumentSyntax::Value(expr),
                        fallc_ast::TemplateParamKind::Value(_),
                    ) => {
                        elements.push(self.template_value_argument(
                            ast,
                            resolutions,
                            types,
                            *expr,
                            param,
                            span,
                        )?);
                    }
                    (_, fallc_ast::TemplateParamKind::Type) => {
                        self.diagnostics.push(
                            Diagnostic::error(format!(
                                "template parameter pack `{}` is a pack of types, but a value \
                                 was given",
                                param.name
                            ))
                            .with_code(DiagnosticCode::sema(73))
                            .with_span(span),
                        );
                        return None;
                    }
                    (_, fallc_ast::TemplateParamKind::Value(_)) => {
                        self.diagnostics.push(
                            Diagnostic::error(format!(
                                "template parameter pack `{}` is a pack of values, but a type \
                                 was given",
                                param.name
                            ))
                            .with_code(DiagnosticCode::sema(73))
                            .with_span(span),
                        );
                        return None;
                    }
                }
            }
            arguments.push(crate::template::TemplateArgument::Pack(elements));
        }
        let _ = (resolutions, types);
        Some(crate::template::Arguments(arguments))
    }

    /// Reads one written *value* template argument.
    ///
    /// Two kinds are allowed, and they are the two a value can be written down
    /// as: an integer, and a character sequence. Nothing else can be known
    /// before the program runs, which is what a template argument has to be.
    pub(super) fn template_value_argument(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        expr: fallc_ast::ExprId,
        param: &fallc_ast::TemplateParam,
        span: fallc_span::Span,
    ) -> Option<crate::template::TemplateArgument> {
        // A string literal is a character sequence, which is the one
        // non-integer a value parameter accepts.
        if let fallc_ast::ExprKind::StringLiteral(text)
        | fallc_ast::ExprKind::ByteStringLiteral(text) = &ast.expr(expr).kind
        {
            let bytes = decode_string_literal(text);
            return Some(crate::template::TemplateArgument::Text(bytes));
        }
        // A raw string is a value parameter's text too, and its characters are
        // the source between the delimiters with nothing decoded.
        if let fallc_ast::ExprKind::RawStringLiteral(text) = &ast.expr(expr).kind {
            let bytes = raw_string_body(text).as_bytes().to_vec();
            return Some(crate::template::TemplateArgument::Text(bytes));
        }
        // Type checked as well as folded. The argument is *lowered* too: one
        // that is not a literal -- `f<sz()>()` -- leaves a call node in the
        // tree, and code generation needs its type and the value the checker
        // worked out. Reading the constant without inferring left that node
        // untyped, and the call reported as a name with no value.
        let previous = self.expected.take();
        let _ = self.infer_expr(ast, resolutions, types, expr);
        self.expected = previous;
        // A template argument is a constant position, so it is held to the
        // language's rule: a call here has to be to a `constexpr` function.
        // Folding whatever happens to work would make a template's *identity*
        // depend on an optimisation -- `f<g()>` would be one instantiation
        // under one compiler and unbuildable under another.
        if !self.require_constant(expr, ast.expr(expr).span, &format!("template argument `{}`", param.name), 74) {
            return None;
        }

        // Anything else must fold to an integer, which is the checker's own
        // constant folder rather than a second one.
        match self.const_eval_int(expr) {
            Some(value) => Some(crate::template::TemplateArgument::Int(value)),
            None => {
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "the value for `{}` is not a constant",
                        param.name
                    ))
                    .with_code(DiagnosticCode::sema(74))
                    .with_span(ast.expr(expr).span)
                    .with_note(
                        "a template argument is known before the program runs: an integer or \
                         a character sequence is, and a variable is not",
                    ),
                );
                let _ = span;
                None
            }
        }
    }

    /// The definition name a special method on this receiver is bound under.
    ///
    /// # The rule
    ///
    /// A member function is defined once per receiver *type*, so its name
    /// carries the type: `__init__` on `Array` is `__init__Array`. Two things
    /// about the receiver are the member's rather than the family's and are
    /// therefore dropped:
    ///
    ///   * the template arguments -- `Array<i32>` is a member of the family
    ///     `Array`, and the method was written once for the family;
    ///   * the module qualification -- `std.Array` is how an importer spells a
    ///     name the declaring module writes as `Array`, and the definition
    ///     carries the name its own module used.
    ///
    /// Written once because it was written eight times, and every copy had to
    /// be taught about templates separately.
    pub(super) fn special_method_qualified(receiver: &str, method: &str) -> String {
        let base = receiver
            .split_once('<')
            .map_or(receiver, |(base, _)| base);
        let base = base.rsplit_once('.').map_or(base, |(_, last)| last);
        format!("{method}{base}")
    }

    /// Whether a method's receiver expression can fill its declared `this`.
    ///
    /// A value fits its own type and the type a reference parameter names,
    /// because a method call takes the value's address. A pointer fits a
    /// reference too: both are the address of the same referent, so
    /// `p.method()` on a `p: *T` is the call `(*p).method()` makes. That is
    /// what lets a generic body construct into raw storage through the pointer
    /// it was handed.
    pub(super) fn receiver_fits(&self, receiver: &Type, expected: &Type) -> bool {
        if self.same_type_named_apart(receiver, expected) {
            return true;
        }
        if let Some(pointee) = expected.pointee()
            && self.same_type_named_apart(receiver, pointee)
        {
            return true;
        }
        self.is_compatible(receiver, expected)
    }

    /// Answers `slot->__init__(value)`, `slot->__assign__(value)` or
    /// `slot->__drop__()` when no declared method applies, returning the
    /// operation the call stands for.
    ///
    /// # The rule
    ///
    /// A primitive has the three lifetime methods built in: `__init__` and
    /// `__assign__` are a store of the value and `__drop__` is nothing, because
    /// it owns nothing. A declared type may define any of them, and a call
    /// finds the declaration first; when it defines none, the language's
    /// implicit rules apply -- a member-wise copy for construction, a
    /// member-wise store for assignment, field-wise destruction for a
    /// destructor -- which are the same operations, so the same record serves
    /// both. The choice is made here, per substituted type, and code
    /// generation emits the operation rather than looking for a function that
    /// does not exist.
    ///
    /// Only a receiver that names storage -- a pointer or a reference -- is
    /// answered. Construction into raw storage is what the spelling means, and
    /// a call on a value that has no `__init__` is still a missing method.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn implicit_in_place_operation(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        field: &FieldExpr,
        node: &CallExpr,
        receiver_ty: &Type,
        span: fallc_span::Span,
    ) -> Option<InPlaceOperation> {
        // The type the slot holds, reached through the pointer the caller has.
        let target = receiver_ty.pointee()?.unqualified().clone();
        if field.field == fallc_ast::special::SpecialMethod::Init.name() {
            if node.args.len() != 1 {
                return None;
            }
            let previous = self.expected.replace(target.clone());
            let found = self.infer_expr(ast, resolutions, types, node.args[0]);
            self.expected = previous;
            let found = found?;
            // A reference stored into raw storage is the value behind it that
            // is constructed there. This is the *implicit* rule, reached only
            // when the type declares no `__init__` that fits: a primitive has
            // none, and its storage takes the value, not the address the value
            // happens to live at. A type that declares a move form never comes
            // here -- that call has a signature, and the signature decides.
            if let Some(referent) = found.referent() {
                self.deref_reads
                    .insert(ast.expr(node.args[0]).span, referent.clone());
            }
            if !self.is_compatible(&found, &target) {
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "`__init__` stores a `{}`, but this slot holds a `{}`",
                        found.spelling(),
                        target.spelling()
                    ))
                    .with_code(DiagnosticCode::sema(63))
                    .with_span(ast.expr(node.args[0]).span),
                );
                return None;
            }
            return Some(InPlaceOperation::Store);
        }
        if field.field == fallc_ast::special::SpecialMethod::Drop.name() {
            if !node.args.is_empty() {
                return None;
            }
            return Some(InPlaceOperation::Drop);
        }
        // Assignment is implicit for every type that does not declare it, so
        // an explicit `__assign__` is only meaningful where the primitive's
        // built-in form applies. A declared type that declares none keeps the
        // ordinary `x = y` spelling.
        if field.field == fallc_ast::special::SpecialMethod::Assign.name() {
            if node.args.len() != 1 || !Self::has_builtin_special_methods(&target) {
                return None;
            }
            let previous = self.expected.replace(target.clone());
            let found = self.infer_expr(ast, resolutions, types, node.args[0]);
            self.expected = previous;
            let found = found?;
            if let Some(referent) = found.referent() {
                self.deref_reads
                    .insert(ast.expr(node.args[0]).span, referent.clone());
            }
            if !self.is_compatible(&found, &target) {
                return None;
            }
            return Some(InPlaceOperation::Store);
        }
        let _ = span;
        None
    }

    /// Whether a type's lifetime methods are the compiler's rather than a
    /// declaration's.
    ///
    /// True for the types that own nothing: a numeric, a character, a boolean
    /// or a pointer. They cannot carry fields, so their `__init__` and
    /// `__assign__` are stores and their `__drop__` is empty by definition.
    pub(super) fn has_builtin_special_methods(ty: &Type) -> bool {
        matches!(
            ty.unqualified(),
            Type::Int(_)
                | Type::Byte
                | Type::Character(_)
                | Type::Bool
                | Type::Float(_)
                | Type::Pointer { .. }
                | Type::NullPointer
        )
    }

    /// Returns `true` if a name is a parameter of some type template.
    ///
    /// Used where a name would otherwise be reported as an undeclared type: a
    /// template's signature is resolved before any member exists, so `T` in
    /// `fn push(this &Array<T>, value T)` has to resolve to *something*. A
    /// declared type of the same name is found first, so this only decides the
    /// names nothing else explains.
    pub(super) fn type_template_parameter(&self, name: &str) -> bool {
        self.type_templates.values().any(|template| {
            template.template_params.iter().any(|param| {
                param.name == name && param.kind == fallc_ast::TemplateParamKind::Type
            })
        })
    }

    /// Records that a templated type's method must be emitted.
    ///
    /// A method written against `Box<T>` becomes a real function only when
    /// with `T` replaced. Which types it was replaced *by* is the member's
    /// arguments, so those are what the instantiation carries.
    ///
    /// The receiver's argument is the member itself, and the method's own
    /// template parameters are bound to the same thing -- that is what makes
    /// `Box<int>`'s constructor take an `int` rather than a `T` of its own
    /// choosing.
    pub(super) fn record_method_instantiation(
        &mut self,
        signature: &FunctionSignature,
        substitution: &crate::template::Substitution,
        symbol: &str,
        argument: &Type,
    ) {
        self.record_method_instantiation_with_pack(signature, substitution, symbol, argument, None);
    }

    /// Records a member instantiation whose parameters include a pack.
    ///
    /// The element types travel with the request because the template
    /// arguments do not always say them: `values ...int` has the same template
    /// arguments at every arity, so how many elements there are -- and what
    /// each is -- is a fact about the call.
    pub(super) fn record_method_instantiation_with_pack(
        &mut self,
        signature: &FunctionSignature,
        substitution: &crate::template::Substitution,
        symbol: &str,
        argument: &Type,
        pack_elements: Option<Vec<Type>>,
    ) {
        if self.instantiations.contains_key(symbol) {
            return;
        }
        // The *definition* carries the node, so the body is found for a
        // template declared in another module too -- which is the ordinary
        // case, because that is where a family is usually declared.
        // Looked up by the *symbol*, not the bound name: `Array` has three
        // `__init__` overloads and they share a bound name, so one entry per
        // name meant the last one registered answered for all of them -- the
        // copy constructor standing in for the empty one.
        // Looked up by the *template's* own symbol, which is what both sides
        // can build: the driver registers it from the declaration, and this
        // rebuilds it from the signature the call selected -- the written name
        // and the mangled parameters, before any of them are substituted.
        //
        // Not by the bound name: `Array` has three `__init__` overloads and
        // they share one, so a single entry per name meant the last one
        // registered answered for all of them.
        // The parameters are mangled from the *written* types as the driver
        // resolves them -- a declared type being unspecified there, because a
        // signature is read before any declaration is in scope. Resolving them
        // here instead gave a different string for the same declaration, and
        // the two sides never met.
        // The parameters are mangled from the types as the *declaration* sees
        // them, which is what the driver registered the body under.
        //
        // A template's signature is read before any declaration is in scope, so
        // a name in it that is not primitive resolves to an unspecified type
        // carrying that name. Resolving it here instead -- where the declared
        // types *are* in scope -- gave a nominal for `Array<T>` while the driver
        // had an unspecified for `Array`, and the two keys never met.
        let template_symbol = template_symbol_of(&signature.written_name, &signature.param_types);
        let Some(body) = self
            .template_body_for(&template_symbol)
            .or_else(|| self.template_body_for(&signature.bound_as))
        else {
            return;
        };
        let TemplateBody {
            tree,
            resolutions,
            constants,
            constant_types,
            function,
            attrs,
            environment,
            bindings,
        } = body;
        let arguments = crate::template::Arguments(
            signature
                .template_params
                .iter()
                .map(|param| match substitution.get(&param.name) {
                    Some(argument) => argument.clone(),
                    None => crate::template::TemplateArgument::Type(argument.clone()),
                })
                .collect(),
        );
        let _ = argument;
        self.instantiations.insert(symbol.to_owned(), symbol.to_owned());
        // The body is checked like any instantiation's, so that the records its
        // lowering needs -- which field is written, which call is made -- exist.
        // Recording the symbol alone left the body's expressions with no types
        // and nothing resolved.
        self.pending_method_instantiations
            .push(PendingMethodInstantiation {
                body: TemplateBody {
                    tree,
                    resolutions,
                    constants,
                    constant_types,
                    function,
                    attrs,
                    environment,
                    bindings,
                },
                symbol: symbol.to_owned(),
                substitution: substitution.clone(),
                arguments: arguments.clone(),
                params: signature.template_params.clone(),
                pack_elements,
            });
    }

    /// The substitution a method of an instantiated type is resolved under.
    ///
    /// A method is written against the declaration's parameters --
    /// `__init__(this &Box<T>, v T)` -- and both `T`s are the same parameter.
    /// The member's arguments are what those names mean, so binding them is
    /// what makes `Box<int>` reach `v int`. It is also what makes the method's
    /// *own* template parameters and the type's the same thing, which is right:
    /// `Box<int>`'s constructor is not free to choose a `T` of its own.
    ///
    /// No substitution at all when the receiver is not an instantiated type,
    /// which is every ordinary method.
    /// The substitution a method call is resolved under, read from the
    /// receiver's own type.
    ///
    /// A member of a family carries the arguments that made it, so the
    /// substitution can be built from the type itself -- which is what a value
    /// that crossed a module boundary has, since the importing file never
    /// resolved the written arguments and so has no other record of them.
    /// [`Self::member_substitution`] answers for a member this file built.
    pub(super) fn receiver_substitution(
        &self,
        receiver: &Type,
    ) -> Option<crate::template::Substitution> {
        let value = receiver.referent().unwrap_or(receiver);
        if let Type::Nominal(nominal) = value.unqualified()
            && let Some((family, arguments)) = &nominal.template_arguments
        {
            let declaration = self.type_templates.get(family).or_else(|| {
                let last = family.rsplit('.').next().unwrap_or(family);
                self.type_templates
                    .values()
                    .find(|declaration| declaration.name == last)
            });
            if let Some(declaration) = declaration
                && let Some(substitution) =
                    crate::template::Substitution::new(&declaration.template_params, arguments)
            {
                return Some(substitution);
            }
        }
        self.member_substitution(value.nominal_name())
    }

    pub(super) fn member_substitution(
        &self,
        receiver: Option<&str>,
    ) -> Option<crate::template::Substitution> {
        let receiver = receiver?;
        // The receiver is the *member*: `std.Array<i32>` in a file that imported
        // the umbrella, `Array<i32>` inside the module that declared it. Both
        // name one type, so both are tried -- and the recorded entry is keyed
        // by whichever name the declaring module built it under.
        let found = self.type_template_arguments.get(receiver).or_else(|| {
            receiver
                .rsplit_once('.')
                .and_then(|(_, last)| self.type_template_arguments.get(last))
        })?;
        let declaration = self.type_templates.get(&found.0)?;
        crate::template::Substitution::new(&declaration.template_params, &found.1)
    }

    /// Builds the member of a type family that `Array<int>` names.
    ///
    /// # One member per argument list
    ///
    /// The member is built once and remembered under the name its arguments
    /// make, so that two mentions of `Array<int>` are one type and not two --
    /// which matters, because a type is compared by identity and two structurally
    /// equal types would still not be the same one.
    ///
    /// The fields are resolved under a substitution, exactly as an
    /// instantiated function's parameters are: `value T` becomes `value int`,
    /// and the type is then an ordinary nominal type.
    /// Splices every pack expansion in a written argument list into the list.
    ///
    /// `Box<Ts...>` becomes one argument per element of `Ts`, so that the rest
    /// of the instantiation sees a flat list and never has to know a pack was
    /// involved. An element of a pack was reduced to its *value* when the pack
    /// was bound, so expansion yields a resolved argument; an ordinary written
    /// argument keeps its syntax, because only the parameter it lands on says
    /// whether it is a type or a value, and that is decided afterwards.
    pub(super) fn expand_type_arguments(
        &mut self,
        written: &[fallc_ast::TemplateArgumentSyntax],
        _span: fallc_span::Span,
    ) -> Option<Vec<ExpandedArgument>> {
        let mut out = Vec::with_capacity(written.len());
        for argument in written {
            // Only a *type* carries the `...` that marks an expansion, since
            // `is_expansion` lives on a written type's name.
            let fallc_ast::TemplateArgumentSyntax::Type(fallc_ast::TypeSyntax::Named(name)) =
                argument
            else {
                out.push(ExpandedArgument::Written(argument.clone()));
                continue;
            };
            if !name.is_expansion {
                out.push(ExpandedArgument::Written(argument.clone()));
                continue;
            }
            // What the name stands for, which has to be a pack. The
            // substitution is the only place a pack lives.
            let Some(elements) = self
                .substitution
                .as_ref()
                .and_then(|substitution| substitution.get(&name.path))
                .and_then(|argument| match argument {
                    crate::template::TemplateArgument::Pack(elements) => Some(elements.clone()),
                    _ => None,
                })
            else {
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "`{}...` expands something that is not a template parameter pack",
                        name.path
                    ))
                    .with_code(DiagnosticCode::sema(71))
                    .with_span(name.span),
                );
                return None;
            };
            out.extend(elements.into_iter().map(ExpandedArgument::Resolved));
        }
        Some(out)
    }

    /// Builds the member of a type family that `Array<int>` names.
    ///
    /// The written arguments are read against the declaration's own parameters,
    /// which is the only thing that says whether a written `int` is a type or a
    /// value, and any pack expansion among them is spliced first.
    pub(super) fn instantiate_type_template(&mut self, named: &fallc_ast::NamedType) -> Option<Type> {
        if !self.type_templates.contains_key(&named.path) {
            return None;
        }
        let written = self.expand_type_arguments(&named.arguments, named.span)?;
        let template = self.type_templates.get(&named.path)?.clone();
        let fixed = template
            .template_params
            .iter()
            .take_while(|param| !param.is_pack)
            .count();
        let has_pack = fixed < template.template_params.len();
        let given = written.len();
        let defaulted_tail = template
            .template_params
            .iter()
            .skip(given)
            .take(fixed.saturating_sub(given))
            .all(|param| param.default.is_some());
        if if has_pack { given < fixed } else { given != fixed && !(given < fixed && defaulted_tail) }
        {
            let expected = if has_pack { format!("at least {fixed}") } else { fixed.to_string() };
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "`{}` takes {expected} template argument(s), but {given} were given",
                    template.name,
                ))
                .with_code(DiagnosticCode::sema(72))
                .with_span(named.span),
            );
            return None;
        }
        let mut arguments = Vec::with_capacity(given);
        // The index picks the parameter as well as the argument: everything
        // past the fixed part is checked against the pack, so `enumerate` is
        // what ties the two lists together.
        for (index, argument) in written.iter().enumerate() {
            let param = if index < fixed {
                template.template_params.get(index)?
            } else {
                template.template_params.get(fixed)?
            };
            let resolved = match argument {
                ExpandedArgument::Resolved(argument) => argument.clone(),
                ExpandedArgument::Written(argument) => {
                    let wants_type = param.kind.is_type();
                    match (argument, wants_type) {
                        (fallc_ast::TemplateArgumentSyntax::Type(ty), true) => {
                            crate::template::TemplateArgument::Type(self.resolve_written_type(ty))
                        }
                        // A dotted name is read by the parameter: a type
                        // argument is that type, and a value argument is the
                        // constant the name stands for -- `Device<OpenMode.READ>`
                        // is the member, whose number is what the member is built
                        // with.
                        (
                            fallc_ast::TemplateArgumentSyntax::Named { path, span },
                            true,
                        ) => crate::template::TemplateArgument::Type(
                            self.resolve_written_type(&fallc_ast::TypeSyntax::Named(
                                fallc_ast::NamedType {
                                    path: path.clone(),
                                    arguments: Vec::new(),
                                    is_expansion: false,
                                    span: *span,
                                },
                            )),
                        ),
                        (
                            fallc_ast::TemplateArgumentSyntax::Named { path, span },
                            false,
                        ) => {
                            let Some(value) = self.constants_by_name.get(path.as_str()).copied()
                            else {
                                self.diagnostics.push(
                                    Diagnostic::error(format!(
                                        "the template argument `{path}` is not a constant"
                                    ))
                                    .with_code(DiagnosticCode::sema(71))
                                    .with_span(*span)
                                    .with_note(
                                        "a value parameter takes a constant: an enum member \
                                         such as `OpenMode.READ`, or a number",
                                    ),
                                );
                                return None;
                            };
                            crate::template::TemplateArgument::Int(value)
                        }
                        // A written value's argument has no tree here -- the
                        // name was written in a type position, and what it
                        // carries is a *type*, not an expression. A value
                        // argument cannot arrive this way, so an empty tree is
                        // the honest answer rather than a guess.
                        (fallc_ast::TemplateArgumentSyntax::Value(_), false) => return None,
                        (_, true) => {
                            self.diagnostics.push(
                                Diagnostic::error(format!(
                                    "template parameter `{}` is a type, but a value was given",
                                    param.name
                                ))
                                .with_code(DiagnosticCode::sema(73))
                                .with_span(named.span),
                            );
                            return None;
                        }
                        // `Device<Mode>` inside a method of the family: `Mode`
                        // is the enclosing template's *value* parameter, and what
                        // it stands for is the argument the member was built with.
                        (fallc_ast::TemplateArgumentSyntax::Type(fallc_ast::TypeSyntax::Named(written)), false) => {
                            let bound = self
                                .substitution
                                .as_ref()
                                .and_then(|substitution| substitution.value_of(&written.path))
                                .cloned();
                            match bound {
                                Some(crate::template::TemplateArgument::Int(value)) => {
                                    crate::template::TemplateArgument::Int(value)
                                }
                                Some(argument @ crate::template::TemplateArgument::Text(_)) => {
                                    argument
                                }
                                _ => {
                                                                        self.diagnostics.push(
                                        Diagnostic::error(format!(
                                            "template parameter `{}` is a value, but a type was \
                                             given",
                                            param.name
                                        ))
                                        .with_code(DiagnosticCode::sema(73))
                                        .with_span(named.span),
                                    );
                                    return None;
                                }
                            }
                        }
                        (_, false) => {
                                                        self.diagnostics.push(
                                Diagnostic::error(format!(
                                    "template parameter `{}` is a value, but a type was given",
                                    param.name
                                ))
                                .with_code(DiagnosticCode::sema(73))
                                .with_span(named.span),
                            );
                            return None;
                        }
                    }
                }
            };
            arguments.push(resolved);
        }
        // What the author left out comes from the parameter's default, in
        // order, so that a default may use the parameters before it.
        if arguments.len() < fixed {
            arguments = self
                .complete_template_arguments(
                    &template.template_params,
                    crate::template::Arguments(arguments),
                    named.span,
                )?
                .0;
        }
        if has_pack {
            let rest = arguments.split_off(fixed);
            arguments.push(crate::template::TemplateArgument::Pack(rest));
        }
        // A declared specialisation of this member, if its pattern matches.
        if let Some(chosen) = self.choose_specialisation(
            &named.path,
            &crate::template::Arguments(arguments.clone()),
            named.span,
        ) {
            if let Some(declaration) = chosen.declaration {
                return self.build_member_with(
                    &declaration,
                    crate::template::Arguments(arguments),
                    chosen.substitution,
                    named.span,
                );
            }
            if let Some(member) = chosen.resolved {
                return self.build_resolved_member(
                    &named.path,
                    crate::template::Arguments(arguments),
                    &chosen.substitution,
                    &member,
                    named.span,
                );
            }
            return None;
        }
        self.build_type_member(&template, crate::template::Arguments(arguments), named.span)
    }

    /// Slices a pack expansion in a written type's argument list.
    ///
    /// Used where a type is *resolved* under a substitution rather than read
    /// from source, so the expansion has to happen without a second parse. The
    /// elements are already-resolved arguments, and the caller turns them back
    /// into a member type.
    pub(super) fn expand_written_type_arguments(
        &mut self,
        written: &[fallc_ast::TemplateArgumentSyntax],
        substitution: &crate::template::Substitution,
    ) -> crate::template::Arguments {
        let mut out = Vec::with_capacity(written.len());
        for argument in written {
            let fallc_ast::TemplateArgumentSyntax::Type(fallc_ast::TypeSyntax::Named(name)) =
                argument
            else {
                if let fallc_ast::TemplateArgumentSyntax::Type(ty) = argument {
                    out.push(crate::template::TemplateArgument::Type(
                        self.resolve_written_type(ty),
                    ));
                }
                continue;
            };
            if !name.is_expansion {
                out.push(crate::template::TemplateArgument::Type(
                    self.resolve_written_type(&fallc_ast::TypeSyntax::Named(name.clone())),
                ));
                continue;
            }
            match substitution.get(&name.path) {
                Some(crate::template::TemplateArgument::Pack(elements)) => {
                    out.extend(elements.iter().cloned());
                }
                _ => {
                    self.diagnostics.push(
                        Diagnostic::error(format!(
                            "`{}...` expands something that is not a template parameter pack",
                            name.path
                        ))
                        .with_code(DiagnosticCode::sema(71))
                        .with_span(name.span),
                    );
                }
            }
        }
        crate::template::Arguments(out)
    }

    /// Chooses the specialisation a member matches, if the program declared one.
    ///
    /// Several may match -- `template <T type> type Box<*T>` and
    /// `template <> type Box<*int>` both accept `Box<*int>` -- and the most
    /// specific wins. Two that neither contains the other are ambiguous, which
    /// is reported rather than resolved by declaration order.
    fn choose_specialisation(
        &mut self,
        name: &str,
        arguments: &crate::template::Arguments,
        span: fallc_span::Span,
    ) -> Option<MatchedSpecialisation> {
        // The declarations in this file, and the ones a module published.
        let mut matching: Vec<MatchedSpecialisation> = Vec::new();
        for spec in self
            .type_specialisations
            .get(name)
            .cloned()
            .unwrap_or_default()
        {
            if let Some((pattern, substitution)) = self.match_specialisation(&spec, arguments) {
                matching.push(MatchedSpecialisation {
                    params: spec.params,
                    declaration: Some(spec.declaration),
                    resolved: None,
                    substitution,
                    pattern,
                });
            }
        }
        for spec in self
            .imported_specialisations
            .get(name)
            .cloned()
            .unwrap_or_default()
        {
            if let Some((pattern, substitution)) =
                self.match_resolved_specialisation(&spec, arguments)
            {
                matching.push(MatchedSpecialisation {
                    params: spec.params,
                    declaration: None,
                    resolved: Some(ResolvedMember {
                        fields: spec.fields,
                        is_union: spec.is_union,
                        mutable_fields: spec.mutable_fields,
                        alignments: spec.alignments,
                        constants: spec.constants,
                    }),
                    substitution,
                    pattern,
                });
            }
        }
        let count = matching.len();
        match count {
            0 => None,
            1 => matching.pop(),
            _ => {
                // A candidate is outranked only by one whose pattern is
                // strictly more specific; what is left is the answer.
                let most: Vec<usize> = (0..count)
                    .filter(|&i| {
                        !(0..count).any(|j| {
                            j != i
                                // `j` is more specific when the candidate's
                                // pattern is the more general of the two and
                                // the other direction does not hold.
                                && covers_patterns(
                                    &matching[i].pattern,
                                    &matching[i].params,
                                    &matching[j].pattern,
                                    &matching[j].params,
                                )
                                && !covers_patterns(
                                    &matching[j].pattern,
                                    &matching[j].params,
                                    &matching[i].pattern,
                                    &matching[i].params,
                                )
                        })
                    })
                    .collect();
                if most.len() == 1 {
                    let chosen = most[0];
                    return Some(matching.swap_remove(chosen));
                }
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "the specialisation of `{name}<{}>` is ambiguous: {count} match",
                        arguments.spelling()
                    ))
                    .with_code(DiagnosticCode::sema(176))
                    .with_span(span)
                    .with_note(
                        "two patterns that neither contains the other cannot be ordered, so \
                         the more specific one has to be written where it differs",
                    ),
                );
                None
            }
        }
    }

    /// The value of a family's constant member for one instantiation.
    ///
    /// A type family answers a question about an argument through a constant
    /// member -- `IsInteger<int>` holds `VALUE = true` -- and the
    /// specialisation machinery chooses *which* body answers: a full
    /// specialisation replaces the generic member, and a partial one replaces
    /// every member whose pattern it matches. This is the same choice a use of
    /// the family makes, so the answer agrees with the layout a member would
    /// have.
    ///
    /// Returns `None` when the family has no such member, or when the chosen
    /// body's initialiser cannot be folded. An imported family has no
    /// initialiser tree here at all, so it answers from the values that crossed
    /// with it.
    pub(super) fn family_member_constant(
        &mut self,
        family: &str,
        template: &fallc_ast::TypeDecl,
        arguments: &crate::template::Arguments,
        member: &str,
        span: fallc_span::Span,
    ) -> Option<i64> {
        if let Some(chosen) = self.choose_specialisation(family, arguments, span) {
            if let Some(declaration) = chosen.declaration {
                return self.declaration_member_constant(&declaration, member, &chosen.substitution);
            }
            if let Some(resolved) = chosen.resolved {
                return resolved
                    .constants
                    .iter()
                    .find(|(name, _)| name == member)
                    .map(|(_, value)| *value);
            }
            return None;
        }
        // The generic member answers. Its initialiser may mention a module
        // constant of the declaring module; folding finds the ones that module
        // already knows. A generic family whose body mentions its own
        // parameters has no single answer, and is not a trait question.
        if let Some(value) = self.declaration_member_constant(
            template,
            member,
            &crate::template::Substitution::default(),
        ) {
            return Some(value);
        }
        self.imported_family_constants
            .get(family)
            .and_then(|constants| constants.get(member).copied())
    }

    /// Folds one constant member of a declaration under the chosen body's
    /// substitution.
    fn declaration_member_constant(
        &mut self,
        declaration: &fallc_ast::TypeDecl,
        member: &str,
        substitution: &crate::template::Substitution,
    ) -> Option<i64> {
        let constant = declaration
            .constants
            .iter()
            .find(|constant| constant.name == member)?;
        let previous = self.substitution.replace(substitution.clone());
        let value = self.const_eval_int(constant.value);
        self.substitution = previous;
        value
    }

    /// Matches one imported specialisation's pattern against a member's arguments.
    ///
    /// The pattern is already resolved, so matching binds parameters and never
    /// reads the declaring module's tree.
    fn match_resolved_specialisation(
        &mut self,
        spec: &ImportedSpecialisation,
        arguments: &crate::template::Arguments,
    ) -> Option<(Vec<Type>, crate::template::Substitution)> {
        if spec.pattern.len() != arguments.0.len() {
            return None;
        }
        let mut bound: std::collections::HashMap<String, Type> = std::collections::HashMap::new();
        for (pattern, argument) in spec.pattern.iter().zip(&arguments.0) {
            let crate::template::TemplateArgument::Type(argument) = argument else {
                return None;
            };
            if !unify_special(pattern, argument, &spec.params, &mut bound) {
                return None;
            }
        }
        let mut ordered = Vec::with_capacity(spec.params.len());
        for param in &spec.params {
            ordered.push(crate::template::TemplateArgument::Type(
                bound.get(&param.name)?.clone(),
            ));
        }
        let substitution =
            crate::template::Substitution::new(&spec.params, &crate::template::Arguments(ordered))?;
        Some((spec.pattern.clone(), substitution))
    }

    /// Builds a member from a specialisation that came from another module.
    fn build_resolved_member(
        &mut self,
        base: &str,
        arguments: crate::template::Arguments,
        substitution: &crate::template::Substitution,
        member: &ResolvedMember,
        span: fallc_span::Span,
    ) -> Option<Type> {
        let _ = span;
        let member_name = format!("{}<{}>", base, arguments.spelling());
        // Methods of the family are looked up by the member's name, exactly as
        // for a member built from the declaration itself.
        self.type_template_arguments
            .insert(member_name.clone(), (base.to_owned(), arguments.clone()));
        if let Some(existing) = self.nominal_types.get(&member_name).cloned() {
            return Some(Type::Nominal(existing.clone()));
        }
        let fields: Vec<(String, Type)> = member
            .fields
            .iter()
            .map(|(name, ty)| {
                (
                    name.clone(),
                    substitute_type_params(ty, substitution),
                )
            })
            .collect();
        let nominal = crate::types::NominalType {
            // The family and the arguments that made this member, so that a
            // method written against the family's parameters can be resolved
            // from the type itself -- including where it arrived from another
            // module, which never built it.
            template_arguments: Some((base.to_owned(), arguments.clone())),
            name: member_name.clone(),
            fields,
            // The member's mutability and layout came with it: an `alignas` is
            // an expression of the declaring module's tree, so it cannot be
            // measured again here.
            mutable_fields: member.mutable_fields.clone(),
            field_alignments: member.alignments.clone(),
            is_union: member.is_union,
            is_incomplete: false,
            union_storage: member.is_union.then_some(0),
        };
        self.nominal_types.insert(member_name, nominal.clone());
        Some(Type::Nominal(nominal))
    }

    /// Matches one specialisation's pattern against a member's arguments.
    ///
    /// Returns the resolved pattern, which the specificity comparison needs,
    /// and the substitution the pattern bound.
    fn match_specialisation(
        &mut self,
        spec: &Specialisation,
        arguments: &crate::template::Arguments,
    ) -> Option<(Vec<Type>, crate::template::Substitution)> {
        if spec.pattern.len() != arguments.0.len() {
            return None;
        }
        let mut bound: std::collections::HashMap<String, Type> = std::collections::HashMap::new();
        let mut pattern = Vec::with_capacity(spec.pattern.len());
        for (written, argument) in spec.pattern.iter().zip(&arguments.0) {
            let crate::template::TemplateArgument::Type(argument) = argument else {
                return None;
            };
            let resolved = self.resolve_written_type(written);
            if !unify_special(&resolved, argument, &spec.params, &mut bound) {
                return None;
            }
            pattern.push(resolved);
        }
        let mut ordered = Vec::with_capacity(spec.params.len());
        for param in &spec.params {
            ordered.push(crate::template::TemplateArgument::Type(
                bound.get(&param.name)?.clone(),
            ));
        }
        let substitution =
            crate::template::Substitution::new(&spec.params, &crate::template::Arguments(ordered))?;
        Some((pattern, substitution))
    }

    /// Builds a member of a type family from arguments that are already read.
    ///
    /// Shared by the two ways a member is named: from source, and from a pack
    /// expansion inside a parameter type.
    pub(super) fn build_type_member(
        &mut self,
        template: &fallc_ast::TypeDecl,
        arguments: crate::template::Arguments,
        span: fallc_span::Span,
    ) -> Option<Type> {
        let substitution =
            crate::template::Substitution::new(&template.template_params, &arguments)?;
        self.build_member_with(template, arguments, substitution, span)
    }

    /// Builds a member from arguments and a substitution already worked out.
    ///
    /// A specialisation's substitution comes from matching its pattern rather
    /// than from the parameter order, so it cannot be derived from the
    /// arguments alone.
    pub(super) fn build_member_with(
        &mut self,
        template: &fallc_ast::TypeDecl,
        arguments: crate::template::Arguments,
        substitution: crate::template::Substitution,
        span: fallc_span::Span,
    ) -> Option<Type> {
        let _ = span;

        // The name the member is known by, which is what makes two mentions one
        // type. It carries the arguments so that `Array<int>` and `Array<u8>`
        // are different names.
        let member_name = format!("{}<{}>", template.name, arguments.spelling());
        // A method of the family is written against the *declaration's*
        // parameters -- `__init__(this &Box<T>, v T)` -- so which one belongs
        // to this member is a question only this knows. The answer is recorded,
        // and the method is instantiated on first use like any other template.
        //
        // Recorded under the name the *declaration* knows it by, which is the
        // name its methods are written against: in a file that imported the
        // umbrella the member is `std.Array<i32>`, and `Array` is what the
        // module that declared it calls the family.
        self.type_template_arguments
            .insert(member_name.clone(), (template.name.clone(), arguments.clone()));
        if let Some(existing) = self.nominal_types.get(&member_name).cloned() {
            // The member being built right now, reached through one of its own
            // pointer fields: there is nothing to compare yet, and nothing to
            // add.
            if existing.is_incomplete {
                return Some(Type::Nominal(existing));
            }
            // A member of the same name may already have been built from a
            // *different* declaration of the same family name -- one this file
            // declares itself. The two cannot be told apart by name, so the
            // fields have to agree; a body would otherwise read members the
            // other declaration does not have.
            let same_fields = existing.fields.len() == template.fields.len()
                && existing
                    .fields
                    .iter()
                    .zip(&template.fields)
                    .all(|((name, _), field)| name == &field.name);
            if !same_fields
                && let Some(environment) = self.type_environments.last().cloned()
            {
                let module = environment.module.clone();
                self.report_environment_conflict(&member_name, &module, span);
            }
            return Some(Type::Nominal(existing));
        }

        // A member may mention *itself* behind a pointer -- a tree's links name
        // the node type they are made of -- so resolving its fields re-enters
        // this function for the same member. The placeholder is what the second
        // entry finds: it has the right name and no fields yet, which is all a
        // pointer's pointee needs, and the real fields replace it below.
        self.nominal_types.insert(
            member_name.clone(),
            crate::types::NominalType {
                template_arguments: Some((template.name.clone(), arguments.clone())),
                name: member_name.clone(),
                fields: Vec::new(),
                mutable_fields: Vec::new(),
                field_alignments: Vec::new(),
                is_union: false,
                is_incomplete: true,
                union_storage: None,
            },
        );
        let mut fields = Vec::with_capacity(template.fields.len());
        let mut mutable_fields = Vec::new();
        let previous = self.substitution.replace(substitution.clone());
        for field in &template.fields {
            // The declaration's own parameters are in scope while its fields
            // are resolved, which is the whole point of the substitution.
            let ty = self.resolve_template_type(&field.ty, &substitution);
            fields.push((field.name.clone(), ty));
            if field.mutable {
                mutable_fields.push(field.name.clone());
            }
        }
        let mut field_alignments =
            self.collect_field_alignments(None, &fields, &template.fields);
        // A member of an imported family has no `alignas` to evaluate -- the
        // declaration arrived without one -- so the measured alignment is added
        // here, where the member is built.
        if let Some(imported) = self
            .imported_field_alignments
            .get(&template.name)
            .cloned()
        {
            for (field, align) in imported {
                if !field_alignments.iter().any(|(name, _)| *name == field) {
                    field_alignments.push((field, align));
                }
            }
        }
        self.substitution = previous;

        let nominal = crate::types::NominalType {
            template_arguments: Some((template.name.clone(), arguments.clone())),
            name: member_name.clone(),
            fields,
            mutable_fields,
            field_alignments,
            // A family is a struct: a `union` cannot have template parameters
            // yet, because there is no rule for what its members would mean.
            is_union: false,
            is_incomplete: false,
            union_storage: None,
        };
        self.nominal_types.insert(member_name.clone(), nominal.clone());
        // The nested types belong to the *member*, so they are resolved under
        // its arguments and filed under its name.
        let previous = self.substitution.replace(substitution.clone());
        self.register_nested_types(&member_name, template);
        self.substitution = previous;
        Some(Type::Nominal(nominal))
    }

    /// Registers the types declared inside `declaration` under `outer`.
    ///
    /// An alias's target is resolved here, at the place the owner is
    /// instantiated, so `Array<T>.Iterator = *T` means `*int` on `Array<int>`
    /// and `*u8` on `Array<u8>`. Nested *structs* are not accepted yet: only an
    /// alias is specified, and the parser says so.
    pub(super) fn register_nested_types(&mut self, outer: &str, declaration: &fallc_ast::TypeDecl) {
        for nested in &declaration.nested {
            let fallc_ast::TypeDeclKind::Alias(target) = &nested.kind else {
                continue;
            };
            let resolved = self.resolve_written_type(target);
            self.aliases
                .insert(format!("{outer}.{}", nested.name), resolved);
        }
    }

    /// Asks for the destructor of every instantiated member type.
    ///
    /// A templated type's methods exist only once something asks for them, and
    /// nothing ever asks for a destructor: code generation looks one up by
    /// symbol when a value's scope ends, which is far too late to instantiate
    /// it. So `Array<int>` had no `__drop__` at all, and every array leaked --
    /// quietly, because a destructor that is never called reports nothing.
    ///
    /// The request is made here, for every member that was built, so that a
    /// value of any member type has a destructor to call. A family that defines
    /// none answers nothing and costs one lookup.
    pub(super) fn instantiate_member_destructors(&mut self, resolutions: &Resolutions) {
        let members: Vec<(String, String, crate::template::Arguments)> = self
            .type_template_arguments
            .iter()
            .map(|(member, (family, arguments))| {
                (member.clone(), family.clone(), arguments.clone())
            })
            .collect();
        let written = fallc_ast::special::SpecialMethod::Drop.name();
        for (member, family, arguments) in members {
            // A mention of the family inside its own template -- `Array<T>` in
            // `array.ftm` -- resolves to a member whose argument is still the
            // parameter. No value has that type: it is a mention, not an
            // instantiation, and asking for its destructor produced a function
            // over an unresolved type that code generation could not emit.
            if arguments
                .0
                .iter()
                .any(|argument| self.template_argument_is_unresolved(argument))
            {
                continue;
            }
            let Some(template) = self
                .type_templates
                .get(&family)
                .or_else(|| self.type_templates.values().find(|t| t.name == family))
                .cloned()
            else {
                continue;
            };
            let Some(substitution) =
                crate::template::Substitution::new(&template.template_params, &arguments)
            else {
                continue;
            };
            // The destructor is bound as `__drop__Array`, after the *family*,
            // because that is solved by the receiver's name and not by what the
            // arguments happen to be.
            let qualified = format!("{written}{}", template.name);
            // A family declared in this file must not borrow the methods of
            // an imported family that happens to be bound under the same name:
            // `__drop__Pair` may be this file's `Pair` or `std.tuple`'s, and
            // instantiating the imported one for the local family produces a
            // method over a type that does not exist. When the family is local,
            // only the local definitions are considered.
            let local_family = self.local_families.contains(&family);
            let Some(definition) = self
                .definitions_named(resolutions, &qualified)
                .into_iter()
                .filter(|def| !local_family || def.imported_via.is_none())
                .find(|def| def.signature.as_ref().is_some_and(|signature| {
                    signature.param_types.len() == 1
                }))
            else {
                continue;
            };
            let Some(signature) = definition.signature.clone() else {
                continue;
            };
            let previous = self.substitution.replace(substitution.clone());
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
            let receiver = Type::unspecified(member.clone(), fallc_span::Span::SYNTHETIC);
            self.record_method_instantiation(&signature, &substitution, &symbol, &receiver);
        }
    }

    /// Asks for the copy constructor of a by-value aggregate parameter.
    ///
    /// A parameter of a family member type is copied with the member's own
    /// `__init__(this &T, other &const T)` at the callee's entry. Nothing in
    /// the source names that constructor, so the checker asks for it here --
    /// but only for the types a function actually takes by value, because
    /// instantiating one for every member that exists costs compile time and
    /// memory without being called.
    pub(super) fn request_parameter_copy_constructor(
        &mut self,
        resolutions: &Resolutions,
        ty: &Type,
    ) {
        if ty.is_indirection() {
            return;
        }
        let Some(member) = ty.nominal_name() else {
            return;
        };
        // Only a member of a family has an instantiated constructor to ask
        // for; a plain declared type's constructor is an ordinary function
        // that is already emitted.
        if !member.contains('<') {
            return;
        }
        let Some((family, arguments)) = self.type_template_arguments.get(member).cloned() else {
            return;
        };
        self.request_member_copy_constructor(resolutions, member, &family, &arguments);
    }

    /// Requests one member's copy constructor, if the member owns something.
    fn request_member_copy_constructor(
        &mut self,
        resolutions: &Resolutions,
        member: &str,
        family: &str,
        arguments: &crate::template::Arguments,
    ) {
        if arguments
            .0
            .iter()
            .any(|argument| self.template_argument_is_unresolved(argument))
        {
            return;
        }
        let Some(template) = self
            .type_templates
            .get(family)
            .or_else(|| self.type_templates.values().find(|t| t.name == family))
            .cloned()
        else {
            return;
        };
        let Some(substitution) =
            crate::template::Substitution::new(&template.template_params, arguments)
        else {
            return;
        };
        // A type with no destructor owns nothing, so a byte-wise copy of it is
        // the member-wise copy the language means; only types that destroy
        // something need their copy constructor to run.
        let drop_name = format!(
            "{}{}",
            fallc_ast::special::SpecialMethod::Drop.name(),
            template.name
        );
        let has_destructor = self
            .definitions_named(resolutions, &drop_name)
            .into_iter()
            .any(|def| {
                def.signature
                    .as_ref()
                    .is_some_and(|signature| signature.param_types.len() == 1)
            });
        if !has_destructor {
            return;
        }
        // The copy constructor is the two-parameter `__init__` whose second
        // parameter is a reference -- to the same member, after the receiver.
        // A value constructor with two parameters (`count`, `value`) has a
        // non-reference there and is not it.
        let qualified = format!(
            "{}{}",
            fallc_ast::special::SpecialMethod::Init.name(),
            template.name
        );
        let local_family = self.local_families.contains(family);
        let Some(definition) = self
            .definitions_named(resolutions, &qualified)
            .into_iter()
            .filter(|def| !local_family || def.imported_via.is_none())
            .find(|def| {
                def.signature.as_ref().is_some_and(|signature| {
                    signature.param_types.len() == 2
                        && matches!(
                            signature.param_types[1],
                            fallc_ast::TypeSyntax::Reference(_)
                                | fallc_ast::TypeSyntax::ConstReference(_)
                        )
                })
            })
        else {
            return;
        };
        let Some(signature) = definition.signature.clone() else {
            return;
        };
        let previous = self.substitution.replace(substitution.clone());
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
        let receiver = Type::unspecified(member.to_owned(), fallc_span::Span::SYNTHETIC);
        self.record_method_instantiation(&signature, &substitution, &symbol, &receiver);
    }

    /// Checks the body of every templated method some member asked for.    /// Checks the body of every templated method some member asked for.
    ///
    /// Each is checked under the member's arguments, exactly as a free
    /// template's instance is, and carries its own type table and records for
    /// the same reason: the spans are the template's, shared by every member.
    pub(super) fn check_pending_method_instantiations(
        &mut self,
        resolutions: &Resolutions,
        types: &mut TypeTable,
    ) {
        while let Some(PendingMethodInstantiation {
            body,
            symbol,
            substitution,
            arguments,
            params,
            pack_elements,
        }) = self.pending_method_instantiations.pop()
        {
            let TemplateBody {
                tree,
                resolutions: module_resolutions,
                constants: module_constants,
                constant_types: module_constant_types,
                function,
                attrs,
                environment,
                bindings,
            } = body;
            if self.instantiated_returns.contains_key(&symbol) {
                continue;
            }
            // The body's own module's declarations are what its names mean,
            // for as long as it is being checked.
            if let Some(environment) = environment.clone() {
                self.type_environments.push(environment);
            }
            // The tree and the function travel together, so nothing is looked
            // up by node id -- which is an index into one tree and names
            // different functions in different translation units.
            // The *module's* resolutions, not the importer's: the body names
            // what its own module imported, and the importing file may never
            // have heard of those.
            let _ = resolutions;
            self.instantiation_constants = module_constants;
            // The module's constant *types* come with it, for the same reason
            // the values do: a constant's type decides the width of the store
            // that initialises a binding from it, and the importing file has no
            // record of `INITIAL_CAPACITY`'s declaration.
            self.constant_types_by_name
                .extend(module_constant_types.clone());
            // The body's ids belong to its own tree: its own constant index is
            // what folding reads while it is checked, which is what makes a
            // value parameter's `@enable_if` a constant for a family declared in
            // another module.
            let previous_index = self.adopt_const_index(&tree);
            self.check_method_instantiation(
                &tree,
                &module_resolutions,
                types,
                &function,
                &substitution,
                &arguments,
                &params,
                &symbol,
                &attrs,
                pack_elements.as_deref(),
                Some(&bindings),
            );
            self.restore_const_index(previous_index);
            if environment.is_some() {
                self.type_environments.pop();
            }
        }
        let _ = types;
    }

    /// Checks one templated method's body for one member.
    ///
    /// The parameters and return type are resolved under the substitution --
    /// `&Box<T>` becomes `&Box<int>` and `v T` becomes `v int` -- and the body
    /// is checked against those, with its own records.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn check_method_instantiation(
        &mut self,
        tree: &Ast,
        resolutions: &Resolutions,
        _types: &mut TypeTable,
        function: &fallc_ast::FunctionItem,
        substitution: &crate::template::Substitution,
        arguments: &crate::template::Arguments,
        params: &[fallc_ast::TemplateParam],
        symbol: &str,
        attrs: &[fallc_ast::Attr],
        pack_elements: Option<&[Type]>,
        module_bindings: Option<&crate::types::BindingTypes>,
    ) {
        // The member's arguments mean the same thing for its condition, its
        // signature and its body, so they are installed once, up front, and
        // taken back at the end.
        //
        // A method may declare template parameters of its own -- the
        // comparator of `Array::sort_by`, the index of `get<0>` -- and the
        // family's substitution does not know them. They are the arguments the
        // call deduced, so binding them here is what makes a body that calls
        // `cmp(...)` resolve: without it `Cmp` stayed a name.
        let mut effective = substitution.clone();
        for (param, argument) in params.iter().zip(arguments.0.iter()) {
            effective.bind(param.name.clone(), argument.clone());
        }
        let substitution = &effective;
        let previous = self.substitution.replace(substitution.clone());
        let previous_expected = self.expected.take();
        let previous_values = std::mem::take(&mut self.template_value_types);
        // A value parameter of the *family* is one the method may name without
        // the signature repeating it: `Device<Mode>`'s `write` mentions `Mode`,
        // and the declaration is the only place that says what type it has.
        // Recorded before the method's own, so that the nearer declaration wins
        // if the two share a name.
        for param in substitution.params() {
            if let fallc_ast::TemplateParamKind::Value(ty) = &param.kind {
                let ty = self.resolve_template_type(ty, substitution);
                self.template_value_types.insert(param.name.clone(), ty);
            }
        }
        for param in params {
            if let fallc_ast::TemplateParamKind::Value(ty) = &param.kind {
                let ty = self.resolve_template_type(ty, substitution);
                self.template_value_types.insert(param.name.clone(), ty);
            }
        }
        // `@enable_if` decides whether this member of the family exists. The
        // condition is checked with the member's arguments substituted, exactly
        // as the body will be.
        if attrs.iter().any(|attr| attr.name == "enable_if") {
            let allowed = self.enable_if(tree, resolutions, _types, attrs, function.name_span);
            if allowed == Some(false) {
                self.template_value_types = previous_values;
                self.expected = previous_expected;
                self.substitution = previous;
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "`{}` is disabled for this member by `@enable_if`",
                        function.name
                    ))
                    .with_code(DiagnosticCode::sema(175))
                    .with_span(function.name_span),
                );
                return;
            }
        }
        let returns = self.resolve_template_type(&function.return_type, substitution);
        self.expected = Some(returns.clone());
        // A method that declares a parameter pack is expanded for this call,
        // exactly as a free template's is: one parameter per element, the
        // elements' names standing for the pack in the body.
        let mut expanded: Option<(packs::Expanded, Resolutions)> = None;
        if function.has_pack() {
            let pack = function
                .params
                .iter()
                .position(|parameter| parameter.is_pack)
                .expect("has_pack says there is one");
            let mut param_types: Vec<Type> = function.params[..pack]
                .iter()
                .map(|parameter| self.resolve_template_type(&parameter.ty, substitution))
                .collect();
            let Some(elements) = pack_elements else {
                self.template_value_types = previous_values;
                self.expected = previous_expected;
                self.substitution = previous;
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "internal error: `{}` was instantiated with a pack and no elements",
                        function.name
                    ))
                    .with_code(DiagnosticCode::sema(71))
                    .with_span(function.name_span),
                );
                return;
            };
            param_types.extend(elements.iter().cloned());
            let expansion = match packs::expand(tree, function, pack, &param_types) {
                Ok(expansion) => expansion,
                Err(problem) => {
                    self.template_value_types = previous_values;
                    self.expected = previous_expected;
                    self.substitution = previous;
                    self.diagnostics.push(
                        Diagnostic::error(problem.message())
                            .with_code(DiagnosticCode::sema(80))
                            .with_span(function.name_span),
                    );
                    return;
                }
            };
            let mut extended = resolutions.clone();
            for (name, name_span) in packs::element_bindings(&expansion, pack) {
                let id = crate::resolve::DefId::from_raw(
                    u32::try_from(extended.defs.len()).expect("definitions fit in u32"),
                );
                extended.defs.push(crate::resolve::Def {
                    id,
                    kind: crate::resolve::DefKind::Param,
                    name,
                    node: None,
                    name_span,
                    span: name_span,
                    scope: crate::resolve::ScopeId::from_raw(0),
                    signature: None,
                    emitted_as: None,
                    imported_as: None,
                    imported_via: None,
                    is_builtin: false,
                });
            }
            // A copied name has a span of its own; what it means is what the
            // name it copied meant.
            for (new_span, old_span) in &expansion.name_spans {
                if let Some(id) = resolutions.resolve(*old_span) {
                    extended.bind(*new_span, id);
                }
            }
            expanded = Some((expansion, extended));
        }
        let (function, tree, resolutions) = match &expanded {
            Some((expansion, extended)) => (&expansion.function, &expansion.tree, extended),
            None => (function, tree, resolutions),
        };
        // The outer bindings are set aside *before* this instantiation's own,
        // or taking them afterwards would take the parameters with them -- and
        // `this` would have no type at all.
        let previous_bindings = std::mem::take(&mut self.binding_types);
        let resolved_params: Vec<Type> = match &expanded {
            Some((expansion, _)) => expansion.param_types.clone(),
            None => function
                .params
                .iter()
                .map(|parameter| self.resolve_template_type(&parameter.ty, substitution))
                .collect(),
        };
        for (parameter, ty) in function.params.iter().zip(&resolved_params) {
            self.binding_types.insert(parameter.name_span, ty.clone());
        }
        // The module's variables, for the same reason a free template's body
        // gets them: a method written beside a global names it by its bare
        // name, and the type lives in the declaring module's binding table.
        if let Some(bindings) = module_bindings {
            for (span, ty) in bindings.iter() {
                self.binding_types.insert(span, ty.clone());
            }
        }
        self.return_types
            .push((function.name.clone(), returns.clone()));

        let mut instance_types = TypeTable::new();
        let saved_records = self.take_records();
        let saved_expectations = std::mem::take(&mut self.expected_types);
        self.throws_allowed.push(function.throws.is_some());
        // The expanded body is a different tree; its expression ids need their
        // own constant index, exactly as a free template's expansion does.
        let expanded_index = if expanded.is_some() {
            self.adopt_const_index(tree)
        } else {
            None
        };
        if let Some(body) = function.body {
            self.check_block(tree, resolutions, &mut instance_types, body);
        }
        self.throws_allowed.pop();
        self.restore_const_index(expanded_index);
        // An interface used inside the body is settled against this member's
        // types, and recorded with the member's records.
        self.resolve_interface_conversions(resolutions, &instance_types);
        self.expected_types = saved_expectations;
        let records = self.restore_records(saved_records);
        let instance_bindings = std::mem::replace(&mut self.binding_types, previous_bindings);

        self.return_types.pop();
        self.template_value_types = previous_values;
        self.expected = previous_expected;
        self.substitution = previous;

        self.instantiated_returns
            .insert(symbol.to_owned(), returns.clone());
        self.instantiation_list.push(Instantiation {
            key: symbol.to_owned(),
            symbol: symbol.to_owned(),
            tree: tree.clone(),
            function: function.clone(),
            arguments: arguments.clone(),
            params: params.to_vec(),
            types: instance_types,
            bindings: instance_bindings,
            records,
            resolved_params,
            resolved_returns: returns.clone(),
        });
    }

    /// Swaps out every per-expression record, returning the previous set.
    ///
    /// Used around an instantiation's check: the records it makes are its own,
    /// because they are keyed by spans the template shares with every other
    /// instantiation. The caller gets the instantiation's set back from
    /// [`Self::restore_records`].
    pub(super) fn take_records(&mut self) -> InstantiationRecords {
        InstantiationRecords {
            deref_reads: std::mem::take(&mut self.deref_reads),
            by_address_arguments: std::mem::take(&mut self.by_address_arguments),
            call_symbols: std::mem::take(&mut self.call_symbols),
            closure_calls: std::mem::take(&mut self.closure_calls),
            interface_conversions: std::mem::take(&mut self.interface_conversions),
            interface_calls: std::mem::take(&mut self.interface_calls),
            lambda_calls: std::mem::take(&mut self.lambda_calls),
            lambda_values: std::mem::take(&mut self.lambda_values),
            method_calls: std::mem::take(&mut self.method_calls),
            call_methods: std::mem::take(&mut self.call_methods),
            throwing_calls: std::mem::take(&mut self.throwing_calls),
            propagating_calls: std::mem::take(&mut self.propagating_calls),
            operator_methods: std::mem::take(&mut self.operator_methods),
            constructor_calls: std::mem::take(&mut self.constructor_calls),
            constructor_types: std::mem::take(&mut self.constructor_types),
            init_calls: std::mem::take(&mut self.init_calls),
            index_methods: std::mem::take(&mut self.index_methods),
            bool_methods: std::mem::take(&mut self.bool_methods),
            unary_methods: std::mem::take(&mut self.unary_methods),
            in_place_methods: std::mem::take(&mut self.in_place_methods),
            in_place_by_address: std::mem::take(&mut self.in_place_by_address),
            assign_methods: std::mem::take(&mut self.assign_methods),
            literal_constructions: std::mem::take(&mut self.literal_constructions),
            literal_construction_types: std::mem::take(&mut self.literal_construction_types),
            adopted_types: std::mem::take(&mut self.adopted_types),
            constant_values: std::mem::take(&mut self.constant_values),
            compile_time_ifs: std::mem::take(&mut self.compile_time_ifs),
            text_calls: std::mem::take(&mut self.text_calls),
            in_place_operations: std::mem::take(&mut self.in_place_operations),
        }
    }

    /// Puts back a set of records and returns the ones made since.
    pub(super) fn restore_records(&mut self, previous: InstantiationRecords) -> InstantiationRecords {
        let made = self.take_records();
        self.deref_reads = previous.deref_reads;
        self.by_address_arguments = previous.by_address_arguments;
        self.call_symbols = previous.call_symbols;
        self.closure_calls = previous.closure_calls;
        self.interface_conversions = previous.interface_conversions;
        self.interface_calls = previous.interface_calls;
        self.lambda_calls = previous.lambda_calls;
        self.lambda_values = previous.lambda_values;
        self.method_calls = previous.method_calls;
        self.call_methods = previous.call_methods;
        self.throwing_calls = previous.throwing_calls;
        self.propagating_calls = previous.propagating_calls;
        self.operator_methods = previous.operator_methods;
        self.constructor_calls = previous.constructor_calls;
        self.constructor_types = previous.constructor_types;
        self.init_calls = previous.init_calls;
        self.index_methods = previous.index_methods;
        self.bool_methods = previous.bool_methods;
        self.unary_methods = previous.unary_methods;
        self.in_place_methods = previous.in_place_methods;
        self.in_place_by_address = previous.in_place_by_address;
        self.assign_methods = previous.assign_methods;
        self.literal_constructions = previous.literal_constructions;
        self.literal_construction_types = previous.literal_construction_types;
        self.adopted_types = previous.adopted_types;
        self.constant_values = previous.constant_values;
        self.compile_time_ifs = previous.compile_time_ifs;
        self.text_calls = previous.text_calls;
        self.in_place_operations = previous.in_place_operations;
        made
    }

}


/// Binds a specialisation's pattern to a member's argument.
///
/// A pattern that names one of the specialisation's own parameters binds it;
/// everything else is structure that must match exactly -- `*T` matches a
/// pointer and binds what it points at, and does not match an `int`.
fn unify_special(
    pattern: &Type,
    argument: &Type,
    params: &[fallc_ast::TemplateParam],
    bound: &mut std::collections::HashMap<String, Type>,
) -> bool {
    let is_param = |text: &str| params.iter().any(|param| param.name == text);
    match pattern.unqualified() {
        Type::Unspecified(spec) if is_param(&spec.text) => match bound.get(&spec.text) {
            Some(existing) => existing == argument,
            None => {
                bound.insert(spec.text.clone(), argument.clone());
                true
            }
        },
        Type::Pointer {
            pointee,
            pointee_is_const,
            pointer_is_const,
        } => match argument.unqualified() {
            Type::Pointer {
                pointee: inner,
                pointee_is_const: inner_const,
                pointer_is_const: outer_const,
            } if inner_const == pointee_is_const && outer_const == pointer_is_const => {
                unify_special(pointee, inner, params, bound)
            }
            _ => false,
        },
        Type::Reference {
            referent,
            referent_is_const,
            is_rvalue,
        } => match argument.unqualified() {
            Type::Reference {
                referent: inner,
                referent_is_const: inner_const,
                is_rvalue: inner_rvalue,
            } if inner_const == referent_is_const && inner_rvalue == is_rvalue => {
                unify_special(referent, inner, params, bound)
            }
            _ => false,
        },
        Type::Array { element, len } => match argument.unqualified() {
            Type::Array {
                element: inner,
                len: inner_len,
            } if inner_len == len => unify_special(element, inner, params, bound),
            _ => false,
        },
        Type::Const(inner) => match argument.unqualified() {
            Type::Const(other) => unify_special(inner, other, params, bound),
            _ => false,
        },
        Type::Volatile(inner) => match argument.unqualified() {
            Type::Volatile(other) => unify_special(inner, other, params, bound),
            _ => false,
        },
        // `[]T` in a specialisation pattern names an array of `T` with any
        // extent: the length is not written, so the pattern leaves it open and
        // matches a sized array as well as an unsized one. That is what lets a
        // family answer `IsArray` for every array type without a wildcard
        // length in the pattern grammar.
        Type::UnsizedArray(inner) => match argument.unqualified() {
            Type::UnsizedArray(other) => unify_special(inner, other, params, bound),
            Type::Array { element: other, .. } => unify_special(inner, other, params, bound),
            _ => false,
        },
        _ => pattern == argument,
    }
}

/// Whether `general`'s pattern accepts every argument `specific`'s does.
///
/// A parameter of the general pattern stands for anything; a parameter of the
/// specific one is a variable, so it is *less* specific than any structure.
fn covers(
    general: &Type,
    general_params: &[fallc_ast::TemplateParam],
    specific: &Type,
    specific_params: &[fallc_ast::TemplateParam],
) -> bool {
    let general_is_param = |text: &str| general_params.iter().any(|param| param.name == text);
    let specific_is_param = |text: &str| specific_params.iter().any(|param| param.name == text);
    match general.unqualified() {
        Type::Unspecified(spec) if general_is_param(&spec.text) => true,
        Type::Unspecified(_) => match specific.unqualified() {
            Type::Unspecified(other) if specific_is_param(&other.text) => false,
            other => general.unqualified() == other,
        },
        Type::Pointer {
            pointee,
            pointee_is_const,
            pointer_is_const,
        } => match specific.unqualified() {
            Type::Pointer {
                pointee: inner,
                pointee_is_const: inner_const,
                pointer_is_const: outer_const,
            } => {
                inner_const == pointee_is_const
                    && outer_const == pointer_is_const
                    && covers(pointee, general_params, inner, specific_params)
            }
            _ => false,
        },
        Type::Reference {
            referent,
            referent_is_const,
            is_rvalue,
        } => match specific.unqualified() {
            Type::Reference {
                referent: inner,
                referent_is_const: inner_const,
                is_rvalue: inner_rvalue,
            } => {
                inner_const == referent_is_const
                    && inner_rvalue == is_rvalue
                    && covers(referent, general_params, inner, specific_params)
            }
            _ => false,
        },
        Type::Array { element, len } => match specific.unqualified() {
            Type::Array {
                element: inner,
                len: inner_len,
            } => inner_len == len && covers(element, general_params, inner, specific_params),
            _ => false,
        },
        // An unsized pattern `[]T` covers any array of `T`: it says nothing
        // about the length, so it is less specific than a pattern that names
        // one.
        Type::UnsizedArray(element) => match specific.unqualified() {
            Type::Array { element: inner, .. } | Type::UnsizedArray(inner) => {
                covers(element, general_params, inner, specific_params)
            }
            _ => false,
        },
        Type::Const(inner) => match specific.unqualified() {
            Type::Const(other) => covers(inner, general_params, other, specific_params),
            _ => false,
        },
        Type::Volatile(inner) => match specific.unqualified() {
            Type::Volatile(other) => covers(inner, general_params, other, specific_params),
            _ => false,
        },
        other => other == specific.unqualified(),
    }
}


/// Whether every position of `general`'s pattern accepts `specific`'s.
fn covers_patterns(
    general: &[Type],
    general_params: &[fallc_ast::TemplateParam],
    specific: &[Type],
    specific_params: &[fallc_ast::TemplateParam],
) -> bool {
    general.len() == specific.len()
        && general
            .iter()
            .zip(specific)
            .all(|(left, right)| covers(left, general_params, right, specific_params))
}


/// One specialised member, as it came from another module.
///
/// Everything the member's *type* consists of: its fields, whether they share
/// an address, which of them may still be written, and the offset each field
/// starts at.
struct ResolvedMember {
    /// The fields, in declaration order.
    fields: Vec<(String, Type)>,
    /// Whether the fields share one address.
    is_union: bool,
    /// Which fields a `const` member function may still write.
    mutable_fields: Vec<String>,
    /// The alignment each field asked for, in bytes.
    alignments: Vec<(String, u64)>,
    /// The constants the specialised member declares, folded where it was
    /// declared. A trait family answers a query through one of these.
    constants: Vec<(String, i64)>,
}

/// A specialisation that accepted a member's arguments, ready to build it.
struct MatchedSpecialisation {
    /// The parameters the pattern bound.
    params: Vec<fallc_ast::TemplateParam>,
    /// The declaration, for a specialisation written in this file.
    declaration: Option<fallc_ast::TypeDecl>,
    /// The fields, union flag, mutability and layout, for one that came from
    /// another module.
    resolved: Option<ResolvedMember>,
    /// What the pattern bound.
    substitution: crate::template::Substitution,
    /// The resolved pattern, for the specificity comparison.
    pattern: Vec<Type>,
}

/// Replaces an imported specialisation's parameters with what the pattern bound.
fn substitute_type_params(
    ty: &Type,
    substitution: &crate::template::Substitution,
) -> Type {
    match ty.unqualified() {
        Type::Unspecified(spec) => match substitution.get(&spec.text) {
            Some(crate::template::TemplateArgument::Type(bound)) => bound.clone(),
            _ => ty.clone(),
        },
        Type::Pointer {
            pointee,
            pointee_is_const,
            pointer_is_const,
        } => Type::Pointer {
            pointee: Box::new(substitute_type_params(pointee, substitution)),
            pointee_is_const: *pointee_is_const,
            pointer_is_const: *pointer_is_const,
        },
        Type::Reference {
            referent,
            referent_is_const,
            is_rvalue,
        } => Type::Reference {
            referent: Box::new(substitute_type_params(referent, substitution)),
            referent_is_const: *referent_is_const,
            is_rvalue: *is_rvalue,
        },
        Type::Array { element, len } => Type::Array {
            element: Box::new(substitute_type_params(element, substitution)),
            len: *len,
        },
        Type::Const(inner) => Type::Const(Box::new(substitute_type_params(inner, substitution))),
        Type::Volatile(inner) => {
            Type::Volatile(Box::new(substitute_type_params(inner, substitution)))
        }
        Type::UnsizedArray(inner) => {
            Type::UnsizedArray(Box::new(substitute_type_params(inner, substitution)))
        }
        Type::Function {
            parameters,
            returns,
            abi,
            variadic,
            throws,
        } => Type::Function {
            parameters: parameters
                .iter()
                .map(|parameter| substitute_type_params(parameter, substitution))
                .collect(),
            returns: Box::new(substitute_type_params(returns, substitution)),
            abi: abi.clone(),
            variadic: *variadic,
            throws: *throws,
        },
        _ => ty.clone(),
    }
}

/// The name of the type pack a pack parameter's element type names.
///
/// `values ...Ts` has `Ts` as its element type, and `Ts` is a *pack* template
/// parameter: the arguments decide its elements. Any other written type --
/// `...int`, or a single `T` -- fixes one type for every element.
pub(super) fn pack_element_parameter<'a>(
    written: &'a fallc_ast::TypeSyntax,
    template_params: &[fallc_ast::TemplateParam],
) -> Option<&'a str> {
    // The element type may be wrapped: `values ...&Ts` is a pack of
    // references, and `&Ts` is what the written parameter says. The name to
    // bind is the one at the middle.
    fn bare(ty: &fallc_ast::TypeSyntax) -> Option<&fallc_ast::NamedType> {
        use fallc_ast::TypeSyntax;
        match ty {
            TypeSyntax::Named(named) => Some(named),
            TypeSyntax::UnsizedArray(inner)
            | TypeSyntax::Pointer(inner)
            | TypeSyntax::PointerToConst(inner)
            | TypeSyntax::ConstPointer(inner)
            | TypeSyntax::Reference(inner)
            | TypeSyntax::ConstReference(inner)
            | TypeSyntax::RvalueReference(inner)
            | TypeSyntax::ConstRvalueReference(inner)
            | TypeSyntax::Volatile(inner)
            | TypeSyntax::Const(inner) => bare(inner),
            _ => None,
        }
    }
    let named = bare(written)?;
    if !named.arguments.is_empty() {
        return None;
    }
    template_params
        .iter()
        .any(|param| param.name == named.path && param.is_pack)
        .then_some(named.path.as_str())
}

/// The element types of a pack whose written type is a type *pack*.
///
/// `values ...Ts` names `Ts`, and the arguments bound `Ts` to their own types.
/// Anything else -- `values ...int`, or a single `T` -- has one type for every
/// element, which the ordinary resolution supplies.
fn bound_pack_types(
    written: &fallc_ast::TypeSyntax,
    substitution: &crate::template::Substitution,
) -> Option<Vec<Type>> {
    let fallc_ast::TypeSyntax::Named(named) = written else {
        return None;
    };
    if !named.arguments.is_empty() {
        return None;
    }
    let crate::template::TemplateArgument::Pack(elements) = substitution.get(&named.path)? else {
        return None;
    };
    let mut types = Vec::with_capacity(elements.len());
    for element in elements {
        let crate::template::TemplateArgument::Type(ty) = element else {
            return None;
        };
        types.push(ty.clone());
    }
    Some(types)
}

/// The name a written type shows, for a diagnostic about its elements.
fn written_name_of(written: &fallc_ast::TypeSyntax) -> String {
    match written {
        fallc_ast::TypeSyntax::Named(named) => named.path.clone(),
        other => format!("{other:?}"),
    }
}
