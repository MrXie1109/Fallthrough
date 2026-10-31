//! Returns, raises, and the shape of a function that may raise.
//!
//! A `??` function returns a result struct instead of a value; a `raise` and a
//! `try` are the operations on it. Getting a value out of one is a branch, so
//! every operation on the shape lives here.

use super::*;

impl<'ctx> Lowerer<'ctx> {
    /// Lowers `return expr;` or `return;`.
    pub(super) fn lower_return(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        bindings: &BindingTypes,
        builder: &Builder<'static>,
        node: &ReturnStmt,
    ) -> Result<(), CodegenError> {
        // Inside a `@coroutine` body, `return` does not leave the machine: it
        // finishes the coroutine with the value, which the resumer reads
        // afterwards.
        if self.coroutine.is_some() {
            return self.lower_coroutine_return(ast, types, bindings, builder, node);
        }
        match node.value {
            Some(value) => {
                // `return expr;` may construct the result: the checker records
                // the constructor it chose at the returned expression's span,
                // exactly as it does for a binding's initialiser. The value is
                // built in a temporary and that temporary is what is returned,
                // so `return 300.0;` in a function returning a type with an
                // `__im_init__` calls it rather than returning the argument.
                if let Some(symbol) = self.init_calls.get(&initialiser_span(ast, value)).cloned() {
                    let Some(return_ty) = self.current_return_type.clone() else {
                        return Err(CodegenError::Emit(
                            "internal error: a `return` outside a function".to_owned(),
                        ));
                    };
                    let mapper = TypeMapper::new(self.backend.context());
                    let Some(llvm_ty) = mapper.map(&return_ty) else {
                        return Err(CodegenError::Emit(
                            "internal error: the return type has no LLVM form".to_owned(),
                        ));
                    };
                    let function = self.current_function.expect("a function is being lowered");
                    // The temporary lives in the entry block, like every other
                    // value this stage allocates.
                    let entry = function
                        .get_first_basic_block()
                        .expect("a function being lowered has a body");
                    let slot = self.create_entry_alloca(builder, entry, "return.value", llvm_ty);
                    let callee = match self.backend.module().get_function(&symbol) {
                        Some(callee) => callee,
                        None => {
                            let qualified = self
                                .external_symbols
                                .iter()
                                .find(|(_, signature)| signature.symbol == symbol)
                                .map(|(qualified, _)| qualified.clone());
                            match qualified {
                                Some(qualified) => self.declare_external(
                                    &symbol,
                                    &qualified,
                                    ast.expr(value).span,
                                )?,
                                None => {
                                    return Err(CodegenError::Emit(format!(
                                        "internal error: `{symbol}` was resolved as a constructor \
                                         but not declared"
                                    )));
                                }
                            }
                        }
                    };
                    // The propagation marker, if any, belongs to the
                    // construction: the argument is the value under it.
                    let argument = match &ast.expr(value).kind {
                        ExprKind::Propagate(inner) => inner.value,
                        _ => value,
                    };
                    let argument =
                        self.lower_argument(ast, types, builder, argument, ast.expr(value).span)?;
                    let call = builder
                        .build_call(callee, &[slot.into(), argument], "")
                        .map_err(|error| CodegenError::Emit(error.to_string()))?;
                    let _ =
                        self.unwrap_throwing(ast, types, builder, ast.expr(value).span, call)?;
                    let returned = builder
                        .build_load(llvm_ty, slot, "returned")
                        .map_err(|error| CodegenError::Emit(error.to_string()))?;
                    let moved_out = None;
                    self.emit_all_drops(ast, types, bindings, builder, moved_out)?;
                    return self.build_function_return(builder, Some(returned));
                }
                // A function declared to return `&T` returns a *place*, so what
                // is returned is the address the expression names -- not the
                // value loaded out of it. The checker allowed a plain `T`
                // expression there because a reference is satisfied by the
                // place a value lives; turning that into an address is this
                // stage's job, and `&` is nowhere involved.
                let returns_reference = self
                    .current_return_type
                    .as_ref()
                    .is_some_and(Type::is_reference);
                // An expression that is *already* a reference names a place by
                // being one: its value is the address to return. Taking its
                // lvalue would return the address of the slot that holds the
                // reference -- one indirection too many, which made
                // `return value;` in `std.move` hand back a stack address.
                let value_is_reference = types.get(value).is_some_and(Type::is_reference);
                // An aggregate returned through the hidden slot is copied from
                // its source's address. Loading it first is what produced the
                // `load [256 x [256 x double]]` that LLVM cannot select.
                let returns_out_of_line = !returns_reference
                    && self
                        .current_return_type
                        .as_ref()
                        .is_some_and(|ty| self.aggregate_is_by_address(ty));
                let lowered = if returns_out_of_line {
                    Some(
                        self.lower_aggregate_source(
                            ast,
                            types,
                            builder,
                            value,
                            ast.expr(value).span,
                        )?
                        .as_basic_value_enum(),
                    )
                // A pointer *is* an address, like a reference: its value is
                // what to return, and taking its lvalue would hand back the
                // slot that holds the pointer. `return this.p;` in a function
                // returning `&Inner`, where `p` is `*Inner`, returned the
                // address of the field instead of what the field points at.
                } else if returns_reference
                    && !value_is_reference
                    && !types.get(value).is_some_and(Type::is_pointer)
                {
                    match self.lower_lvalue(ast, types, builder, value) {
                        Some(binding) => Some(binding.storage.as_basic_value_enum()),
                        // Not a place: the checker let it through only when it
                        // named storage, so there is nothing to take.
                        None => self.lower_expr(ast, types, builder, value)?,
                    }
                } else {
                    self.lower_expr(ast, types, builder, value)?
                };
                match lowered {
                    Some(returned) => {
                        // A returned *variable* is moved out, so it is not
                        // destroyed here. Any other expression -- a call, a
                        // temporary -- is not a binding and has nothing to
                        // skip.
                        // A lossless integer promotion is a conversion the
                        // checker recorded but the source never wrote.
                        let returned = self.convert_adopted(
                            ast,
                            types,
                            builder,
                            value,
                            returned,
                            ast.expr(value).span,
                        )?;
                        let returned = self.convert_implicit(
                            ast,
                            types,
                            builder,
                            value,
                            returned,
                            ast.expr(value).span,
                        )?;
                        let moved_out = match &ast.expr(value).kind {
                            // A reference is borrowed, not owned, so returning
                            // one moves nothing out of the thing it names.
                            ExprKind::Name(_) if !value_is_reference => self
                                .lower_lvalue(ast, types, builder, value)
                                .map(|binding| binding.storage),
                            _ => None,
                        };
                        // The `finally` blocks run after the value is computed,
                        // so a `return` expression that itself propagates an
                        // exception leaves through the escape path -- which runs
                        // them -- rather than running them here and again there.
                        let finally_blocks: Vec<fallc_ast::BlockId> = self
                            .exc_frames
                            .iter()
                            .filter_map(|frame| match frame {
                                ExcFrame::Finally(block) => Some(*block),
                                ExcFrame::Handler { .. } => None,
                            })
                            .collect();
                        for block in finally_blocks.into_iter().rev() {
                            if Self::is_terminated(builder) {
                                return Ok(());
                            }
                            self.lower_block(ast, types, bindings, builder, block)?;
                        }
                        if Self::is_terminated(builder) {
                            return Ok(());
                        }
                        self.emit_all_drops(ast, types, bindings, builder, moved_out)?;
                        self.build_function_return(builder, Some(returned))
                    }
                    None => Err(CodegenError::waiting_for_spec(
                        "this return value",
                        ast.expr(value).span,
                    )),
                }
            }
            None => {
                let finally_blocks: Vec<fallc_ast::BlockId> = self
                    .exc_frames
                    .iter()
                    .filter_map(|frame| match frame {
                        ExcFrame::Finally(block) => Some(*block),
                        ExcFrame::Handler { .. } => None,
                    })
                    .collect();
                for block in finally_blocks.into_iter().rev() {
                    if Self::is_terminated(builder) {
                        return Ok(());
                    }
                    self.lower_block(ast, types, bindings, builder, block)?;
                }
                if Self::is_terminated(builder) {
                    return Ok(());
                }
                // Nothing is leaving by value, so every live binding dies here.
                self.emit_all_drops(ast, types, bindings, builder, None)?;
                self.build_function_return(builder, None)
            }
        }
    }

    // ------------------------------------------------------------- exceptions

    /// Returns `std.Exception`'s LLVM type.
    ///
    /// Found by its declared name: that name is the only thing tying the value
    /// code generation builds to the type a handler reads.
    pub(super) fn exception_type(&mut self) -> Option<inkwell::types::BasicTypeEnum<'static>> {
        if let Some(ty) = self.exc_type {
            return Some(ty);
        }
        let nominal = self
            .nominal_types
            .iter()
            .find(|(name, _)| name.rsplit('.').next() == Some("Exception"))
            .map(|(_, nominal)| nominal.clone())
            .or_else(|| {
                self.module_declarations
                    .iter()
                    .find(|nominal| nominal.name.rsplit('.').next() == Some("Exception"))
                    .cloned()
            })?;
        let ty =
            TypeMapper::new(self.backend.context()).map(&fallc_sema::Type::Nominal(nominal))?;
        self.exc_type = Some(ty);
        Some(ty)
    }

    /// Returns the type a throwing function returns: `{ ok, value, error }`.
    ///
    /// The value is left out for a function that returns nothing. Exceptions are
    /// carried in the result rather than in global state, so a call site gets
    /// its answer in the value it already has to look at.
    pub(super) fn throwing_result_type(
        &mut self,
        value: Option<inkwell::types::BasicTypeEnum<'static>>,
    ) -> Option<inkwell::types::BasicTypeEnum<'static>> {
        let context = self.backend.context();
        let error = self.exception_type()?;
        let mut fields: Vec<inkwell::types::BasicTypeEnum<'static>> =
            vec![context.bool_type().into()];
        if let Some(value) = value {
            fields.push(value);
        }
        fields.push(error);
        Some(context.struct_type(&fields, false).into())
    }

    /// Writes the function's result into the slot it returns from.
    pub(super) fn store_result(
        &mut self,
        builder: &Builder<'static>,
        ok: BasicValueEnum<'static>,
        value: Option<BasicValueEnum<'static>>,
        error: Option<BasicValueEnum<'static>>,
    ) -> Result<(), CodegenError> {
        let Some(slot) = self.exc_result else {
            return Err(CodegenError::Emit(
                "internal error: an escape from a function that cannot raise".to_owned(),
            ));
        };
        // A coroutine's escape slot *is* the exception: the frame keeps the
        // `Exception` itself, not a tagged result with a field for it. Writing
        // through the tagged layout here stored the whole exception into its
        // own first field, which ran past the slot and over the frame's other
        // storage.
        if self.coroutine.is_some() {
            builder
                .build_store(slot, ok)
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
            if let Some(error) = error {
                builder
                    .build_store(slot, error)
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
            }
            return Ok(());
        }
        // The *actual* result type: a function that returns a value has three
        // fields, and one that does not has two. Using a type without the value
        // put the tag and the error at the offsets of the two-field shape, which
        // wrote the error over the value and read back whatever the slot
        // happened to hold.
        let ty = self
            .exc_result_type
            .ok_or_else(|| CodegenError::Emit("internal error: no result type".to_owned()))?;
        let tag = build_field_gep(builder, ty, slot, 0)?;
        builder
            .build_store(tag, ok)
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        if let Some(value) = value {
            let field = build_field_gep(builder, ty, slot, 1)?;
            // An aggregate value is handed over as its *address* -- an array has
            // no SSA form -- and the result struct holds the bytes, not a
            // pointer to them. Storing the pointer would leave the field
            // reading whatever the address happened to be.
            let field_ty = ty.into_struct_type().get_field_types()[1];
            if let BasicValueEnum::PointerValue(source) = value
                && !matches!(field_ty, inkwell::types::BasicTypeEnum::PointerType(_))
            {
                let size = field_ty.size_of().ok_or_else(|| {
                    CodegenError::Emit("internal error: a result value of unknown size".to_owned())
                })?;
                builder
                    .build_memcpy(field, 1, source, 1, size)
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
            } else {
                builder
                    .build_store(field, value)
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
            }
        }
        if let Some(error) = error {
            let count = ty.into_struct_type().count_fields();
            let field = build_field_gep(builder, ty, slot, count - 1)?;
            builder
                .build_store(field, error)
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
        }
        Ok(())
    }

    /// Returns the loaded result, for the `ret` an escape ends with.
    pub(super) fn load_result(
        &self,
        builder: &Builder<'static>,
    ) -> Result<BasicValueEnum<'static>, CodegenError> {
        let Some(slot) = self.exc_result else {
            return Err(CodegenError::Emit(
                "internal error: an escape from a function that cannot raise".to_owned(),
            ));
        };
        let ty = self
            .exc_type
            .ok_or_else(|| CodegenError::Emit("internal error: no exception type".to_owned()))?;
        let _ = ty;
        // The slot's own pointee is the result struct, which LLVM knows.
        builder
            .build_load(
                self.exc_result_type.ok_or_else(|| {
                    CodegenError::Emit("internal error: no result type".to_owned())
                })?,
                slot,
                "result",
            )
            .map_err(|error| CodegenError::Emit(error.to_string()))
    }

    /// Returns the pointer `std.exception()` reads.
    ///
    /// A handler stores the exception it caught there; that is the only state
    /// the runtime has, and it is written once per handler.
    /// Reports a `!!` exception and stops the program.
    ///
    /// It is the report a program makes for an exception nothing caught --
    /// the same two texts, read the same way -- with `panic:` where that says
    /// `uncaught exception:`. `diagnostic` says nothing here; it is kept for the
    /// caller's error messages.
    ///
    /// The reading is the delicate part and is copied from the uncaught path on
    /// purpose: a `String` may keep its characters inside itself, so the field
    /// is stored and `data()` is called on it, rather than the field being used
    /// as the address of the text.
    pub(super) fn emit_abort_report(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        error: BasicValueEnum<'static>,
    ) -> Result<(), CodegenError> {
        let _ = (ast, types);
        let context = self.backend.context();
        let module = self.backend.module();
        let pointer = context.ptr_type(inkwell::AddressSpace::default());
        let i32_type = context.i32_type();
        let function = builder
            .get_insert_block()
            .and_then(|block| block.get_parent())
            .ok_or_else(|| CodegenError::Emit("internal error: no function".to_owned()))?;

        let stderr = match module.get_global("stderr") {
            Some(global) => global,
            None => {
                let global = module.add_global(pointer, None, "stderr");
                global.set_linkage(inkwell::module::Linkage::External);
                global
            }
        };
        let fprintf = match module.get_function("fprintf") {
            Some(function) => function,
            None => module.add_function(
                "fprintf",
                i32_type.fn_type(&[pointer.into(), pointer.into()], true),
                None,
            ),
        };
        fn private_text(
            module: &inkwell::module::Module<'static>,
            text: &str,
            name: &str,
        ) -> inkwell::values::PointerValue<'static> {
            let bytes = module.get_context().const_string(text.as_bytes(), true);
            let global = module.add_global(bytes.get_type(), None, name);
            global.set_initializer(&bytes);
            global.set_constant(true);
            global.set_linkage(inkwell::module::Linkage::Private);
            global.as_pointer_value()
        }
        let format = private_text(module, "panic: %s", ".abort.format");
        let stream = builder
            .build_load(pointer, stderr.as_pointer_value(), "stderr")
            .map_err(|error| CodegenError::Emit(error.to_string()))?
            .into_pointer_value();

        // Each of the exception's two `String`s: stored, then `data()` on it.
        let empty = private_text(module, "", ".abort.empty");
        let mut texts: Vec<BasicValueEnum<'static>> = Vec::new();
        if let BasicValueEnum::StructValue(aggregate) = error {
            for index in 0..2 {
                let text = (|| -> Option<BasicValueEnum<'static>> {
                    let field = builder.build_extract_value(aggregate, index, "text").ok()?;
                    let slot = builder.build_alloca(field.get_type(), "held").ok()?;
                    builder.build_store(slot, field).ok()?;
                    let mut string = self
                        .nominal_types
                        .iter()
                        .find(|(name, _)| name.rsplit('.').next() == Some("String"))
                        .map(|(_, nominal)| nominal.clone())?;
                    if let Some(bare) = string.name.rsplit('.').next() {
                        string.name = bare.to_owned();
                    }
                    let receiver = fallc_sema::Type::Reference {
                        referent: Box::new(fallc_sema::Type::Nominal(string)),
                        referent_is_const: true,
                        is_rvalue: false,
                    };
                    let data = self
                        .std_function(
                            "data",
                            std::slice::from_ref(&receiver),
                            fallc_span::Span::SYNTHETIC,
                        )
                        .ok()?;
                    let call = builder
                        .build_call(data, &[slot.as_basic_value_enum().into()], "text")
                        .ok()?;
                    match call.try_as_basic_value() {
                        inkwell::values::ValueKind::Basic(value) => Some(value),
                        _ => None,
                    }
                })()
                .unwrap_or_else(|| empty.as_basic_value_enum());
                texts.push(text);
            }
        }
        let mut arguments: Vec<inkwell::values::BasicMetadataValueEnum<'static>> =
            vec![stream.into(), format.into()];
        if let Some(name) = texts.first() {
            arguments.push((*name).into());
        }
        let _ = builder.build_call(fprintf, &arguments, "panic");
        if let Some(BasicValueEnum::PointerValue(message)) = texts.get(1).copied() {
            let first = builder
                .build_load(context.i8_type(), message, "first")
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
            let has_text = builder
                .build_int_compare(
                    inkwell::IntPredicate::NE,
                    first.into_int_value(),
                    context.i8_type().const_zero(),
                    "has.message",
                )
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
            let with_message = context.append_basic_block(function, "panic.message");
            let after = context.append_basic_block(function, "panic.after");
            builder
                .build_conditional_branch(has_text, with_message, after)
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
            builder.position_at_end(with_message);
            // `stderr` is loaded again here: the value loaded in the entry
            // block does not reach this one, and a stale pointer made the
            // separator write through a stream that was never a stream.
            let stream = builder
                .build_load(pointer, stderr.as_pointer_value(), "stderr.message")
                .map_err(|error| CodegenError::Emit(error.to_string()))?
                .into_pointer_value();
            let separator = private_text(module, ": %s", ".abort.message");
            let _ = builder.build_call(
                fprintf,
                &[stream.into(), separator.into(), message.into()],
                "panic.message",
            );
            let _ = builder.build_unconditional_branch(after);
            builder.position_at_end(after);
        }
        let stream = builder
            .build_load(pointer, stderr.as_pointer_value(), "stderr.end")
            .map_err(|error| CodegenError::Emit(error.to_string()))?
            .into_pointer_value();
        let newline = private_text(module, "\n", ".abort.end");
        let _ = builder.build_call(fprintf, &[stream.into(), newline.into()], "panic.end");
        let abort = self.declare_libc("abort", &[], None);
        builder
            .build_call(abort, &[], "")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        builder
            .build_unreachable()
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        Ok(())
    }

    pub(super) fn handled_global(&mut self) -> inkwell::values::PointerValue<'static> {
        if let Some(global) = self.handled_global {
            return global;
        }
        let context = self.backend.context();
        let module = self.backend.module();
        // The storage's name differs from the accessor's: LLVM keeps one symbol
        // table for globals and functions, so two names in one would make the
        // second one `__ft_handled.1` and nothing could find it.
        let global = module.add_global(
            context.ptr_type(inkwell::AddressSpace::default()),
            None,
            "__ft_handled_exception",
        );
        global.set_initializer(
            &context
                .ptr_type(inkwell::AddressSpace::default())
                .const_null(),
        );
        global.set_linkage(inkwell::module::Linkage::WeakAny);
        let pointer = global.as_pointer_value();
        self.handled_global = Some(pointer);
        // The accessor `std.exception()` calls. Defined next to the global so
        // the two cannot drift apart.
        let accessor = module.add_function(
            "__ft_handled",
            context
                .ptr_type(inkwell::AddressSpace::default())
                .fn_type(&[], false),
            Some(inkwell::module::Linkage::WeakAny),
        );
        let entry = context.append_basic_block(accessor, "entry");
        let builder = context.create_builder();
        builder.position_at_end(entry);
        if let Ok(pointer) = builder.build_load(
            context.ptr_type(inkwell::AddressSpace::default()),
            pointer,
            "handled",
        ) {
            let _ = builder.build_return(Some(&pointer));
        }
        pointer
    }

    /// Whether the exception in `slot` is one of the names in `names`.
    ///
    /// The exception's name is a `std.String`; its `data()` is compared with
    /// each literal with `strcmp`, because the names are C strings by the time
    /// they reach the runtime and that is what the runtime stores.
    fn exception_name_matches(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        slot: inkwell::values::PointerValue<'static>,
        names: &[fallc_ast::ExprId],
    ) -> Result<inkwell::values::IntValue<'static>, CodegenError> {
        let error_ty = self
            .exception_type()
            .ok_or_else(|| CodegenError::Emit("internal error: no exception type".to_owned()))?;
        let caught = builder
            .build_load(error_ty, slot, "caught")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        let BasicValueEnum::StructValue(aggregate) = caught else {
            return Err(CodegenError::Emit(
                "internal error: an exception is not a struct".to_owned(),
            ));
        };
        // Field 0 is the name; the message is field 1.
        let name = builder
            .build_extract_value(aggregate, 0, "name")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        let held = builder
            .build_alloca(name.get_type(), "name.held")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        builder
            .build_store(held, name)
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        // `data()` is the borrowed C string, exactly as `raise` builds the name
        // from a C string.
        let mut string = self
            .nominal_types
            .iter()
            .find(|(name, _)| name.rsplit('.').next() == Some("String"))
            .map(|(_, nominal)| nominal.clone())
            .ok_or_else(|| {
                CodegenError::Emit("internal error: no `String` type for an exception".to_owned())
            })?;
        if let Some(bare) = string.name.rsplit('.').next() {
            string.name = bare.to_owned();
        }
        let receiver = fallc_sema::Type::Reference {
            referent: Box::new(fallc_sema::Type::Nominal(string)),
            referent_is_const: true,
            is_rvalue: false,
        };
        let data = self.std_function(
            "data",
            std::slice::from_ref(&receiver),
            fallc_span::Span::SYNTHETIC,
        )?;
        let call = builder
            .build_call(data, &[held.as_basic_value_enum().into()], "name.text")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        let name_ptr = match call.try_as_basic_value() {
            inkwell::values::ValueKind::Basic(BasicValueEnum::PointerValue(pointer)) => pointer,
            _ => {
                return Err(CodegenError::Emit(
                    "internal error: `String.data` returns no pointer".to_owned(),
                ));
            }
        };
        let context = self.backend.context();
        let i32_type = context.i32_type();
        let strcmp = match self.backend.module().get_function("strcmp") {
            Some(function) => function,
            None => {
                let pointer = context.ptr_type(inkwell::AddressSpace::default());
                self.backend.module().add_function(
                    "strcmp",
                    i32_type.fn_type(&[pointer.into(), pointer.into()], false),
                    None,
                )
            }
        };
        let mut result: Option<inkwell::values::IntValue<'static>> = None;
        for &name in names {
            let Some(literal) = self.lower_text(ast, types, builder, name)? else {
                return Err(CodegenError::waiting_for_spec(
                    "an exception name in an `except` filter",
                    ast.expr(name).span,
                ));
            };
            let compared = builder
                .build_call(strcmp, &[name_ptr.into(), literal.into()], "compared")
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
            let order = match compared.try_as_basic_value() {
                inkwell::values::ValueKind::Basic(BasicValueEnum::IntValue(value)) => value,
                _ => {
                    return Err(CodegenError::Emit(
                        "internal error: `strcmp` returns nothing".to_owned(),
                    ));
                }
            };
            let equal = builder
                .build_int_compare(
                    inkwell::IntPredicate::EQ,
                    order,
                    i32_type.const_zero(),
                    "match",
                )
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
            result = Some(match result {
                Some(previous) => builder
                    .build_or(previous, equal, "any.match")
                    .map_err(|error| CodegenError::Emit(error.to_string()))?,
                None => equal,
            });
        }
        Ok(result.expect("a filter has at least one name"))
    }

    /// Leaves the current function by the exception path.
    ///
    /// The frames are walked from the inside out: each `finally` runs as it is
    /// passed, and the first `except` takes the exception. With no handler in
    /// the way the exception is the result of this function, which is what makes
    /// `??` a claim about the ABI rather than a flag somewhere.
    pub(super) fn lower_escape(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        bindings: &BindingTypes,
        builder: &Builder<'static>,
        error: Option<BasicValueEnum<'static>>,
        skip_handlers: bool,
    ) -> Result<(), CodegenError> {
        let _ = bindings;
        let frames: Vec<ExcFrame> = self.exc_frames.clone();
        for frame in frames.into_iter().rev() {
            match frame {
                ExcFrame::Finally(block) => {
                    if Self::is_terminated(builder) {
                        return Ok(());
                    }
                    self.lower_block(ast, types, bindings, builder, block)?;
                }
                ExcFrame::Handler { block, slot, depth } => {
                    if skip_handlers {
                        // A `??` sends the exception past this function's own
                        // handlers: `foo()??` means "leave", not "handle here".
                        continue;
                    }
                    if Self::is_terminated(builder) {
                        return Ok(());
                    }
                    // The body's own scopes end here: their destructors run
                    // before the handler, exactly as they would on a `return`.
                    // Without this a `try` block leaked everything it declared
                    // as soon as the exception reached its handler.
                    self.emit_drops_to(ast, types, bindings, builder, depth)?;
                    if let Some(error) = error {
                        builder
                            .build_store(slot, error)
                            .map_err(|error| CodegenError::Emit(error.to_string()))?;
                    }
                    builder
                        .build_unconditional_branch(block)
                        .map_err(|error| CodegenError::Emit(error.to_string()))?;
                    return Ok(());
                }
            }
        }
        if Self::is_terminated(builder) {
            return Ok(());
        }
        if error.is_some() {
            let false_value = self.backend.context().bool_type().const_int(0, false);
            self.store_result(builder, false_value.as_basic_value_enum(), None, error)?;
        }
        // A coroutine's own variables live in its frame, not in this
        // activation: the frame outlives the `raise` and is released by the
        // coroutine object. Destroying them here would leave the frame holding
        // released storage that `result()` and the next `resume` still read.
        if self.coroutine.is_none() {
            self.emit_all_drops(ast, types, bindings, builder, None)?;
        }
        // A coroutine body returns to its resumer rather than to a caller, and
        // it returns nothing: what it raises has already been written into the
        // frame, where `result()` reads it. Returning the tagged result here
        // would give a `void` function a value to return. The function's own
        // return type is the test, because `result()` is lowered with a
        // coroutine in hand too and it does return a value.
        let returns_nothing = builder
            .get_insert_block()
            .and_then(|block| block.get_parent())
            .is_some_and(|function| function.get_type().get_return_type().is_none());
        if returns_nothing {
            // The tag says how the body ended. `result()` reads it: `RETURNED`
            // answers the payload, `RAISED` re-raises what the frame holds.
            if self.coroutine.is_some() {
                self.store_tag(builder, super::coroutines::PAYLOAD_RAISED)?;
            }
            builder
                .build_return(None)
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
            return Ok(());
        }
        let result = self.load_result(builder)?;
        builder
            .build_return(Some(&result))
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        Ok(())
    }

    /// Lowers `raise "Type": "message";` and the bare re-raising `raise;`.
    pub(super) fn lower_raise(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        bindings: &BindingTypes,
        builder: &Builder<'static>,
        node: &fallc_ast::RaiseStmt,
    ) -> Result<(), CodegenError> {
        let span = fallc_span::Span::SYNTHETIC;
        if node.is_reraise() {
            // The exception being handled is the one the enclosing `except`
            // caught, still in its slot: re-raising passes that same value on.
            let Some(slot) = self.handling.last().copied() else {
                return Err(CodegenError::Emit(
                    "internal error: `raise;` outside an `except`".to_owned(),
                ));
            };
            let ty = self.exception_type().ok_or_else(|| {
                CodegenError::Emit("internal error: no exception type".to_owned())
            })?;
            let caught = builder
                .build_load(ty, slot, "raised")
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
            return self.lower_escape(ast, types, bindings, builder, Some(caught), false);
        }

        let text_type = fallc_sema::Type::Pointer {
            pointee: Box::new(fallc_sema::Type::C8),
            pointee_is_const: true,
            pointer_is_const: false,
        };
        let type_ptr = match node.exception_type {
            Some(expr) => self
                .lower_text(ast, types, builder, expr)?
                .ok_or_else(|| CodegenError::waiting_for_spec("the exception's name", span))?,
            None => {
                return Err(CodegenError::Emit(
                    "internal error: `raise` without a name".to_owned(),
                ));
            }
        };
        let message_ptr = match node.message {
            Some(expr) => self
                .lower_text(ast, types, builder, expr)?
                .ok_or_else(|| CodegenError::waiting_for_spec("the exception's message", span))?,
            None => self.empty_c_string().as_basic_value_enum(),
        };
        // One call builds the value. Running out of memory while copying the
        // texts is answered inside, by substituting `MemoryError`, whose own
        // texts fit in their strings.
        let build = self.std_function("__ft_exception", &[text_type.clone(), text_type], span)?;
        let call = builder
            .build_call(build, &[type_ptr.into(), message_ptr.into()], "exception")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        let raised = match call.try_as_basic_value() {
            inkwell::values::ValueKind::Basic(value) => value,
            _ => {
                return Err(CodegenError::Emit(
                    "internal error: `__ft_exception` returns no value".to_owned(),
                ));
            }
        };
        self.lower_escape(ast, types, bindings, builder, Some(raised), false)
    }

    /// Lowers `try { .. }` with its `except` clauses, `else` and `finally`.
    ///
    /// The `except` clauses are tried in the order they were written: the first
    /// one whose names match the exception -- or a bare one, which matches
    /// everything -- runs. If none matches, the exception keeps going outward,
    /// through this `try`'s own `finally`.
    pub(super) fn lower_try(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        bindings: &BindingTypes,
        builder: &Builder<'static>,
        node: &fallc_ast::TryStmt,
    ) -> Result<(), CodegenError> {
        let context = self.backend.context();
        let error_ty = self
            .exception_type()
            .ok_or_else(|| CodegenError::Emit("internal error: no exception type".to_owned()))?;
        let function = builder
            .get_insert_block()
            .and_then(|block| block.get_parent())
            .ok_or_else(|| CodegenError::Emit("internal error: no function".to_owned()))?;
        let handler_block = context.append_basic_block(function, "except");
        let clean_block = context.append_basic_block(function, "no-exception");
        let join_block = context.append_basic_block(function, "handled");
        let has_except = !node.except_clauses.is_empty();

        // The exception a handler caught gets its own storage, because
        // `std.exception()` hands out its address.
        let caught_slot = if has_except {
            Some(
                builder
                    .build_alloca(error_ty, "caught")
                    .map_err(|error| CodegenError::Emit(error.to_string()))?,
            )
        } else {
            None
        };
        // Registered inside-out: `finally` first, so a handler pushed after it
        // is the innermost thing an escape meets.
        if let Some(finally) = node.finally_block {
            self.exc_frames.push(ExcFrame::Finally(finally));
        }
        if let (true, Some(slot)) = (has_except, caught_slot) {
            // The body declares its own scopes; an escape from inside them
            // stops at the handler, so the depth outside the body is what its
            // destructors are emitted down to.
            let depth = self.drop_scopes.len();
            self.exc_frames.push(ExcFrame::Handler {
                block: handler_block,
                slot,
                depth,
            });
        }

        self.lower_block(ast, types, bindings, builder, node.body)?;
        // The body's handler frame was only for the body: an exception raised
        // *inside* a handler goes somewhere else.
        if has_except {
            self.exc_frames.pop();
        }
        if !Self::is_terminated(builder) {
            builder
                .build_unconditional_branch(clean_block)
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
        }

        // The handler: try each clause in order.
        builder.position_at_end(handler_block);
        if let Some(slot) = caught_slot {
            let handled = self.handled_global();
            builder
                .build_store(handled, slot.as_basic_value_enum())
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
            let matched: Vec<inkwell::basic_block::BasicBlock<'static>> = node
                .except_clauses
                .iter()
                .enumerate()
                .map(|(index, _)| context.append_basic_block(function, &format!("except.{index}")))
                .collect();
            let escaped = context.append_basic_block(function, "except.escape");
            let mut dispatch_ends = false;
            for (index, clause) in node.except_clauses.iter().enumerate() {
                if clause.names.is_empty() {
                    // A bare clause catches everything, so nothing after it can
                    // run. It still has to be the branch taken here.
                    builder
                        .build_unconditional_branch(matched[index])
                        .map_err(|error| CodegenError::Emit(error.to_string()))?;
                    dispatch_ends = true;
                    break;
                }
                let matches =
                    self.exception_name_matches(ast, types, builder, slot, &clause.names)?;
                let next = context.append_basic_block(function, "except.next");
                builder
                    .build_conditional_branch(matches, matched[index], next)
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
                builder.position_at_end(next);
            }
            if !dispatch_ends {
                // Nothing matched: let the exception leave, through this
                // `try`'s own `finally` and on to whatever encloses it.
                let caught = builder
                    .build_load(error_ty, slot, "unmatched")
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
                self.lower_escape(ast, types, bindings, builder, Some(caught), false)?;
                if !Self::is_terminated(builder) {
                    builder
                        .build_unconditional_branch(escaped)
                        .map_err(|error| CodegenError::Emit(error.to_string()))?;
                }
            }
            if !Self::is_terminated(builder) {
                builder
                    .build_unconditional_branch(escaped)
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
            }
            builder.position_at_end(escaped);
            if !Self::is_terminated(builder) {
                builder
                    .build_unconditional_branch(join_block)
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
            }
            for (index, clause) in node.except_clauses.iter().enumerate() {
                builder.position_at_end(matched[index]);
                self.handling.push(slot);
                let lowered = self.lower_block(ast, types, bindings, builder, clause.block);
                self.handling.pop();
                lowered?;
                // The handler may have left the function -- a re-raise, or a
                // `return` of its own -- and nothing can follow a terminator.
                if !Self::is_terminated(builder) {
                    let null = context
                        .ptr_type(inkwell::AddressSpace::default())
                        .const_null();
                    builder
                        .build_store(handled, null)
                        .map_err(|error| CodegenError::Emit(error.to_string()))?;
                    builder
                        .build_unconditional_branch(join_block)
                        .map_err(|error| CodegenError::Emit(error.to_string()))?;
                }
            }
        }

        // A `try` with no `except` still has a handler block, and every block
        // needs a terminator even when nothing branches to it. It is
        // unreachable, so joining the normal path is all it has to do.
        if caught_slot.is_none() && !Self::is_terminated(builder) {
            builder
                .build_unconditional_branch(join_block)
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
        }

        // The no-exception path, which is where `else` belongs.
        builder.position_at_end(clean_block);
        if let Some(block) = node.else_block {
            self.lower_block(ast, types, bindings, builder, block)?;
        }
        if !Self::is_terminated(builder) {
            builder
                .build_unconditional_branch(join_block)
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
        }

        builder.position_at_end(join_block);
        if let Some(finally) = node.finally_block {
            // Popped before it runs: a `return` inside `finally` must not run it
            // a second time on the way out.
            self.exc_frames.pop();
            self.lower_block(ast, types, bindings, builder, finally)?;
        }
        Ok(())
    }

    /// Returns a function in the standard library that code generation calls.
    ///
    /// The checker never saw these calls -- they are the compiler's own way of
    /// writing the exception record -- so the symbol and the declaration are
    /// built here, from the types the language would have resolved them to.
    pub(super) fn std_function(
        &mut self,
        name: &str,
        params: &[fallc_sema::Type],
        span: fallc_span::Span,
    ) -> Result<inkwell::values::FunctionValue<'static>, CodegenError> {
        let mangled: Vec<String> = params.iter().map(fallc_sema::mangle_type).collect();
        let symbol = fallc_ast::special::symbol_for(name, &mangled);
        self.resolved_function(&symbol, name, span)
    }

    /// Returns the empty C string, as a shared private global.
    pub(super) fn empty_c_string(&mut self) -> inkwell::values::PointerValue<'static> {
        let context = self.backend.context();
        let bytes = context.const_string(b"\0", false);
        let module = self.backend.module();
        let global = module.add_global(bytes.get_type(), None, ".empty.text");
        global.set_initializer(&bytes);
        global.set_constant(true);
        global.set_linkage(inkwell::module::Linkage::Private);
        global.as_pointer_value()
    }

    /// Lowers an expression as the C text of an exception.
    ///
    /// The three forms the checker allows are a literal, a `*const c8`, and a
    /// `String`; the last lends its buffer for the length of the call that copies
    /// it, which is why the copy happens before anything can release it.
    pub(super) fn lower_text(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        expr: ExprId,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        let span = ast.expr(expr).span;
        let ty = self
            .expression_type(ast, types, expr)
            .ok_or_else(|| CodegenError::waiting_for_spec("the text of an exception", span))?;
        if ty.is_nominal()
            && ty.nominal_name().and_then(|name| name.rsplit('.').next()) == Some("String")
        {
            // `data()` is the borrowed C string. The receiver is taken by
            // address, so a temporary is given storage first.
            let storage = match self.lower_lvalue(ast, types, builder, expr) {
                Some(binding) => binding.storage,
                None => {
                    let value = self
                        .lower_expr(ast, types, builder, expr)?
                        .ok_or_else(|| CodegenError::waiting_for_spec("this string", span))?;
                    self.store_temporary(builder, value, "text")?
                }
            };
            let receiver = fallc_sema::Type::Reference {
                referent: Box::new(ty),
                referent_is_const: true,
                is_rvalue: false,
            };
            let function = self.std_function("data", std::slice::from_ref(&receiver), span)?;
            let call = builder
                .build_call(function, &[storage.into()], "text")
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
            return self.unwrap_throwing(ast, types, builder, span, call);
        }
        // A literal is a static buffer and a pointer already is one; both lower
        // to the address of the text.
        self.lower_expr(ast, types, builder, expr)
    }

    /// Returns from the current function, wrapping the value if it may raise.
    ///
    /// A `??` function's result is `{ ok, value, error }`; the ordinary path
    /// fills in `ok` and the value and leaves the error arm undefined, because
    /// nothing reads the arm the tag does not name.
    pub(super) fn build_function_return(
        &mut self,
        builder: &Builder<'static>,
        value: Option<BasicValueEnum<'static>>,
    ) -> Result<(), CodegenError> {
        // An aggregate return is copied into the caller's slot and the function
        // returns nothing. The value here is the address the aggregate lives
        // at, never the aggregate itself.
        if let Some(slot) = self.sret_slot {
            let Some(BasicValueEnum::PointerValue(source)) = value else {
                // A bare `return;` in a function declared to return an
                // aggregate is refused by the checker; emitting `ret void` is
                // the least wrong thing left, and it cannot happen for a
                // well-typed program.
                return builder
                    .build_return(None)
                    .map(|_| ())
                    .map_err(|error| CodegenError::Emit(error.to_string()));
            };
            let ty = self
                .current_return_type
                .as_ref()
                .and_then(|ty| TypeMapper::new(self.backend.context()).map(ty))
                .ok_or_else(|| {
                    CodegenError::Emit(
                        "internal error: an aggregate return has no LLVM type".to_owned(),
                    )
                })?;
            let size = ty.size_of().ok_or_else(|| {
                CodegenError::Emit("internal error: an aggregate return has no size".to_owned())
            })?;
            builder
                .build_memcpy(slot, 1, source, 1, size)
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
            builder
                .build_return(None)
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
            return Ok(());
        }
        if self.exc_result.is_some() {
            let true_value = self
                .backend
                .context()
                .bool_type()
                .const_int(1, false)
                .as_basic_value_enum();
            self.store_result(builder, true_value, value, None)?;
            let result = self.load_result(builder)?;
            builder
                .build_return(Some(&result))
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
            return Ok(());
        }
        match value {
            Some(value) => {
                builder
                    .build_return(Some(&value))
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
            }
            None => {
                builder
                    .build_return(None)
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
            }
        }
        Ok(())
    }
}
