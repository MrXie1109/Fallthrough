//! Passing arguments: placement, adoption, defaults, and the by-address
//! convention.
//!
//! The checker decided which argument fills which parameter; this places the
//! values, takes addresses where a parameter is indirect, and supplies the
//! defaults a call omitted.

use super::*;

impl<'ctx> Lowerer<'ctx> {
    /// Lowers a call to a named external symbol.
    ///
    /// The symbol is passed in rather than derived from the name at the call
    /// site, because the two differ: a name imported with `from C import
    /// printf` is `printf` in both, but one reached through a module alias is
    /// `Libc.printf` in source and `printf` at link time.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn lower_symbol_call(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        qualified: &str,
        symbol: &str,
        call: &CallExpr,
        span: fallc_span::Span,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        let callee = match self.backend.module().get_function(symbol) {
            Some(function) => function,
            None => self.declare_external(symbol, qualified, span)?,
        };

        // A named argument is placed by the parameter it names, exactly as it
        // is in a call of a function of this module.
        let mut slots = self.argument_slots(ast, types, builder, call, symbol, span, 0, true)?;
        self.fill_argument_defaults(ast, types, builder, symbol, &mut slots, span)?;
        let args: Vec<inkwell::values::BasicMetadataValueEnum<'static>> =
            slots.into_iter().flatten().collect();

        self.build_value_call(builder, callee, symbol, args, span)
    }

    /// Lowers one argument, decaying an array to a pointer to its first
    /// element when that is what the parameter wants.
    ///
    /// The type checker accepts that decay, so leaving it out here produces IR
    /// that type-checks and then passes an aggregate *by value* where a pointer
    /// is expected. `printf("%s", "abc")` then loads `[4 x i8]` into the
    /// variadic slot and the callee reads it as an address. That is a segfault
    /// with no diagnostic anywhere, which is exactly what it did.
    pub(super) fn lower_argument(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        arg: ExprId,
        span: fallc_span::Span,
    ) -> Result<inkwell::values::BasicMetadataValueEnum<'static>, CodegenError> {
        // A lossless integer promotion is a conversion even though the source
        // wrote no `as`. The checker recorded the target type; perform it here
        // rather than in every caller.
        if let Some(wanted) = self.adopted_types.get(&ast.expr(arg).span).cloned()
            && !self.aggregate_is_by_address(&wanted)
        {
            let value = self
                .lower_expr(ast, types, builder, arg)?
                .ok_or_else(|| CodegenError::waiting_for_spec("this argument", span))?;
            return Ok(self
                .convert_value(ast, types, builder, arg, value, &wanted, span)?
                .into());
        }
        let value = self.lower_argument_raw(ast, types, builder, arg, span)?;
        // A conversion of a value that fits a register is a call; a small
        // aggregate arrives as a struct value, which `basic_value_of` does not
        // cover because it is written for the call-argument paths that only
        // need scalars.
        let basic = match &value {
            inkwell::values::BasicMetadataValueEnum::StructValue(value) => {
                Some(value.as_basic_value_enum())
            }
            inkwell::values::BasicMetadataValueEnum::ArrayValue(value) => {
                Some(value.as_basic_value_enum())
            }
            other => Self::basic_value_of(other),
        };
        if let Some(basic) = basic {
            let converted =
                self.convert_implicit(ast, types, builder, arg, basic, span)?;
            return Ok(converted.into());
        }
        Ok(value)
    }

    /// Lowers one argument without applying a recorded conversion.
    pub(super) fn lower_argument_raw(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        arg: ExprId,
        span: fallc_span::Span,
    ) -> Result<inkwell::values::BasicMetadataValueEnum<'static>, CodegenError> {
        // A string literal that has to become the parameter's declared type is
        // constructed first: `f("text")` with `fn f(s String)` builds a
        // `String` and passes its address, which is what the by-value aggregate
        // path expects. This comes first because the paths below see the
        // literal's own array type, not the type the call wants.
        if self.lowering_literal != Some(ast.expr(arg).span)
            && let Some(symbol) = self
                .literal_constructions
                .get(&ast.expr(arg).span)
                .cloned()
        {
            let address =
                self.lower_literal_construction(ast, types, builder, arg, &symbol, span)?;
            // A parameter that takes the value -- `fn f(s String)` -- gets the
            // constructed value; one that takes an address -- `&const String` --
            // gets the storage. Which one is the call site's own record.
            let argument_span = ast.expr(arg).span;
            if self.by_address_arguments.contains(&argument_span) {
                return Ok(address.as_basic_value_enum().into());
            }
            let target = self
                .literal_construction_types
                .get(&argument_span)
                .cloned()
                .unwrap_or_else(|| fallc_sema::Type::unspecified("", span));
            let llvm = TypeMapper::new(self.backend.context())
                .map(&target)
                .ok_or_else(|| CodegenError::waiting_for_spec("this constructed value", span))?;
            let value = builder
                .build_load(llvm, address, "literal")
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
            return Ok(value.into());
        }
        // A value used as an interface is boxed here, and the box is what the
        // parameter receives. This has to come before the by-address path
        // below: that path sees the *concrete* type -- a buffer, an aggregate
        // -- and would pass the buffer's own storage, a different shape from
        // the box the callee reads.
        if self
            .interface_conversions
            .contains_key(&ast.expr(arg).span)
        {
            return Ok(self
                .lower_expr(ast, types, builder, arg)?
                .ok_or_else(|| CodegenError::waiting_for_spec("this argument", span))?
                .into());
        }
        // A by-value aggregate parameter is passed as an address too: the
        // callee copies into its own storage. This must happen before the
        // ordinary value path, which would otherwise load the aggregate and
        // pass an SSA value the signature does not accept.
        if types
            .get(arg)
            .is_some_and(|ty| self.aggregate_is_by_address(ty))
        {
            return Ok(
                self.lower_aggregate_source(ast, types, builder, arg, span)?
                    .into(),
            );
        }
        // A reference or pointer parameter takes the argument's address, which
        // the checker recorded. This is the same thing a method call does with
        // its receiver, and it is why `f(other)` reaches `fn f(x &T)`.
        //
        // The address is taken *after* the value is produced, not before: a
        // temporary the compiler made -- the result of `str(x)`, say -- only
        // becomes storage while it is being lowered, so asking for an address
        // first would find nothing.
        let address = self.by_address_arguments.contains(&ast.expr(arg).span);
        if address {
            let value = self
                .lower_expr(ast, types, builder, arg)?
                .ok_or_else(|| CodegenError::waiting_for_spec("this argument", span))?;
            // An expression that *names storage through a pointer* -- `*p`, a
            // variable of reference type -- already evaluates to the address.
            // Passing the address *of that pointer* would hand over a pointer
            // to a pointer, which is what made `result.push(other)` inside
            // `__add__` read a garbage length out of the parameter slot.
            //
            // The two are told apart by what the expression is: storage reached
            // through a pointer yields the address as its value, while a plain
            // value has no address until one is taken.
            // A *name* of pointer or reference type already evaluates to the
            // address it holds, so its value is what to pass. Anything else --
            // `*p`, a field, an index, a call -- is resolved through
            // `lower_lvalue`, which yields the storage the expression names.
            // Lowering `*p` as a value instead reads through the pointer, which
            // is the opposite of what an indirect parameter wants.
            if matches!(&ast.expr(arg).kind, ExprKind::Name(_))
                && types.get(arg).is_some_and(fallc_sema::Type::is_indirection)
            {
                return Ok(value.into());
            }
            // A call is *not* asked for its address again: resolving it as an
            // lvalue lowers it a second time, and the call would run twice --
            // which for `push(make())` means two arrays made and one leaked.
            // The value is already in hand, so it is given storage directly.
            if !matches!(&ast.expr(arg).kind, ExprKind::Call(_))
                && let Some(binding) = self.lower_lvalue(ast, types, builder, arg)
            {
                return Ok(binding.storage.into());
            }
            // A call returning an aggregate out of line already produced its
            // address; the parameter wants exactly that, not the address of a
            // slot holding the pointer.
            if types
                .get(arg)
                .is_some_and(|ty| self.aggregate_is_by_address(ty))
                && matches!(value, BasicValueEnum::PointerValue(_))
            {
                return Ok(value.into());
            }
            // Nothing named the storage -- the argument is a value the compiler
            // produced, such as the result of a call. The parameter wants an
            // address, so the value is given one: a temporary of this frame,
            // which lives as long as the call needs it.
            let storage = self.store_temporary(builder, value, "argument")?;
            return Ok(storage.into());
        }
        let value = self
            .lower_expr(ast, types, builder, arg)?
            .ok_or_else(|| CodegenError::waiting_for_spec("this argument", span))?;
        // A call returning an aggregate out of line yields the slot's address,
        // which is what an aggregate parameter (or a reference to one) takes.
        // There is no aggregate value to pass instead.
        if types
            .get(arg)
            .is_some_and(|ty| self.aggregate_is_by_address(ty))
            && matches!(value, BasicValueEnum::PointerValue(_))
        {
            return Ok(value.into());
        }
        // An array argument is passed as its address. An array *value* in LLVM
        // is an aggregate, and every parameter that accepts one in this
        // language accepts a pointer instead.
        if matches!(value, BasicValueEnum::ArrayValue(_)) {
            let storage = self.lower_lvalue(ast, types, builder, arg);
            if let Some(binding) = storage {
                return Ok(binding.storage.into());
            }
        }
        Ok(value.into())
    }

    /// Lowers an argument, converting it to the type its parameter gave it.
    ///
    /// An unsuffixed integer literal adopts its parameter's type, and the
    /// checker records what that was. Lowering the literal as it stands passes
    /// the default `int` to a parameter declared `usize`, which LLVM rejects --
    /// and it is a real difference in width, not a formality.
    pub(super) fn lower_adopted_argument(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        argument: ExprId,
        span: fallc_span::Span,
    ) -> Result<inkwell::values::BasicMetadataValueEnum<'static>, CodegenError> {
        // With nothing to convert, the ordinary argument path applies -- and it
        // does more than lower a value: an array argument is passed as its
        // address. Lowering the value directly here passed the array by value,
        // which is what the decay rule exists to prevent.
        let Some(wanted) = self.adopted_types.get(&ast.expr(argument).span).cloned() else {
            return self.lower_argument(ast, types, builder, argument, span);
        };
        let value = self
            .lower_expr(ast, types, builder, argument)?
            .ok_or_else(|| CodegenError::waiting_for_spec("this argument", span))?;
        Ok(self
            .convert_value(ast, types, builder, argument, value, &wanted, span)?
            .into())
    }

    /// Lowers a call's arguments, supplying any the call omitted.
    ///
    /// A default is evaluated **at the call site**, as in C++, and then type
    /// checked as usual by the checker, which has already done so.
    pub(super) fn lower_arguments(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        call: &CallExpr,
        symbol: &str,
        span: fallc_span::Span,
    ) -> Result<Vec<inkwell::values::BasicMetadataValueEnum<'static>>, CodegenError> {
        let mut slots = self.argument_slots(ast, types, builder, call, symbol, span, 0, false)?;
        self.fill_argument_defaults(ast, types, builder, symbol, &mut slots, span)?;
        Ok(slots.into_iter().flatten().collect())
    }

    /// Places a call's written arguments in the parameters they fill.
    ///
    /// A named argument goes to the parameter its name says, so the list the
    /// callee receives is in parameter order however the call was written. A
    /// slot stays empty when the argument was left out, which is what a default
    /// fills; a slot past the declared parameters is a variadic argument and is
    /// kept as it was.
    ///
    /// `taken` is the number of leading parameters the caller already supplied:
    /// one for a method call, whose receiver is written before the dot. When
    /// `adopted` is set an argument is converted to the type its parameter gave
    /// it, which is what a method call does with an unsuffixed literal.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn argument_slots(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        call: &CallExpr,
        symbol: &str,
        span: fallc_span::Span,
        taken: usize,
        adopted: bool,
    ) -> Result<Vec<Option<inkwell::values::BasicMetadataValueEnum<'static>>>, CodegenError> {
        let mut names = self
            .function_parameters
            .get(symbol)
            .cloned()
            .unwrap_or_default();
        // A function of another module, or an instantiation emitted there, has
        // its parameters recorded with its external signature rather than in
        // this module's table.
        if names.is_empty()
            && let Some(signature) = self
                .external_symbols
                .values()
                .find(|signature| signature.symbol == symbol)
        {
            names = signature
                .param_names
                .iter()
                .cloned()
                .zip(signature.params.iter().cloned())
                .collect();
        }
        let mut slots: Vec<Option<inkwell::values::BasicMetadataValueEnum<'static>>> =
            vec![None; names.len().max(call.args.len() + taken)];
        for (index, &arg) in call.args.iter().enumerate() {
            let value = if adopted {
                self.lower_adopted_argument(ast, types, builder, arg, span)?
            } else {
                self.lower_argument(ast, types, builder, arg, span)?
            };
            slots[index + taken] = Some(value);
        }
        for named in &call.named {
            let Some(index) = names.iter().position(|(name, _)| name == &named.name) else {
                return Err(CodegenError::Emit(format!(
                    "internal error: `{symbol}` has no parameter `{}`",
                    named.name
                )));
            };
            let value = if adopted {
                self.lower_adopted_argument(ast, types, builder, named.value, span)?
            } else {
                self.lower_argument(ast, types, builder, named.value, span)?
            };
            slots[index] = Some(value);
        }
        Ok(slots)
    }

    /// Fills the default of every parameter a call left without a value.
    ///
    /// A default is an expression written against the function's own
    /// parameters -- `fn add(a int, b int = a + 1)` -- and it is evaluated at
    /// the call site. The arguments given are therefore bound under their
    /// parameter names while it is lowered, in a scope of this call's own.
    pub(super) fn fill_argument_defaults(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        symbol: &str,
        slots: &mut Vec<Option<inkwell::values::BasicMetadataValueEnum<'static>>>,
        span: fallc_span::Span,
    ) -> Result<(), CodegenError> {
        // The call may hand over fewer slots than the callee has parameters --
        // the omitted ones are exactly what this fills -- so the length comes
        // from the function itself, which is declared by the time a call is
        // emitted.
        if let Some(function) = self.backend.module().get_function(symbol) {
            let declared = function.count_params() as usize;
            if slots.len() < declared {
                slots.resize(declared, None);
            }
        }
        // A call in another module, or one to an instantiated template, has no
        // expression this tree could lower: the checker folded the default
        // where it was written and recorded the value with the parameter's
        // type, and that is what fills the slot here.
        if let Some(values) = self.default_argument_values.get(&span).cloned() {
            for (index, number, ty) in values {
                if slots.get(index).is_some_and(Option::is_some) {
                    continue;
                }
                let value = self
                    .lower_template_value(
                        ast,
                        Some(ty),
                        builder,
                        &fallc_sema::TemplateArgument::Int(number),
                        span,
                    )?
                    .ok_or_else(|| {
                        CodegenError::waiting_for_spec("this default argument", span)
                    })?;
                // A call to a template whose signature is not in this unit's
                // tables may hand over fewer slots than there are parameters:
                // the omitted ones are what is being filled.
                if let Some(slot) = slots.get_mut(index) {
                    *slot = Some(value.into());
                }
            }
        }
        // A default that is an expression was written in another module's
        // tree, and is lowered with that tree and those tables: `b int = a + 1`
        // is written against `a`, whose value is one of this call's arguments.
        if let Some(expressions) = self.default_expressions.get(&span).cloned() {
            for (index, expr, module, preceding) in expressions {
                if slots.get(index).is_some_and(Option::is_some) {
                    continue;
                }
                let Some(tree) = self.default_trees.get(&module).cloned() else {
                    continue;
                };
                let saved = self.install(&tree.resolutions);
                self.binding_scopes.push(HashMap::new());
                for (position, (name, ty)) in preceding.iter().enumerate() {
                    let given = slots
                        .get(position)
                        .and_then(Option::as_ref)
                        .and_then(Self::basic_value_of);
                    if let Some(value) = given {
                        self.bind_default_parameter(builder, name, ty, value);
                    }
                }
                let value = self.lower_expr(&tree.ast, &tree.types, builder, expr)?;
                self.binding_scopes.pop();
                self.install(&saved);
                if let Some(value) = value
                    && let Some(slot) = slots.get_mut(index)
                {
                    *slot = Some(value.into());
                }
            }
        }
        let Some(defaults) = self.function_defaults.get(symbol).cloned() else {
            return Ok(());
        };
        let names = self
            .function_parameters
            .get(symbol)
            .cloned()
            .unwrap_or_default();
        self.binding_scopes.push(HashMap::new());
        for (index, (name, ty)) in names.iter().enumerate() {
            let given = slots.get(index).and_then(Option::as_ref);
            if let Some(value) = given.and_then(Self::basic_value_of) {
                self.bind_default_parameter(builder, name, ty, value);
            }
        }
        for (index, default) in defaults.iter().enumerate() {
            if slots.get(index).is_some_and(Option::is_some) {
                continue;
            }
            let Some(default) = default else {
                return Err(CodegenError::Emit(format!(
                    "internal error: `{symbol}` needs argument {} but has no default",
                    index + 1
                )));
            };
            let value = self
                .lower_expr(ast, types, builder, *default)?
                .ok_or_else(|| CodegenError::waiting_for_spec("this default argument", span))?;
            // The default just computed is itself an argument for the ones
            // after it: `fn f(a int, b int = 1, c int = a + b)`.
            if let Some((name, ty)) = names.get(index) {
                self.bind_default_parameter(builder, name, ty, value);
            }
            if let Some(slot) = slots.get_mut(index) {
                *slot = Some(value.into());
            }
        }
        self.binding_scopes.pop();
        Ok(())
    }

    /// Returns the basic value inside a call argument, if it is one.
    pub(super) fn basic_value_of(
        argument: &inkwell::values::BasicMetadataValueEnum<'static>,
    ) -> Option<BasicValueEnum<'static>> {
        use inkwell::values::BasicMetadataValueEnum as Metadata;
        match argument {
            Metadata::IntValue(value) => Some(value.as_basic_value_enum()),
            Metadata::FloatValue(value) => Some(value.as_basic_value_enum()),
            Metadata::PointerValue(value) => Some(value.as_basic_value_enum()),
            _ => None,
        }
    }

    /// Binds one call argument under its parameter's name, for a default.
    ///
    /// The storage is the call's own and disappears when the default is done.
    /// An aggregate is skipped: the call site holds its address, while the
    /// parameter names a copy, and a default that mentions one would be reading
    /// a value this stage does not have.
    pub(super) fn bind_default_parameter(
        &mut self,
        builder: &Builder<'static>,
        name: &str,
        ty: &fallc_sema::Type,
        value: BasicValueEnum<'static>,
    ) {
        if self.aggregate_is_by_address(ty) {
            return;
        }
        let Some(llvm_ty) = TypeMapper::new(self.backend.context()).map(ty) else {
            return;
        };
        let Some(entry) = self
            .current_function
            .and_then(|function| function.get_first_basic_block())
        else {
            return;
        };
        let storage = self.create_entry_alloca(builder, entry, name, llvm_ty);
        if builder.build_store(storage, value).is_err() {
            return;
        }
        self.register_binding(name, ty, storage, llvm_ty);
    }

    /// Chooses which overload's symbol a call refers to.
    ///
    /// The arguments' types select it, by the same rule the checker used: the
    /// parameter types must match exactly. The checker has already rejected an
    /// ambiguous call, so the first exact match is the one it meant.
    pub(super) fn overload_for(&self, name: &str, call: &CallExpr, types: &TypeTable) -> Option<String> {
        // Every function's symbol carries its parameter types, so a call is
        // resolved by building that symbol from the arguments. The alternatives
        // cover the cases where the argument's recorded type is not exactly the
        // parameter's: an unsuffixed integer literal adopts its parameter's
        // type, so its recorded type may be the default `int` while the
        // parameter is narrower.
        let argument_types: Vec<Type> = call
            .args
            .iter()
            .map(|arg| types.get(*arg).cloned())
            .collect::<Option<Vec<_>>>()?;

        let exact: Vec<String> = argument_types.iter().map(fallc_sema::mangle_type).collect();
        let candidate = fallc_ast::special::symbol_for(name, &exact);
        if self.backend.module().get_function(&candidate).is_some() {
            return Some(candidate);
        }

        // A literal may have been accepted for a parameter of a different type,
        // so any emitted symbol with the same name and argument count is tried.
        let prefix = format!("{name}_");
        let wanted = argument_types.len();
        self.emitted_symbols
            .iter()
            .find(|symbol| {
                symbol.starts_with(&prefix)
                    && symbol.matches('_').count() >= wanted
                    && self.backend.module().get_function(symbol).is_some()
            })
            .cloned()
    }

    /// Finds the symbol a method call refers to.
    ///
    /// The receiver's const qualifiers are part of the method's signature, so
    /// the candidates are tried in order of how specific they are and the first
    /// that exists wins: `*const T` first, because a method taking a const
    /// receiver accepts more, then `*T`.
    ///
    /// This is a small search rather than a derivation because the object's own
    /// type does not carry the receiver's qualifiers -- a `Point` value can be
    /// passed to either -- and inventing them would silently pick one of two
    /// defined methods.
    /// Returns the symbol a method call was resolved to.
    ///
    /// The checker recorded it, because only the checker knows which definition
    /// the call selected. This replaces a search that rebuilt the symbol from
    /// the receiver and argument types and then looked through every known
    /// symbol for a match -- a second implementation of the resolution rule,
    /// and one that could not tell two types' same-named methods apart.
    pub(super) fn method_symbol(&self, span: Span) -> Option<String> {
        self.method_calls.get(&span).cloned()
    }

}
