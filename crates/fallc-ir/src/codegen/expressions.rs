//! Expressions: the entry point, indexing, and the small forms built out of
//! them.
//!
//! `lower_expr` dispatches on the expression; everything else here is the
//! lowering of one kind of expression or of one operation on values.

use super::*;

impl<'ctx> Lowerer<'ctx> {
    /// Lowers one expression to an LLVM value.
    pub(super) fn lower_expr(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        expr: ExprId,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        // The builder emits the loads and arithmetic that most expression
        // forms need; literals alone do not use it.
        let node = ast.expr(expr);
        // A value used as an interface is boxed here: the conversion is a
        // property of where the value goes, and the checker recorded it.
        if let Some(conversion) = self.interface_conversions.get(&node.span).cloned() {
            return self.lower_interface_conversion(ast, types, builder, expr, &conversion);
        }
        match &node.kind {
            // A pack expansion and a comma fold are gone by the time a body is
            // lowered: the instantiation replaces them with the elements' own
            // expressions. Reaching one means the expansion did not happen.
            fallc_ast::ExprKind::PackExpand(_) | fallc_ast::ExprKind::CommaFold { .. } => {
                Err(CodegenError::Emit(
                    "internal error: a parameter pack was not expanded before code generation"
                        .to_owned(),
                ))
            }
            // A lambda that captures nothing is a function, so its value is the
            // address of the function its body was emitted as.
            ExprKind::Lambda(lambda) => {
                self.lower_lambda_value(ast, types, builder, expr, lambda, node.span)
            }
            ExprKind::CharLiteral(literal) => {
                let value = decode_char_literal(&literal.text, node.span)?;
                let context = self.backend.context();
                // The literal's type is its width prefix: `c8` without one.
                let bits = std::num::NonZeroU32::new(literal.width.unwrap_or(8))
                    .expect("a character literal always has a width");
                // The widths a prefix can name are all LLVM integer widths, so
                // the fallible lookup cannot fail here.
                let ty = context
                    .custom_width_int_type(bits)
                    .map_err(|error| CodegenError::Emit(error.to_owned()))?;
                Ok(Some(
                    ty.const_int(u64::from(value), false).as_basic_value_enum(),
                ))
            }
            ExprKind::FloatLiteral(literal) => {
                let value: f64 = literal.text.parse().map_err(|_| {
                    CodegenError::Emit(format!("internal error: `{}` is not a float", literal.text))
                })?;
                let context = self.backend.context();
                // The suffix selects the LLVM type; without one the literal is
                // a `double`.
                let constant = match literal.suffix.as_deref() {
                    Some("f") => context.f32_type().const_float(value),
                    _ => context.f64_type().const_float(value),
                };
                Ok(Some(constant.as_basic_value_enum()))
            }
            ExprKind::IntLiteral(literal) => {
                let value = parse_integer_literal(&literal.digits).ok_or_else(|| {
                    CodegenError::Emit(format!(
                        "`{}` is not a valid integer, at {:?}",
                        literal.digits, node.span
                    ))
                })?;
                // The literal's LLVM type comes from the type checker, so that
                // `return 0;` in a `u8` function produces an `i8` zero rather
                // than an `i32` one.
                //
                // A literal in a variadic argument position has no declared
                // type to adopt, so it takes the default. `int` is the
                // language's default integer, and it matches what C's variadic
                // calling convention expects for an unadorned integer.
                //
                // A variadic argument is not promoted the way C promotes one: a
                // literal with no declared type takes `int`, the default, but a
                // `float` stays `float` and a narrow integer keeps its width. A
                // C callee reading the slot as `double` or `int` therefore
                // reads something other than what was passed.
                let ty = types.get(expr).cloned().unwrap_or(Type::INT);
                let mapper = TypeMapper::new(self.backend.context());
                let llvm_ty = mapper.map(&ty).ok_or_else(|| {
                    CodegenError::waiting_for_spec("this literal's type", node.span)
                })?;
                let int_ty = match llvm_ty {
                    inkwell::types::BasicTypeEnum::IntType(int) => int,
                    _ => {
                        return Err(CodegenError::waiting_for_spec(
                            "a non-integer literal type",
                            node.span,
                        ));
                    }
                };
                Ok(Some(int_ty.const_int(value, false).as_basic_value_enum()))
            }
            ExprKind::BoolLiteral(value) => {
                let context = self.backend.context();
                Ok(Some(
                    context
                        .bool_type()
                        .const_int(u64::from(*value), false)
                        .as_basic_value_enum(),
                ))
            }
            // A byte string is `[N]c8` and carries *no* trailing NUL: it is a
            // container of characters, not a C string. `b"ab"` is two bytes.
            ExprKind::ByteStringLiteral(_) => Ok(self
                .intern_literal(&node.kind, ".bytes")
                .map(|pointer| pointer.as_basic_value_enum())),
            ExprKind::StringLiteral(_) => Ok(self
                .intern_literal(&node.kind, ".str")
                .map(|pointer| pointer.as_basic_value_enum())),
            // A raw string is a string whose characters are taken literally:
            // the bytes between the delimiters, no escape decoded, and a
            // trailing NUL so it can be handed to C like any other string.
            ExprKind::RawStringLiteral(_) => Ok(self
                .intern_literal(&node.kind, ".str")
                .map(|pointer| pointer.as_basic_value_enum())),
            ExprKind::Name(name) => {
                // A constant is its value: the checker folded it, and a
                // constant has no storage to load from. This is checked before
                // the binding lookup, because a constant is not a binding.
                if let Some(value) = self.constant_values.get(&node.span).copied() {
                    let ty = types.get(expr).cloned().unwrap_or(fallc_sema::Type::INT);
                    let llvm = TypeMapper::new(self.backend.context())
                        .map(&ty)
                        .ok_or_else(|| {
                            CodegenError::waiting_for_spec(
                                format!("a constant of type `{}`", ty.spelling()),
                                node.span,
                            )
                        })?;
                    let inkwell::types::BasicTypeEnum::IntType(int) = llvm else {
                        return Err(CodegenError::waiting_for_spec(
                            "a constant that is not an integer",
                            node.span,
                        ));
                    };
                    return Ok(Some(
                        int.const_int(value as u64, false).as_basic_value_enum(),
                    ));
                }
                // A value template parameter is replaced by its value, which
                // is what an instantiation means: `string` in
                // `template <string []char>` *is* the sequence the caller
                // wrote. An array value has no storage of its own here, so it
                // is materialised wherever it is used -- which is what makes
                // `string[index]` index a constant.
                if let Some(argument) = self
                    .substitution
                    .as_ref()
                    .and_then(|substitution| substitution.get(name))
                    .cloned()
                {
                    let ty = self.template_value_types.get(name).cloned();
                    return self.lower_template_value(ast, ty, builder, &argument, node.span);
                }
                // A name reads from storage: a local's slot, a module-level
                // variable's global, or -- when it names a function -- its
                // address, which is the decay a function name gets in value
                // position.
                match self.lookup_binding(name) {
                    Some(binding) => {
                        // `build_load` takes the pointee type, which the
                        // binding records because opaque pointers cannot
                        // supply it.
                        let loaded = builder
                            .build_load(binding.ty, binding.storage, name)
                            .map_err(|error| CodegenError::Emit(error.to_string()))?;
                        // A binding declared `&T` names storage, and using it
                        // as a value reads through it -- `r * 2` is arithmetic
                        // on the referent, not on the address. The checker
                        // records which uses read through, exactly as it does
                        // for a call that returns a reference.
                        if let Some(referent) = self.deref_reads.get(&node.span).cloned()
                            && let Some(llvm) = TypeMapper::new(self.backend.context()).map(&referent)
                            && let BasicValueEnum::PointerValue(address) = loaded
                        {
                            let value = builder
                                .build_load(llvm, address, "referent")
                                .map_err(|error| CodegenError::Emit(error.to_string()))?;
                            return Ok(Some(value));
                        }
                        Ok(Some(loaded))
                    }
                    // A module-level variable is a global rather than a slot,
                    // so it is not in any scope. Reading it is a load from its
                    // address, exactly as reading a local is.
                    None => {
                        // One declared by *another* module is not in this
                        // module's table under its bare name: the checker
                        // recorded which qualified variable the name means, and
                        // the symbol it was declared with.
                        if let Some(qualified) =
                            self.external_global_refs.get(&node.span).cloned()
                            && let Some((global, llvm)) = self.external_global(&qualified)
                        {
                            let loaded = builder
                                .build_load(llvm, global.as_pointer_value(), name)
                                .map_err(|error| CodegenError::Emit(error.to_string()))?;
                            return Ok(Some(loaded));
                        }
                        let Some((global, llvm)) = self.globals.get(name).copied() else {
                            // Not storage: it may be a *function*, whose value
                            // as an expression is its address. That is the
                            // decay a function name gets wherever a value is
                            // wanted.
                            if let Some(pointer) =
                                self.function_pointer(name, types.get(expr))
                            {
                                return Ok(Some(pointer.as_basic_value_enum()));
                            }
                            return Err(CodegenError::waiting_for_spec(
                                "a reference to a global or function name",
                                node.span,
                            ));
                        };
                        let loaded = builder
                            .build_load(llvm, global.as_pointer_value(), name)
                            .map_err(|error| CodegenError::Emit(error.to_string()))?;
                        if let Some(referent) = self.deref_reads.get(&node.span).cloned()
                            && let Some(referent_llvm) =
                                TypeMapper::new(self.backend.context()).map(&referent)
                            && let BasicValueEnum::PointerValue(address) = loaded
                        {
                            let value = builder
                                .build_load(referent_llvm, address, "referent")
                                .map_err(|error| CodegenError::Emit(error.to_string()))?;
                            return Ok(Some(value));
                        }
                        Ok(Some(loaded))
                    }
                }
            }
            ExprKind::Binary(binary) => self.lower_binary(ast, types, builder, binary, node.span),
            ExprKind::Unary(unary) => self.lower_unary(ast, types, builder, unary, node.span),
            ExprKind::Assign(assign) => self.lower_assign(ast, types, builder, assign, node.span),
            ExprKind::IncDec(incdec) => self.lower_incdec(ast, types, builder, incdec, node.span),
            ExprKind::Comma(comma) => {
                // The left side runs for its effects; its value is discarded.
                // Emitting it first preserves the evaluation order.
                let _ = self.lower_expr(ast, types, builder, comma.lhs)?;
                self.lower_expr(ast, types, builder, comma.rhs)
            }
            // A fold or a pack count is a constant: the checker combined or
            // counted the pack's elements, and what is left is the number.
            // `call()?` -- the call knows it is propagating from the record
            // the checker made, so this only has to produce its value.
            ExprKind::Propagate(node) => self.lower_expr(ast, types, builder, node.value),
            // `call()!!` -- the value, or the program stops. The call is
            // lowered inside a handler of our own: if it raised, the exception
            // is thrown away and the panic runs, so nothing leaves the
            // function and no `except` is needed around it.
            ExprKind::Abort(node) => {
                self.abort_lowering += 1;
                let value = self.lower_expr(ast, types, builder, node.value);
                self.abort_lowering -= 1;
                value
            }
            // A query is a constant the checker recorded; nothing is left of
            // the expression it asked about.
            ExprKind::ThrowQuery(_) => {
                let value = self
                    .constant_values
                    .get(&node.span)
                    .copied()
                    .ok_or_else(|| {
                        CodegenError::Emit(
                            "internal error: a `??` query has no answer".to_owned(),
                        )
                    })?;
                Ok(Some(
                    self.backend
                        .context()
                        .bool_type()
                        .const_int(u64::from(value != 0), false)
                        .as_basic_value_enum(),
                ))
            }
            ExprKind::Fold(_) | ExprKind::SizeofPack(_) => {
                if let Some(value) = self.constant_values.get(&node.span).copied() {
                    let ty = types.get(expr).cloned().unwrap_or(fallc_sema::Type::INT);
                    if let Some(llvm) = TypeMapper::new(self.backend.context()).map(&ty)
                        && let inkwell::types::BasicTypeEnum::IntType(int) = llvm
                    {
                        return Ok(Some(int.const_int(value as u64, false).as_basic_value_enum()));
                    }
                }
                Err(CodegenError::Emit(format!(
                    "internal error: a compile-time value was not recorded (span {:?}, known {:?})",
                    node.span,
                    self.constant_values.keys().collect::<Vec<_>>()
                )))
            }
            ExprKind::Call(call) => {
                // A call the checker evaluated is its value, with no call
                // emitted at all. That is what makes `square(3)` a constant
                // expression: the function ran during checking, before the
                // program exists, and what is left is the number it returned.
                //
                // A function with `static` storage is the exception: the folded
                // evaluation treats the static as a fresh local per call, but
                // the program's calls share one object. Using the folded number
                // turned three calls to a counter into `1`.
                let folds = match &ast.expr(call.callee).kind {
                    ExprKind::Name(name) => !self.static_function_names.contains(name),
                    _ => true,
                };
                if let Some(value) = self.constant_values.get(&node.span).copied().filter(|_| folds) {
                    let ty = types.get(expr).cloned().unwrap_or(fallc_sema::Type::INT);
                    if let Some(llvm) = TypeMapper::new(self.backend.context()).map(&ty)
                        && let inkwell::types::BasicTypeEnum::IntType(int) = llvm
                    {
                        return Ok(Some(int.const_int(value as u64, false).as_basic_value_enum()));
                    }
                }
                let value = self.lower_call(ast, types, builder, call, node.span)?;
                // A method returning a reference is read as its referent unless
                // an address is what the context wants, exactly as `a[i]` and
                // `*p` are. The checker recorded which it is; without this the
                // pointer was used as the value, so `v int = a.last();` stored
                // an address in an `int` slot and printed garbage.
                match (value, self.deref_reads.get(&node.span).cloned()) {
                    (Some(BasicValueEnum::PointerValue(address)), Some(referent)) => {
                        match TypeMapper::new(self.backend.context()).map(&referent) {
                            Some(llvm) => {
                                let loaded = builder
                                    .build_load(llvm, address, "call.result")
                                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
                                // Kept as storage as well as returned, because
                                // the result is a temporary and something may
                                // still ask for its address: `str(line).data()`
                                // passes the loaded `String` to a method taking
                                // `&const String`. Without this the receiver had
                                // no address and the call was reported as one
                                // this compiler cannot yet generate.
                                let storage =
                                    self.store_temporary(builder, loaded, "call.result.addr")?;
                                self.temporaries
                                    .insert(node.span, LocalBinding { storage, ty: llvm });
                                Ok(Some(loaded))
                            }
                            None => Ok(Some(address.as_basic_value_enum())),
                        }
                    }
                    (other, _) => Ok(other),
                }
            }
            ExprKind::TypedInit(init) => {
                self.lower_typed_init(ast, types, builder, init, node.span)
            }
            ExprKind::NestedInit(init) => {
                self.lower_nested_init(ast, types, builder, init, node.span)
            }
            ExprKind::Field(field) => {
                // `C.stdin` is an `extern` variable of another module: a name
                // whose storage is in another library. The checker recorded
                // which qualified name this use means, because only it can tell
                // a module member from a field of a value.
                if let Some(qualified) = self.external_global_refs.get(&node.span).cloned()
                    && let Some((global, llvm)) = self.external_global(&qualified)
                {
                    return builder
                        .build_load(llvm, global.as_pointer_value(), &field.field)
                        .map(Some)
                        .map_err(|error| CodegenError::Emit(error.to_string()));
                }
                // `Color.RED` is an enumerator: a constant, not a field of any
                // value. The checker folded it, so what is left is the number --
                // and reading the object as an expression would find a type
                // name with no value at all.
                if let Some(value) = self.constant_values.get(&node.span).copied() {
                    let ty = types.get(expr).cloned().unwrap_or(fallc_sema::Type::INT);
                    if let Some(llvm) = TypeMapper::new(self.backend.context()).map(&ty)
                        && let inkwell::types::BasicTypeEnum::IntType(int) = llvm
                    {
                        return Ok(Some(int.const_int(value as u64, false).as_basic_value_enum()));
                    }
                }
                self.lower_field(ast, types, builder, field, node.span)
            }
            ExprKind::NullPointer => Ok(Some(
                self.backend
                    .context()
                    .ptr_type(inkwell::AddressSpace::default())
                    .const_null()
                    .as_basic_value_enum(),
            )),
            ExprKind::Index(index) => {
                // An element of a parameter pack is a number the checker
                // already chose: a pack has no storage to load from.
                if let Some(value) = self.constant_values.get(&node.span).copied() {
                    let ty = types.get(expr).cloned().unwrap_or(fallc_sema::Type::INT);
                    if let Some(llvm) = TypeMapper::new(self.backend.context()).map(&ty)
                        && let inkwell::types::BasicTypeEnum::IntType(int) = llvm
                    {
                        return Ok(Some(int.const_int(value as u64, false).as_basic_value_enum()));
                    }
                }
                let value = self.lower_index(ast, types, builder, index, node.span)?;
                // A `__getitem__` that returns a *reference* names storage, and
                // reading it as a value is a fact about the use -- which the
                // checker recorded, exactly as it does for `*p`.
                match (value, self.deref_reads.get(&node.span).cloned()) {
                    (Some(BasicValueEnum::PointerValue(address)), Some(referent)) => {
                        let llvm = TypeMapper::new(self.backend.context()).map(&referent);
                        match llvm {
                            Some(llvm) => Ok(Some(
                                builder
                                    .build_load(llvm, address, "element")
                                    .map_err(|error| CodegenError::Emit(error.to_string()))?,
                            )),
                            None => Ok(Some(address.as_basic_value_enum())),
                        }
                    }
                    (other, _) => Ok(other),
                }
            }
            ExprKind::Cast(cast) => self.lower_cast(ast, types, builder, cast, node.span),
            ExprKind::Conditional(conditional) => {
                self.lower_conditional(ast, types, builder, conditional, node.span)
            }
            // A `type(expr)` is not a value: the checker refuses one outside a
            // trait query, which is folded at compile time and never reaches
            // here.
            ExprKind::TypeOf(_) | ExprKind::TypeLiteral(_) => {
                Err(CodegenError::waiting_for_spec(
                    "a type used as a value",
                    node.span,
                ))?
            }
            // `sizeof(T)` and `alignof(T)` were folded by the checker into a
            // number, which is what a constant is. The lowering here produces
            // that number so the expression has a value like any other; there
            // is nothing to compute at run time.
            ExprKind::Sizeof(_) | ExprKind::Alignof(_) => {
                let value = self
                    .constant_values
                    .get(&node.span)
                    .copied()
                    .ok_or_else(|| {
                        CodegenError::waiting_for_spec(
                            format!("the value of this `{}`", name_of(&node.kind)),
                            node.span,
                        )
                    })?;
                Ok(Some(
                    self.backend
                        .context()
                        .i64_type()
                        .const_int(value as u64, false)
                        .as_basic_value_enum(),
                ))
            }
            ExprKind::Unresolved(_) => {
                Err(CodegenError::waiting_for_spec("this expression", node.span))
            }
        }
    }

    /// Lowers `a op b` as a call to an operator method, if that is what it is.
    ///
    /// Returns `None` when the operands are numbers, which is the ordinary case
    /// and is lowered to an instruction instead.
    ///
    /// The method's symbol follows the same rule as every other function's, so
    /// it is built from the parameters the call has: the receiver -- which for
    /// `a + b` is `a`, and for the reflected form is `b` -- and the other
    /// operand. No table is consulted, because there is nothing to look up: the
    /// symbol is a function of the types.
    pub(super) fn lower_operator_method(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        binary: &BinaryExpr,
        span: fallc_span::Span,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        // Which method this operation reached was decided by the checker, from
        // the parameter declarations. The written name, the receiver's pointer
        // form and whether the other operand is passed by address are all facts
        // about that decision, so they are read rather than worked out again.
        //
        // Nothing is concluded from the operand types here. Two numbers mean the
        // built-in operation, which is true -- but so is `false` for a recorded
        // method, and a recorded method is the stronger fact.
        let Some(resolved) = self.operator_methods.get(&span).cloned() else {
            return Ok(None);
        };
        let _ = types;
        let function = match self.backend.module().get_function(&resolved.symbol) {
            Some(function) => function,
            None => {
                // The method belongs to a type defined in another module, whose
                // definition was compiled there. This is the declaration that
                // makes the call resolvable at link time, exactly as a method
                // call to an imported type declares its callee.
                let qualified = self
                    .external_symbols
                    .iter()
                    .find(|(_, signature)| signature.symbol == resolved.symbol)
                    .map(|(qualified, _)| qualified.clone())
                    .ok_or_else(|| {
                        CodegenError::waiting_for_spec(
                            format!(
                                "a call to `{}`, which is not defined in this module",
                                resolved.symbol
                            ),
                            span,
                        )
                    })?;
                self.declare_external(&resolved.symbol, &qualified, span)?
            }
        };

        // The receiver is the left operand, unless this is the reflected form,
        // which is written for the case where the *right* operand's type is the
        // one that defines the operation.
        let (receiver, argument) = if resolved.reflected {
            (binary.rhs, binary.lhs)
        } else {
            (binary.lhs, binary.rhs)
        };

        // The receiver is an address: a pointer operand already is one, and a
        // value has its address taken. That is what lets an operator method
        // declare `this *N` and be applied to a value.
        // A string-literal receiver is constructed into the method's type
        // first: `"ab" * 3` builds a `String` and calls `__mul__` on it.
        let receiver_address = if let Some(symbol) = self
            .literal_constructions
            .get(&ast.expr(receiver).span)
            .cloned()
        {
            self.lower_literal_construction(ast, types, builder, receiver, &symbol, span)?
        } else {
            let receiver_value = self
                .lower_expr(ast, types, builder, receiver)?
                .ok_or_else(|| CodegenError::waiting_for_spec("this operand", span))?;
            match receiver_value {
                BasicValueEnum::PointerValue(pointer) => pointer,
                value => self.store_temporary(builder, value, "receiver")?,
            }
        };

        // The other operand is passed as the parameter says. A value parameter
        // takes the value; a pointer parameter takes an address, which is why
        // the checker recorded which one it is.
        let argument_value: inkwell::values::BasicMetadataValueEnum<'static> =
            if resolved.argument_by_address {
            let value = self
                .lower_expr(ast, types, builder, argument)?
                .ok_or_else(|| CodegenError::waiting_for_spec("this operand", span))?;
            match value {
                    BasicValueEnum::PointerValue(pointer) => pointer.as_basic_value_enum().into(),
                    value => self
                        .store_temporary(builder, value, "operand")?
                        .as_basic_value_enum()
                        .into(),
                }
            } else {
                // A string literal that has to become a declared type is built
                // into a temporary, and that temporary's address is the
                // operand: the method's parameter is a pointer to the type, not
                // the type.
                if let Some(symbol) = self
                    .literal_constructions
                    .get(&ast.expr(argument).span)
                    .cloned()
                {
                    let address = self.lower_literal_construction(
                        ast, types, builder, argument, &symbol, span,
                    )?;
                    address.as_basic_value_enum().into()
                } else {
                    // An unsuffixed literal adopted the method's parameter type,
                    // so it is *converted* to it here. Lowering it as it stands
                    // would pass the default `int` to a parameter declared
                    // `usize`, which LLVM rejects -- and the difference is real,
                    // not a formality.
                    let value = self
                        .lower_expr(ast, types, builder, argument)?
                        .ok_or_else(|| CodegenError::waiting_for_spec("this operand", span))?;
                    match self.adopted_types.get(&ast.expr(argument).span).cloned() {
                        Some(wanted) => self
                            .convert_value(ast, types, builder, argument, value, &wanted, span)?
                            .into(),
                        None => value.into(),
                    }
                }
            };
        let value = self.build_value_call(
            builder,
            function,
            &resolved.symbol,
            vec![receiver_address.into(), argument_value],
            span,
        )?;
        let Some(value) = self.unwrap_throwing_value(ast, types, builder, span, value)? else {
            return Ok(None);
        };
        // The result is stored so a member of it can be reached:
        // `("ab" * 3).data()` asks the result for its address. An operator call
        // returns a value with no storage otherwise. An out-of-line result
        // already has its slot, recorded by `build_value_call`.
        if !self.temporaries.contains_key(&span) {
            let storage = self.store_temporary(builder, value, "op.value")?;
            self.temporaries.insert(
                span,
                LocalBinding {
                    storage,
                    ty: value.get_type(),
                },
            );
        }
        Ok(Some(value))
    }

    /// Stores a value in a fresh allocation and returns the address.
    ///
    /// Used where an address is wanted for a value that has none -- passing an
    /// operand to a pointer parameter, say. The storage is unnamed and lives
    /// for the enclosing function.
    pub(super) fn store_temporary(
        &self,
        builder: &Builder<'static>,
        value: BasicValueEnum<'static>,
        name: &str,
    ) -> Result<inkwell::values::PointerValue<'static>, CodegenError> {
        let storage = builder
            .build_alloca(value.get_type(), name)
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        builder
            .build_store(storage, value)
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        Ok(storage)
    }

    /// Returns an expression's type, from the table or from a template
    /// parameter that the table has no entry for.
    ///
    /// A value template parameter is not an expression the checker saw in the
    /// ordinary way: it names a value that only exists inside an instantiation,
    /// so its type is the one the template declared rather than one recorded
    /// against a span.
    pub(super) fn expression_type(&self, ast: &Ast, types: &TypeTable, expr: ExprId) -> Option<fallc_sema::Type> {
        if let Some(ty) = types.get(expr) {
            return Some(ty.clone());
        }
        let ExprKind::Name(name) = &ast.expr(expr).kind else {
            return None;
        };
        self.template_value_types.get(name).cloned()
    }

    /// Returns the address `a[i]` counts from.
    ///
    /// A pointer or a reference *is* an address, so it is read as a value. An
    /// array is not: its address is its storage, which is the decay an argument
    /// gets. Reading an array as a value would load the whole of it -- for
    /// `m [256][256]double` half a megabyte that was then thrown away, and a
    /// value LLVM's instruction selection cannot handle at all.
    pub(super) fn indexed_address(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        object: ExprId,
        object_ty: &fallc_sema::Type,
    ) -> Option<inkwell::values::PointerValue<'static>> {
        // An object whose type already denotes an address is read for it. A
        // pointer, an unsized array and a reference all do; anything else --
        // an array, a nominal type -- is a value with storage.
        let names_address = object_ty.referent().is_some()
            || matches!(object_ty, Type::Pointer { .. } | Type::UnsizedArray(_));
        if names_address
            && let Ok(Some(BasicValueEnum::PointerValue(pointer))) =
                self.lower_expr(ast, types, builder, object)
        {
            return Some(pointer);
        }
        // A call's result lives in the temporary its lowering kept; asking for
        // its lvalue would lower the call a second time and run it twice.
        if matches!(&ast.expr(object).kind, ExprKind::Call(_))
            && let Some(binding) = self.temporaries.get(&ast.expr(object).span).copied()
        {
            return Some(binding.storage);
        }
        self.lower_lvalue(ast, types, builder, object)
            .map(|binding| binding.storage)
    }

    /// Returns the element type an index selects.
    ///
    /// A reference names its referent, so it is the referent that is indexed:
    /// `c &[N][N]double` is an array to subscript, and `c[i]` is a row of it.
    /// An unsized array is a pointer whose length is not part of the type.
    pub(super) fn indexed_element_type(
        &self,
        object_ty: &fallc_sema::Type,
    ) -> Option<inkwell::types::BasicTypeEnum<'static>> {
        let mapper = TypeMapper::new(self.backend.context());
        let indexed = object_ty.referent().unwrap_or(object_ty);
        match indexed {
            Type::Array { element, .. } => mapper.map(element),
            Type::Pointer { pointee, .. } => mapper.map(pointee),
            Type::UnsizedArray(element) => mapper.map(element),
            _ => None,
        }
    }

    /// Lowers `a[i]`.
    ///
    /// An array or pointer is indexed with a GEP, which is what the syntax
    /// means for them. Any other type resolved to `__getitem__`, and that call
    /// is what this emits: the checker recorded which overload, so nothing is
    /// looked up here.
    pub(super) fn lower_index(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        index: &IndexExpr,
        span: fallc_span::Span,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        // A recorded method means the object's own type defines the operation.
        if let Some(symbol) = self.index_methods.get(&span).cloned() {
            return self.lower_index_method(ast, types, builder, index, &symbol, span);
        }

        let object_ty = self
            .expression_type(ast, types, index.object)
            .ok_or_else(|| CodegenError::waiting_for_spec("this indexed value's type", span))?;
        let element_ty = self
            .indexed_element_type(&object_ty)
            .ok_or_else(|| CodegenError::waiting_for_spec("an index into this type", span))?;

        let base = self
            .indexed_address(ast, types, builder, index.object, &object_ty)
            .ok_or_else(|| {
                CodegenError::waiting_for_spec("this indexed value's address", span)
            })?;

        let subscript = self
            .lower_expr(ast, types, builder, index.index)?
            .ok_or_else(|| CodegenError::waiting_for_spec("this subscript", span))?;
        let BasicValueEnum::IntValue(subscript) = subscript else {
            return Err(CodegenError::waiting_for_spec("a non-integer subscript", span));
        };
        // The subscript may be narrower or wider than the pointer's index type,
        // so it is widened; a negative subscript is the caller's business, as
        // in C.
        let subscript = self.widen_to_pointer_size(builder, subscript)?;
        let address = build_index_gep(builder, element_ty, base, subscript)?;
        let value = builder
            .build_load(element_ty, address, "elem")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        Ok(Some(value))
    }

    /// Lowers `a[i]` as a call to `__getitem__`.
    pub(super) fn lower_index_method(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        index: &IndexExpr,
        symbol: &str,
        span: fallc_span::Span,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        let function = self.resolved_function(symbol, "`__getitem__`", span)?;
        let receiver = self
            .lower_expr(ast, types, builder, index.object)?
            .ok_or_else(|| CodegenError::waiting_for_spec("this indexed value", span))?;
        let receiver_address = match receiver {
            BasicValueEnum::PointerValue(pointer) => pointer,
            value => {
                let storage = builder
                    .build_alloca(value.get_type(), "receiver")
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
                builder
                    .build_store(storage, value)
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
                storage
            }
        };
        let subscript = self.lower_adopted_argument(ast, types, builder, index.index, span)?;
        let value = self.build_value_call(
            builder,
            function,
            symbol,
            vec![receiver_address.into(), subscript],
            span,
        )?;
        self.unwrap_throwing_value(ast, types, builder, span, value)
    }

    /// Widens an integer to the width a pointer index needs.
    pub(super) fn widen_to_pointer_size(
        &self,
        builder: &Builder<'static>,
        value: inkwell::values::IntValue<'static>,
    ) -> Result<inkwell::values::IntValue<'static>, CodegenError> {
        // The index type is the target's pointer width, so that a GEP takes the
        // same width the pointer does on any platform.
        let target_data = self
            .backend
            .target_machine(inkwell::OptimizationLevel::None)
            .map_err(|error| error.to_diagnostic())
            .map(|machine| machine.get_target_data())
            .map_err(|_| CodegenError::Emit("cannot create a target machine".to_owned()))?;
        let target_bits = self.backend.context().ptr_sized_int_type(&target_data, None);
        let from_bits = value.get_type().get_bit_width();
        let to_bits = target_bits.get_bit_width();
        if from_bits == to_bits {
            return Ok(value);
        }
        let wider = if from_bits < to_bits {
            builder
                .build_int_z_extend(value, target_bits, "idx")
                .map_err(|error| CodegenError::Emit(error.to_string()))?
        } else {
            builder
                .build_int_truncate(value, target_bits, "idx")
                .map_err(|error| CodegenError::Emit(error.to_string()))?
        };
        Ok(wider)
    }

    /// Returns the target data that sizes are measured with.
    pub(super) fn target_data(&self) -> Result<inkwell::targets::TargetData, CodegenError> {
        self.backend
            .target_machine(inkwell::OptimizationLevel::None)
            .map_err(|error| error.to_diagnostic())
            .map(|machine| machine.get_target_data())
            .map_err(|_| CodegenError::Emit("cannot create a target machine".to_owned()))
    }

    /// Returns the type an assignment target or an increment operand has.
    ///
    /// A dereference names the pointee rather than the reference `*p` has as an
    /// expression, so the syntactic form decides, exactly as `infer_lvalue`
    /// does in the checker. Everything else has a recorded type, and a reference
    /// there reads as its referent, which is what an lvalue is.
    pub(super) fn lvalue_value_type(&self, ast: &Ast, types: &TypeTable, target: ExprId) -> Option<Type> {
        match &ast.expr(target).kind {
            ExprKind::Unary(unary) if unary.op == UnaryOp::Deref => {
                let pointer = self.expression_type(ast, types, unary.operand)?;
                let pointer = pointer.referent().cloned().unwrap_or(pointer);
                pointer.pointee().cloned()
            }
            _ => {
                let ty = self.expression_type(ast, types, target)?;
                Some(ty.referent().cloned().unwrap_or(ty))
            }
        }
    }

    /// Returns what a pointer expression points at, looking through a reference
    /// to the pointer as well.
    pub(super) fn pointer_pointee_of(&self, ast: &Ast, types: &TypeTable, expr: ExprId) -> Option<Type> {
        let ty = self.expression_type(ast, types, expr)?;
        let ty = ty.referent().cloned().unwrap_or(ty);
        ty.pointee().cloned()
    }

    /// Emits `base` advanced by `index` elements, scaled by the pointee's size.
    ///
    /// A GEP does the scaling and the struct layout, so `p + 1` on a `*Pair`
    /// moves by `sizeof(Pair)` without this code knowing the number. It is the
    /// same helper `p[i]` uses, and it carries the same safety argument: the
    /// element type comes from the pointer's own pointee through the mapper that
    /// built the pointer. The in-bounds form is the one used everywhere else in
    /// this crate, and it matches C: arithmetic within an array and one past its
    /// end is defined, anything further is not.
    pub(super) fn build_pointer_offset(
        &self,
        builder: &Builder<'static>,
        base: inkwell::values::PointerValue<'static>,
        index: inkwell::values::IntValue<'static>,
        pointee: &Type,
        span: fallc_span::Span,
    ) -> Result<inkwell::values::PointerValue<'static>, CodegenError> {
        let element_ty = TypeMapper::new(self.backend.context())
            .map(pointee)
            .ok_or_else(|| {
                CodegenError::waiting_for_spec(
                    "pointer arithmetic on a type with no known size",
                    span,
                )
            })?;
        build_index_gep(builder, element_ty, base, index)
    }

    /// Declares `llvm.pow` for a float width, if it is not there.
    pub(super) fn declare_float_pow(
        &self,
        float_ty: inkwell::types::FloatType<'static>,
    ) -> Result<inkwell::values::FunctionValue<'static>, CodegenError> {
        // The intrinsic's name carries its type, which LLVM checks.
        let name = format!("llvm.pow.f{}", float_ty.get_bit_width());
        if let Some(existing) = self.backend.module().get_function(&name) {
            return Ok(existing);
        }
        let signature = float_ty.fn_type(&[float_ty.into(), float_ty.into()], false);
        Ok(self.backend.module().add_function(&name, signature, None))
    }

    /// Raises an integer to an integer power.
    ///
    /// LLVM has no integer power instruction, and `llvm.powi` takes a
    /// floating-point exponent, so this is a loop: square-and-multiply, which is
    /// the standard way and costs a logarithmic number of multiplications
    /// rather than a linear one.
    ///
    /// A negative exponent is not a whole number, so it yields `0` -- which is
    /// what truncating the true result does for every base but `1` and `-1`,
    /// and those two are given their exact answers. `0 ** 0` is `1`, because a
    /// zero exponent performs no multiplication.
    pub(super) fn build_integer_power(
        &mut self,
        builder: &Builder<'static>,
        base: inkwell::values::IntValue<'static>,
        exponent: inkwell::values::IntValue<'static>,
        name: &str,
    ) -> Result<BasicValueEnum<'static>, CodegenError> {
        let function = self
            .current_function
            .ok_or_else(|| CodegenError::Emit("a power outside a function".to_owned()))?;
        let context = self.backend.context();
        // The exponent is truncated to the base's width, so the loop counter and
        // the base agree; a wider exponent than the base cannot be represented
        // anyway.
        let int_type = base.get_type();
        let exponent = if exponent.get_type().get_bit_width() > int_type.get_bit_width() {
            builder
                .build_int_truncate(exponent, int_type, "exp")
                .map_err(|error| CodegenError::Emit(error.to_string()))?
        } else if exponent.get_type().get_bit_width() < int_type.get_bit_width() {
            builder
                .build_int_z_extend(exponent, int_type, "exp")
                .map_err(|error| CodegenError::Emit(error.to_string()))?
        } else {
            exponent
        };

        let one = int_type.const_int(1, false);
        let zero = int_type.const_zero();

        let entry = builder.get_insert_block().expect("a current block");
        let loop_block = context.append_basic_block(function, &format!("{name}.loop"));
        let body_block = context.append_basic_block(function, &format!("{name}.body"));
        let done_block = context.append_basic_block(function, &format!("{name}.done"));

        // `result` and `base` are carried through the loop as phi nodes, which
        // is what lets the optimiser turn this back into a constant when both
        // operands are known.
        builder
            .build_unconditional_branch(loop_block)
            .map_err(|error| CodegenError::Emit(error.to_string()))?;

        builder.position_at_end(loop_block);
        let result = builder
            .build_phi(int_type, "result")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        let factor = builder
            .build_phi(int_type, "factor")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        let counter = builder
            .build_phi(int_type, "counter")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        result.add_incoming(&[(&one, entry)]);
        factor.add_incoming(&[(&base, entry)]);
        counter.add_incoming(&[(&exponent, entry)]);

        // A negative exponent terminates immediately with the truncated answer.
        let exponent_positive = builder
            .build_int_compare(
                inkwell::IntPredicate::SGT,
                counter.as_basic_value().into_int_value(),
                zero,
                "exp_positive",
            )
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        builder
            .build_conditional_branch(exponent_positive, body_block, done_block)
            .map_err(|error| CodegenError::Emit(error.to_string()))?;

        builder.position_at_end(body_block);
        let result_value = result.as_basic_value().into_int_value();
        let factor_value = factor.as_basic_value().into_int_value();
        let counter_value = counter.as_basic_value().into_int_value();
        // Square-and-multiply: multiply the result when the low bit is set.
        let low_bit = builder
            .build_and(counter_value, one, "low")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        let is_odd = builder
            .build_int_compare(inkwell::IntPredicate::NE, low_bit, zero, "is_odd")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        let multiplied = builder
            .build_int_mul(result_value, factor_value, "mul")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        let next_result = builder
            .build_select(is_odd, multiplied, result_value, "next_result")
            .map_err(|error| CodegenError::Emit(error.to_string()))?
            .into_int_value();
        let squared = builder
            .build_int_mul(factor_value, factor_value, "square")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        // An arithmetic shift halves a signed counter, a logical one an
        // unsigned; the exponent's sign was already handled above, so either
        // works, and the shift matches how the loop counts down.
        let halved = builder
            .build_right_shift(counter_value, one, false, "halve")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        builder
            .build_unconditional_branch(loop_block)
            .map_err(|error| CodegenError::Emit(error.to_string()))?;

        result.add_incoming(&[(&next_result, body_block)]);
        factor.add_incoming(&[(&squared, body_block)]);
        counter.add_incoming(&[(&halved, body_block)]);

        builder.position_at_end(done_block);
        Ok(result.as_basic_value())
    }

    /// Lowers `a <=> b` for floating-point operands.
    pub(super) fn lower_float_cmp(
        &mut self,
        builder: &Builder<'static>,
        lhs: BasicValueEnum<'static>,
        rhs: BasicValueEnum<'static>,
        span: fallc_span::Span,
    ) -> Result<BasicValueEnum<'static>, CodegenError> {
        let (BasicValueEnum::FloatValue(lhs), BasicValueEnum::FloatValue(rhs)) = (lhs, rhs) else {
            return Err(CodegenError::waiting_for_spec(
                "a three-way comparison of these operands",
                span,
            ));
        };
        let is_less = builder
            .build_float_compare(inkwell::FloatPredicate::OLT, lhs, rhs, "cmp.lt")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        let is_greater = builder
            .build_float_compare(inkwell::FloatPredicate::OGT, lhs, rhs, "cmp.gt")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        let int_type = self.backend.context().i32_type();
        let negative = builder
            .build_select(
                is_less,
                int_type.const_int(u64::MAX, true),
                int_type.const_zero(),
                "cmp.neg",
            )
            .map_err(|error| CodegenError::Emit(error.to_string()))?
            .into_int_value();
        builder
            .build_select(
                is_greater,
                int_type.const_int(1, false),
                negative,
                "cmp",
            )
            .map_err(|error| CodegenError::Emit(error.to_string()))
    }

}
