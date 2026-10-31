//! Method calls, and the type rules they share with every other expression.
//!
//! A method call is a call whose receiver is the first parameter; the overload
//! rules, the writability rules and the `const` rules meet here. Compatibility
//! and castability close the file, because every check above ends in one of
//! them.

use super::*;

impl TypeChecker {
    /// Chooses which overload of a method a call refers to.
    ///
    /// # The rule
    ///
    /// The same rule the free-function overloads use, with the receiver counted
    /// as the first argument: a candidate applies when the argument count fits
    /// and every argument is compatible with its parameter. The receiver is
    /// compared against the `this` parameter, which may be a value or a pointer
    /// to one.
    ///
    /// # Why the receiver is not used to *find* the candidates
    ///
    /// The object's type does not carry the receiver's qualifiers: a `String`
    /// value can be passed to `this *String` or `this *const String`, and both
    /// are defined. Narrowing by the object first would pick one without
    /// looking at the arguments, which is how an overloaded method came to be
    /// chosen by declaration order.
    #[allow(clippy::too_many_arguments)]
    /// The pack-taking method a call reaches, if that is what it reaches.
    ///
    /// A pack method takes the receiver and then any number of arguments, so
    /// its parameter count is a lower bound and the ordinary arity comparison
    /// rejects it. Only when no fixed-arity candidate of the same name takes the
    /// call is a pack one considered: `write` and `print` may live side by side
    /// on one type without the pack swallowing the fixed forms.
    fn choose_pack_method(
        &mut self,
        candidates: &[FunctionSignature],
        receiver_ty: &Type,
        given: usize,
    ) -> Option<(FunctionSignature, Option<crate::template::Substitution>)> {
        let fixed_takes_the_call = candidates.iter().any(|candidate| {
            candidate.pack.is_none()
                && (candidate.variadic || candidate.param_types.len() == given + 1)
        });
        if fixed_takes_the_call {
            return None;
        }
        let member_substitution = self.receiver_substitution(receiver_ty);
        let candidate = candidates.iter().find(|candidate| {
            let Some(pack) = candidate.pack else {
                return false;
            };
            if given + 1 < pack {
                return false;
            }
            let Some(receiver_syntax) = candidate.param_types.first() else {
                return false;
            };
            if self.mentions_template_parameter_syntax(receiver_syntax)
                && member_substitution.is_none()
            {
                return false;
            }
            let receiver = self.resolve_type_syntax(receiver_syntax, true);
            self.receiver_fits(receiver_ty, &receiver)
        })?;
        Some((candidate.clone(), member_substitution))
    }

    pub(super) fn choose_method_overload(
        &mut self,
        candidates: &[FunctionSignature],
        field: &FieldExpr,
        node: &CallExpr,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
    ) -> Option<FunctionSignature> {
        // The receiver's type, inferred once. It cannot be inferred through
        // `infer_expr` more than once per candidate: doing so would record the
        // same expression's decisions repeatedly, and for `f().g()` would emit
        // the call to `f` as many times as there are candidates.
        let previous = self.expected.take();
        let receiver_ty = self.infer_expr(ast, resolutions, types, field.object);
        self.expected = previous;
        let receiver_ty = receiver_ty?;

        // The member's arguments, from the receiver.
        //
        // `numbers.push(10)` has a receiver of type `Array<int>`, and that is
        // what says `T` is `int` -- before any candidate is looked at. The
        // candidates are written `push(this &Array<T>, value T)`, so without
        // this their parameters cannot be resolved at all, and every method of
        // a templated type was reported as naming an unknown type.
        // The argument types, also once each.
        //
        // Inferred with no expectation, because the candidate is what decides
        // what they should be. That has a cost, which is why the records this
        // makes are not left behind: `*p` inferred with no expectation is read
        // as a *value*, and deciding an overload must not be what settles that
        // for good. The chosen signature is checked again below with the
        // parameter's type as the expectation, and that is the inference the
        // later passes see.
        // A single candidate needs no comparison, so its arguments are left to
        // the check against the parameters. Probing them here would infer
        // `.{1, 2}` with no expectation and report it as untyped, when the
        // parameter it fills is right there.
        let mut argument_types = Vec::with_capacity(node.args.len());
        let mut named_types: Vec<(String, Option<Type>, fallc_ast::ExprId)> = Vec::new();
        // A special method is compared even when it is the only candidate: it
        // may be a family's constructor, and its parameters are written against
        // the family's arguments, so fitting is a real question -- `a.__init__(v)`
        // on an imported member found no overload without it.
        if candidates.len() > 1 || fallc_ast::special::is_special(&field.field) {
            for &arg in &node.args {
                let previous = self.expected.take();
                let saved_reads = self.deref_reads.clone();
                let inferred = self.infer_expr(ast, resolutions, types, arg);
                self.expected = previous;
                // Anything this probe decided about reading through a reference
                // is provisional.
                self.deref_reads = saved_reads;
                argument_types.push(inferred);
            }
            for argument in &node.named {
                let previous = self.expected.take();
                let saved_reads = self.deref_reads.clone();
                let inferred = self.infer_expr(ast, resolutions, types, argument.value);
                self.expected = previous;
                self.deref_reads = saved_reads;
                named_types.push((argument.name.clone(), inferred, argument.value));
            }
        }

        // The member's arguments, derived from the receiver, are installed
        // while the candidates are compared: a candidate is written
        // `push(this &Array<T>, value T)`, and without the substitution `T` is
        // no type at all -- so every member of a family matched nothing.
        //
        // It is deliberately *not* put back on the way out. The caller mangles
        // the chosen symbol from these same parameters, so the substitution has
        // to still be in place when this returns; restoring it here made every
        // member of a family resolve its receiver against the bare `Array<T>`
        // and match nothing.
        // A method of a templated type is written against the *family's*
        // parameters -- `get(this &Box<T>)` -- so `T` means what the member's
        // arguments say. Installing it here is what makes `Box<int>`'s method
        // resolve its receiver to `&Box<i32>` instead of leaving `T` in it,
        // which no value could ever fit.
        let member_substitution = self.receiver_substitution(&receiver_ty);
        match &member_substitution {
            Some(substitution) => self.substitution = Some(substitution.clone()),
            None => self.substitution = None,
        }
        let mut applicable = Vec::new();
        for (index, candidate) in candidates.iter().enumerate() {
            if self.method_candidate_fits(
                candidate,
                &receiver_ty,
                &argument_types,
                &node.args,
                &named_types,
                ast,
                resolutions,
            ) {
                applicable.push(index);
            }
        }
        match applicable.len() {
            // Exactly one, which is the ordinary case.
            1 => Some(candidates[applicable[0]].clone()),
            0 => None,
            // Several fit. A method may be overloaded on its receiver alone --
            // `front(this &const Array<T>)` reads and `front(this &Array<T>)`
            // writes -- and then the receiver is what decides. Without this the
            // first declaration won for both, so `a.front() = v` reached the
            // read-only one and was refused as writing through a `const`
            // reference.
            //
            // The receiver's writability is the test, and it is the same one
            // assignment uses: a mutable receiver takes the `&T` overload, a
            // `const` one can only take `&const T`.
            _ => {
                // A dying argument prefers the parameter that takes it by
                // rvalue reference, exactly as a constructor's does: the value
                // is about to disappear, so it may be moved from rather than
                // copied. The declaration order decides among equals, which is
                // why this is a filter and not a sort.
                let dying: Vec<usize> = node
                    .args
                    .iter()
                    .enumerate()
                    .filter(|(_, argument)| Self::argument_is_dying(ast, **argument))
                    .map(|(index, _)| index)
                    .collect();
                let by_move: Vec<usize> = applicable
                    .iter()
                    .copied()
                    .filter(|index| {
                        dying.iter().all(|argument| {
                            candidates[*index]
                                .param_types
                                .get(argument + 1)
                                .is_some_and(is_rvalue_reference_syntax)
                        })
                    })
                    .collect();
                let applicable = if by_move.is_empty() {
                    applicable
                } else {
                    by_move
                };
                let writable = self.receiver_is_writable(field, ast, resolutions, types);
                let const_matches: Vec<usize> = applicable
                    .iter()
                    .copied()
                    .filter(|index| {
                        self.candidate_receiver_is_const(&candidates[*index]) == !writable
                    })
                    .collect();
                let pick = const_matches.first().copied().unwrap_or(applicable[0]);
                Some(candidates[pick].clone())
            }
        }
    }

    /// Returns the span of the `const` binding an expression's storage comes
    /// from, if it comes from one.
    ///
    /// The path is followed through fields and subscripts, because a field of a
    /// field of an element of a const object is still inside that object. A
    /// dereference ends the search: the pointee's own qualifier is what decides
    /// there, and that is checked where the dereference is.
    pub(super) fn const_root(
        &self,
        ast: &Ast,
        resolutions: &Resolutions,
        expr: ExprId,
    ) -> Option<Span> {
        match &ast.expr(expr).kind {
            ExprKind::Name(name) => {
                let def = resolutions
                    .defs
                    .iter()
                    .find(|def| def.name == *name && def.scope.as_raw() != 0)?;
                self.const_bindings
                    .contains(&def.name_span)
                    .then_some(def.name_span)
            }
            ExprKind::Field(field) => self.const_root(ast, resolutions, field.object),
            ExprKind::Index(index) => self.const_root(ast, resolutions, index.object),
            _ => None,
        }
    }

    /// Whether a method call's receiver may be written through.
    ///
    /// A `const` binding is not writable, and neither is a receiver reached
    /// through a reference to const -- which is what makes `a.front()` on a
    /// `const Array` reach the read-only overload. Everything else is writable:
    /// the address is taken and the method may write through it.
    pub(super) fn receiver_is_writable(
        &self,
        field: &FieldExpr,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &TypeTable,
    ) -> bool {
        // A `const` binding, found the way assignment finds it.
        if let ExprKind::Name(name) = &ast.expr(field.object).kind
            && let Some(def) = resolutions.defs.iter().find(|def| def.name == *name)
            && self.const_bindings.contains(&def.name_span)
        {
            return false;
        }
        // A receiver whose own type is a reference to const: `*p` where `p` is
        // `*const T`, or a `&const T` returned by a call.
        !types
            .get(field.object)
            .is_some_and(fallc_sema_type_is_const_reference)
    }

    /// Whether a method's receiver parameter is a reference to const.
    ///
    /// A pointer receiver is treated as writable: the pointer itself carries
    /// the qualifier, and reading it without resolving the declaration would
    /// guess. Only the reference form is overloaded on in practice.
    pub(super) fn candidate_receiver_is_const(&mut self, candidate: &FunctionSignature) -> bool {
        let Some(this) = candidate.param_types.first() else {
            return false;
        };
        self.resolve_type_syntax(this, true).pointee_is_const()
    }

    /// Returns `true` if a method's parameters accept this receiver and these
    /// arguments.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn method_candidate_fits(
        &mut self,
        candidate: &FunctionSignature,
        receiver: &Type,
        arguments: &[Option<Type>],
        argument_exprs: &[fallc_ast::ExprId],
        named: &[(String, Option<Type>, fallc_ast::ExprId)],
        ast: &Ast,
        resolutions: &Resolutions,
    ) -> bool {
        // A templated method's receiver is a *pattern*: `this &const [N]int`
        // says the receiver is an array and binds `N` to its length. Without
        // this the length would be undecided while the candidate is compared,
        // and a method written against `[N]T` would fit no receiver at all.
        if !candidate.template_params.is_empty() {
            let previous = self.substitution.clone();
            let mut deduced = std::collections::HashMap::new();
            let mut values = std::collections::HashMap::new();
            // A receiver is written `&T` or `&const T`; the deduction unwraps
            // the reference itself, so the pattern is what is compared.
            if let Some(pattern) = candidate.param_types.first() {
                self.deduce_from_type(
                    ast,
                    pattern,
                    receiver,
                    &candidate.template_params,
                    &mut deduced,
                    &mut values,
                );
            }
            let mut mine = crate::template::Substitution::default();
            for (name, ty) in deduced {
                mine.bind(name, crate::template::TemplateArgument::Type(ty));
            }
            for (name, value) in values {
                mine.bind(name, crate::template::TemplateArgument::Int(value));
            }
            let merged = match previous.clone() {
                Some(outer) => {
                    let mut merged = outer;
                    for (name, argument) in mine.int_bindings() {
                        merged.bind(name, crate::template::TemplateArgument::Int(argument));
                    }
                    merged
                }
                None => mine,
            };
            self.substitution = Some(merged);
        }
        let declared = candidate.param_types.len().saturating_sub(1);
        // Which parameter after `this` each written argument fills: a
        // positional one fills the parameter in its position, a named one the
        // parameter with its name.
        let mut slots: Vec<Option<(&Type, fallc_ast::ExprId)>> = vec![None; declared];
        for (index, argument) in arguments.iter().enumerate() {
            if index >= declared {
                if !candidate.variadic {
                    return false;
                }
                continue;
            }
            if let Some(found) = argument {
                slots[index] = Some((found, argument_exprs[index]));
            }
        }
        for (name, argument, value) in named {
            let Some(index) = candidate.param_names.iter().position(|p| p == name) else {
                return false;
            };
            // The receiver is not written in the list, so a name that would
            // mean it names nothing.
            let Some(index) = index.checked_sub(1).filter(|index| *index < declared) else {
                return false;
            };
            if slots[index].is_some() {
                return false;
            }
            if let Some(found) = argument {
                slots[index] = Some((found, *value));
            }
        }
        let required = candidate.required_params.saturating_sub(1);
        for (index, slot) in slots.iter().enumerate() {
            if slot.is_none() && index < required {
                return false;
            }
        }
        // The receiver, which is the first parameter.
        let Some(this) = candidate.param_types.first() else {
            return false;
        };
        // Resolved with no substitution, because the member's arguments are not
        // known until the candidate is chosen -- which is what this is for. A
        // parameter that *mentions* a template parameter cannot be judged here,
        // so it is left to the chosen signature's own check below.
        //
        // Mentioning rather than *being*: `&Array<T>` is a reference, not an
        // unspecified type, so testing the top level alone would still try to
        // resolve `T` and report it.
        //
        // A receiver written against a family's parameters belongs to that
        // family and no other, so the name is compared before anything is
        // resolved: resolving `&Device<Mode>` without a member's arguments has
        // no `Mode` to put in it, and asking for one reported the parameter as
        // an argument of the wrong kind -- for every overload in the program
        // that happened to share the method's name.
        if self.mentions_template_parameter_syntax(this)
            && let Some(written) = Self::written_nominal_name(this)
            && let Some(given) = receiver.referent().unwrap_or(receiver).nominal_name()
            && short_name(&written) != short_name(given)
        {
            return false;
        }
        // A receiver written as a bare template parameter -- `this &const T` --
        // is a pattern: it fits whatever the receiver is, and the instantiation
        // binds `T` to it. The name is what says so, because the parameter is
        // not a type family's and so has no family to compare against.
        if self.written_names_a_template_parameter(this) {
            return true;
        }
        let this_ty = self.resolve_type_syntax(this, true);
        if self.mentions_unspecified(&this_ty) {
            // A receiver written against a family's parameter fits whatever the
            // receiver is; the member's arguments decide it, and they are not in
            // hand while candidates are compared. A receiver naming a type this
            // check cannot see -- another module's private type -- is not this
            // call's candidate at all. Keeping it made the *first* such method
            // the chosen one, and the call then reported the receiver as wrong.
            return self.mentions_template_parameter_syntax(this);
        }
        if !self.receiver_fits(receiver, &this_ty) {
            return false;
        }
        // The declared arguments, after the receiver.
        for (index, slot) in slots.iter().enumerate() {
            let Some((found, argument)) = slot else {
                continue;
            };
            let expected_syntax = &candidate.param_types[index + 1];
            let expected = self.resolve_type_syntax(expected_syntax, true);
            // A parameter that mentions a template parameter fits whatever the
            // argument is: the member's arguments decide it, and they are not
            // in hand while candidates are being compared.
            if self.mentions_unspecified(&expected) {
                continue;
            }
            // An unsuffixed integer literal fits any integer parameter: it has
            // no width of its own, so it takes the one it is given. The
            // arguments were inferred with no expectation -- which is what
            // deciding an overload requires -- so a literal is still an `int`
            // here, and comparing that against a `usize` parameter rejected
            // `a.resize(4, 7)`. Passing the same value through a `usize`
            // variable worked, which is what made the literal the culprit.
            if matches!(
                expected,
                Type::Reference {
                    is_rvalue: true,
                    ..
                }
            ) && TypeChecker::value_is_lvalue(ast, *argument, found)
            {
                return false;
            }
            if self.is_default_int_literal(ast, *argument) && expected.is_integer() {
                continue;
            }
            if !self.is_compatible(found, &expected) {
                // A string literal becomes the declared type it is passed as,
                // exactly as it does in an initialiser: `s.find("ll")` is the
                // String constructor's job. The conversion is recorded when the
                // call is decided; here only the fit is asked.
                if matches!(
                    &ast.expr(*argument).kind,
                    fallc_ast::ExprKind::StringLiteral(_)
                ) && self
                    .literal_constructor(resolutions, ast, &expected, *argument)
                    .is_some()
                {
                    continue;
                }
                // A value fits a reference or a pointer to it: the call takes
                // its address, exactly as it does for the receiver. Without
                // this, a method declared `other &const Path` matched no call
                // with a `Path`, and the same declaration resolved differently
                // depending on which module's table was consulted.
                let indirect = expected
                    .referent()
                    .or_else(|| expected.pointee())
                    .is_some_and(|inner| self.is_compatible(found, inner));
                if !indirect {
                    return false;
                }
            }
        }
        true
    }

    /// Returns `true` if two types are the same, ignoring how they were named.
    ///
    /// A declared type is `std.String` to a program that imported the umbrella
    /// and `String` inside the module that declared it. They are one type, so a
    /// comparison that insists on the spelling rejects valid calls.
    pub(super) fn same_type_named_apart(&self, left: &Type, right: &Type) -> bool {
        if left == right {
            return true;
        }
        let (Some(a), Some(b)) = (left.nominal_name(), right.nominal_name()) else {
            return false;
        };
        let last = |name: &str| {
            name.rsplit_once('.')
                .map_or(name, |(_, last)| last)
                .to_owned()
        };
        // The *structures* must also match, so that only the naming differs:
        // a pointer to one and a value of the other are still different.
        last(a) == last(b) && std::mem::discriminant(left) == std::mem::discriminant(right)
    }

    /// Returns every definition whose name is `name` or ends in `.{name}`.
    ///
    /// # Why the last segment matters
    ///
    /// An imported definition is bound under its qualified name, so a method
    /// of an imported type is `std.length` rather than `length`. A call site
    /// writes the bare name -- `s.length()` -- and the two have to meet. The
    /// comparison is on the last segment of the bound name, which is the name
    /// the declaration was written with.
    ///
    /// # Why there is a fallback
    ///
    /// A template body is checked against the table of the module that
    /// declared it, but the types it is instantiated with are the caller's.
    /// The body of `Array<T>::push` says `T`'s storage calls `T`'s methods,
    /// and when `T` is `String` those methods belong to the string module,
    /// which `array.ftm` never imported. The file that asked for
    /// `Array<String>` has every exported definition in its own table, so it
    /// is consulted -- but only when the declaring module has nothing under
    /// the name for a *free function*. Preferring it entry by entry would
    /// resolve the body's own `reserve` twice, once for the module's definition
    /// and once for the imported copy of it.
    ///
    /// A *method* is different: it is identified by its receiver as well as its
    /// name, so both tables are consulted and the receiver decides. A container
    /// calls `memory.free(block)` on whatever allocator it was given, and the
    /// allocator's `free` lives in the module that supplied the type -- which
    /// the declaring module has never heard of. Stopping at the declaring
    /// module's `free` made every user-defined allocator unusable.
    pub(super) fn definitions_named(
        &self,
        resolutions: &Resolutions,
        name: &str,
    ) -> Vec<crate::Def> {
        let mut found: Vec<crate::Def> = Self::definitions_named_split(resolutions, name)
            .into_iter()
            .filter(|def| !self.disabled_functions.contains(&def.name_span))
            .cloned()
            .collect();
        if !found.is_empty() {
            return found;
        }
        let Some(fallback) = &self.fallback_resolutions else {
            return Vec::new();
        };
        found.extend(
            Self::definitions_named_split(fallback, name)
                .into_iter()
                .filter(|def| !self.disabled_functions.contains(&def.name_span))
                .cloned(),
        );
        found
    }

    /// The definitions of `name`, plus the definitions of the implicit name it
    /// also stands for.
    ///
    /// `__im_init__` declares both the implicit and the explicit constructor,
    /// and `__im_as__` both the implicit and the explicit conversion: asking
    /// for `__init__` or `__as__` must therefore find them too. The other
    /// direction is not symmetrical -- asking for the implicit name does not
    /// find an explicit-only declaration -- which is what makes an `__init__`
    /// explicit.
    ///
    /// A special method is bound under `name + receiver`, so the alias is the
    /// same suffix on the implicit spelling: `__init__String` also finds
    /// `__im_init__String`, and `__as__Fraction` finds `__im_as__Fraction`.
    fn definitions_named_split<'a>(
        resolutions: &'a Resolutions,
        name: &str,
    ) -> Vec<&'a crate::Def> {
        fn matches(def: &crate::Def, name: &str) -> bool {
            def.name == name
                || def
                    .name
                    .rsplit_once('.')
                    .is_some_and(|(_, last)| last == name)
        }
        let mut found: Vec<&'a crate::Def> = resolutions
            .defs
            .iter()
            .filter(|def| matches(def, name))
            .collect();
        let implicit = if let Some(rest) = name.strip_prefix("__init__") {
            Some(format!("__im_init__{rest}"))
        } else {
            name.strip_prefix("__as__")
                .map(|rest| format!("__im_as__{rest}"))
        };
        if let Some(implicit) = implicit {
            found.extend(
                resolutions
                    .defs
                    .iter()
                    .filter(|def| matches(def, &implicit)),
            );
        }
        found
    }

    /// Every definition of a name in the declaring module and in the unit being
    /// instantiated.
    ///
    /// Used where a *receiver* decides which definition is meant -- a method
    /// call, a `__call__`, an interface's concrete method -- because then a
    /// definition in the caller's module is not a shadowed name but the answer:
    /// the allocator's `free`, the comparator's `__call__`, the printed type's
    /// `__format__`.
    pub(super) fn definitions_named_for_method(
        &self,
        resolutions: &Resolutions,
        name: &str,
    ) -> Vec<crate::Def> {
        let mut found: Vec<crate::Def> = Self::definitions_named_split(resolutions, name)
            .into_iter()
            .filter(|def| !self.disabled_functions.contains(&def.name_span))
            .cloned()
            .collect();
        let Some(fallback) = &self.fallback_resolutions else {
            return found;
        };
        for definition in Self::definitions_named_split(fallback, name) {
            if self.disabled_functions.contains(&definition.name_span) {
                continue;
            }
            if found
                .iter()
                .any(|existing| existing.name_span == definition.name_span)
            {
                continue;
            }
            found.push(definition.clone());
        }
        found
    }

    /// The type a value of this type names, with references and `const` peeled.
    ///
    /// `&ICount` and `&const ICount` name `ICount`: a reference is an address of
    /// the thing, and whether the thing behind it is an interface is the same
    /// question for the reference and for the value.
    pub(super) fn behind_references(ty: &Type) -> &Type {
        let mut ty = ty;
        loop {
            match ty {
                Type::Reference { referent, .. } => ty = referent,
                Type::Const(inner) => ty = inner,
                other => return other,
            }
        }
    }

    /// The name a written type's outermost type is written with.
    ///
    /// `&Device<Mode>` starts from `Device`, and so does `Device<Mode>`: the
    /// qualifiers and references around a type do not change which type it is.
    pub(super) fn written_nominal_name(ty: &fallc_ast::TypeSyntax) -> Option<String> {
        use fallc_ast::TypeSyntax;
        match ty {
            TypeSyntax::Named(named) => Some(named.path.clone()),
            TypeSyntax::UnsizedArray(inner)
            | TypeSyntax::Pointer(inner)
            | TypeSyntax::PointerToConst(inner)
            | TypeSyntax::ConstPointer(inner)
            | TypeSyntax::Reference(inner)
            | TypeSyntax::ConstReference(inner)
            | TypeSyntax::RvalueReference(inner)
            | TypeSyntax::ConstRvalueReference(inner)
            | TypeSyntax::Volatile(inner)
            | TypeSyntax::Const(inner) => Self::written_nominal_name(inner),
            _ => None,
        }
    }

    /// Types a method call, which is `receiver.method(args)`.
    ///
    /// The function is found by name, and is a member of the receiver's type
    /// when its first parameter is called `this` and is a pointer. The receiver
    /// is passed as that parameter, with its address taken if it is a value.
    pub(super) fn type_of_method_call(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        field: &FieldExpr,
        node: &CallExpr,
        span: fallc_span::Span,
    ) -> Option<Type> {
        // The receiver's type, inferred first: it is the receiver that says
        // which member of a family the method belongs to, and for a special
        // method it is also part of the name the definition is bound under --
        // `__init__` on `String` is `__init__String`.
        //
        // Inferred once, because inferring it per candidate would record the
        // same expression's decisions repeatedly -- and for `f().g()` would
        // emit the call to `f` as many times as there are candidates.
        let previous_expected = self.expected.take();
        let receiver_ty = self.infer_expr(ast, resolutions, types, field.object);
        self.expected = previous_expected;
        let receiver_ty = receiver_ty?;

        // A coroutine's `resume` and `result` are the compiler's, not the
        // library's. The frame's layout and the machine's states are known only
        // to the compiler that built them, so the two members are answered here
        // from the coroutine's own three types rather than looked up among
        // declared overloads. What they are is then a property of the type
        // rather than of a spelling: `Send` decides whether `resume` takes an
        // argument, `Yield` decides what it answers, and neither depends on an
        // `@enable_if` that would have told two otherwise identical
        // declarations apart.
        let probe_shape = crate::typeck::CoroutineShape::of(&receiver_ty);
        if let Some(coroutine) = probe_shape
            && let Some(answer) = self.coroutine_member(
                &coroutine,
                &receiver_ty,
                &field.field,
                node,
                ast,
                resolutions,
                types,
                span,
            )
        {
            return Some(answer);
        }

        // A call on an interface value is virtual: the interface says which
        // methods exist, and the table inside the value says which concrete
        // function runs.
        //
        // A reference to an interface is the same call: `it &ICount` is a
        // reference to a box, so what the method reaches is the box behind it.
        // Looking only at the reference found no interface and fell through to
        // the concrete members, every one of which wanted a receiver the
        // reference was not.
        if let Some((base, arguments)) = self.interface_of(Self::behind_references(&receiver_ty)) {
            return self.type_of_interface_call(
                ast,
                resolutions,
                types,
                field,
                node,
                span,
                &base,
                &arguments,
            );
        }

        // Every definition of this name is a candidate, and which one applies
        // is decided by the arguments -- including the receiver, which is the
        // first parameter. Taking the first definition instead meant an
        // overloaded method was never selected: `s.push(other String)` reached
        // `push(c8)` because that definition came first in the file, and the
        // call was reported as a type error rather than as an ambiguity.
        //
        // A name the language knows as a special method is bound with the
        // receiver's type appended, so the receiver has to be known before the
        // lookup. A type parameter is stripped first: `Array<i32>` is a member
        // of the family `Array`, and the method was written once for the family.
        let lookup = if fallc_ast::special::is_special(&field.field) {
            // The *value's* type says which method it is, and a receiver of a
            // reference type is the value behind it: `value.__hash__()` on a
            // `value &const T` is the hash of `T`. A primitive has no nominal
            // name, but it has one spelling, and its special methods are bound
            // under that.
            let base = receiver_ty.referent().unwrap_or(&receiver_ty);
            match receiver_ty.nominal_name().or_else(|| base.nominal_name()) {
                Some(name) => Self::special_method_qualified(name, &field.field),
                None if crate::types::Type::from_name(&base.spelling()).is_some() => {
                    Self::special_method_qualified(&base.spelling(), &field.field)
                }
                None => field.field.clone(),
            }
        } else {
            field.field.clone()
        };
        // The declaring module's table comes first, as it always has: a name it
        // knows for *this receiver* is the method, and nothing else is
        // consulted. When it knows the name only for other receivers, the
        // method belongs to a type the caller supplied -- `memory.free(block)`
        // inside a container, where the allocator is the caller's -- so the
        // caller's table is consulted as well.
        //
        // A special method keeps to the lexical rule, because when its
        // declarations do not fit the call, the *built-in* member-wise meaning
        // is what the language falls back to, and that decision is made against
        // the declarations the body's module knows.
        let special = fallc_ast::special::is_special(&field.field);
        let caller_substitution = self.substitution.clone();
        let member_substitution = self.receiver_substitution(&receiver_ty);
        let lexical: Vec<FunctionSignature> = self
            .definitions_named(resolutions, &lookup)
            .into_iter()
            .filter_map(|def| def.signature.clone())
            .collect();
        // Whether the declaring module knows the name *for this receiver*. The
        // receiver is resolved with the receiver's own arguments installed, so
        // that a family's `&Array<T>` means `&Array<String>` here and the test
        // is a real one rather than a guess about an unresolved `T`.
        let fits_here = {
            // The member's own arguments when the receiver names a member, and
            // otherwise whatever the body is already being checked under: a
            // method of a family is checked *with* its substitution installed,
            // so taking it away here would resolve its own receiver with
            // nothing bound -- `Device<Mode>` with no `Mode` at all.
            let previous = match &member_substitution {
                Some(substitution) => self.substitution.replace(substitution.clone()),
                None => self.substitution.clone(),
            };
            let fits = lexical.iter().any(|candidate| {
                candidate
                    .param_types
                    .first()
                    .map(|written| {
                        // A receiver written against a family's parameters can
                        // only be resolved with the member's arguments, and a
                        // module that imported a *value* of the member type may
                        // have no record of them. What can be compared without
                        // them is the family the receiver is written against --
                        // `&Device<Mode>` belongs to `Device<1>` -- so a
                        // candidate whose receiver starts from the same family
                        // is one this table knows.
                        if member_substitution.is_none() {
                            let receiver_name = receiver_ty
                                .referent()
                                .unwrap_or(&receiver_ty)
                                .nominal_name()
                                .map(short_name);
                            return match (Self::written_nominal_name(written), receiver_name) {
                                (Some(path), Some(name)) => short_name(&path) == name,
                                // Nothing to compare the name with: a receiver
                                // written against a family's parameters is one
                                // this table owns, because only the declaring
                                // module wrote it. A receiver written against a
                                // *value* parameter -- `this &const [N]const c8`
                                // -- is the same case with a length instead of
                                // a type: the declaration is a template, so no
                                // member of the receiver exists yet and this
                                // table is where the method came from.
                                // A receiver written as a template parameter
                                // -- `this &const T` -- names no type at all:
                                // it is a pattern, and the call binds it. The
                                // name is what has to be looked at, because
                                // every undeclared name in a signature is one
                                // of these and nothing else is.
                                _ => {
                                    self.mentions_template_parameter_syntax(written)
                                        || self.written_mentions_a_value_parameter(written)
                                        || self.written_names_a_template_parameter(written)
                                }
                            };
                        }
                        let expected = self.resolve_type_syntax(written, true);
                        // A receiver written against a length the declaration
                        // parameterised -- `this &const [N]const c8` -- cannot
                        // be resolved until an instantiation supplies `N`, so
                        // what is compared is the family it names, not its
                        // shape. The instantiation re-checks the fit with `N`
                        // in hand, which is where a wrong receiver is caught.
                        (self.mentions_unspecified(&expected)
                            && self.written_mentions_a_value_parameter(written))
                            || (!self.mentions_unspecified(&expected)
                                && self.receiver_fits(&receiver_ty, &expected))
                    })
                    .unwrap_or(false)
            });
            self.substitution = previous;
            fits
        };
        let candidates: Vec<FunctionSignature> = if special || fits_here {
            lexical
        } else {
            self.definitions_named_for_method(resolutions, &lookup)
                .into_iter()
                .filter_map(|def| def.signature.clone())
                .collect()
        };
        // A member `@enable_if` removed for this receiver is not a candidate:
        // the two directions of a device are two overloads that are named and
        // parametered alike, and the mode is what chooses between them.
        //
        // When *every* member of the name is disabled, the call is not a
        // missing method: it names one that the condition removed, and saying
        // so is what lets a reader find the condition instead of looking for a
        // spelling mistake. The message is the one the instantiation would
        // have reported.
        let (enabled, disabled): (Vec<FunctionSignature>, Vec<FunctionSignature>) = candidates
            .into_iter()
            .partition(|signature| self.signature_enabled(signature, &member_substitution));
        let candidates = enabled;
        if candidates.is_empty()
            && let Some(signature) = disabled.iter().find(|signature| {
                signature
                    .param_names
                    .first()
                    .is_some_and(|name| name == "this")
            })
        {
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "`{}` is disabled for this member by `@enable_if`",
                    signature.written_name
                ))
                .with_code(DiagnosticCode::sema(175))
                .with_span(span),
            );
            return None;
        }
        // A member call can only reach a member: a free function that happens
        // to share the name -- `util.alloc(size)` beside
        // `alloc(this &DefaultAllocator, usize)` -- is not a candidate, and
        // taking it for one reported the free function as a badly written
        // method. When nothing named is a member, the list is left as it is, so
        // that the diagnostic can still say why the name is not a method.
        let members: Vec<FunctionSignature> = candidates
            .iter()
            .filter(|candidate| {
                candidate
                    .param_names
                    .first()
                    .is_some_and(|name| name == "this")
            })
            .cloned()
            .collect();
        let mut candidates = if members.is_empty() {
            candidates
        } else {
            members
        };
        // Nothing takes the call on the receiver as it stands, so `.` reaches
        // through an indirection, exactly as `->` does: `it.length()` on an
        // iterator is `it->length()`, which is the sugar this rule exists for.
        //
        // The referent is tried only now, after the receiver's own methods, so
        // that a wrapper's own `size()` still means its own.
        // "No candidate takes the receiver" is not the same as "no candidate
        // exists": the name is declared for other receivers, so the list is
        // full and every entry is filtered out later. The test has to be
        // whether one of them fits, not whether the list is empty.
        let original_candidates = candidates.clone();
        // "Fits" includes "is a pattern": a receiver written `this &const T`
        // belongs to every type, so the instantiation decides afterwards.
        let any_fits = candidates.iter().any(|signature| {
            signature
                .param_names
                .first()
                .is_some_and(|name| name == "this")
                && signature.param_types.first().is_some_and(|written| {
                    if self.written_names_a_template_parameter(written) {
                        return true;
                    }
                    let expected = self.resolve_type_syntax(written, true);
                    self.receiver_fits(&receiver_ty, &expected)
                })
        });
        if !any_fits
            && let Some((inner, symbol)) = self.indirect_receiver_call(resolutions, &receiver_ty)
        {
            let lookup_inner = lookup.clone();
            let referent_substitution = self.receiver_substitution(&inner);
            let previous = match &referent_substitution {
                Some(substitution) => self.substitution.replace(substitution.clone()),
                None => self.substitution.take(),
            };
            candidates = self
                .definitions_named_for_method(resolutions, &lookup_inner)
                .into_iter()
                .filter_map(|def| def.signature.clone())
                .filter(|signature| {
                    signature
                        .param_names
                        .first()
                        .is_some_and(|name| name == "this")
                        && signature.param_types.first().is_some_and(|written| {
                            let expected = self.resolve_type_syntax(written, true);
                            self.receiver_fits(&inner, &expected)
                        })
                })
                .collect();
            self.substitution = previous;
            if candidates.is_empty() {
                // Nothing on the referent either: the original list is what
                // the diagnostic describes, so it is what the call keeps.
                candidates = original_candidates;
            } else {
                self.indirect_receivers.insert(span, symbol);
            }
        }
        self.substitution = caller_substitution.clone();
        let Some(first) = candidates.first() else {
            // A primitive's lifetime methods are built in, and a declared type
            // that writes none keeps the language's member-wise rules. Both
            // have an answer even though neither has a definition to call, and
            // both are answered here so that a generic body may spell
            // construction and destruction the same way for every type.
            if let Some(operation) = self.implicit_in_place_operation(
                ast,
                resolutions,
                types,
                field,
                node,
                &receiver_ty,
                span,
            ) {
                self.in_place_operations.insert(span, operation);
                return None;
            }
            let mut diagnostic = Diagnostic::error(format!(
                "`{}` has no method `{}`",
                receiver_ty.spelling(),
                field.field
            ))
            .with_code(DiagnosticCode::sema(2))
            .with_span(span);
            // `begin` and `end` are what a range loop asks for, which is worth
            // saying where one is missing: the call the reader wrote may be the
            // loop rather than a call at all.
            if field.field == "begin" || field.field == "end" {
                diagnostic = diagnostic.with_note(
                    "a range loop asks the range for `begin()` and `end()`, and the \
                     iterator it returns for `*`, `++` and `!=`",
                );
            }
            self.diagnostics.push(diagnostic);
            return None;
        };
        // A member function's first parameter is the receiver.
        let is_member = first.param_names.first().is_some_and(|name| name == "this");
        if !is_member {
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "`{}` is not a member function: its first parameter is not called `this`",
                    field.field
                ))
                .with_code(DiagnosticCode::sema(62))
                .with_span(span)
                .with_note("a member function's first parameter must be named `this`"),
            );
            return None;
        }
        // A method whose last parameter is a pack takes the rest of the
        // arguments: its declared arity is a lower bound, and the declaration is
        // expanded per call -- `f.print("x = ", x, '\n')` is three elements, and
        // `f.print()` is none. It is chosen before the fixed-arity selection,
        // which compares counts, but only when no fixed-arity overload of the
        // same name takes this call: an exact match outranks a pack.
        if let Some((signature, substitution)) =
            self.choose_pack_method(&candidates, &receiver_ty, node.args.len())
        {
            return self.finish_pack_call(
                ast,
                resolutions,
                types,
                node,
                span,
                &receiver_ty,
                &signature,
                &substitution,
                caller_substitution,
                super::operators::PackCallKind::Method,
            );
        }
        // A special method whose declarations do not fit the call falls back to
        // the built-in or member-wise meaning, exactly as `T x = value;` does
        // when no constructor accepts `value`. An ordinary method has no
        // implicit spelling, so a call that fits none of its overloads is an
        // error there.
        let signature = if candidates.len() == 1 && !special {
            // One overload is still a member of the receiver's type, so its
            // `T`s mean the receiver's arguments. Choosing between overloads
            // installs this; with nothing to choose, it is installed here.
            self.substitution = member_substitution.clone();
            first.clone()
        } else {
            match self.choose_method_overload(&candidates, field, node, ast, resolutions, types) {
                Some(chosen) => chosen,
                None => {
                    self.substitution = caller_substitution.clone();
                    if let Some(operation) = self.implicit_in_place_operation(
                        ast,
                        resolutions,
                        types,
                        field,
                        node,
                        &receiver_ty,
                        span,
                    ) {
                        self.in_place_operations.insert(span, operation);
                        return None;
                    }
                    // Whether *any* overload takes this receiver decides
                    // which mistake this is. A name that exists only on other
                    // types is "no such method here" -- which is what a range
                    // loop over an `int` says -- while a method that does exist
                    // but wants different arguments is the argument count.
                    let takes_this_receiver = candidates.iter().any(|candidate| {
                        candidate
                            .param_types
                            .first()
                            .map(|written| {
                                // A receiver written as a template parameter is
                                // a pattern and takes any type, so it is not
                                // "a method for other receivers".
                                if self.written_names_a_template_parameter(written) {
                                    return true;
                                }
                                let expected = self.resolve_type_syntax(written, true);
                                self.receiver_fits(&receiver_ty, &expected)
                            })
                            .unwrap_or(false)
                    });
                    if !takes_this_receiver {
                        let mut diagnostic = Diagnostic::error(format!(
                            "`{}` has no method `{}`",
                            receiver_ty.spelling(),
                            field.field
                        ))
                        .with_code(DiagnosticCode::sema(2))
                        .with_span(span);
                        if field.field == "begin" || field.field == "end" {
                            diagnostic = diagnostic.with_note(
                                "a range loop asks the range for `begin()` and `end()`, and \
                                 the iterator it returns for `*`, `++` and `!=`",
                            );
                        }
                        self.diagnostics.push(diagnostic);
                        return None;
                    }
                    self.diagnostics.push(
                        Diagnostic::error(format!(
                            "no overload of `{}` matches {} argument(s)",
                            field.field,
                            node.args.len()
                        ))
                        .with_code(DiagnosticCode::sema(36))
                        .with_span(span)
                        .with_note(format!(
                            "`{}` has {} overload(s); check the argument types",
                            field.field,
                            candidates.len()
                        )),
                    );
                    return None;
                }
            }
        };

        let result = self.finish_method_call(
            ast,
            resolutions,
            types,
            field,
            node,
            span,
            &receiver_ty,
            &signature,
            &member_substitution,
            &caller_substitution,
        );
        self.substitution = caller_substitution;
        result
    }

    /// Finishes a method call once its overload is chosen.
    ///
    /// The signature's parameters, the receiver check, the arguments, the
    /// symbol and the return type are all resolved under the member's
    /// arguments. The caller puts its own substitution back afterwards, since
    /// this runs inside an enclosing instantiation as often as at top level.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn finish_method_call(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        field: &FieldExpr,
        node: &CallExpr,
        span: fallc_span::Span,
        receiver_ty: &Type,
        signature: &FunctionSignature,
        member_substitution: &Option<crate::template::Substitution>,
        caller_substitution: &Option<crate::template::Substitution>,
    ) -> Option<Type> {
        // The *whole* signature is resolved under the substitution the caller
        // installed, so that every parameter of a templated type's method means
        // what the member says. Resolving one at a time left the others naming
        // `T`, and deriving the substitution again here hid the caller's.
        let resolved_params: Vec<Type> = signature
            .param_types
            .iter()
            .map(|syntax| self.resolve_type_syntax(syntax, true))
            .collect();
        let expected = resolved_params.first().cloned()?;
        // A receiver may be a pointer, a reference -- `&T`, `&const T`,
        // `&volatile T` -- or the value itself. A value receiver gets a copy,
        // so a method that modifies `this` through one does not change the
        // object; the language allows it because reading through a copy is
        // sometimes what a small type wants, and the alternative is forcing
        // every caller to write an address.
        let _ = &expected;

        // Calling on a value takes its address; calling on a pointer uses it
        // directly. Both reach the same `this`.
        // A value receiver matches the pointee, a pointer receiver matches the
        // pointer itself. Both reach the same `this` once code generation has
        // taken the address where it is needed.
        // The comparison ignores how the type was *named*: `std.String` in the
        // calling file and `String` inside the module are one type, and asking
        // for both spellings to match rejected every method call on an
        // imported type.
        // `.` reaches through an indirection: a type that defines `__deref__`
        // or `__pointer__` can be called on as the thing it points at, exactly
        // as a pointer can. The method is looked up on the receiver first --
        // that is the plain member rule, and it is what a type's own method
        // means -- and the indirection is tried only when no method of the
        // receiver's own type applies.
        let dereferenced = if self.receiver_fits(receiver_ty, &expected) {
            None
        } else {
            self.dereferenced_receiver(receiver_ty, resolutions)
        };
        let mut through_indirection = false;
        if let Some(inner) = &dereferenced
            && self.receiver_fits(inner, &expected)
        {
            through_indirection = true;
            // The call that reaches the referent is recorded under the call's
            // own span, so code generation can make it before it passes the
            // receiver. Without this the receiver's own bits were handed to a
            // method of another type, which read the wrapper's first word as
            // the referent's.
            for method in [
                fallc_ast::special::SpecialMethod::Deref,
                fallc_ast::special::SpecialMethod::Pointer,
            ] {
                if let Some(call) =
                    self.special_method_call(resolutions, receiver_ty, method.name(), 1)
                {
                    self.indirect_receivers.insert(span, call.symbol);
                    break;
                }
            }
        }
        // A receiver written against a template parameter is a *pattern*: `this
        // &const T` binds `T` to whatever the receiver is, and `this &const
        // [N]T` binds `N` and `T` together. The check is therefore not "does it
        // fit" but "this is the binding", and the instantiation then checks the
        // body with the parameters standing for what they became. Something
        // still unspecified anywhere inside is a parameter that has no binding
        // yet, which is exactly the case a pattern describes.
        let bound_by_pattern = self.mentions_unspecified(&expected);
        if !through_indirection && !bound_by_pattern && !self.receiver_fits(receiver_ty, &expected)
        {
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "`{}` expects `this` to be `{}`, but `{}` was given",
                    field.field,
                    expected.spelling(),
                    receiver_ty.spelling()
                ))
                .with_code(DiagnosticCode::sema(63))
                .with_span(span),
            );
            return None;
        }

        // A method that writes through its receiver needs a receiver it may
        // write through. `this &T` on a `const` object would hand out a way to
        // change something declared constant, and the language has one answer
        // for that: the method says `this &const T` if it does not write, and
        // the fields it does write are marked `mutable`.
        if !expected.pointee_is_const()
            && !self.receiver_is_writable(field, ast, resolutions, types)
        {
            // What the author wanted to write, spelled out: the only way to
            // read through a const object is a const reference, and the only
            // way to write inside one is a `mutable` field.
            let read_only = match &expected {
                Type::Reference { referent, .. } => format!("&const {}", referent.spelling()),
                Type::Pointer { pointee, .. } => format!("*const {}", pointee.spelling()),
                other => other.spelling(),
            };
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "`{}` cannot be called on a `const` object, because its `this` is `{}`",
                    field.field,
                    expected.spelling()
                ))
                .with_code(DiagnosticCode::sema(63))
                .with_span(span)
                .with_note(format!(
                    "declare it `this {read_only}` if it does not write, or mark the fields it \
                     writes `mutable`"
                )),
            );
            return None;
        }

        // The remaining arguments are checked against the parameters after
        // `this`.
        let declared = signature.param_types.len().saturating_sub(1);
        let given = node.args.len() + node.named.len();
        let required = signature.required_params.saturating_sub(1);
        if signature.variadic {
            if given < required {
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "`{}` takes at least {required} argument(s), but {given} were given",
                        field.field
                    ))
                    .with_code(DiagnosticCode::sema(36))
                    .with_span(span),
                );
                return None;
            }
        } else if given < required || given > declared {
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "`{}` takes {declared} argument(s), but {given} were given",
                    field.field
                ))
                .with_code(DiagnosticCode::sema(36))
                .with_span(span),
            );
            return None;
        }

        // A named argument may fill a parameter the positional prefix does not
        // reach, so the names are matched before the values are checked.
        let arguments = self.match_named_arguments(signature, node, 1, &field.field, span)?;
        // A parameter the call leaves empty is filled by its default, and a
        // default written in another module reaches here either as its value or
        // as an expression in that module's tree. Recording it under the call's
        // span is what lets the code generator put it in the slot the call did
        // not fill.
        let mut filled = vec![false; signature.param_types.len()];
        for &(index, _) in &arguments {
            if let Some(slot) = filled.get_mut(index) {
                *slot = true;
            }
        }
        for index in 1..signature.param_types.len() {
            if filled.get(index).copied().unwrap_or(false)
                || signature.defaults.get(index).copied().flatten().is_none()
            {
                continue;
            }
            if !self.record_omitted_default(&field.field, signature, index, span) {
                return None;
            }
        }
        // The method's own template parameters, deduced from the arguments as
        // a free template's are: `sort_by(order Cmp)` called with a
        // `Greater<int>` has `Cmp` = `Greater<int>`, and the body's `cmp(...)`
        // is a call of that. They are collected into the member's substitution
        // so that the instantiation carries them; without this the body was
        // checked with `Cmp` still a name.
        let mut deduced = member_substitution.clone().unwrap_or_default();
        // Each parameter is checked against whatever fills it -- written by
        // position after the receiver, or by name -- because the signature does
        // not care which.
        for &(index, argument) in &arguments {
            let Some(expected_syntax) = signature.param_types.get(index) else {
                continue;
            };
            // Resolved under the member's arguments, so that a parameter
            // written `v T` on `Box<int>` means `v int`. A parameter that is
            // *still* a template parameter has nothing to be substituted -- the
            // signature is the template's own, and the argument is what decides
            // it -- so the inferred type is taken as the answer, which is the
            // same deduction a free template does.
            // The *parameter* is resolved under the member's arguments --
            // `v T` on `Box<int>` means `v int` -- and the *argument* under the
            // caller's. They are two different substitutions: the argument is
            // an expression of the enclosing body, and its `T` is the one that
            // body was instantiated with. Checking it under the member's left
            // `std.fake<&&T>` unsubstituted inside a template.
            self.substitution = member_substitution.clone();
            let mut expected_ty = self.resolve_type_syntax(expected_syntax, true);
            self.substitution = caller_substitution.clone();
            // A parameter that is still a template parameter has no type to
            // expect yet: the argument is what decides it, so it is inferred
            // with no expectation at all. Handing the unspecified type to the
            // inferencer made a literal have no type, and the parameter was
            // never deduced.
            let emphasis = expected_ty.is_unspecified();
            let previous = self.expected.take();
            self.expected = if emphasis {
                None
            } else {
                Some(expected_ty.clone())
            };
            let found = self.infer_expr(ast, resolutions, types, argument);
            self.expected = previous;
            if let Some(found) = found {
                if expected_ty.is_unspecified() {
                    expected_ty = found.clone();
                }
                // The *method's own* parameters only. A name the receiver's
                // member already bound is the family's: `__call__` on
                // `Less<int>` has `E` = `int` before it is called, and taking
                // it from the argument would make it `*int` -- the address the
                // argument was passed as.
                let bare = Self::bare_parameter_name(expected_syntax);
                let simple_name = bare.is_some_and(|(named, _)| named.arguments.is_empty());
                let bound_by_name = simple_name
                    && bare.is_some_and(|(named, _)| {
                        member_substitution
                            .as_ref()
                            .is_none_or(|substitution| substitution.get(&named.path).is_none())
                            && signature.template_params.iter().any(|param| {
                                param.name == named.path
                                    && param.kind == fallc_ast::TemplateParamKind::Type
                            })
                    });
                if bound_by_name {
                    let (named, indirect) = bare.expect("a bare parameter was just matched");
                    let bound = if indirect {
                        found.referent().cloned().unwrap_or_else(|| found.clone())
                    } else {
                        found.clone()
                    };
                    deduced.bind_type(named.path.clone(), bound);
                } else if !simple_name {
                    // A parameter that *contains* a type parameter -- `*Array<T>`,
                    // `&Pair<K, V>` -- is deduced the way a free template's is:
                    // the pattern is walked against the argument's type. A bare
                    // name is the case above, and walking it here as well made a
                    // `T` bound by the receiver's member arguments be taken from
                    // the argument instead.
                    let mut names = std::collections::HashMap::new();
                    let mut values = std::collections::HashMap::new();
                    self.deduce_from_type(
                        ast,
                        expected_syntax,
                        &found,
                        &signature.template_params,
                        &mut names,
                        &mut values,
                    );
                    for (name, ty) in names {
                        deduced.bind_type(name, ty);
                    }
                    for (name, value) in values {
                        deduced.bind(name, crate::template::TemplateArgument::Int(value));
                    }
                }
                // The parameter may *wrap* the deduced name -- `value &T` is a
                // reference to it, `*Array<T>` a pointer to a member -- and the
                // comparison needs the wrapped type with `T` decided.
                if bound_by_name || !simple_name {
                    let previous = self.substitution.replace(deduced.clone());
                    expected_ty = self.resolve_type_syntax(expected_syntax, true);
                    self.substitution = previous;
                }
                self.note_indirect_argument(argument, &found, &expected_ty, ast);
                if !self.adopt_literal_argument(resolutions, ast, argument, &expected_ty, &found) {
                    let was_parameter = self.in_parameter_position;
                    self.in_parameter_position = true;
                    self.types_must_match(
                        &expected_ty,
                        &found,
                        ast.expr(argument).span,
                        "argument",
                    );
                    self.in_parameter_position = was_parameter;
                }
            }
        }

        // Extra variadic arguments still need a type for the code generator.
        self.substitution = caller_substitution.clone();
        for arg in node.args.iter().skip(declared) {
            let previous = self.expected.take();
            let _ = self.infer_expr(ast, resolutions, types, *arg);
            self.expected = previous;
        }

        // The symbol is recorded here, from the signature's own parameter
        // types, because this is the only place that knows *which* definition
        // the call selected. Code generation must not search for it again: a
        // name is not a key, and looking one up by the shape of the mangled
        // string is guesswork that happens to work until two types share a
        // method name.
        // The parameters are resolved under the member's arguments, so that a
        // method of `Box<f64>` is emitted as a function over `Box<f64>` rather
        // than over `Box<T>`. Without this every member of a family shares one
        // symbol and therefore one function -- and the first member built wins
        // for all of them.
        // The member's arguments *and* the method's own, which the call just
        // deduced: `sort_by(order Cmp)` is one function per comparator, and a
        // symbol mangled without `Cmp` made `Less<int>` and `Greater<int>`
        // share one body -- so whichever was emitted first sorted both ways.
        // The receiver is a *pattern*: `this &const [N]int` says the receiver
        // is an array and binds `N` to its length, which no argument supplies.
        // Deduced before the symbol is built, because the symbol carries the
        // receiver's type and an undecided `N` would give it `<invalid array
        // length>` -- one symbol for every length, and a body whose `N` has no
        // number.
        if let Some(written) = signature.param_types.first() {
            let mut names = std::collections::HashMap::new();
            let mut values = std::collections::HashMap::new();
            self.deduce_from_type(
                ast,
                written,
                receiver_ty,
                &signature.template_params,
                &mut names,
                &mut values,
            );
            for (name, ty) in names {
                deduced.bind_type(name, ty);
            }
            for (name, value) in values {
                deduced.bind(name, crate::template::TemplateArgument::Int(value));
            }
        }
        let mangling = if deduced.is_empty() {
            member_substitution.clone()
        } else {
            Some(deduced.clone())
        };
        let previous = match &mangling {
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
        self.method_calls.insert(span, symbol.clone());
        self.note_throwing_call(span, signature, ast, None);
        // A method of a templated type is emitted only when some member uses
        // it, so the instance is recorded here for the same reason a free
        // template's is.
        if member_substitution.is_some() || !signature.template_params.is_empty() {
            let receiver_argument = types
                .get(field.object)
                .cloned()
                .unwrap_or_else(|| receiver_ty.clone());
            self.record_method_instantiation(signature, &deduced, &symbol, &receiver_argument);
        }

        if signature.return_type.as_name() == Some("") {
            return None;
        }
        // Whether an address is what the context wants decides whether a
        // reference result is read, so it is captured before the substitution
        // is installed and the return type resolved.
        let wanted_address = self
            .expected
            .as_ref()
            .is_some_and(|wanted| wanted.is_indirection());
        // The return type is resolved under the same substitution as the
        // symbol: the receiver's arguments *and* the method's own, which the
        // call just deduced. A method of `Box<int>` returning `T` returns
        // `int`; a template method `uniform<T>` returning `T` returns what the
        // arguments deduced, not a bare `T`.
        let previous = match &mangling {
            Some(substitution) => self.substitution.replace(substitution.clone()),
            None => self.substitution.take(),
        };
        let returns = self.signature_result_type(signature);
        self.substitution = previous;
        // A method returning a *reference* is read as its referent unless an
        // address is what the context wants -- the rule `*p` follows, and for
        // the same reason: whether to read is a fact about the use, and only
        // the checker is looking at it.
        if !wanted_address && let Some(referent) = returns.referent().cloned() {
            self.deref_reads.insert(span, referent.clone());
            return Some(referent);
        }
        Some(returns)
    }

    /// Reports that two types differ where they were required to match.
    ///
    /// Returns `true` if they are compatible, so that every caller checks
    /// compatibility the same way. Reporting unconditionally here was wrong:
    /// it rejected the const-widening conversion that is legal.
    pub(super) fn types_must_match(
        &mut self,
        expected: &Type,
        found: &Type,
        span: fallc_span::Span,
        context: &str,
    ) -> bool {
        // Passing or returning a value of an incomplete type would need its
        // layout, and there is none yet.
        if !self.require_complete(expected, span, context) {
            return false;
        }
        if self.is_compatible(found, expected) {
            return true;
        }
        // A reference to an interface is the one mismatch that needs more than
        // "the types differ": the value is a boxable one, and the program is
        // one line away from being right.
        if let Type::Reference { referent, .. } = expected
            && self.interface_of(referent).is_some()
        {
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "`{}` cannot stand where `{}` is wanted: an interface value is a box, \
                     and a reference to one points at a box",
                    found.spelling(),
                    expected.spelling()
                ))
                .with_code(DiagnosticCode::sema(10))
                .with_span(span)
                .with_note(format!(
                    "box it first -- `b {} = value;` -- and pass `&b`, or take the \
                     interface by value",
                    expected
                        .spelling()
                        .trim_start_matches("&const ")
                        .trim_start_matches('&')
                )),
            );
            return false;
        }
        self.diagnostics.push(
            Diagnostic::error(format!(
                "type mismatch in {context}: expected `{}`, found `{}`",
                expected.spelling(),
                found.spelling()
            ))
            .with_code(DiagnosticCode::sema(10))
            .with_span(span)
            .with_note(
                "there are no implicit conversions between numeric types; \
                 use `as`",
            ),
        );
        false
    }

    /// Infers the type of a name from its definition.
    ///
    /// A local or a parameter has the type its declaration gave it; a constant
    /// the type its declaration gave it, with the use also folded to its value;
    /// an imported variable the type the exporting module gave it; and a
    /// function name its function type. A type name is not a value, so it has
    /// no type here.
    pub(super) fn type_of_name(
        &mut self,
        _ast: &Ast,
        resolutions: &Resolutions,
        name: &str,
        span: fallc_span::Span,
    ) -> Option<Type> {
        // The binding recorded for *this* occurrence, not the first definition
        // of the name. Every function's receiver is called `this`, so looking
        // one up by name gave every body the first function's receiver type --
        // which is how `this->inner` in one constructor was reported as a
        // missing field of another type.
        if let Some(id) = resolutions.resolve(span)
            && let Some(def) = resolutions.def(id)
        {
            // A constant is not a binding but it *has* a type, and its use is
            // folded to a value. Returning `None` for everything that is not a
            // local made a constant's use look like a name with no type at all.
            if def.kind == crate::resolve::DefKind::Const {
                let folded = self
                    .instantiation_constants
                    .get(&def.name)
                    .copied()
                    // A constant that came from another module has no
                    // expression in this tree: its value was folded where it
                    // was written, and that is the number to emit.
                    .or_else(|| self.constants_by_name.get(&def.name).copied())
                    .or_else(|| {
                        let value = self.constants.get(&def.name).copied()?;
                        self.const_eval_int(value)
                    });
                if let Some(folded) = folded {
                    self.constant_values.insert(span, folded);
                }
                let resolved = self
                    .binding_types
                    .get(def.name_span)
                    .cloned()
                    .or_else(|| self.constant_types_by_name.get(&def.name).cloned());
                return resolved;
            }
            // An imported variable has no declaration in this unit, so its
            // type comes from the exporting module. The definition remembers
            // which name it came from, which is how the qualified entry is
            // found.
            // A deprecated name says so wherever it is used, not only when it
            // is called: `BUILD` on the right of an assignment is a use too.
            if matches!(
                def.kind,
                crate::resolve::DefKind::Local | crate::resolve::DefKind::Const
            ) {
                let name = def.name.as_str();
                if let Some(message) = self.deprecated_functions.get(name).cloned() {
                    self.warn_deprecated(name, message, span);
                }
            }
            if def.kind == crate::resolve::DefKind::Local
                && let (Some(module), Some(member)) = (&def.imported_via, &def.imported_as)
            {
                let qualified = format!("{module}.{member}");
                if let Some(ty) = self.imported_variables.get(&qualified).cloned() {
                    self.external_global_refs.insert(span, qualified);
                    return Some(ty);
                }
            }
            return match def.kind {
                crate::resolve::DefKind::Local | crate::resolve::DefKind::Param => {
                    self.binding_types.get(def.name_span).cloned()
                }
                crate::resolve::DefKind::Function => self.function_name_type(resolutions, name),
                crate::resolve::DefKind::Type => self.type_name_as_expression(name, span),
                _ => None,
            };
        }
        let Some(def) = resolutions.defs.iter().find(|def| def.name == name) else {
            // A primitive type has no declaration to point at: `int` is a type
            // because the language says so, and `x int = int;` names one just
            // as `x Thing = Thing;` does. Recognised by name for that reason.
            if Type::from_name(name).is_some() {
                return self.type_name_as_expression(name, span);
            }
            return None;
        };
        match def.kind {
            // A local or a parameter has whatever type its declaration gave it.
            crate::resolve::DefKind::Local | crate::resolve::DefKind::Param => {
                self.binding_types.get(def.name_span).cloned()
            }
            // A function name used as a value has the function's type, and
            // decays to a pointer wherever one is wanted.
            //
            // A type name is not a value either: it names a type, and a type
            // reaches an expression only through a declaration.
            // A constant's type is the one its declaration gave it. Its *use*
            // is folded to its value, which is recorded so that code
            // generation emits a number rather than a load from storage that
            // does not exist.
            crate::resolve::DefKind::Const => {
                // Recorded under the *use*'s span, which is where code
                // generation looks: a constant has no storage, so the
                // expression that names it has to become its value.
                let folded = self
                    .instantiation_constants
                    .get(&def.name)
                    .copied()
                    .or_else(|| self.constants_by_name.get(&def.name).copied())
                    .or_else(|| {
                        let value = self.constants.get(&def.name).copied()?;
                        self.const_eval_int(value)
                    });
                if let Some(folded) = folded {
                    self.constant_values.insert(span, folded);
                }
                self.binding_types
                    .get(def.name_span)
                    .cloned()
                    .or_else(|| self.constant_types_by_name.get(&def.name).cloned())
            }
            // A template parameter stands for whatever the instantiation
            // supplies. Inside one, a value parameter has the type it was
            // declared with -- `string []char` is an array of characters --
            // and its *value* is replaced; outside, it has no type at all.
            crate::resolve::DefKind::TemplateParam => {
                // A *value* parameter's number is known where the member is
                // instantiated, and recording it here is what lets an expression
                // that mentions it fold: `@enable_if(Mode != OpenMode.READ)` is
                // decided by the instantiation's numbers.
                if let Some(crate::template::TemplateArgument::Int(value)) = self
                    .substitution
                    .as_ref()
                    .and_then(|substitution| substitution.value_of(&def.name))
                    .cloned()
                {
                    self.constant_values.insert(span, value);
                }
                // A type variable used where a value belongs: `it.push(T)` in a
                // requirement states the parameter's type, exactly as the type
                // name it stands for would. The substitution is what the
                // question binds it to.
                if self.in_trait_requirement
                    && let Some(ty) = self
                        .substitution
                        .as_ref()
                        .and_then(|substitution| substitution.type_of(&def.name))
                        .cloned()
                {
                    return Some(ty);
                }
                self.template_value_types.get(&def.name).cloned()
            }
            // A function name used as a value has the function's type. When
            // the name is overloaded, the context decides which one: the
            // expected type of `p *fn(int) int = &f;` names `f(int)` and not
            // `f(double)`.
            crate::resolve::DefKind::Function => self.function_name_type(resolutions, name),
            // A type name used where a value belongs, which is `S0135` -- see
            // [`Self::type_name_as_expression`] -- except inside a requirement,
            // where naming a type is how a parameter is stated.
            crate::resolve::DefKind::Type => self.type_name_as_expression(name, span),
            crate::resolve::DefKind::Unspecified => None,
        }
    }

    /// The type a *type name* stands for when it is written as an expression.
    ///
    /// A type is not a value: `x int = int;` names a type where a value
    /// belongs, and there is nothing to emit for it. Saying so here is what
    /// stops such a program from reaching code generation, where a name with no
    /// storage was previously reported as an internal error.
    ///
    /// A restriction is the one place where a type name *is* the statement the
    /// author is writing. `it.push(int)` says the operation takes an `int`, and
    /// `it.get() -> T` says it yields `T`; the type stands for a value of that
    /// type so that the requirement can be checked against a concrete method,
    /// which is what makes a failed restriction mean "this call would not
    /// compile".
    fn type_name_as_expression(&mut self, name: &str, span: fallc_span::Span) -> Option<Type> {
        if !self.in_trait_requirement {
            self.diagnostics.push(
                Diagnostic::error("a type is not a value")
                    .with_code(DiagnosticCode::sema(135))
                    .with_span(span)
                    .with_note(format!(
                        "`{name}` names a type; a value of it is what an expression can hold"
                    )),
            );
            return None;
        }
        // A requirement's subject is substituted, so a type variable written
        // here means the type the question was asked about.
        let written = fallc_ast::TypeSyntax::Named(fallc_ast::NamedType {
            path: name.to_owned(),
            arguments: Vec::new(),
            is_expansion: false,
            span,
        });
        let substitution = self.substitution.clone().unwrap_or_default();
        Some(self.resolve_template_type(&written, &substitution))
    }

    /// Reports a use of a name whose declaration is marked `@deprecated`.
    pub(super) fn warn_deprecated(
        &mut self,
        name: &str,
        message: Option<String>,
        span: fallc_span::Span,
    ) {
        let text = match message {
            Some(message) if !message.is_empty() => {
                format!("`{name}` is deprecated: {message}")
            }
            _ => format!("`{name}` is deprecated"),
        };
        self.diagnostics.push(
            Diagnostic::warning(text)
                .with_code(DiagnosticCode::sema(160))
                .with_warning("deprecated")
                .with_span(span),
        );
    }

    /// The type a name denotes when it names a function.
    ///
    /// An overloaded name has as many types as it has overloads, and only the
    /// expectation says which one is meant: `p *fn(int) int = &f;` names
    /// `f(int)` and not `f(double)`.
    pub(super) fn function_name_type(
        &mut self,
        resolutions: &Resolutions,
        name: &str,
    ) -> Option<Type> {
        let mut candidates = Vec::new();
        for def in resolutions
            .defs
            .iter()
            .filter(|def| def.name == name && def.kind == crate::resolve::DefKind::Function)
        {
            if let Some(ty) = self.function_type_of(def) {
                candidates.push(ty);
            }
        }
        match candidates.len() {
            0 => None,
            1 => candidates.pop(),
            _ => {
                let wanted = self.expected.clone()?;
                let target = match wanted.unqualified() {
                    Type::Pointer { pointee, .. } => pointee.unqualified().clone(),
                    other => other.clone(),
                };
                candidates
                    .into_iter()
                    .find(|candidate| *candidate == target)
            }
        }
    }

    /// The type a function's declaration gives it.
    ///
    /// This is what a bare function name means as a value, and what a pointer
    /// to one is a pointer to: `*fn(int) int` and `&f` agree because both are
    /// built from this.
    pub(super) fn function_type_of(&mut self, def: &crate::resolve::Def) -> Option<Type> {
        let signature = def.signature.as_ref()?;
        // A template is not a function: it has no `T` to call with, so it has
        // no function type either.
        if !signature.template_params.is_empty() || signature.pack.is_some() {
            return None;
        }
        let parameters = signature
            .param_types
            .iter()
            .map(|written| self.resolve_type_syntax(written, true))
            .collect();
        let returns = Box::new(self.signature_result_type(signature));
        Some(Type::Function {
            parameters,
            returns,
            abi: signature.abi.clone(),
            variadic: signature.variadic,
            // The declaration's own marker: a function that may raise has its
            // result shaped by the exception ABI, so its type is not a plain
            // `fn(..) ..`.
            throws: signature.throws,
        })
    }

    /// Records that a reference-typed expression is being *read* here.
    ///
    /// A reference names storage, and using it as a value reads through it: `a
    /// int = r;` and `f(r)` want the referent, while `x &int = r;` wants the
    /// address. The expected type is what tells the two apart -- an indirection
    /// wants the address, anything else the value -- and the record is what
    /// tells code generation, which would otherwise store the address in an
    /// `int` slot.
    pub(super) fn note_reference_read(
        &mut self,
        ast: &Ast,
        expr: ExprId,
        found: &Type,
        expected: &Type,
    ) {
        if expected.is_indirection() {
            // An address is wanted, so a read recorded while the expression was
            // inferred would be wrong: `r &T = *p;` and `f(&x, ...)` store or
            // pass the address of the place, not the value in it. Inferring the
            // initialiser without an expectation records the read, which is why
            // it is removed here rather than merely not added.
            self.deref_reads.remove(&ast.expr(expr).span);
            return;
        }
        let Some(referent) = found.referent().cloned() else {
            return;
        };
        self.deref_reads.insert(ast.expr(expr).span, referent);
    }

    /// Checks an expression against an expected type.
    ///
    /// The body is the standard bidirectional pattern: infer, then require
    /// compatibility, recording the inferred type either way so later passes
    /// can read it without re-deriving. Compatibility is `is_compatible`,
    /// which is the whole list of the language's implicit conversions.
    pub fn check_expr(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        expr: ExprId,
        expected: &Type,
    ) -> bool {
        // Publish the expectation so a literal can adopt it, then restore the
        // previous one: checking is nested, and an inner expectation must not
        // leak outward.
        let previous = self.expected.replace(expected.clone());
        let inferred = self.infer_expr(ast, resolutions, types, expr);
        self.expected = previous;
        let Some(found) = inferred else {
            return false;
        };
        if self.is_compatible(&found, expected) {
            self.note_reference_read(ast, expr, &found, expected);
            true
        } else if integer_promotes(&found, expected) {
            // A lossless integer widening. Code generation converts the value
            // at this expression, so the target type is recorded here.
            self.adopted_types
                .insert(ast.expr(expr).span, expected.clone());
            true
        } else if self.try_implicit_conversion(resolutions, ast, types, expr, expected) {
            true
        } else {
            // The *expression's* span, not the node's. They are the same thing
            // for most nodes, but a mismatch on a binding's initialiser
            // reported the enclosing function -- so the caret pointed at `fn
            // main() int {` rather than at the value that was wrong.
            self.report_type_mismatch(ast.expr(expr).span, &found, expected);
            false
        }
    }

    /// Returns `true` if `source` may be explicitly converted to `target` with
    /// `as`.
    ///
    /// # What is allowed
    ///
    /// * between any two integer types, and between `byte` and any integer;
    /// * between any two floating-point types;
    /// * between integers and floats;
    /// * `bool` to an integer, `nullptr` to any indirection, and an array
    ///   behind a pointer to a pointer to its element;
    /// * an indirection to one of the same shape modulo `const`, and the
    ///   byte-pointer conversion in either direction.
    ///
    /// A lossy conversion wraps, because narrowing truncates: `300 as u8` is
    /// 44. A cast to or from a type with no numeric meaning is refused here; a
    /// declared type takes part in `as` by defining `__as__`.
    #[must_use]
    pub fn is_castable(&self, source: &Type, target: &Type) -> bool {
        source.is_castable_to(target)
    }

    /// Returns `true` if a value of type `found` may be used where `expected`
    /// is required.
    ///
    /// There are no implicit conversions between values, except these: adding
    /// `const` to an indirection, which only takes away the right to write; a
    /// function name decaying to a pointer to it; an array decaying to a pointer
    /// to its first element; a `T` standing for a `&T` in a parameter position,
    /// because a reference names a place; a pointer standing for a reference of
    /// the same shape; a value standing for a `const` or `volatile` one; the two
    /// names of one imported type; and the byte-pointer conversion.
    #[must_use]
    pub fn is_compatible(&self, found: &Type, expected: &Type) -> bool {
        if found == expected {
            return true;
        }
        // "No value" has two spellings, the empty name and the word `void`,
        // and the comparison above looks at the whole type -- span included --
        // so the two are never equal. A restriction that says `-> void` is
        // about a method that returns nothing, and this is what makes it hold.
        if found.is_void() || expected.is_void() {
            return found.is_void() && expected.is_void();
        }
        // A concrete value satisfies an interface when it has the methods.
        // Deciding that needs the resolution table, so it is answered in one
        // pass after checking; here the shape alone must not refuse it.
        if self.interface_of(expected).is_some() {
            return true;
        }
        // A reference to an interface is a reference to a *box*, and only the
        // same interface may stand behind it: the callee reads the value's two
        // fields as an object and a table. A concrete value satisfies an
        // interface as a *value* conversion -- boxing copies the value and
        // builds the table -- and there is no reference to that conversion, so
        // `&value` where `&ICount` is wanted is not one. Allowing it pointed the
        // callee at the value's own fields, which are not a box: the call read
        // an object pointer as if it were a method table.
        if let Type::Reference { referent, .. } = expected
            && let Some(interface) = self.interface_of(referent)
        {
            return self.interface_of(behind_one_indirection(found)) == Some(interface);
        }
        // A writable array satisfies a read-only one, which is the same
        // widening a `*T` gets where a `*const T` is wanted: the callee may read
        // the elements and cannot write them, so nothing the caller can observe
        // changes. The other direction is refused -- a read-only array may not
        // become writable.
        if let (
            Type::Array {
                element: found_element,
                len: found_len,
            },
            Type::Array {
                element: expected_element,
                len: expected_len,
            },
        ) = (found.unqualified(), expected.unqualified())
            && found_len == expected_len
            && matches!(**expected_element, Type::Const(_))
            && !matches!(**found_element, Type::Const(_))
            && found_element.unqualified() == expected_element.unqualified()
        {
            return true;
        }
        // A function name used as a value decays to a pointer to it, exactly as
        // an array decays to a pointer to its first element. `p *fn(int) int =
        // inc;` is therefore a conversion and not a mistake, and `&inc` yields
        // the same pointer by saying so.
        if let Type::Pointer { pointee, .. } = expected
            && matches!(found.unqualified(), Type::Function { .. })
            && found.unqualified() == pointee.unqualified()
        {
            return true;
        }
        // `&x` yields a `*T` and `*p` yields a `&T`: the two are the same
        // indirection with the same qualifiers, and the language spells one in
        // each direction. A pointer becomes a reference of the same shape, so
        // that `f(&x)` reaches `fn f(v &T)` and `return &a[i]` satisfies a
        // reference return type.
        if let (
            Type::Pointer {
                pointee,
                pointee_is_const,
                pointer_is_const,
            },
            Type::Reference {
                referent,
                referent_is_const,
                is_rvalue,
            },
        ) = (found, expected)
            && !pointer_is_const
            && !is_rvalue
            // A mutable pointer satisfies a reference to a constant too: the
            // promise "I will not write through this" is weaker than the one a
            // `*T` already keeps. The other direction is not a conversion --
            // a `*const T` cannot become a `&T`.
            && (*referent_is_const || !*pointee_is_const)
            && self.is_compatible(pointee, referent)
        {
            return true;
        }
        // A `T` satisfies a `&T`, because a reference *is* the place the value
        // lives and the expression that produced it already named that place.
        //
        // This is the whole of what a reference means. There is no operator
        // that "makes" a reference: `&` produces a *pointer* (`*T`), which is a
        // different type, and a program that wants a `&T` simply hands over the
        // `T`. So `return this._data[index];` in a function declared `&T` is
        // right, and `return &this._data[index];` is a pointer being converted
        // -- which happens to work here and obscures what is meant.
        //
        // It applies wherever a reference is wanted, not only at a parameter:
        // a call, a `return`, and an initialiser all name a place the same way.
        // The one place it must *not* apply is a binding that copies --
        // `T x = y` is still a copy -- which `copy_bindings` says.
        if self.in_parameter_position
            && let Some(_) = self.operand_fits_indirect(found, expected)
        {
            return true;
        }
        // `const` qualifies a *value*: it says the value may not be written,
        // not that it is a different kind of value. So a value satisfies a
        // `const` binding, and a `const` value reads as what it holds --
        // `x const int = 7;` works, and so does `n as int` where `n` is
        // `const int`. Dropping the qualifier *through* an indirection is a
        // different question, answered where pointer shapes are compared.
        if let Type::Const(inner) = found {
            return self.is_compatible(inner, expected);
        }
        if let Type::Const(inner) = expected {
            return self.is_compatible(found, inner);
        }
        // `volatile T` accepts a `T` and vice versa. The qualifier says how the
        // memory is accessed, not what value is stored, so an `int` is a valid
        // initialiser for a `volatile int` and reading one gives an `int`. What
        // it is *not* is silently dropped: a call taking `*int` does not accept
        // a `*volatile int`, because reading through it would be an ordinary
        // access to memory that requires a volatile one.
        match (found, expected) {
            (Type::Volatile(inner), _) => return self.is_compatible(inner, expected),
            (_, Type::Volatile(inner)) => return self.is_compatible(found, inner),
            _ => {}
        }
        // One type may be known by two names. A type declared in a module is
        // `String` there and `std.String` in a file that imported the umbrella,
        // and the two are the same type: the qualification records how this
        // file came to know it, not which type it is.
        //
        // Compared on the last name segment, which is what
        // `definitions_named` already does for definitions; leaving the types
        // out of step with that made an imported type's own constructor reject
        // the value it had just produced.
        if self.same_type_named_apart(found, expected) {
            return true;
        }
        // A reference reads as its referent, so it is usable wherever the
        // referent is. `*p` has type `&T` and `return *p;` in a function
        // returning `T` is an ordinary thing to write; what the reference adds
        // is that it names the storage, which matters for assignment rather
        // than for passing the value on.
        //
        // Not when a reference is what is *wanted*: two references are compared
        // at the level they name, so `&(*Array)` -- a reference to the pointer
        // variable -- must not satisfy `&Array`. Reading through the referent
        // there dropped one level of indirection, and a reference became a
        // window onto the wrong object.
        if !expected.is_reference()
            && let Some(referent) = found.referent()
            && self.is_compatible(referent, expected)
        {
            return true;
        }
        // An array decays to a pointer to its first element, which is what
        // lets `b"Hello"` — an `[N]const byte` — be passed where a
        // `*const byte` is wanted. C does the same, and for the same reason:
        // an array expression denotes its own address in every context.
        //
        // The element's `const` qualifier has to line up: an array of `const`
        // bytes may become a pointer to `const` bytes, never to writable ones.
        if let Type::Array { element, .. } = found
            && let Type::Pointer {
                pointee,
                pointee_is_const,
                pointer_is_const,
            } = expected
            && !pointer_is_const
        {
            let element_is_const = element.is_const_value();
            // Both sides are compared without their `const`, because the
            // qualifier is judged by the line below this one: what is being
            // asked is whether the *element* is the pointee, and whether the
            // array's elements are constant is a separate fact. Comparing the
            // stripped element against an unstripped pointee made the two never
            // equal for `"text"`, whose type is `[N]const c8` while the
            // parameter is `*const c8` -- so a string literal could not be
            // passed to a constructor taking a C string.
            let element_base = element.unqualified();
            if element_base == pointee.unqualified()
                && (element_is_const == *pointee_is_const
                    || (*pointee_is_const && !element_is_const))
            {
                return true;
            }
            // The pointee may itself be a pointer that needs const added.
            if is_const_widening(element, pointee) {
                return true;
            }
        }

        // A byte pointer converts to and from a pointer to anything without a
        // cast, because `*byte` is the untyped pointer: it is what raw memory
        // and the C library traffic in.
        if is_byte_pointer_conversion(found, expected) {
            return true;
        }

        // The one implicit conversion that is not an arithmetic one: adding
        // `const` to an indirection. `*T` may be used where `*const T` is
        // wanted, because reading through it is always safe.
        //
        // The reverse direction is deliberately *not* allowed: dropping
        // `const` would let a caller write through a pointer the program
        // promised not to write through, so it needs an explicit `as`.
        is_const_widening(found, expected)
    }

    /// The type a `.` reaches through `__deref__` or `__pointer__`.
    ///
    /// A pointer is not the only type that stands in front of another: a
    /// wrapper that defines `__deref__` names the same relationship, and `*w`
    /// already reaches what it holds. This asks the same question for the
    /// method-call spelling `w.m()`, which is the sugar for `w->m()`.
    /// The referent of a receiver, and the call that reaches it.
    ///
    /// `__deref__` is preferred to `__pointer__` because it is the method that
    /// says what the type *is*; `__pointer__` says what a member access
    /// follows. Both reach one type, and a type that declares one has one
    /// answer, so the order only matters for a type that declares both.
    fn indirect_receiver_call(
        &mut self,
        resolutions: &Resolutions,
        receiver: &Type,
    ) -> Option<(Type, String)> {
        for method in [
            fallc_ast::special::SpecialMethod::Deref,
            fallc_ast::special::SpecialMethod::Pointer,
        ] {
            if let Some(call) = self.special_method_call(resolutions, receiver, method.name(), 1) {
                let referent = call
                    .returns
                    .referent()
                    .cloned()
                    .unwrap_or_else(|| call.returns.clone());
                return Some((referent, call.symbol));
            }
        }
        None
    }

    fn dereferenced_receiver(
        &mut self,
        receiver: &Type,
        resolutions: &Resolutions,
    ) -> Option<Type> {
        for method in [
            fallc_ast::special::SpecialMethod::Deref,
            fallc_ast::special::SpecialMethod::Pointer,
        ] {
            if let Some(call) = self.special_method_call(resolutions, receiver, method.name(), 1) {
                if let Some(referent) = call.returns.referent() {
                    return Some(referent.clone());
                }
                return Some(call.returns);
            }
        }
        None
    }

    /// Reports that `found` is not usable where `expected` was required.
    pub(super) fn report_type_mismatch(&mut self, span: Span, found: &Type, expected: &Type) {
        self.diagnostics.push(
            Diagnostic::error(format!(
                "type mismatch: expected {}, found {}",
                self.describe(expected),
                self.describe(found)
            ))
            .with_code(DiagnosticCode::sema(10))
            .with_span(span),
        );
    }

    /// Renders `ty` for a diagnostic message.
    ///
    /// This is the canonical spelling, with aliases resolved: `int` prints as
    /// `i32` because that is what it is. An `Unspecified` type prints the source
    /// text it was written with, and pointers and references print the way they
    /// are written, so `*const T` and `const *T` do not look alike.
    #[must_use]
    pub fn describe(&self, ty: &Type) -> String {
        ty.spelling()
    }

    /// Records a diagnostic produced by the type checker.
    pub fn push_diagnostic(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    /// Returns the diagnostics accumulated so far.
    #[must_use]
    pub fn diagnostics(&self) -> &DiagnosticBag {
        &self.diagnostics
    }
}

/// The type one indirection reaches, with any `const` peeled.
///
/// `*ODevice` and `&ODevice` both reach `ODevice`; `**ODevice` reaches
/// `*ODevice`, which is not an interface. Exactly one level is peeled on
/// purpose: a reference to an interface reads the box at the address it holds,
/// and a pointer to a pointer would read a pointer as the box.
fn behind_one_indirection(ty: &Type) -> &Type {
    let mut ty = match ty {
        Type::Pointer { pointee, .. } => pointee.as_ref(),
        Type::Reference { referent, .. } => referent.as_ref(),
        other => other,
    };
    while let Type::Const(inner) = ty {
        ty = inner;
    }
    ty
}

/// The last segment of a type's written name, without its arguments.
///
/// `libdev.Device<Mode>` is `Device`, and so is `Device`: a type is the same
/// type however its module is spelled out in front of it, and whatever its
/// family's arguments are.
pub(super) fn short_name(name: &str) -> &str {
    let name = name.split('<').next().unwrap_or(name);
    name.rsplit('.').next().unwrap_or(name)
}

/// Whether a value of integer type `found` may be implicitly promoted to the
/// integer type `expected`.
///
/// The rule is value-range containment: every value of the source type must be
/// representable in the target. That allows unsigned-to-unsigned and
/// signed-to-signed widening, and unsigned-to-signed only when the target is
/// strictly wider (a signed type of the same width has one bit less of
/// magnitude). Signed to unsigned is never implicit, because the source can be
/// negative. `bool`, the character types, `byte` and nominal types are not
/// integers for this purpose.
#[must_use]
pub(crate) fn integer_promotes(found: &Type, expected: &Type) -> bool {
    let (Type::Int(from), Type::Int(to)) = (found.unqualified(), expected.unqualified()) else {
        return false;
    };
    let from_bits = from.bits();
    let to_bits = to.bits();
    if from.is_unsigned() {
        if to.is_unsigned() {
            from_bits <= to_bits
        } else {
            from_bits < to_bits
        }
    } else {
        // A signed source may only reach a signed target; reaching an unsigned
        // one would make negative values wrap, which is exactly what an
        // implicit conversion must never do.
        !to.is_unsigned() && from_bits <= to_bits
    }
}

impl TypeChecker {
    /// Answers `resume` or `result` on a coroutine, without the library.
    ///
    /// The frame's layout and the machine's states are the compiler's, so these
    /// two members are the compiler's too. Their shape is read off the
    /// coroutine's own three types, and nothing about it depends on a spelling
    /// in a library that two otherwise identical declarations would have had to
    /// share:
    ///
    /// - `Send` decides whether `resume` takes an argument;
    /// - `Yield` decides what `resume` answers -- `?Yield` when there is a value,
    ///   `void` when the `yield` hands nothing over;
    /// - `Return` is what `result` answers, and it is always `except`.
    ///
    /// A call is recorded against the span exactly as any other member's is, so
    /// the two stages agree about which coroutine it belongs to.
    fn coroutine_member(
        &mut self,
        coroutine: &CoroutineShape,
        receiver_ty: &Type,
        member: &str,
        node: &CallExpr,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        span: fallc_span::Span,
    ) -> Option<Type> {
        if member != "resume" && member != "result" {
            return None;
        }
        let send_is_void = coroutine.send.is_void();
        let yield_is_void = coroutine.yield_ty.is_void();
        // What the call may be written with, and what it answers.
        // What a `resume` answers is `?Yield`, named as the library's family
        // member so that it is the same type a program writes and the same one
        // whose fields `Optional` declares. A `Yield` of `void` has no optional
        // to answer with: the call returns nothing.
        // `?Yield` degrades to `bool` when `Yield` is `void`: `void` has no
        // representation, so `?void` cannot be written, and what is left of
        // "was there a value" is the bit itself. `true` is `Some(())` -- the
        // coroutine suspended again -- and `false` is `none`, that it finished.
        let answered = if yield_is_void {
            Type::BOOL
        } else {
            let written = fallc_ast::TypeSyntax::Named(fallc_ast::NamedType {
                path: "Optional".to_owned(),
                arguments: vec![fallc_ast::TemplateArgumentSyntax::Type(
                    fallc_ast::TypeSyntax::Named(fallc_ast::NamedType {
                        path: coroutine.yield_ty.spelling(),
                        arguments: Vec::new(),
                        is_expansion: false,
                        span: fallc_span::Span::SYNTHETIC,
                    }),
                )],
                is_expansion: false,
                span: fallc_span::Span::SYNTHETIC,
            });
            self.resolve_nested(&written)
        };
        let (takes_argument, answers) = match (member, send_is_void) {
            // A coroutine that takes a value in is resumed with it; one that
            // takes nothing in is resumed without.
            ("resume", false) => (true, answered),
            ("resume", true) => (false, answered),
            // `result` is a method of the coroutine, not of a value it holds.
            ("result", _) => (false, coroutine.return_ty.clone()),
            _ => return None,
        };
        // A call whose argument count is already wrong is reported in the
        // language's own terms: `resume()` on a coroutine that takes a value in
        // is a call missing its argument, and `resume(v)` on one that takes
        // nothing is a call with one too many.
        let expected_count = usize::from(takes_argument);
        if node.args.len() != expected_count {
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "`{member}` of this coroutine takes {expected_count} argument(s), but {} \
                     were given",
                    node.args.len()
                ))
                .with_code(DiagnosticCode::sema(36))
                .with_span(span)
                .with_note(if takes_argument {
                    "this coroutine takes a value in, so `resume` is written with it"
                } else {
                    "this coroutine takes nothing in, so `resume` is written without one"
                }),
            );
            return None;
        }
        // The signature code generation builds the function from: the receiver
        // first, then the value a `resume` hands in. A method's receiver is
        // always the object, so it is spelled as the reference every member
        // call takes.
        let mut params = vec![Type::Reference {
            referent: Box::new(crate::Type::unspecified(
                receiver_ty.spelling(),
                fallc_span::Span::SYNTHETIC,
            )),
            referent_is_const: false,
            is_rvalue: false,
        }];
        if takes_argument {
            // The value handed in is checked against `Send` exactly as any
            // argument is checked against its parameter.
            let argument = node.args[0];
            let previous = self.expected.replace(coroutine.send.clone());
            let _ = self.check_expr(ast, resolutions, types, argument, &coroutine.send);
            self.expected = previous;
            params.push(coroutine.send.clone());
        }
        // The symbol a coroutine's member is emitted under, built from the
        // coroutine's own type so that every call of the same coroutine -- from
        // any module -- names the same one.
        let receiver = crate::types::mangle_type(receiver_ty);
        let mut mangled = vec![receiver];
        if takes_argument && member == "resume" {
            mangled.push(crate::types::mangle_type(&coroutine.send));
        }
        let symbol = fallc_ast::special::symbol_for(member, &mangled);
        self.method_calls.insert(span, symbol.clone());
        // What the call takes and answers, for code generation: neither member
        // is declared anywhere, so a signature cannot be looked up by symbol.
        self.coroutine_calls
            .insert(span, (params.clone(), answers.clone()));
        // `result` may raise; `resume` never does.
        if member == "result" {
            self.throwing_calls.insert(span);
        }
        types.insert(node.callee, answers.clone());
        Some(answers)
    }
}
