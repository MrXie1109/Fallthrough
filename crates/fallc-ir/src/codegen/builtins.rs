//! Builtins, casts and the checks that make a bad value a diagnostic.
//!
//! `declare_builtin`, `lower_cast`, `lower_conversion` and the arithmetic
//! guards share one shape: a value the language allows but the machine does
//! not, turned into code that reports instead of misbehaving.

use super::*;

impl<'ctx> Lowerer<'ctx> {
    /// Declares a builtin function in the module.
    ///
    /// The signature is taken from the builtin table, so the declaration
    /// matches what the type checker accepted.
    pub(super) fn declare_builtin(
        &mut self,
        name: &str,
        span: fallc_span::Span,
    ) -> Result<inkwell::values::FunctionValue<'static>, CodegenError> {
        let builtin = fallc_sema::builtins::lookup(name).ok_or_else(|| {
            CodegenError::waiting_for_spec(format!("a call to the unknown function `{name}`"), span)
        })?;

        let context = self.backend.context();
        let mut param_types = Vec::with_capacity(builtin.params.len());
        for param in builtin.params {
            param_types.push(match param {
                // A string is a pointer to its bytes for now.
                fallc_sema::builtins::BuiltinType::Bytes => {
                    context.ptr_type(inkwell::AddressSpace::default()).into()
                }
                // The bare `int`, which is what an integer builtin parameter
                // is declared as.
                fallc_sema::builtins::BuiltinType::Integer => {
                    context.i32_type().into()
                }
                fallc_sema::builtins::BuiltinType::Void => {
                    return Err(CodegenError::waiting_for_spec(
                        "a builtin with a void parameter",
                        span,
                    ));
                }
                // `str` and `repr` never reach here: the checker resolves them
                // into a method call or a rendering, so no declaration of them
                // is ever emitted. Reaching this would mean the checker let a
                // call through that it should have taken over.
                fallc_sema::builtins::BuiltinType::Any
                | fallc_sema::builtins::BuiltinType::Text => {
                    return Err(CodegenError::Emit(format!(
                        "internal error: `{name}` is lowered by the compiler and has no \
                         declaration"
                    )));
                }
            });
        }
        let signature = match builtin.ret {
            fallc_sema::builtins::BuiltinType::Void => {
                context.void_type().fn_type(&param_types, false)
            }
            fallc_sema::builtins::BuiltinType::Bytes => context
                .ptr_type(inkwell::AddressSpace::default())
                .fn_type(&param_types, false),
            fallc_sema::builtins::BuiltinType::Integer => {
                context.i32_type().fn_type(&param_types, false)
            }
            fallc_sema::builtins::BuiltinType::Any
            | fallc_sema::builtins::BuiltinType::Text => {
                return Err(CodegenError::Emit(format!(
                    "internal error: `{name}` is lowered by the compiler and has no \
                     declaration"
                )));
            }
        };
        Ok(self.backend.module().add_function(name, signature, None))
    }

    /// Returns the address an aggregate-valued expression lives at.
    ///
    /// An aggregate returned out of line has no value form: what it has is the
    /// slot the callee wrote. A place -- a name, a field, an element, a
    /// dereference -- has an address of its own. Either way the aggregate is
    /// never loaded, so a `[256][256]double` never becomes an SSA value.
    pub(super) fn lower_aggregate_source(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        expr: ExprId,
        span: fallc_span::Span,
    ) -> Result<inkwell::values::PointerValue<'static>, CodegenError> {
        let is_reference = types.get(expr).is_some_and(Type::is_reference);
        let names_storage = matches!(
            &ast.expr(expr).kind,
            ExprKind::Name(_) | ExprKind::Unary(_) | ExprKind::Field(_) | ExprKind::Index(_)
        );
        if names_storage
            && !is_reference
            && let Some(binding) = self.lower_lvalue(ast, types, builder, expr)
        {
            return Ok(binding.storage);
        }
        // A call already wrote its aggregate into the temporary its lowering
        // recorded. Lowering it again would run it twice; asking it for an
        // lvalue would do the same.
        if matches!(&ast.expr(expr).kind, ExprKind::Call(_))
            && let Some(binding) = self.temporaries.get(&ast.expr(expr).span).copied()
        {
            return Ok(binding.storage);
        }
        match self.lower_expr(ast, types, builder, expr)? {
            Some(BasicValueEnum::PointerValue(address)) => Ok(address),
            // The expression produced its aggregate *as a value* -- a call
            // whose result is an array, say -- and lowering it wrote that value
            // into a temporary of its own. The address exists now, even though
            // the value it returned is the aggregate.
            Some(_) => self
                .temporaries
                .get(&ast.expr(expr).span)
                .map(|binding| binding.storage)
                .ok_or_else(|| {
                    CodegenError::waiting_for_spec(
                        "an aggregate value that has no address",
                        span,
                    )
                }),
            None => Err(CodegenError::waiting_for_spec(
                "an aggregate value that has no address",
                span,
            )),
        }
    }

    /// Resolves an expression to the storage it denotes, if it denotes any.
    pub(super) fn lower_lvalue(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        target: ExprId,
    ) -> Option<LocalBinding<'static>> {
        match &ast.expr(target).kind {
            ExprKind::Field(field) if self.external_global_refs.contains_key(&ast.expr(target).span) => {
                let qualified = self.external_global_refs.get(&ast.expr(target).span)?.clone();
                let _ = field;
                let (global, ty) = self.external_global(&qualified)?;
                Some(LocalBinding {
                    storage: global.as_pointer_value(),
                    ty,
                })
            }
            ExprKind::Name(name) => {
                // A bare name bound to another module's exported variable: the
                // checker recorded which qualified name it stands for, and the
                // symbol is that module's object. Without this a write to an
                // imported global looked for a local one and found nothing.
                if let Some(qualified) = self.external_global_refs.get(&ast.expr(target).span).cloned()
                    && let Some((global, ty)) = self.external_global(&qualified)
                {
                    return Some(LocalBinding {
                        storage: global.as_pointer_value(),
                        ty,
                    });
                }
                self.lookup_binding(name)
                .or_else(|| {
                    self.globals
                        .get(name)
                        .copied()
                        .map(|(global, ty)| LocalBinding {
                            storage: global.as_pointer_value(),
                            ty,
                        })
                })
                // A function has an address but no storage of its own: `&f` is
                // the function pointer, and there is nothing to load.
                .or_else(|| {
                    let wanted = types.get(target).cloned();
                    self.function_pointer(name, wanted.as_ref())
                        .map(|pointer| LocalBinding {
                            storage: pointer,
                            ty: pointer.get_type().into(),
                        })
                })
            }
            // A dereference resolves to the pointer's value used as storage.
            //
            // The pointee type cannot be recovered from the LLVM pointer, since
            // LLVM pointers are opaque, so it comes from the type table.
            ExprKind::Unary(unary) if unary.op == UnaryOp::Deref => {
                // `*it` on a class iterator calls its `__deref__`, and that
                // call's result is the address: `*it = v` writes the element.
                let span = ast.expr(target).span;
                if let Some(symbol) = self.unary_methods.get(&span).cloned() {
                    let address = self
                        .lower_unary_method(ast, types, builder, unary.operand, &symbol, span)
                        .ok()??;
                    let BasicValueEnum::PointerValue(storage) = address else {
                        return None;
                    };
                    // What the reference points at: the type the checker read
                    // through it, or the expression's own type.
                    let referent = self
                        .deref_reads
                        .get(&span)
                        .cloned()
                        .or_else(|| types.get(target).cloned())
                        .unwrap_or(Type::INT);
                    let ty = TypeMapper::new(self.backend.context()).map(&referent)?;
                    return Some(LocalBinding { storage, ty });
                }
                let pointee = types
                    .get(unary.operand)
                    .and_then(|ty| ty.pointee().cloned())?;
                let mapper = TypeMapper::new(self.backend.context());
                let llvm_pointee = mapper.map(&pointee)?;
                let pointer = self.lower_expr(ast, types, builder, unary.operand).ok()??;
                let BasicValueEnum::PointerValue(storage) = pointer else {
                    return None;
                };
                Some(LocalBinding {
                    storage,
                    ty: llvm_pointee,
                })
            }
            // A field denotes storage: its address is the GEP.
            ExprKind::Field(field) => {
                let (storage, ty) = self
                    .field_storage(ast, types, builder, field, ast.expr(target).span)
                    .ok()??;
                Some(LocalBinding { storage, ty })
            }
            // An index denotes storage: its address is the element's.
            ExprKind::Index(index) => {
                // A type whose `__getitem__` returns a *reference* names
                // storage, and that call's result is the address. This is what
                // makes `a[i] = v` work with no second method: `[]` means "the
                // element", and an element is a place.
                let span = ast.expr(target).span;
                if let Some(symbol) = self.index_methods.get(&span).cloned() {
                    let address = self
                        .lower_index_method(ast, types, builder, index, &symbol, span)
                        .ok()??;
                    let BasicValueEnum::PointerValue(storage) = address else {
                        return None;
                    };
                    // What the reference points at, which is the type the
                    // checker gave the whole expression.
                    let referent = types
                        .get(target)
                        .and_then(|ty| ty.pointee().cloned())
                        .unwrap_or(Type::INT);
                    let ty = TypeMapper::new(self.backend.context()).map(&referent)?;
                    return Some(LocalBinding { storage, ty });
                }
                let (storage, ty) = self
                    .index_storage(ast, types, builder, index, span)
                    .ok()??;
                Some(LocalBinding { storage, ty })
            }
            // A call that returns a reference names storage: its result *is*
            // the address, so it is the slot to write. `a.last_mut() = 99` and
            // `a[i] = v` mean the same thing, and the checker has already
            // refused a call that returns a value rather than a place.
            ExprKind::Call(call) => {
                let span = ast.expr(target).span;
                // `lower_call` and not `lower_expr`: the latter reads a
                // reference result as its referent, which is right for a value
                // and wrong here -- the address *is* the storage being written.
                //
                // A call that yields a reference *is* storage; one that yields a
                // value is not, and its result is a temporary the lowering kept.
                // `str(line).data()` is the second kind: the call produces a
                // `String` and the method wants its address. Reporting those as
                // having no address made every such call unimplementable.
                match self.lower_call(ast, types, builder, call, span).ok()? {
                    Some(BasicValueEnum::PointerValue(storage))
                        if types.get(target).is_some_and(Type::is_reference) =>
                    {
                        // What the reference points at, which is the type the
                        // checker gave the whole expression.
                        let referent = types.get(target).cloned().unwrap_or(Type::INT);
                        let ty = TypeMapper::new(self.backend.context()).map(&referent)?;
                        Some(LocalBinding { storage, ty })
                    }
                    // A value: the lowering kept it, so it has storage after
                    // all -- just not one the expression names.
                    _ => self.temporaries.get(&span).copied(),
                }
            }
            // A value the compiler produced and kept, such as the result of
            // `str(x)`. It is storage like any other; it simply was never
            // given a name.
            _ => self.temporaries.get(&ast.expr(target).span).copied(),
        }
    }

    /// Returns the address of `a[i]` and the element's type.
    ///
    /// Only an array or a pointer reaches here, because a type that defines
    /// `__getitem__` returns a value rather than denoting storage, and the
    /// checker refuses to assign to one.
    pub(super) fn index_storage(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        index: &IndexExpr,
        span: fallc_span::Span,
    ) -> Result<Option<(inkwell::values::PointerValue<'static>, inkwell::types::BasicTypeEnum<'static>)>, CodegenError> {
        let object_ty = types
            .get(index.object)
            .cloned()
            .ok_or_else(|| CodegenError::waiting_for_spec("this indexed value's type", span))?;
        let Some(element_ty) = self.indexed_element_type(&object_ty) else {
            return Ok(None);
        };

        let Some(base) = self.indexed_address(ast, types, builder, index.object, &object_ty) else {
            return Ok(None);
        };
        let Some(BasicValueEnum::IntValue(subscript)) =
            self.lower_expr(ast, types, builder, index.index)?
        else {
            return Ok(None);
        };
        let subscript = self.widen_to_pointer_size(builder, subscript)?;
        let address = build_index_gep(builder, element_ty, base, subscript)?;
        Ok(Some((address, element_ty)))
    }

    /// Returns the Fallthrough type of an operand, if it is an int or float.
    pub(super) fn int_or_float_type(&self, ast: &Ast, types: &TypeTable, expr: ExprId) -> Option<Type> {
        let _ = ast;
        // A literal expression may have no recorded type when it appears where
        // no expectation was published, such as inside a binary operation
        // whose other operand is also a literal. The default integer is the
        // same choice the literal lowering makes.
        types.get(expr).cloned().or(Some(Type::INT))
    }

    /// Returns `true` if the LLVM type is an integer.
    pub(super) fn type_is_numeric(&self, ty: inkwell::types::BasicTypeEnum<'static>) -> bool {
        matches!(
            ty,
            inkwell::types::BasicTypeEnum::IntType(_) | inkwell::types::BasicTypeEnum::FloatType(_)
        )
    }

    /// Emits a divide-by-zero guard in debug builds.
    ///
    /// In release builds division by zero is undefined and the instruction is
    /// emitted bare. In debug builds it traps with a panic, which is the
    /// specified behaviour and turns a silent wrong answer into a diagnosable
    /// one.
    ///
    /// The trap calls `__builtin_panic`, which takes a `String` and does not
    /// return. `std.panic` is not defined yet, so this builtin is the interim
    /// abort; the message is a fixed string rather than anything formatted.
    pub(super) fn emit_divide_by_zero_check(
        &mut self,
        builder: &Builder<'static>,
        divisor: BasicValueEnum<'static>,
        op: BinaryOp,
        span: fallc_span::Span,
    ) -> Result<(), CodegenError> {
        let BasicValueEnum::IntValue(divisor) = divisor else {
            // Floating point division by zero yields infinity, not a trap.
            return Ok(());
        };

        let zero = divisor.get_type().const_zero();
        let is_zero = builder
            .build_int_compare(inkwell::IntPredicate::EQ, divisor, zero, "iszero")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;

        let function = self.current_function.expect("a function is being lowered");
        let context = self.backend.context();
        let trap = context.append_basic_block(function, "divzero");
        let ok = context.append_basic_block(function, "divok");
        builder
            .build_conditional_branch(is_zero, trap, ok)
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        for target in [trap, ok] {
            if !self.branched_to.contains(&target) {
                self.branched_to.push(target);
            }
        }

        builder.position_at_end(trap);
        let message = match op {
            BinaryOp::Rem => "remainder by zero",
            _ => "division by zero",
        };
        self.emit_panic(builder, message, span)?;

        builder.position_at_end(ok);
        Ok(())
    }

    /// Emits an overflow guard for a signed operation in debug builds.
    ///
    /// # How
    ///
    /// LLVM's overflow-reporting intrinsics -- `llvm.sadd.with.overflow` and
    /// its siblings -- compute the result and a flag in one instruction, so the
    /// check costs nothing beyond a branch when it does not fire. inkwell does
    /// not wrap them, so the declaration is built here and called by name; that
    /// is also what lets the same routine serve `+`, `-`, and `*`.
    ///
    /// The intrinsic returns a struct of the result and the flag, so the result
    /// is extracted here and *returned*: the caller uses it in place of the
    /// plain arithmetic, which is what keeps the operation from being computed
    /// twice.
    ///
    /// Only signed operations are checked. Unsigned arithmetic wraps, which is
    /// well defined, so a check there would reject a valid program.
    ///
    /// Returns `None` when there is nothing to check -- a floating-point
    /// operand, or an unsigned type -- and the caller emits the plain
    /// operation.
    pub(super) fn emit_signed_overflow_check(
        &mut self,
        builder: &Builder<'static>,
        lhs: BasicValueEnum<'static>,
        rhs: BasicValueEnum<'static>,
        op: BinaryOp,
        span: fallc_span::Span,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        let (BasicValueEnum::IntValue(lhs), BasicValueEnum::IntValue(rhs)) = (lhs, rhs) else {
            // Floating-point overflow is not a trap; it becomes infinity.
            return Ok(None);
        };
        if lhs.get_type() != rhs.get_type() {
            // The checker guarantees the operands agree; if they do not, the
            // plain operation will fail verification and say so.
            return Ok(None);
        }

        let base = match op {
            BinaryOp::Add => "llvm.sadd.with.overflow",
            BinaryOp::Sub => "llvm.ssub.with.overflow",
            BinaryOp::Mul => "llvm.smul.with.overflow",
            // Nothing else reports overflow.
            _ => return Ok(None),
        };
        let int_type = lhs.get_type();
        // An intrinsic's name carries its argument types: LLVM refuses a plain
        // `llvm.sadd.with.overflow` and says so, which is how this was found.
        let name = format!("{base}.i{}", int_type.get_bit_width());
        let context = self.backend.context();
        // The intrinsic takes the operands and returns `{ iN, i1 }`.
        let struct_type = context.struct_type(
            &[int_type.into(), context.bool_type().into()],
            false,
        );
        let signature = struct_type.fn_type(&[int_type.into(), int_type.into()], false);
        let intrinsic = self.backend.module().get_function(&name).unwrap_or_else(|| {
            let function = self.backend.module().add_function(&name, signature, None);
            // The intrinsic's own attributes, so LLVM knows it has no side
            // effects and may be optimised like the arithmetic it replaces.
            function.add_attribute(
                inkwell::attributes::AttributeLoc::Function,
                context.create_enum_attribute(
                    inkwell::attributes::Attribute::get_named_enum_kind_id("nounwind"),
                    0,
                ),
            );
            function
        });

        let call = builder
            .build_call(intrinsic, &[lhs.into(), rhs.into()], "ovf")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        let Some(BasicValueEnum::StructValue(pair)) = call.try_as_basic_value().basic() else {
            return Ok(None);
        };
        let result = builder
            .build_extract_value(pair, 0, "ovf.result")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        let overflowed = builder
            .build_extract_value(pair, 1, "ovf.flag")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        let BasicValueEnum::IntValue(overflowed) = overflowed else {
            return Ok(None);
        };

        let function = self.current_function.expect("a function is being lowered");
        let trap = context.append_basic_block(function, "overflow");
        let ok = context.append_basic_block(function, "nooverflow");
        builder
            .build_conditional_branch(overflowed, trap, ok)
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        for target in [trap, ok] {
            if !self.branched_to.contains(&target) {
                self.branched_to.push(target);
            }
        }

        builder.position_at_end(trap);
        let message = match op {
            BinaryOp::Add => "integer overflow in addition",
            BinaryOp::Sub => "integer overflow in subtraction",
            _ => "integer overflow in multiplication",
        };
        self.emit_panic(builder, message, span)?;

        builder.position_at_end(ok);
        Ok(Some(result))
    }

    /// Emits a call to the panic builtin.
    pub(super) fn emit_panic(
        &mut self,
        builder: &Builder<'static>,
        message: &str,
        span: fallc_span::Span,
    ) -> Result<(), CodegenError> {
        let _ = span;
        let context = self.backend.context();
        let module = self.backend.module();

        // Declared on first use, so a program that never traps does not carry
        // the declaration.
        let callee = match module.get_function("__builtin_panic") {
            Some(function) => function,
            None => {
                let void = context.void_type();
                let i8_ptr = context.ptr_type(inkwell::AddressSpace::default());
                let signature = void.fn_type(&[i8_ptr.into()], false);
                module.add_function("__builtin_panic", signature, None)
            }
        };
        // The body of the panic is generated with the module (see
        // `runtime.rs`); there is no C stub to link against any more.
        self.define_panic()?;

        // The message is a constant string in a private global. It is
        // NUL-terminated for the same reason a source-level literal is: a
        // byte pointer is read until a NUL, so without one the consumer runs
        // past the end.
        let bytes = context.const_string(message.as_bytes(), true);
        let global = module.add_global(bytes.get_type(), None, ".panic.msg");
        global.set_initializer(&bytes);
        global.set_constant(true);
        global.set_linkage(inkwell::module::Linkage::Private);
        let pointer = global.as_pointer_value();

        builder
            .build_call(callee, &[pointer.into()], "")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        builder
            .build_unreachable()
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        Ok(())
    }

    /// Lowers `expr as Type` to the LLVM conversion instruction.
    ///
    /// # Choosing the instruction
    ///
    /// LLVM integers are sign-agnostic, so the source type's signedness selects
    /// the instruction: widening from a signed type sign-extends, from an
    /// unsigned type or `byte` zero-extends, and narrowing truncates. Integer
    /// to float likewise picks `sitofp` or `uitofp`. Float to integer picks
    /// `fptosi` or `fptoui`.
    ///
    /// A lossy conversion wraps, because narrowing truncates: `300 as u8` is
    /// 44 and `-1 as u32` is `UINT_MAX`. `byte` widens as unsigned, matching
    /// the `u8` it is based on.
    /// Lowers a cast that resolves to an `__as__` method.
    pub(super) fn lower_conversion(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        cast: &CastExpr,
        method: &fallc_sema::ConversionMethod,
        span: fallc_span::Span,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        // The symbol comes from the checker, which is the only side that knows
        // which `__as__` overload this cast selected. Reconstructing the
        // mangling here was a mistake: it built a name nothing had been emitted
        // under, and the call silently reached the wrong overload.
        // A conversion declared in another module was compiled there: this
        // side declares it from the signature the defining module exported,
        // exactly as an ordinary cross-module call does.
        let function = match self.backend.module().get_function(&method.symbol) {
            Some(function) => function,
            None => {
                let qualified = self
                    .external_symbols
                    .iter()
                    .find(|(_, signature)| signature.symbol == method.symbol)
                    .map(|(qualified, _)| qualified.clone())
                    .ok_or_else(|| {
                        CodegenError::Emit(format!(
                            "internal error: `{}` was resolved as a conversion but not declared",
                            method.symbol
                        ))
                    })?;
                self.declare_external(&method.symbol, &qualified, span)?
            }
        };

        // The receiver is an address, as for any member function.
        let receiver_value = self
            .lower_expr(ast, types, builder, cast.operand)?
            .ok_or_else(|| CodegenError::waiting_for_spec("this cast operand", span))?;
        let receiver_address = match receiver_value {
            BasicValueEnum::PointerValue(pointer) => pointer,
            value => {
                let storage = builder
                    .build_alloca(value.get_type(), "converted")
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
                builder
                    .build_store(storage, value)
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
                storage
            }
        };

        // An `__as__` is selected by a second parameter, so the call supplies
        // one: a null pointer of the target's pointer type, whose value is
        // never read. `__int__` and its siblings are named by their own
        // identifier and take the receiver alone.
        let mut args: Vec<inkwell::values::BasicMetadataValueEnum<'static>> =
            vec![receiver_address.into()];
        if method.takes_selector {
            let selector = self.backend.context().ptr_type(inkwell::AddressSpace::default());
            args.push(selector.const_null().into());
        }

        // A conversion that may raise carries its answer in a tagged result:
        // the value is what the cast produces, and an error is the exception
        // the caller's `?!` passes on. Lowering it as a plain value handed the
        // caller the result struct where a number was wanted.
        if method.signature.throws {
            let call_site = builder
                .build_call(function, &args, "conversion")
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
            return self.unwrap_throwing(ast, types, builder, span, call_site);
        }
        self.build_value_call(builder, function, &method.symbol, args, span)
    }

    pub(super) fn lower_cast(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        cast: &CastExpr,
        span: fallc_span::Span,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        // A cast the checker resolved to a conversion method is that call, not
        // a bit-level conversion: the type defined what it means to become
        // another type, and no instruction can reproduce it.
        //
        // `frac as double` is `__as__(&frac, nullptr)`: the receiver by address
        // and a null pointer whose type is the target. The selector carries no
        // value; it exists so that a conversion is chosen by an argument rather
        // than by its return type, which could not tell two apart.
        if let Some(method) = self.conversions.get(&span).cloned() {
            return self.lower_conversion(ast, types, builder, cast, &method, span);
        }

        let mut operand = self
            .lower_expr(ast, types, builder, cast.operand)?
            .ok_or_else(|| CodegenError::waiting_for_spec("this cast operand", span))?;
        let source = types
            .get(cast.operand)
            .cloned()
            .ok_or_else(|| CodegenError::waiting_for_spec("this cast operand's type", span))?;

        // A reference converts as its referent does: the checker resolved the
        // cast against the referent's type, so the address is read before the
        // conversion rather than converted itself.
        if let Some(referent) = source.referent()
            && let Some(llvm) = TypeMapper::new(self.backend.context()).map(referent)
            && let BasicValueEnum::PointerValue(address) = operand
        {
            operand = builder
                .build_load(llvm, address, "referent")
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
        }
        let target = self.resolve_type(&cast.ty);

        let mapper = TypeMapper::new(self.backend.context());
        let target_llvm = mapper
            .map(&target)
            .ok_or_else(|| CodegenError::waiting_for_spec("this cast target", span))?;

        // Every `build_*` conversion returns `Result<_, BuilderError>`; this
        // collapses it to the codegen result shape.
        fn convert<T: Into<BasicValueEnum<'static>>>(
            result: Result<T, inkwell::builder::BuilderError>,
        ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
            result
                .map(|value| Some(value.into()))
                .map_err(|error| CodegenError::Emit(error.to_string()))
        }

        use inkwell::types::BasicTypeEnum;
        match (operand, target_llvm) {
            // Integer to integer. The source's signedness decides whether a
            // widening conversion sign- or zero-extends.
            (BasicValueEnum::IntValue(from), BasicTypeEnum::IntType(to)) => {
                let from_bits = from.get_type().get_bit_width();
                let to_bits = to.get_bit_width();
                if from_bits == to_bits {
                    // Same width: a conversion between `byte` and `u8`, or
                    // between an alias and its target. No instruction is
                    // needed, but the type system still treats them as
                    // distinct, which is why the cast was written.
                    return Ok(Some(from.as_basic_value_enum()));
                }
                if from_bits > to_bits {
                    return convert(builder.build_int_truncate(from, to, "cast"));
                }
                let signed = source.is_signed_for_conversion();
                if signed {
                    convert(builder.build_int_s_extend(from, to, "cast"))
                } else {
                    convert(builder.build_int_z_extend(from, to, "cast"))
                }
            }
            (BasicValueEnum::IntValue(from), BasicTypeEnum::FloatType(to)) => {
                if source.is_signed_for_conversion() {
                    convert(builder.build_signed_int_to_float(from, to, "cast"))
                } else {
                    convert(builder.build_unsigned_int_to_float(from, to, "cast"))
                }
            }
            (BasicValueEnum::FloatValue(from), BasicTypeEnum::IntType(to)) => {
                if target.is_signed_for_conversion() {
                    convert(builder.build_float_to_signed_int(from, to, "cast"))
                } else {
                    convert(builder.build_float_to_unsigned_int(from, to, "cast"))
                }
            }
            // Float to float: `fptrunc` or `fpext`.
            (BasicValueEnum::FloatValue(from), BasicTypeEnum::FloatType(to)) => {
                let from_bits = from.get_type().get_bit_width();
                let to_bits = to.get_bit_width();
                if from_bits == to_bits {
                    return Ok(Some(from.as_basic_value_enum()));
                }
                if from_bits > to_bits {
                    convert(builder.build_float_trunc(from, to, "cast"))
                } else {
                    convert(builder.build_float_ext(from, to, "cast"))
                }
            }
            // A pointer-to-pointer cast changes only the `const` qualifiers,
            // which LLVM does not track: every pointer is `ptr`. So the cast is
            // a no-op at this level, exactly as `byte as u8` is.
            (BasicValueEnum::PointerValue(pointer), BasicTypeEnum::PointerType(_)) => {
                Ok(Some(pointer.as_basic_value_enum()))
            }
            (from, to) => Err(CodegenError::Emit(format!(
                "internal error: cannot lower a cast from {} to {}",
                from.get_type(),
                to
            ))),
        }
    }

    /// Builds a value of a declared type from a literal, returning its address.
    ///
    /// The constructor takes the freshly allocated storage as its receiver and
    /// the literal as its argument -- the same call `T x = "text";` emits. The
    /// storage is a temporary of the enclosing function, so the value lives as
    /// long as the expression that needed it.
    pub(super) fn lower_literal_construction(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        literal: ExprId,
        symbol: &str,
        span: fallc_span::Span,
    ) -> Result<inkwell::values::PointerValue<'static>, CodegenError> {
        // The constructed type is the one the constructor's receiver points at,
        // which is what its symbol was mangled from.
        let target = self
            .literal_construction_types
            .get(&ast.expr(literal).span)
            .cloned()
            .ok_or_else(|| {
                CodegenError::waiting_for_spec("the type this literal is constructed into", span)
            })?;
        let llvm_ty = TypeMapper::new(self.backend.context())
            .map(&target)
            .ok_or_else(|| {
                CodegenError::waiting_for_spec(
                    format!("a value of type `{}`", target.spelling()),
                    span,
                )
            })?;
        let storage = self.create_entry_alloca(
            builder,
            self.current_function
                .and_then(|function| function.get_first_basic_block())
                .ok_or_else(|| {
                    CodegenError::Emit("internal error: no function is being lowered".to_owned())
                })?,
            "literal",
            llvm_ty,
        );
        let callee = self
            .backend
            .module()
            .get_function(symbol)
            .or_else(|| {
                let qualified = self
                    .external_symbols
                    .iter()
                    .find(|(_, signature)| signature.symbol == symbol)
                    .map(|(qualified, _)| qualified.clone())?;
                self.declare_external(symbol, &qualified, span).ok()
            })
            .ok_or_else(|| {
                CodegenError::Emit(format!(
                    "internal error: `{symbol}` constructs a value from a literal but is not \
                     declared"
                ))
            })?;
        let previous_literal = self.lowering_literal.replace(ast.expr(literal).span);
        let literal_value = self.lower_argument(ast, types, builder, literal, span)?;
        self.lowering_literal = previous_literal;
        let call_site = builder
            .build_call(callee, &[storage.into(), literal_value], "literal")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        let _ = self.unwrap_throwing(ast, types, builder, span, call_site)?;
        Ok(storage)
    }

    /// Converts a value from one type to another, as a cast would.
    ///
    /// Used where a value has to *become* another type without a cast being
    /// written: an unsuffixed integer literal that adopted its parameter's
    /// type is the case this exists for. The rules are the cast's own, because
    /// a cast is exactly what the language would have asked for.
    #[allow(clippy::too_many_arguments)]
    /// Converts a value to the type the checker recorded for its expression,
    /// when it recorded one.
    ///
    /// The checker uses the adopted-type table for two rules: an unsuffixed
    /// integer literal takes its context type, and a lossless integer promotion
    /// widens a value where a wider integer is wanted. Both are conversions
    /// code generation has to perform even though the source wrote no `as`.
    pub(super) fn convert_adopted(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        expr: ExprId,
        value: BasicValueEnum<'static>,
        span: fallc_span::Span,
    ) -> Result<BasicValueEnum<'static>, CodegenError> {
        let Some(wanted) = self.adopted_types.get(&ast.expr(expr).span).cloned() else {
            return Ok(value);
        };
        self.convert_value(ast, types, builder, expr, value, &wanted, span)
    }

    /// Performs an implicit conversion the checker recorded for `expr`.
    ///
    /// An `__im_as__` is a call the source never wrote: the target type of a
    /// binding, argument, assignment or return asked for it, and the checker
    /// resolved which method answers. Returning the value unchanged when the
    /// map has no entry is what makes this usable at every target-type position.
    pub(super) fn convert_implicit(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        expr: ExprId,
        value: BasicValueEnum<'static>,
        span: fallc_span::Span,
    ) -> Result<BasicValueEnum<'static>, CodegenError> {
        // An explicit `as` already applied its conversion while lowering, and
        // its entry in the same table is keyed by the cast's span.
        if matches!(ast.expr(expr).kind, fallc_ast::ExprKind::Cast(_)) {
            return Ok(value);
        }
        let Some(method) = self.conversions.get(&ast.expr(expr).span).cloned() else {
            return Ok(value);
        };
        let function = self.resolved_function(&method.symbol, "an implicit conversion", span)?;
        // The receiver is an address, as for every method. A value goes to a
        // temporary; a pointer already is the address of the source value.
        let receiver = match value {
            BasicValueEnum::PointerValue(pointer) => pointer,
            value => self.store_temporary(builder, value, "converted")?,
        };
        let mut args: Vec<inkwell::values::BasicMetadataValueEnum<'static>> =
            vec![receiver.into()];
        if method.takes_selector {
            let selector = self.backend.context().ptr_type(inkwell::AddressSpace::default());
            args.push(selector.const_null().into());
        }
        if method.signature.throws {
            let call = builder
                .build_call(function, &args, "implicit")
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
            return self
                .unwrap_throwing(ast, types, builder, span, call)?
                .ok_or_else(|| {
                    CodegenError::waiting_for_spec("this implicit conversion", span)
                });
        }
        self.build_value_call(builder, function, &method.symbol, args, span)?
            .ok_or_else(|| CodegenError::waiting_for_spec("this implicit conversion", span))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn convert_value(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        operand: ExprId,
        value: BasicValueEnum<'static>,
        target: &fallc_sema::Type,
        span: fallc_span::Span,
    ) -> Result<BasicValueEnum<'static>, CodegenError> {
        let source = types
            .get(operand)
            .cloned()
            .ok_or_else(|| CodegenError::waiting_for_spec("this operand's type", span))?;
        let mapper = TypeMapper::new(self.backend.context());
        let target_llvm = mapper
            .map(target)
            .ok_or_else(|| CodegenError::waiting_for_spec("this target type", span))?;

        use inkwell::types::BasicTypeEnum;
        let converted = match (value, target_llvm) {
            (BasicValueEnum::IntValue(from), BasicTypeEnum::IntType(to)) => {
                let from_bits = from.get_type().get_bit_width();
                let to_bits = to.get_bit_width();
                if from_bits == to_bits {
                    from.as_basic_value_enum()
                } else if from_bits > to_bits {
                    emitted(builder.build_int_truncate(from, to, "adopt"))?
                } else if source.is_signed_for_conversion() {
                    emitted(builder.build_int_s_extend(from, to, "adopt"))?
                } else {
                    emitted(builder.build_int_z_extend(from, to, "adopt"))?
                }
            }
            // Anything else is a conversion this language only performs when
            // one is written, so saying so is better than inventing one.
            (value, _) => {
                let _ = value;
                return Err(CodegenError::waiting_for_spec(
                    format!(
                        "converting `{}` to `{}` without a cast",
                        source.spelling(),
                        target.spelling()
                    ),
                    span,
                ));
            }
        };
        let _ = ast;
        Ok(converted)
    }

}
