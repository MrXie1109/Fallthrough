//! Calls, the in-place operations around them, and the string literal data a
//! call may need.
//!
//! A call is lowered from the symbol the checker recorded; the ABI decides how
//! the arguments travel, and a `??` call is unwrapped at the call site.

use super::*;

impl<'ctx> Lowerer<'ctx> {
    /// Lowers a call to an LLVM `call`.
    /// Unwraps a call the checker marked as able to raise.
    ///
    /// The implicit operations -- construction, assignment, operators, indexing,
    /// rendering -- are not written as calls, but a type's method for one can
    /// still raise, and the result then carries the exception. Every lowering
    /// site for those operations funnels through here, so the tag is read once
    /// and the exception leaves the way any other does.
    /// Returns whether `ty` is the shape a throwing call returns: a tag, an
    /// optional value, and the exception itself.
    pub(super) fn is_throwing_result_type(&mut self, ty: inkwell::types::StructType<'static>) -> bool {
        if ty.count_fields() < 2 {
            return false;
        }
        let Some(error) = self.exception_type() else {
            return false;
        };
        let fields = ty.get_field_types();
        if Some(fields.last().copied()) != Some(Some(error)) {
            return false;
        }
        let inkwell::types::BasicTypeEnum::IntType(tag) = fields[0] else {
            return false;
        };
        tag.get_bit_width() == 1
    }

    pub(super) fn unwrap_throwing(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        span: fallc_span::Span,
        call: inkwell::values::CallSiteValue<'static>,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        let value = match call.try_as_basic_value() {
            inkwell::values::ValueKind::Basic(value) => Some(value),
            _ => None,
        };
        self.unwrap_throwing_value(ast, types, builder, span, value)
    }

    /// Unwraps a value an operation produced, escaping to the enclosing
    /// handler when it carries a raised exception.
    ///
    /// The shape of the value decides, not a record of the call: every implicit
    /// member call -- a constructor, an operator, a conversion -- is covered
    /// without having to be listed, so a site the checker has not heard of
    /// cannot leak a tagged union where a plain value belongs.
    pub(super) fn unwrap_throwing_value(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        span: fallc_span::Span,
        value: Option<BasicValueEnum<'static>>,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        let Some(value) = value else {
            return Ok(None);
        };
        let BasicValueEnum::StructValue(aggregate) = value else {
            return Ok(Some(value));
        };
        // Whether the call can raise is read from the value it produced rather
        // than from the checker's record of the call: every site that calls a
        // member implicitly -- a constructor, an operator, a conversion -- gets
        // the answer without having to be listed, so a site the checker has not
        // heard of cannot leak a tagged union where a plain value belongs.
        if !self.is_throwing_result_type(aggregate.get_type()) {
            return Ok(Some(value));
        }
        let fields = aggregate.get_type().count_fields();
        let tag = builder
            .build_extract_value(aggregate, 0, "ok")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        let raised = builder
            .build_extract_value(aggregate, fields - 1, "raised")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        let BasicValueEnum::IntValue(condition) = tag else {
            return Err(CodegenError::Emit(
                "internal error: a result tag is not a boolean".to_owned(),
            ));
        };
        let context = self.backend.context();
        let function = builder
            .get_insert_block()
            .and_then(|block| block.get_parent())
            .ok_or_else(|| CodegenError::Emit("internal error: no function".to_owned()))?;
        let escape_block = context.append_basic_block(function, "raise");
        let value_block = context.append_basic_block(function, "op-value");
        builder
            .build_conditional_branch(condition, value_block, escape_block)
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        builder.position_at_end(escape_block);
        if self.abort_lowering > 0 {
            // This raise is inside an `expr!!`, which is `try { expr }
            // except { panic }`. The report is the one a program makes for an
            // exception nothing caught -- the same texts, read the same way --
            // with `panic:` in place of `uncaught exception:`.
            self.emit_abort_report(ast, types, builder, raised)?;
            builder.position_at_end(value_block);
            if fields >= 3 {
                return Ok(Some(
                    builder
                        .build_extract_value(aggregate, 1, "value")
                        .map_err(|error| CodegenError::Emit(error.to_string()))?,
                ));
            }
            return Ok(None);
        }
        let bindings = self.current_bindings.clone();
        let skip_handlers = self.propagating_calls.contains(&span);
        self.lower_escape(ast, types, &bindings, builder, Some(raised), skip_handlers)?;
        builder.position_at_end(value_block);
        if fields >= 3 {
            return Ok(Some(
                builder
                    .build_extract_value(aggregate, 1, "value")
                    .map_err(|error| CodegenError::Emit(error.to_string()))?,
            ));
        }
        Ok(None)
    }

    /// Lowers a call, and unwraps the exception if the callee may raise.
    ///
    /// A `??` callee returns `{ ok, value, error }`. The tag decides whether the
    /// call produced a value or an exception, and the exception leaves through
    /// the same path a `raise` takes -- the handler if one encloses the call, or
    /// this function's own result.
    pub(super) fn lower_call(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        call: &CallExpr,
        span: fallc_span::Span,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        let value = self.lower_call_inner(ast, types, builder, call, span)?;
        let value = self.unwrap_throwing_value(ast, types, builder, span, value)?;
        // A record returned as a value still needs an address somewhere: a
        // caller may ask for a field of it -- `make().x` -- and there is none
        // otherwise. Keeping the result costs one store and makes field access
        // work without lowering the call a second time.
        if let Some(found) = value
            && matches!(
                &found,
                BasicValueEnum::StructValue(_) | BasicValueEnum::ArrayValue(_)
            )
            && !self.temporaries.contains_key(&span)
        {
            let storage = self.store_temporary(builder, found, "call.value")?;
            self.temporaries.insert(
                span,
                LocalBinding {
                    storage,
                    ty: found.get_type(),
                },
            );
        }
        Ok(value)
    }

    /// Emits a call and returns the value the call expression has.
    ///
    /// A function that returns an aggregate out of line takes a hidden first
    /// pointer argument. The slot is allocated here, and the expression's value
    /// is that slot's address: a huge aggregate has no value form. The slot is
    /// recorded as the expression's temporary, because the caller may index it,
    /// take a field of it, or copy out of it, and neither re-evaluating the
    /// call nor loading the aggregate is acceptable.
    pub(super) fn build_value_call(
        &mut self,
        builder: &Builder<'static>,
        callee: FunctionValue<'static>,
        symbol: &str,
        args: Vec<inkwell::values::BasicMetadataValueEnum<'static>>,
        span: Span,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        let kind = self
            .return_kinds
            .get(symbol)
            .cloned()
            .unwrap_or(ReturnKind::Value);
        let ReturnKind::OutOfLine(ty) = kind else {
            let call = builder
                .build_call(callee, &args, "call")
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
            return Ok(match call.try_as_basic_value() {
                inkwell::values::ValueKind::Basic(value) => Some(value),
                inkwell::values::ValueKind::Instruction(_) => None,
            });
        };
        let llvm = TypeMapper::new(self.backend.context())
            .map(&ty)
            .ok_or_else(|| {
                CodegenError::waiting_for_spec(
                    format!("a return slot of type `{}`", ty.spelling()),
                    span,
                )
            })?;
        let entry = self
            .current_function
            .and_then(|function| function.get_first_basic_block())
            .ok_or_else(|| {
                CodegenError::Emit("internal error: no function is being lowered".to_owned())
            })?;
        let slot = self.create_entry_alloca(builder, entry, ".return", llvm);
        let mut args = args;
        args.insert(0, slot.into());
        builder
            .build_call(callee, &args, "call")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        self.temporaries.insert(
            span,
            LocalBinding {
                storage: slot,
                ty: llvm,
            },
        );
        Ok(Some(slot.as_basic_value_enum()))
    }

    /// The body of [`Lowerer::lower_call`], before the exception is unwrapped.
    pub(super) fn lower_call_inner(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        call: &CallExpr,
        span: fallc_span::Span,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        // A closure value called by name: its fields are the captured
        // addresses, and the body is the one the checker recorded.
        if let Some(info) = self.closure_calls.get(&span).cloned() {
            return self.lower_closure_call(ast, types, builder, call, &info, span);
        }

        // A lambda is called directly: the checker recorded which body the call
        // reaches, and its captures are the addresses the body was written
        // against.
        if self.lambda_calls.contains_key(&span) {
            return self.lower_lambda_call(ast, types, builder, call, span);
        }

        // An explicit construction or destruction the element type does not
        // declare was answered by the checker as a plain store or a field-wise
        // destroy. It is read before the call paths below, because there is no
        // symbol to call: the operation *is* the lowering.
        if let Some(operation) = self.in_place_operations.get(&span).copied() {
            return self.lower_in_place_operation(ast, types, builder, call, operation, span);
        }

        // `str(x)` and `repr(x)` were resolved by the checker into either a
        // call of the argument's own `__str__`/`__repr__` or a request that the
        // compiler render the value. Which one it is was decided by the type
        // system, so it is read here rather than decided again.
        if let Some(resolved) = self.text_calls.get(&span).cloned() {
            return match resolved {
                Some(symbol) => self.lower_text_method(ast, types, builder, call, &symbol, span),
                None => self.lower_render(ast, types, builder, call, span),
            };
        }

        // `T(args)` constructs: the storage is allocated here, the constructor
        // writes into it, and the value it holds is what the expression has.
        if let Some(symbol) = self.constructor_calls.get(&span).cloned() {
            return self.lower_constructor_call(ast, types, builder, call, &symbol, span);
        }

        // A call of a value whose type defines `__call__` is that method, with
        // the value as the receiver. The checker recorded which one, so this
        // only emits the call.
        if let Some(symbol) = self.call_methods.get(&span).cloned() {
            return self.lower_call_method(ast, types, builder, call, &symbol, span);
        }

        // A method call passes the receiver as the first argument, taking its
        // address if it is a value. `p.show()` becomes `show(&p)`.
        if let ExprKind::Field(field) = &ast.expr(call.callee).kind {
            // The checker decided what this call is, and its decision is the
            // one that counts: a method it recorded is a method, even when the
            // receiver's name is also a module's. A parameter called `memory`
            // was read here as the module `std.memory`, and the call went out
            // without its receiver.
            if self.method_calls.contains_key(&span) {
                return self.lower_method_call(ast, types, builder, field, call, span);
            }
            // A module call is not a method call: the object is a module name
            // rather than a value, so the symbol is the bare function name.
            // A call the checker resolved to a *symbol* is lowered as that
            // symbol, module-qualified or not: an instantiation of a template
            // from another module is emitted here, and its external signature
            // describes the template rather than the member.
            if let Some(symbol) = self.call_symbols.get(&span).cloned() {
                let qualified = format!("{}.{}", name_of(&ast.expr(field.object).kind), field.field);
                return self.lower_symbol_call(ast, types, builder, &qualified, &symbol, call, span);
            }
            if self.external_symbols.contains_key(
                format!("{}.{}", name_of(&ast.expr(field.object).kind), field.field).as_str(),
            ) || matches!(&ast.expr(field.object).kind, ExprKind::Name(object) if self.module_names.contains(object))
            {
                let qualified =
                    format!("{}.{}", name_of(&ast.expr(field.object).kind), field.field);
                return self.lower_external_call(ast, types, builder, &qualified, call, span);
            }
            return self.lower_method_call(ast, types, builder, field, call, span);
        }

        let ExprKind::Name(name) = &ast.expr(call.callee).kind else {
            // Not a name: a value whose type is callable is called through,
            // which is how `pick()(1)` calls what a call returned.
            if let Some(value) = self.lower_indirect_call(ast, types, builder, call, span)? {
                return Ok(value);
            }
            return Err(CodegenError::waiting_for_spec(
                "a call to something other than a named function",
                span,
            ));
        };

        // The definition this call reached was chosen by the checker, which is
        // the only stage that knows how a decayed array or an unsuffixed
        // literal fitted the parameters. The symbol is read back rather than
        // rebuilt from the argument types.
        let resolved = self
            .call_symbols
            .get(&span)
            .cloned()
            .or_else(|| self.overload_for(name, call, types));
        // A builtin has no definition in this module, so it is declared on
        // first use. Consulting the builtin table rather than hardcoding a name
        // means adding a builtin needs no change here.
        if let Some(symbol) = resolved.filter(|symbol| {
            // A name that came from an import is handled below, where the
            // defining module's symbol is known; the checker recorded the
            // symbol as written, which for an import is the local name.
            self.backend.module().get_function(symbol).is_some()
                || self.private_symbol_defined(symbol)
                || self.module_function_symbols.contains_key(symbol)
        }) {
            // A private free function is emitted under its module's prefix
            // while the checker recorded the bare name; the prefixed spelling
            // is the one that exists. Which module that is comes from where the
            // function was declared -- an instantiation's body belongs to the
            // module that wrote it, not to the file being compiled.
            let symbol = if self.backend.module().get_function(&symbol).is_some() {
                symbol
            } else if let Some(qualified) = self.module_function_symbols.get(&symbol) {
                qualified.clone()
            } else {
                format!("{}.{}", self.current_module, symbol)
            };
            let Some(callee) = self.backend.module().get_function(&symbol) else {
                return Err(CodegenError::Emit(format!(
                    "internal error: `{name}` was emitted as `{symbol}`, which is missing"
                )));
            };
            let args = self.lower_arguments(ast, types, builder, call, &symbol, span)?;
            return self.build_value_call(builder, callee, &symbol, args, span);
        }

        // A call through a *value* whose type is a function or a pointer to
        // one: `p(1)` with `p *fn(int) int` is how a function pointer is
        // called, and the callee's type is the only place its parameters exist.
        if let Some(value) = self.lower_indirect_call(ast, types, builder, call, span)? {
            return Ok(value);
        }

        // A call inside a template body belongs to the module that *wrote* it,
        // but the instantiation is emitted where it is *used*. A function of
        // the writing module is therefore not in this module, and the bare name
        // the body used reaches it only through the signature the importing
        // unit holds. Declaring it from that signature is what an ordinary
        // import does, so the call is lowered the same way.
        if let Some(symbol) = self.call_symbols.get(&span).cloned()
            && self.backend.module().get_function(&symbol).is_none()
            && let Some(qualified) = self
                .external_symbols
                .iter()
                .find(|(_, signature)| signature.symbol == symbol)
                .map(|(qualified, _)| qualified.clone())
        {
            return self.lower_symbol_call(ast, types, builder, &qualified, &symbol, call, span);
        }

        // A name that came from an import calls the symbol its defining module
        // emitted, which the resolver recorded. The *second* half of that
        // record is the module the name is reachable through, which is the key
        // its signature is filed under: a function lives in `Stdio` but is
        // reached as `C.printf`, and it is `C.printf` that was registered.
        if let Some((symbol, qualified)) = self.imported_symbols.get(name.as_str()).cloned() {
            return self.lower_symbol_call(ast, types, builder, &qualified, &symbol, call, span);
        }

        // A qualified name such as `C.printf` refers to a symbol declared in
        // another module, where it is named without the prefix. Only the member
        // half is the linker symbol.
        let symbol = match name.split_once('.') {
            Some((module, member)) => {
                if self.external_symbols.contains_key(name.as_str()) {
                    member.to_owned()
                } else {
                    let _ = module;
                    name.clone()
                }
            }
            None => name.clone(),
        };

        let callee = match self.backend.module().get_function(&symbol) {
            Some(function) => function,
            // A mangled name means an overload: the symbol was not emitted
            // under the plain name, so the plain name is tried as well.
            None if self.backend.module().get_function(name).is_some() => self
                .backend
                .module()
                .get_function(name)
                .expect("just checked"),
            None if fallc_sema::builtins::is_builtin(name) => self.declare_builtin(name, span)?,
            // An `extern "C"` declaration with no body is emitted as an LLVM
            // declaration on demand, which is what makes the symbol resolvable
            // at link time without a definition in this module.
            None if self.external_symbols.contains_key(name.as_str()) => {
                self.declare_external(&symbol, name, span)?
            }
            None => {
                if std::env::var_os("FALLC_STAGES").is_some() {
                    eprintln!(
                        "  missing call `{name}` symbol={:?} current_module={:?} mapped={:?}",
                        self.call_symbols.get(&span),
                        self.current_module,
                        self.module_function_symbols.get(name.as_str()),
                    );
                }
                return Err(CodegenError::waiting_for_spec(
                    format!("a call to `{name}`, which is not defined in this module"),
                    span,
                ));
            }
        };

        let args = self.lower_arguments(ast, types, builder, call, &symbol, span)?;

        // A call to a `void` function produces no value.
        self.build_value_call(builder, callee, &symbol, args, span)
    }

    /// Lowers an explicit `__init__`/`__drop__` call answered without a
    /// declared method.
    ///
    /// The receiver names storage, so this is the address a method call would
    /// pass as `this`: a pointer is already that address, and a reference that
    /// names storage -- `*p`, `a[i]` -- is the storage's own. A store writes
    /// the argument there, which is all construction is for a type that owns
    /// nothing; a destroy runs whatever destructor the value has, including the
    /// field-by-field case, which emits nothing when no field needs one.
    pub(super) fn lower_in_place_operation(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        call: &CallExpr,
        operation: fallc_sema::InPlaceOperation,
        span: fallc_span::Span,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        let ExprKind::Field(field) = &ast.expr(call.callee).kind else {
            return Err(CodegenError::Emit(
                "internal error: an in-place construction is not a method call".to_owned(),
            ));
        };
        let receiver_ty = types
            .get(field.object)
            .cloned()
            .ok_or_else(|| CodegenError::waiting_for_spec("this receiver", span))?;
        let target = receiver_ty
            .pointee()
            .cloned()
            .ok_or_else(|| {
                CodegenError::waiting_for_spec("construction into raw storage", span)
            })?;

        let lowered = self.lower_expr(ast, types, builder, field.object)?;
        let storage = if receiver_ty.is_pointer() {
            match lowered {
                Some(BasicValueEnum::PointerValue(pointer)) => pointer,
                _ => return Err(CodegenError::waiting_for_spec("this slot", span)),
            }
        } else if receiver_ty.is_reference() {
            // A reference that names storage yields that storage; one produced
            // by a call yields its address as its value.
            let named_storage = matches!(
                &ast.expr(field.object).kind,
                ExprKind::Unary(unary) if unary.op == UnaryOp::Deref
            ) || matches!(&ast.expr(field.object).kind, ExprKind::Index(_));
            if named_storage {
                self.lower_lvalue(ast, types, builder, field.object)
                    .ok_or_else(|| CodegenError::waiting_for_spec("this slot", span))?
                    .storage
            } else {
                match lowered {
                    Some(BasicValueEnum::PointerValue(pointer)) => pointer,
                    _ => return Err(CodegenError::waiting_for_spec("this slot", span)),
                }
            }
        } else {
            return Err(CodegenError::waiting_for_spec("construction into raw storage", span));
        };

        match operation {
            fallc_sema::InPlaceOperation::Store => {
                let Some(&argument) = call.args.first() else {
                    return Err(CodegenError::waiting_for_spec("a value to store", span));
                };
                let value = self
                    .lower_expr(ast, types, builder, argument)?
                    .ok_or_else(|| CodegenError::waiting_for_spec("this value", span))?;
                // A literal that adopted the slot's type is converted here, as
                // it is for any other parameter: the expression's own recorded
                // type is the default `int`, and LLVM will not store an `i32`
                // into an `i64` slot.
                let value = match self.adopted_types.get(&ast.expr(argument).span).cloned() {
                    Some(wanted) => {
                        self.convert_value(ast, types, builder, argument, value, &wanted, span)?
                    }
                    None => value,
                };
                builder
                    .build_store(storage, value)
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
                Ok(None)
            }
            fallc_sema::InPlaceOperation::Drop => {
                let entry = DropEntry {
                    storage,
                    symbol: self.drop_symbol_for(&target),
                    ty: target,
                };
                self.emit_drop(builder, &entry)?;
                Ok(None)
            }
        }
    }

    /// Finds the symbol a type's constructor is emitted under, given its
    /// arguments.
    ///
    /// Like a method call, the receiver's qualifiers are not recoverable from
    /// the object, so the pointer forms are tried in order.
    pub(super) fn init_symbol_for(&self, receiver: &Type, arguments: &[Type]) -> Option<String> {
        let candidates = pointer_forms(receiver);
        let module = self.backend.module();
        for receiver in candidates {
            let mut params = vec![receiver];
            params.extend_from_slice(arguments);
            let mangled: Vec<String> = params.iter().map(fallc_sema::mangle_type).collect();
            // `__init__` is the explicit constructor; `__im_init__` may also be
            // used explicitly, so both names are candidates.
            for written in [
                fallc_ast::special::SpecialMethod::Init.name(),
                "__im_init__",
            ] {
                let symbol = fallc_ast::special::symbol_for(written, &mangled);
                if module.get_function(&symbol).is_some() {
                    return Some(symbol);
                }
            }
        }
        None
    }

    /// Returns `true` if this type defines an `__init__`.
    ///
    /// A type's constructor is looked up by the symbol its receiver type
    /// implies, since several types may each have one.
    pub(super) fn type_has_init(&self, ty: &Type) -> bool {
        // Every `__init__` for this receiver, whatever arguments it takes. The
        // symbol carries the parameter types, so the receiver is only a prefix
        // of it and the receiver alone cannot be looked up.
        //
        // The receiver is passed *by pointer* -- `this *T` -- so every pointer
        // form is tried. Passing the value type here produced `__init___N1_P`
        // while the emitted symbol was `__init___P00N1_P_I3_i32`, and the
        // constructor was silently never called.
        pointer_forms(ty).iter().any(|receiver| {
            [constructor_symbol(receiver), implicit_constructor_symbol(receiver)]
                .iter()
                .any(|prefix| {
                    self.emitted_symbols
                        .iter()
                        .any(|symbol| symbol == prefix || symbol.starts_with(&format!("{prefix}_")))
                })
        })
    }

    /// Decodes a byte string literal's source text into bytes.
    ///
    /// The text includes the `b` prefix and the quotes, which are not part of
    /// the value. The decoding is `fallc_ast::literal::decode_escapes`, the one
    /// decoder the checker uses as well, so the two cannot disagree about a
    /// literal: C's escapes plus `\u{..}`, and an unrecognised escape keeps its
    /// backslash rather than losing the text.
    /// Returns the characters of a raw string literal, undecoded.
    ///
    /// `r"C:\dir"` is the characters `C:\dir`: a raw string has no escapes,
    /// so its content is exactly the source between the delimiters.
    pub(super) fn raw_string_bytes(&self, text: &str) -> Vec<u8> {
        let body = text
            .find('"')
            .zip(text.rfind('"'))
            .and_then(|(open, close)| (open < close).then(|| &text[open + 1..close]))
            .unwrap_or("");
        body.as_bytes().to_vec()
    }

    pub(super) fn byte_string_bytes(&self, text: &str) -> Vec<u8> {
        let body = text
            .strip_prefix('b')
            .and_then(|rest| rest.strip_prefix('"'))
            .and_then(|rest| rest.strip_suffix('"'))
            .unwrap_or("");
        fallc_ast::literal::decode_escapes(body)
    }

    /// Returns the bit width a string literal's prefix names, defaulting to 8.
    ///
    /// `u16"ab"` holds `c16` elements; a bare `"ab"` holds `c8`.
    pub(super) fn string_literal_width(&self, text: &str) -> u32 {
        match text.find('"') {
            Some(0) | None => 8,
            Some(open) => match &text[..open] {
                "u16" => 16,
                "u32" => 32,
                _ => 8,
            },
        }
    }

    /// Returns the code units a string literal holds, NUL excluded.
    ///
    /// The trailing NUL the literal carries is not part of this: it is added by
    /// the caller, so that the two spellings (`b"ab"` without, `"ab"` with)
    /// differ only in that one decision. `""".."""` is the same literal with a
    /// longer delimiter, so it is the shared body helper that finds its text.
    pub(super) fn string_literal_elements(&self, text: &str) -> Vec<u32> {
        fallc_ast::literal::decode_escapes(fallc_ast::literal::string_body(text))
            .iter()
            .map(|byte| u32::from(*byte))
            .collect()
    }

}
