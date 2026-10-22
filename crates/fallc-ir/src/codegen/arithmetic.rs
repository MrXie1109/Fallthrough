//! Arithmetic and comparison on values.
//!
//! Binary operators, comparisons of mixed signedness, short-circuit forms and
//! conditionals. The checks a debug build inserts -- overflow and division by
//! zero -- are emitted here too, because they belong to the operation rather
//! than to the statement it appears in.

use super::*;

impl<'ctx> Lowerer<'ctx> {
    /// Lowers a binary operation.
    ///
    /// Short-circuiting operators need control flow, so they are handled
    /// separately; everything else is a single instruction.
    pub(super) fn lower_binary(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        binary: &BinaryExpr,
        span: fallc_span::Span,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        if binary.op.is_short_circuit() {
            return self.lower_short_circuit(ast, types, builder, binary, span);
        }

        // When neither operand is a number, the operation is a call to a
        // special method rather than an instruction.
        if let Some(result) = self.lower_operator_method(ast, types, builder, binary, span)? {
            return Ok(Some(result));
        }

        // The operands are evaluated left to right. LLVM does not fix an
        // evaluation order, so where the language wants one, the lowering must
        // preserve it: emitting the left operand's instructions first does
        // that for everything except reordering by the optimiser, which is
        // free to do so because neither operand here can have a side effect
        // that the other observes.
        let lhs = self
            .lower_expr(ast, types, builder, binary.lhs)?
            .ok_or_else(|| CodegenError::waiting_for_spec("this left operand", span))?;
        let rhs = self
            .lower_expr(ast, types, builder, binary.rhs)?
            .ok_or_else(|| CodegenError::waiting_for_spec("this right operand", span))?;

        let op = binary.op;
        // ---- pointer arithmetic ----
        //
        // The checker has already decided that these operands are a pointer and
        // an offset, or two pointers with the same pointee. Which of the three
        // forms it is, is read from the lowered values: a pointer is a
        // `PointerValue` and an offset is an integer, and `p - q` is two
        // pointers.
        //
        // Every form is emitted before the numeric path below, because that
        // path looks for an arithmetic instruction and a pointer has none.
        if matches!(op, BinaryOp::Add | BinaryOp::Sub) {
            if let (BasicValueEnum::PointerValue(left), BasicValueEnum::IntValue(offset)) =
                (lhs, rhs)
            {
                let pointee = self
                    .pointer_pointee_of(ast, types, binary.lhs)
                    .ok_or_else(|| CodegenError::waiting_for_spec("this pointer's pointee", span))?;
                let index = self.widen_to_pointer_size(builder, offset)?;
                let index = if op == BinaryOp::Sub {
                    builder
                        .build_int_neg(index, "neg")
                        .map_err(|error| CodegenError::Emit(error.to_string()))?
                } else {
                    index
                };
                let address = self.build_pointer_offset(builder, left, index, &pointee, span)?;
                return Ok(Some(address.as_basic_value_enum()));
            }
            if let (BasicValueEnum::PointerValue(left), BasicValueEnum::PointerValue(right)) =
                (lhs, rhs)
            {
                // A difference of addresses, divided by the pointee's size so
                // that it counts elements rather than bytes. Signed, because
                // either pointer may be the earlier one.
                let pointee = self
                    .pointer_pointee_of(ast, types, binary.lhs)
                    .ok_or_else(|| CodegenError::waiting_for_spec("this pointer's pointee", span))?;
                let element_ty = TypeMapper::new(self.backend.context())
                    .map(&pointee)
                    .ok_or_else(|| {
                        CodegenError::waiting_for_spec(
                            "a pointer difference in a type with no known size",
                            span,
                        )
                    })?;
                let target_data = self.target_data()?;
                let element_size = target_data.get_store_size(&element_ty);
                let int_ty = self.backend.context().ptr_sized_int_type(&target_data, None);
                let left = builder
                    .build_ptr_to_int(left, int_ty, "lhs.addr")
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
                let right = builder
                    .build_ptr_to_int(right, int_ty, "rhs.addr")
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
                let bytes = builder
                    .build_int_sub(left, right, "ptrdiff.bytes")
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
                let size = int_ty.const_int(element_size, false);
                let difference = builder
                    .build_int_signed_div(bytes, size, "ptrdiff")
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
                return Ok(Some(difference.as_basic_value_enum()));
            }
            if let (BasicValueEnum::IntValue(offset), BasicValueEnum::PointerValue(right)) =
                (lhs, rhs)
            {
                // `n + p`, the same operation as `p + n`.
                let pointee = self
                    .pointer_pointee_of(ast, types, binary.rhs)
                    .ok_or_else(|| CodegenError::waiting_for_spec("this pointer's pointee", span))?;
                let index = self.widen_to_pointer_size(builder, offset)?;
                let address = self.build_pointer_offset(builder, right, index, &pointee, span)?;
                return Ok(Some(address.as_basic_value_enum()));
            }
        }

        let operand_ty = self
            .int_or_float_type(ast, types, binary.lhs)
            .ok_or_else(|| CodegenError::waiting_for_spec("this operand type", span))?;

        // A debug-mode check guards the operations that can trap. An overflow
        // check *computes* the operation, so its result is used directly and
        // the plain arithmetic below is skipped: computing it twice would leave
        // the second copy as dead code for the optimiser to remove, and would
        // trap twice on a value that already trapped.
        let mut checked_result = None;
        if self.mode.checks_arithmetic() {
            if op.can_divide_by_zero() {
                self.emit_divide_by_zero_check(builder, rhs, op, span)?;
            } else if op.can_overflow() && operand_ty.is_signed_for_conversion() {
                checked_result = self.emit_signed_overflow_check(builder, lhs, rhs, op, span)?;
            }
        }
        if let Some(result) = checked_result {
            return Ok(Some(result));
        }

        let result = match op {
            // ---- arithmetic ----
            BinaryOp::Add
            | BinaryOp::Sub
            | BinaryOp::Mul
            | BinaryOp::Div
            | BinaryOp::Rem
            | BinaryOp::Pow => {
                match (lhs, rhs) {
                    (BasicValueEnum::IntValue(lhs), BasicValueEnum::IntValue(rhs)) => {
                        // Signedness picks the instruction, since LLVM integers
                        // are sign-agnostic.
                        let signed = operand_ty.is_signed_for_conversion();
                        match op {
                            BinaryOp::Add => emitted(builder.build_int_add(lhs, rhs, "add"))?,
                            BinaryOp::Sub => emitted(builder.build_int_sub(lhs, rhs, "sub"))?,
                            BinaryOp::Mul => emitted(builder.build_int_mul(lhs, rhs, "mul"))?,
                            BinaryOp::Div if signed => {
                                emitted(builder.build_int_signed_div(lhs, rhs, "div"))?
                            }
                            BinaryOp::Div => {
                                emitted(builder.build_int_unsigned_div(lhs, rhs, "div"))?
                            }
                            BinaryOp::Rem if signed => {
                                emitted(builder.build_int_signed_rem(lhs, rhs, "rem"))?
                            }
                            BinaryOp::Rem => {
                                emitted(builder.build_int_unsigned_rem(lhs, rhs, "rem"))?
                            }
                            BinaryOp::Pow => {
                                self.build_integer_power(builder, lhs, rhs, "pow")?
                            }
                            _ => unreachable!("only arithmetic reaches this arm"),
                        }
                    }
                    (BasicValueEnum::FloatValue(lhs), BasicValueEnum::FloatValue(rhs)) => {
                        match op {
                            BinaryOp::Add => emitted(builder.build_float_add(lhs, rhs, "add"))?,
                            BinaryOp::Sub => emitted(builder.build_float_sub(lhs, rhs, "sub"))?,
                            BinaryOp::Mul => emitted(builder.build_float_mul(lhs, rhs, "mul"))?,
                            BinaryOp::Div => emitted(builder.build_float_div(lhs, rhs, "div"))?,
                            // `**` on floats is `llvm.pow`, which takes a
                            // floating exponent. `powi` is the integer-exponent
                            // form and would need a conversion here, which
                            // would make `2.0 ** 0.5` wrong.
                            BinaryOp::Pow => {
                                let callee = self.declare_float_pow(lhs.get_type())?;
                                let call = builder
                                    .build_call(callee, &[lhs.into(), rhs.into()], "pow")
                                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
                                call.try_as_basic_value().basic().ok_or_else(|| {
                                    CodegenError::Emit("`llvm.pow` returned no value".to_owned())
                                })?
                            }
                            // `%` on floats has no LLVM instruction; it needs
                            // the remainder intrinsic.
                            BinaryOp::Rem => {
                                return Err(CodegenError::waiting_for_spec(
                                    "a floating-point remainder",
                                    span,
                                ));
                            }
                            _ => unreachable!("only arithmetic reaches this arm"),
                        }
                    }
                    _ => {
                        return Err(CodegenError::waiting_for_spec(
                            "operands of different kinds",
                            span,
                        ));
                    }
                }
            }
            // ---- bitwise ----
            BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor => {
                let (BasicValueEnum::IntValue(lhs), BasicValueEnum::IntValue(rhs)) = (lhs, rhs)
                else {
                    return Err(CodegenError::waiting_for_spec(
                        "a bitwise operation on non-integers",
                        span,
                    ));
                };
                match op {
                    BinaryOp::BitAnd => emitted(builder.build_and(lhs, rhs, "and"))?,
                    BinaryOp::BitOr => emitted(builder.build_or(lhs, rhs, "or"))?,
                    BinaryOp::BitXor => emitted(builder.build_xor(lhs, rhs, "xor"))?,
                    _ => unreachable!("only bitwise reaches this arm"),
                }
            }
            // ---- shifts ----
            BinaryOp::Shl | BinaryOp::Shr => {
                let (BasicValueEnum::IntValue(lhs), BasicValueEnum::IntValue(rhs)) = (lhs, rhs)
                else {
                    return Err(CodegenError::waiting_for_spec(
                        "a shift of non-integers",
                        span,
                    ));
                };
                if op == BinaryOp::Shl {
                    emitted(builder.build_left_shift(lhs, rhs, "shl"))?
                } else {
                    // An arithmetic shift preserves the sign bit, which is what
                    // a signed right shift means; an unsigned one shifts in
                    // zeroes.
                    let arithmetic = operand_ty.is_signed_for_conversion();
                    emitted(builder.build_right_shift(lhs, rhs, arithmetic, "shr"))?
                }
            }
            // ---- three-way comparison ----
            //
            // Integer operands give -1, 0, or 1. The sign is what the operator
            // reports, so a subtraction would overflow at the extremes --
            // `i8::MIN <=> 1` cannot be computed as a difference -- which is
            // why this is two comparisons rather than one subtraction.
            BinaryOp::Cmp => match (lhs, rhs) {
                // Integer operands give -1, 0, or 1. Two comparisons rather
                // than a subtraction, because a difference overflows at the
                // extremes: `i8::MIN <=> 1` has no `i8` difference, and the
                // subtraction would wrap and report the wrong sign.
                (BasicValueEnum::IntValue(lhs), BasicValueEnum::IntValue(rhs)) => {
                    let signed = operand_ty.is_signed_for_conversion();
                    let (less, greater) = if signed {
                        (inkwell::IntPredicate::SLT, inkwell::IntPredicate::SGT)
                    } else {
                        (inkwell::IntPredicate::ULT, inkwell::IntPredicate::UGT)
                    };
                    let is_less = builder
                        .build_int_compare(less, lhs, rhs, "cmp.lt")
                        .map_err(|error| CodegenError::Emit(error.to_string()))?;
                    let is_greater = builder
                        .build_int_compare(greater, lhs, rhs, "cmp.gt")
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
                        .map_err(|error| CodegenError::Emit(error.to_string()))?
                }
                // Floats compare the same way, with the float predicates.
                _ => self.lower_float_cmp(builder, lhs, rhs, span)?,
            },
            // ---- comparison ----
            op if op.is_comparison() => {
                // Comparison needs both operand types, because it may mix
                // arithmetic types and the lowering depends on how they differ.
                return self.lower_comparison(
                    builder,
                    Comparison {
                        lhs,
                        rhs,
                        lhs_ty: types.get(binary.lhs).cloned().unwrap_or(Type::INT),
                        rhs_ty: types.get(binary.rhs).cloned().unwrap_or(Type::INT),
                        op,
                        span,
                    },
                );
            }
            _ => {
                return Err(CodegenError::waiting_for_spec("this operator", span));
            }
        };

        Ok(Some(result))
    }

    /// Lowers a comparison to an `i1`.
    pub(super) fn lower_comparison(
        &mut self,
        builder: &Builder<'static>,
        comparison: Comparison<'static>,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        let Comparison {
            lhs,
            rhs,
            lhs_ty,
            rhs_ty,
            op,
            span,
        } = comparison;
        let (lhs_ty, rhs_ty) = (&lhs_ty, &rhs_ty);
        // # Mixed signedness
        //
        // Comparing a signed value with an unsigned one cannot be done with a
        // single instruction, because neither operand's signedness describes
        // the relationship. Widening both to a common signed type would make a
        // large unsigned value look negative; widening to unsigned would do the
        // same to a negative signed value.
        //
        // The rule used here is the one that compares by mathematical value:
        //
        //   * if the signed side is negative, the unsigned side is larger, so
        //     the answer is known without looking at the unsigned value at all;
        //   * otherwise the signed side is non-negative and both fit a common
        //     unsigned type, so a plain unsigned comparison is exact.
        //
        // That is what makes `-1i8 < 1u32` true and `1i8 < 4294967295u32` true,
        // where a naive bit-width conversion gets both wrong.
        let lhs_is_float = matches!(lhs, BasicValueEnum::FloatValue(_));
        let rhs_is_float = matches!(rhs, BasicValueEnum::FloatValue(_));

        // ---- float against float -------------------------------------------
        if lhs_is_float && rhs_is_float {
            let (BasicValueEnum::FloatValue(lhs), BasicValueEnum::FloatValue(rhs)) = (lhs, rhs)
            else {
                unreachable!("both operands were just checked to be floats");
            };
            let predicate = float_predicate(op)
                .ok_or_else(|| CodegenError::waiting_for_spec("this float comparison", span))?;
            return builder
                .build_float_compare(predicate, lhs, rhs, "cmp")
                .map(|value| Some(value.as_basic_value_enum()))
                .map_err(|error| CodegenError::Emit(error.to_string()));
        }

        // ---- float against integer, or integer against float ---------------
        //
        // LLVM has no mixed `fcmp`, so the integer is converted to the float
        // type. That loses precision for very large integers, which is the
        // usual and expected behaviour for such a comparison: the comparison is
        // exact only when the integer is representable in the float type, and a
        // rounding comparison is not an error.
        if lhs_is_float != rhs_is_float {
            let predicate = float_predicate(op)
                .ok_or_else(|| CodegenError::waiting_for_spec("this mixed comparison", span))?;
            let (float, int, int_ty) = if lhs_is_float {
                let BasicValueEnum::FloatValue(float) = lhs else {
                    unreachable!("checked above");
                };
                let BasicValueEnum::IntValue(int) = rhs else {
                    unreachable!("the other operand is an integer");
                };
                (float, int, rhs_ty)
            } else {
                let BasicValueEnum::FloatValue(float) = rhs else {
                    unreachable!("checked above");
                };
                let BasicValueEnum::IntValue(int) = lhs else {
                    unreachable!("the other operand is an integer");
                };
                (float, int, lhs_ty)
            };

            let target = float.get_type();
            let converted = if int_ty.is_signed_for_conversion() {
                builder.build_signed_int_to_float(int, target, "tofloat")
            } else {
                builder.build_unsigned_int_to_float(int, target, "tofloat")
            }
            .map_err(|error| CodegenError::Emit(error.to_string()))?;

            // The converted integer must sit on the correct side, which depends
            // on which operand was the float.
            let (left, right) = if lhs_is_float {
                (float, converted)
            } else {
                (converted, float)
            };
            return builder
                .build_float_compare(predicate, left, right, "cmp")
                .map(|value| Some(value.as_basic_value_enum()))
                .map_err(|error| CodegenError::Emit(error.to_string()));
        }

        // ---- pointer against pointer ---------------------------------------
        //
        // A pointer comparison is a comparison of addresses. Only `==` and `!=`
        // are meaningful for a type with no order, but LLVM will compare
        // addresses for the others too, and whether the language permits them
        // is the checker's decision rather than this one's.
        if let (BasicValueEnum::PointerValue(lhs), BasicValueEnum::PointerValue(rhs)) = (lhs, rhs) {
            let predicate = match comparison.op {
                BinaryOp::Eq => inkwell::IntPredicate::EQ,
                BinaryOp::Ne => inkwell::IntPredicate::NE,
                BinaryOp::Lt => inkwell::IntPredicate::ULT,
                BinaryOp::Le => inkwell::IntPredicate::ULE,
                BinaryOp::Gt => inkwell::IntPredicate::UGT,
                BinaryOp::Ge => inkwell::IntPredicate::UGE,
                _ => {
                    return Err(CodegenError::waiting_for_spec(
                        "this comparison of pointers",
                        comparison.span,
                    ));
                }
            };
            return builder
                .build_int_compare(
                    predicate,
                    builder
                        .build_ptr_to_int(
                            lhs,
                            self.backend
                                .context()
                                .ptr_sized_int_type(
                                    &self
                                        .backend
                                        .target_machine(inkwell::OptimizationLevel::None)
                                        .map_err(|error| error.to_diagnostic())
                                        .map_err(|_| {
                                            CodegenError::Emit(
                                                "cannot create a target machine".to_owned(),
                                            )
                                        })?
                                        .get_target_data(),
                                    None,
                                ),
                            "lhs.addr",
                        )
                        .map_err(|error| CodegenError::Emit(error.to_string()))?,
                    builder
                        .build_ptr_to_int(
                            rhs,
                            self.backend
                                .context()
                                .ptr_sized_int_type(
                                    &self
                                        .backend
                                        .target_machine(inkwell::OptimizationLevel::None)
                                        .map_err(|error| error.to_diagnostic())
                                        .map_err(|_| {
                                            CodegenError::Emit(
                                                "cannot create a target machine".to_owned(),
                                            )
                                        })?
                                        .get_target_data(),
                                    None,
                                ),
                            "rhs.addr",
                        )
                        .map_err(|error| CodegenError::Emit(error.to_string()))?,
                    "cmp",
                )
                .map(|value| Some(value.as_basic_value_enum()))
                .map_err(|error| CodegenError::Emit(error.to_string()));
        }

        // ---- integer against integer ---------------------------------------
        let (BasicValueEnum::IntValue(lhs), BasicValueEnum::IntValue(rhs)) = (lhs, rhs) else {
            return Err(CodegenError::waiting_for_spec(
                "comparing these values",
                span,
            ));
        };

        let lhs_signed = lhs_ty.is_signed_for_conversion();
        let rhs_signed = rhs_ty.is_signed_for_conversion();

        // Same signedness: widen both to the wider type and compare directly.
        if lhs_signed == rhs_signed {
            let (lhs, rhs) = self.widen_ints(builder, lhs, rhs, lhs_ty, rhs_ty, lhs_signed)?;
            let predicate = int_predicate(op, lhs_signed)
                .ok_or_else(|| CodegenError::waiting_for_spec("this comparison", span))?;
            return builder
                .build_int_compare(predicate, lhs, rhs, "cmp")
                .map(|value| Some(value.as_basic_value_enum()))
                .map_err(|error| CodegenError::Emit(error.to_string()));
        }

        self.lower_mixed_sign_comparison(builder, lhs, rhs, op, lhs_signed, span)
    }

    /// Widens two integers of the same signedness to a common width.
    pub(super) fn widen_ints(
        &self,
        builder: &Builder<'static>,
        lhs: inkwell::values::IntValue<'static>,
        rhs: inkwell::values::IntValue<'static>,
        lhs_ty: &Type,
        rhs_ty: &Type,
        signed: bool,
    ) -> Result<
        (
            inkwell::values::IntValue<'static>,
            inkwell::values::IntValue<'static>,
        ),
        CodegenError,
    > {
        let lhs_bits = lhs.get_type().get_bit_width();
        let rhs_bits = rhs.get_type().get_bit_width();
        if lhs_bits == rhs_bits {
            return Ok((lhs, rhs));
        }
        let target = if lhs_bits > rhs_bits {
            lhs.get_type()
        } else {
            rhs.get_type()
        };
        let _ = (lhs_ty, rhs_ty);

        let extend = |value: inkwell::values::IntValue<'static>| {
            if value.get_type().get_bit_width() == target.get_bit_width() {
                return Ok(value);
            }
            let result = if signed {
                builder.build_int_s_extend(value, target, "sext")
            } else {
                builder.build_int_z_extend(value, target, "zext")
            };
            result.map_err(|error| CodegenError::Emit(error.to_string()))
        };
        Ok((extend(lhs)?, extend(rhs)?))
    }

    /// Compares integers of differing signedness by mathematical value.
    ///
    /// Neither operand can simply be converted to the other's type: a negative
    /// signed value would appear enormous as unsigned, and a large unsigned
    /// value would appear negative as signed. Instead the signed operand is
    /// tested for negativity first, which settles the comparison for the
    /// negative case, and leaves both operands non-negative and therefore
    /// exactly comparable as unsigned in the other.
    ///
    /// ```text
    ///   signed < 0 ?  (signed <op> unsigned is decided by sign)
    ///               :  (compare both as unsigned)
    /// ```
    ///
    /// The result is a `phi` over the two paths, because they are separate
    /// blocks.
    pub(super) fn lower_mixed_sign_comparison(
        &mut self,
        builder: &Builder<'static>,
        lhs: inkwell::values::IntValue<'static>,
        rhs: inkwell::values::IntValue<'static>,
        op: BinaryOp,
        lhs_signed: bool,
        span: fallc_span::Span,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        let (signed, unsigned, signed_on_left) = if lhs_signed {
            (lhs, rhs, true)
        } else {
            (rhs, lhs, false)
        };

        // Is the signed operand negative?
        let zero = signed.get_type().const_zero();
        let is_negative = builder
            .build_int_compare(inkwell::IntPredicate::SLT, signed, zero, "isneg")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;

        let function = self.current_function.expect("a function is being lowered");
        let context = self.backend.context();
        let negative_block = context.append_basic_block(function, "cmp.negative");
        let nonneg_block = context.append_basic_block(function, "cmp.nonneg");
        let end_block = context.append_basic_block(function, "cmp.end");

        builder
            .build_conditional_branch(is_negative, negative_block, nonneg_block)
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        for target in [negative_block, nonneg_block] {
            if !self.branched_to.contains(&target) {
                self.branched_to.push(target);
            }
        }
        let entry_block = builder
            .get_insert_block()
            .expect("a block is being emitted");

        // The signed operand is negative, so it is definitely less than the
        // unsigned one. That decides `<`, `<=`, and their reflections without
        // looking at the unsigned value.
        //
        // The answer for the negated operators is the mirror image, and for the
        // equality operators it is always false: a negative number is never
        // equal to an unsigned one.
        let negative_result = match op {
            BinaryOp::Lt | BinaryOp::Le => signed_on_left,
            BinaryOp::Gt | BinaryOp::Ge => !signed_on_left,
            // `==` is false and `!=` is true, either way round.
            BinaryOp::Eq => false,
            BinaryOp::Ne => true,
            _ => {
                return Err(CodegenError::waiting_for_spec("this comparison", span));
            }
        };

        builder.position_at_end(negative_block);
        let negative_value = context
            .bool_type()
            .const_int(u64::from(negative_result), false);
        self.branch_to(builder, end_block)?;
        let negative_end = builder
            .get_insert_block()
            .expect("a block is being emitted");

        // The signed operand is non-negative, so both values fit the unsigned
        // type and an unsigned comparison is exact.
        builder.position_at_end(nonneg_block);
        let (left, right) = if signed_on_left {
            (signed, unsigned)
        } else {
            (unsigned, signed)
        };
        let (left, right) = self.widen_ints_unsigned(builder, left, right)?;
        let predicate = int_predicate(op, false)
            .ok_or_else(|| CodegenError::waiting_for_spec("this comparison", span))?;
        let nonneg_value = builder
            .build_int_compare(predicate, left, right, "cmp")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        self.branch_to(builder, end_block)?;
        let nonneg_end = builder
            .get_insert_block()
            .expect("a block is being emitted");

        builder.position_at_end(end_block);
        let phi = builder
            .build_phi(context.bool_type(), "cmp")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        phi.add_incoming(&[(&negative_value, negative_end), (&nonneg_value, nonneg_end)]);
        let _ = entry_block;

        Ok(Some(phi.as_basic_value()))
    }

    /// Widens two non-negative integers to a common unsigned width.
    pub(super) fn widen_ints_unsigned(
        &self,
        builder: &Builder<'static>,
        lhs: inkwell::values::IntValue<'static>,
        rhs: inkwell::values::IntValue<'static>,
    ) -> Result<
        (
            inkwell::values::IntValue<'static>,
            inkwell::values::IntValue<'static>,
        ),
        CodegenError,
    > {
        let lhs_bits = lhs.get_type().get_bit_width();
        let rhs_bits = rhs.get_type().get_bit_width();
        if lhs_bits == rhs_bits {
            return Ok((lhs, rhs));
        }
        let target = if lhs_bits > rhs_bits {
            lhs.get_type()
        } else {
            rhs.get_type()
        };
        let extend = |value: inkwell::values::IntValue<'static>| {
            if value.get_type().get_bit_width() == target.get_bit_width() {
                return Ok(value);
            }
            // Zero-extension is exact here because the value is known
            // non-negative on this path.
            builder
                .build_int_z_extend(value, target, "zext")
                .map_err(|error| CodegenError::Emit(error.to_string()))
        };
        Ok((extend(lhs)?, extend(rhs)?))
    }

    /// Lowers `&&` and `||` with control flow.
    ///
    /// Both short-circuit: the right operand is evaluated only when the left
    /// does not already determine the result. That cannot be a single LLVM
    /// instruction, because LLVM's `and`/`or` always evaluate both sides.
    ///
    /// ```text
    ///   a && b:            a || b:
    ///   lhs = truthy(a)    lhs = truthy(a)
    ///   br lhs ? rhs : end br lhs ? end : rhs
    ///   rhs: r = truthy(b) rhs: r = truthy(b)
    ///        br end            br end
    ///   end: phi(lhs, r)   end: phi(lhs, r)
    /// ```
    ///
    /// The phi node is what makes the result correct on both paths: it merges
    /// the left operand's value with the right one's.
    pub(super) fn lower_short_circuit(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        binary: &BinaryExpr,
        _span: fallc_span::Span,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        let is_and = binary.op == BinaryOp::LogicalAnd;

        let lhs = self.lower_condition(ast, types, builder, binary.lhs)?;
        let lhs_block = builder
            .get_insert_block()
            .expect("a block is being emitted");

        let function = self.current_function.expect("a function is being lowered");
        let context = self.backend.context();
        let rhs_block = context.append_basic_block(function, "logic.rhs");
        let end_block = context.append_basic_block(function, "logic.end");

        // For `&&` the right side runs when the left is true; for `||` when it
        // is false.
        let (then_block, else_block) = if is_and {
            (rhs_block, end_block)
        } else {
            (end_block, rhs_block)
        };
        builder
            .build_conditional_branch(lhs, then_block, else_block)
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        for target in [rhs_block, end_block] {
            if !self.branched_to.contains(&target) {
                self.branched_to.push(target);
            }
        }

        builder.position_at_end(rhs_block);
        let rhs = self.lower_condition(ast, types, builder, binary.rhs)?;
        let rhs_end_block = builder
            .get_insert_block()
            .expect("a block is being emitted");
        self.branch_to(builder, end_block)?;

        // The phi merges the two paths. Its incoming value for the left path is
        // the left operand itself, which is what short-circuiting returns.
        builder.position_at_end(end_block);
        let phi = builder
            .build_phi(context.bool_type(), "logic")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        phi.add_incoming(&[(&lhs, lhs_block), (&rhs, rhs_end_block)]);

        Ok(Some(phi.as_basic_value()))
    }

    /// Lowers `cond ? a : b` as a branch and a phi.
    ///
    /// # Only one branch runs
    ///
    /// The point of the operator is that it does not evaluate both sides, so
    /// this cannot be "lower both and select". The condition branches, each
    /// side is emitted in its own block, and a phi node merges the two values.
    /// That is the same shape `&&` and `||` already use, which is why this is
    /// written the same way.
    pub(super) fn lower_conditional(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        conditional: &fallc_ast::ConditionalExpr,
        span: fallc_span::Span,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        let condition = self.lower_condition(ast, types, builder, conditional.condition)?;
        let function = self.current_function.expect("a function is being lowered");
        let context = self.backend.context();
        let then_block = context.append_basic_block(function, "cond.then");
        let else_block = context.append_basic_block(function, "cond.else");
        let end_block = context.append_basic_block(function, "cond.end");

        builder
            .build_conditional_branch(condition, then_block, else_block)
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        for target in [then_block, else_block, end_block] {
            if !self.branched_to.contains(&target) {
                self.branched_to.push(target);
            }
        }

        builder.position_at_end(then_block);
        let then_value = self.lower_branch(
            ast,
            types,
            builder,
            conditional.then_value,
            span,
        )?;
        let then_end = builder.get_insert_block().expect("a block is being emitted");
        self.branch_to(builder, end_block)?;

        builder.position_at_end(else_block);
        let else_value = self.lower_branch(
            ast,
            types,
            builder,
            conditional.else_value,
            span,
        )?;
        let else_end = builder.get_insert_block().expect("a block is being emitted");
        self.branch_to(builder, end_block)?;

        builder.position_at_end(end_block);
        let phi = builder
            .build_phi(then_value.get_type(), "cond")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        phi.add_incoming(&[(&then_value, then_end), (&else_value, else_end)]);
        Ok(Some(phi.as_basic_value()))
    }

    /// Lowers one branch of a conditional, reading through a reference where
    /// the checker said a value is wanted.
    ///
    /// `lhs < rhs ? lhs : rhs` has branches that *name storage*, and the
    /// expression's result is the value in it. Whether to read is a fact about
    /// the surrounding context, which the checker recorded; this is where that
    /// record is honoured, exactly as it is for `*p`.
    pub(super) fn lower_branch(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        branch: ExprId,
        span: fallc_span::Span,
    ) -> Result<BasicValueEnum<'static>, CodegenError> {
        let value = self
            .lower_expr(ast, types, builder, branch)?
            .ok_or_else(|| CodegenError::waiting_for_spec("the value of this branch", span))?;
        let Some(referent) = self.deref_reads.get(&ast.expr(branch).span).cloned() else {
            return Ok(value);
        };
        let Some(llvm) = TypeMapper::new(self.backend.context()).map(&referent) else {
            return Ok(value);
        };
        let BasicValueEnum::PointerValue(address) = value else {
            return Ok(value);
        };
        builder
            .build_load(llvm, address, "branch")
            .map_err(|error| CodegenError::Emit(error.to_string()))
    }

    /// Lowers a prefix operation.
    /// Lowers a unary operator as a call to its method.
    ///
    /// The operand is the receiver, passed by address, and the result is
    /// whatever the method returns -- which for `v++` is the value the
    /// expression yields rather than the modified object.
    pub(super) fn lower_unary_method(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        operand: ExprId,
        symbol: &str,
        span: fallc_span::Span,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        let function = self.resolved_function(symbol, "an operator", span)?;
        let value = self
            .lower_expr(ast, types, builder, operand)?
            .ok_or_else(|| CodegenError::waiting_for_spec("this operand", span))?;
        let receiver = match value {
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
        let value = self.build_value_call(
            builder,
            function,
            symbol,
            vec![receiver.into()],
            span,
        )?;
        self.unwrap_throwing_value(ast, types, builder, span, value)
    }

    pub(super) fn lower_unary(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        unary: &UnaryExpr,
        span: fallc_span::Span,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        // A type that defines the method for this operator is called instead of
        // the built-in instruction: `-v` is `__neg__(&v)`. The checker recorded
        // which, so nothing is looked up here.
        if let Some(symbol) = self.unary_methods.get(&span).cloned() {
            let value =
                self.lower_unary_method(ast, types, builder, unary.operand, &symbol, span)?;
            // `__deref__` yields a reference, and in LLVM a reference is an
            // address. The checker recorded, for this expression, whether the
            // reference is to be read as a value or passed on as an address:
            // which one it is depends on what surrounds it, and that is the
            // checker's answer to give rather than code generation's to guess.
            if unary.op == UnaryOp::Deref
                && let Some(value) = value
                && let Some(referent) = self.deref_reads.get(&span).cloned()
                && let Some(llvm) = TypeMapper::new(self.backend.context()).map(&referent)
                && let BasicValueEnum::PointerValue(address) = value
            {
                let loaded = builder
                    .build_load(llvm, address, "deref")
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
                return Ok(Some(loaded));
            }
            return Ok(value);
        }

        // `!` truth-tests its operand instead of reading it, which is a
        // different lowering from the others -- and doing it after the shared
        // operand was lowered evaluated the operand twice, so `if (!f())`
        // called `f` twice. It is handled before the operand is touched.
        if unary.op == UnaryOp::Not {
            let condition = self.lower_condition(ast, types, builder, unary.operand)?;
            return Ok(Some(emitted(builder.build_not(condition, "lnot"))?));
        }
        // `&x` wants x's address, not x's value, so the operand is not read:
        // lowering it first read the whole of `a` for `&a`, and an aggregate
        // that big is a value LLVM cannot select.
        if unary.op == UnaryOp::AddressOf {
            // A reference *is* an address: `&r` is the object's address, which
            // is the reference's value. Taking the address of its slot handed
            // out a pointer to the pointer, which is why `&this` in a
            // reference-receiver method wrote through the wrong address.
            if types
                .get(unary.operand)
                .is_some_and(fallc_sema::Type::is_reference)
            {
                // `*p` and `a[i]` name storage, so the address is the lvalue's
                // storage. A reference that is a *value* -- a `this`
                // parameter, a call returning a reference -- carries the
                // address as its value.
                let names_storage = match &ast.expr(unary.operand).kind {
                    ExprKind::Index(_) => true,
                    ExprKind::Unary(inner) => inner.op == UnaryOp::Deref,
                    _ => false,
                };
                if names_storage
                    && let Some(binding) =
                        self.lower_lvalue(ast, types, builder, unary.operand)
                {
                    return Ok(Some(binding.storage.as_basic_value_enum()));
                }
                let value = self
                    .lower_expr(ast, types, builder, unary.operand)?
                    .ok_or_else(|| CodegenError::waiting_for_spec("this reference", span))?;
                return Ok(Some(value));
            }
            return match self.lower_lvalue(ast, types, builder, unary.operand) {
                Some(binding) => Ok(Some(binding.storage.as_basic_value_enum())),
                None => Err(CodegenError::waiting_for_spec(
                    "taking the address of this expression",
                    span,
                )),
            };
        }

        let operand = self
            .lower_expr(ast, types, builder, unary.operand)?
            .ok_or_else(|| CodegenError::waiting_for_spec("this operand", span))?;

        let result = match unary.op {
            // Unary plus is the identity.
            UnaryOp::Plus => return Ok(Some(operand)),
            UnaryOp::Neg => match operand {
                BasicValueEnum::IntValue(value) => {
                    // `0 - x`, which is correct for the most negative value too
                    // in two's complement.
                    let zero = value.get_type().const_zero();
                    emitted(builder.build_int_sub(zero, value, "neg"))?
                }
                BasicValueEnum::FloatValue(value) => {
                    emitted(builder.build_float_neg(value, "neg"))?
                }
                _ => {
                    return Err(CodegenError::waiting_for_spec("negating this value", span));
                }
            },
            // Handled before the operand was lowered.
            UnaryOp::AddressOf => unreachable!("`&x` is lowered before its operand"),
            UnaryOp::Deref => {
                // Load through the pointer. The pointee's type comes from the
                // checker, because LLVM pointers are opaque.
                let pointee = types
                    .get(unary.operand)
                    .and_then(|ty| ty.pointee().cloned())
                    .ok_or_else(|| {
                        CodegenError::waiting_for_spec("dereferencing this value", span)
                    })?;
                let mapper = TypeMapper::new(self.backend.context());
                let llvm_pointee = mapper
                    .map(&pointee)
                    .ok_or_else(|| CodegenError::waiting_for_spec("this pointee type", span))?;
                let BasicValueEnum::PointerValue(pointer) = operand else {
                    return Err(CodegenError::waiting_for_spec(
                        "dereferencing a non-pointer",
                        span,
                    ));
                };
                // `*p` denotes the storage the pointer names. Where a *value*
                // is wanted, that storage is read; where an address is wanted
                // -- `&*p`, or returning a reference -- the pointer itself is
                // what the expression yields, and loading it would produce the
                // value instead of the reference to it.
                //
                // The checker recorded which, because the answer depends on
                // what surrounds the expression.
                if self.deref_reads.contains_key(&span) {
                    return builder
                        .build_load(llvm_pointee, pointer, "deref")
                        .map(Some)
                        .map_err(|error| CodegenError::Emit(error.to_string()));
                }
                return Ok(Some(pointer.as_basic_value_enum()));
            }
            UnaryOp::BitNot => match operand {
                BasicValueEnum::IntValue(value) => {
                    let all_ones = value.get_type().const_all_ones();
                    emitted(builder.build_xor(value, all_ones, "not"))?
                }
                _ => {
                    return Err(CodegenError::waiting_for_spec(
                        "a bitwise complement of this value",
                        span,
                    ));
                }
            },
            // Handled above, before the operand is lowered: see the note there.
            UnaryOp::Not => unreachable!("`!` is lowered before its operand"),
            // `UnaryOp` is #[non_exhaustive], so a later operator lands here
            // rather than silently doing nothing.
            _ => {
                return Err(CodegenError::waiting_for_spec("this prefix operator", span));
            }
        };

        Ok(Some(result))
    }

}
