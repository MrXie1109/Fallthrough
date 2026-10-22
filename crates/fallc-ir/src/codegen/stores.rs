//! Places and the values written into them.
//!
//! Initialisers, field access, assignment, compound assignment and increment
//! all answer the same question: where is the storage, and what value goes
//! there.

use super::*;

impl<'ctx> Lowerer<'ctx> {
    /// Lowers a typed initialiser, `Point{42, 37}`.
    ///
    /// Unlike the top-level form this produces a *value* rather than filling a
    /// binding, so it either calls the type's constructor into a temporary or
    /// builds the struct directly, depending on which the type has.
    pub(super) fn lower_typed_init(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        init: &TypedInitExpr,
        span: fallc_span::Span,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        let ty = types
            .get(init.init.expr)
            .cloned()
            .ok_or_else(|| CodegenError::waiting_for_spec("this initialiser's type", span))?;
        let mapper = TypeMapper::new(self.backend.context());
        let llvm_ty = mapper
            .map(&ty)
            .ok_or_else(|| CodegenError::waiting_for_spec("this initialiser's type", span))?;

        // A constructor writes the fields, so the value needs somewhere to
        // live first: a temporary, whose contents are then the value.
        if self.type_has_init(&ty) {
            let entry = self
                .current_function
                .and_then(|function| function.get_first_basic_block())
                .ok_or_else(|| {
                    CodegenError::Emit("internal error: no function is being lowered".to_owned())
                })?;
            let alloca = self.create_entry_alloca(builder, entry, ".init", llvm_ty);

            // Which `__init__` this initialiser calls was decided by the
            // checker, from the argument as well as the type: `String s = "hi"`
            // and `String s = other` are the same type and different
            // constructors. Rebuilding the symbol from the type alone picked
            // the copy constructor for both, which then received a pointer and
            // a string literal where a `String` was declared.
            let symbol = self
                .init_calls
                .get(&ast.expr(init.init.expr).span)
                .cloned()
                .unwrap_or_else(|| {
                    // A type may declare its empty constructor as either
                    // spelling; prefer the explicit one when both exist.
                    let explicit = constructor_symbol(&ty);
                    if self.backend.module().get_function(&explicit).is_some() {
                        explicit
                    } else {
                        implicit_constructor_symbol(&ty)
                    }
                });
            let callee = self
                .backend
                .module()
                .get_function(&symbol)
                .or_else(|| {
                    // The constructor may belong to another module, in which
                    // case it is declared here from the exported signature.
                    let qualified = self
                        .external_symbols
                        .iter()
                        .find(|(_, signature)| signature.symbol == symbol)
                        .map(|(qualified, _)| qualified.clone())?;
                    self.declare_external(&symbol, &qualified, span).ok()
                })
                .ok_or_else(|| {
                    CodegenError::Emit(format!(
                        "internal error: `{symbol}`, the `__init__` for `{}`, was not emitted",
                        ty.spelling()
                    ))
                })?;
            let ordered = match ty.as_nominal() {
                Some(nominal) => Self::init_field_values(&init.init, &nominal.fields),
                None => init.init.elements.iter().copied().map(Some).collect(),
            };
            let mut args: Vec<inkwell::values::BasicMetadataValueEnum<'static>> =
                Vec::with_capacity(ordered.len() + 1);
            args.push(alloca.as_basic_value_enum().into());
            for element in ordered.into_iter().flatten() {
                let value = self
                    .lower_expr(ast, types, builder, element)?
                    .ok_or_else(|| {
                        CodegenError::waiting_for_spec("this initialiser element", span)
                    })?;
                args.push(value.into());
            }
            let call_site = builder
                .build_call(callee, &args, "")
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
            let _ = self.unwrap_throwing(ast, types, builder, span, call_site)?;

            return builder
                .build_load(llvm_ty, alloca, "init")
                .map(Some)
                .map_err(|error| CodegenError::Emit(error.to_string()));
        }

        // No constructor: build the struct from its elements.
        let inkwell::types::BasicTypeEnum::StructType(struct_ty) = llvm_ty else {
            return Err(CodegenError::Emit(
                "internal error: a typed initialiser is not a struct".to_owned(),
            ));
        };
        // The elements are given in declaration order; the member index is
        // translated, because the representation carries an alignment marker
        // before every field.
        let nominal = ty.as_nominal().cloned();
        let ordered = match &nominal {
            Some(nominal) => Self::init_field_values(&init.init, &nominal.fields),
            None => init.init.elements.iter().copied().map(Some).collect(),
        };
        let mut aggregate = struct_ty.get_undef();
        for (index, element) in ordered.into_iter().enumerate() {
            let Some(element) = element else {
                continue;
            };
            let value = self
                .lower_expr(ast, types, builder, element)?
                .ok_or_else(|| CodegenError::waiting_for_spec("this initialiser element", span))?;
            let member = match &nominal {
                Some(nominal) => llvm_member_index(nominal, index),
                None => u32::try_from(index).map_err(|_| {
                    CodegenError::Emit("internal error: a field index exceeds u32".to_owned())
                })?,
            };
            aggregate = builder
                .build_insert_value(aggregate, value, member, "field")
                .map_err(|error| CodegenError::Emit(error.to_string()))?
                .into_struct_value();
        }
        Ok(Some(aggregate.as_basic_value_enum()))
    }

    /// Lowers a nested initialiser.
    ///
    /// The type comes from the checker, which recorded it against this
    /// expression from the field it fills.
    pub(super) fn lower_nested_init(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        init: &fallc_ast::DesignatedInit,
        span: fallc_span::Span,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        let ty = types
            .get(init.expr)
            .cloned()
            .ok_or_else(|| CodegenError::waiting_for_spec("this initialiser's type", span))?;
        let mapper = TypeMapper::new(self.backend.context());
        let struct_ty = mapper
            .map(&ty)
            .ok_or_else(|| CodegenError::waiting_for_spec("this struct type", span))?;
        let inkwell::types::BasicTypeEnum::StructType(struct_ty) = struct_ty else {
            return Err(CodegenError::Emit(
                "internal error: a nested initialiser is not a struct".to_owned(),
            ));
        };

        // Declaration order in, member index out: see `llvm_member_index`.
        let nominal = ty.as_nominal().cloned();
        let ordered = match &nominal {
            Some(nominal) => Self::init_field_values(init, &nominal.fields),
            None => init.elements.iter().copied().map(Some).collect(),
        };
        let mut aggregate = struct_ty.get_undef();
        for (index, element) in ordered.into_iter().enumerate() {
            let Some(element) = element else {
                continue;
            };
            let value = self
                .lower_expr(ast, types, builder, element)?
                .ok_or_else(|| CodegenError::waiting_for_spec("this initialiser element", span))?;
            let member = match &nominal {
                Some(nominal) => llvm_member_index(nominal, index),
                None => u32::try_from(index).map_err(|_| {
                    CodegenError::Emit("internal error: a field index exceeds u32".to_owned())
                })?,
            };
            aggregate = builder
                .build_insert_value(aggregate, value, member, "field")
                .map_err(|error| CodegenError::Emit(error.to_string()))?
                .into_struct_value();
        }
        Ok(Some(aggregate.as_basic_value_enum()))
    }

    /// Lowers a member access to a `getelementptr` and a load.
    ///
    /// `pointer->field` is `(*pointer).field`, so the arrow form first loads
    /// the pointer's value.
    /// Lowers `a->f` by following `__pointer__` until a pointer is reached.
    ///
    /// Returns the address the field is at. Each step is a call the checker
    /// recorded, so nothing is looked up here.
    pub(super) fn lower_pointer_chain(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        field: &FieldExpr,
        span: fallc_span::Span,
    ) -> Result<inkwell::values::PointerValue<'static>, CodegenError> {
        // The chain's symbols were recorded one per link by the checker, keyed
        // by the link's ordinal. Walking them is therefore a matter of
        // replaying calls rather than deciding anything here.
        let mut value = self
            .lower_expr(ast, types, builder, field.object)?
            .ok_or_else(|| CodegenError::waiting_for_spec("this field's object", span))?;

        for step in 1..=16 {
            let step_span = fallc_span::Span::new(
                span.file,
                span.lo.saturating_add(u32::try_from(step).unwrap_or(0)),
                span.lo.saturating_add(u32::try_from(step).unwrap_or(0)),
            );
            let Some(symbol) = self.unary_methods.get(&step_span).cloned() else {
                break;
            };
            let function = match self.backend.module().get_function(&symbol) {
                Some(function) => function,
                None => {
                    let qualified = self
                        .external_symbols
                        .iter()
                        .find(|(_, signature)| signature.symbol == symbol)
                        .map(|(qualified, _)| qualified.clone())
                        .ok_or_else(|| {
                            CodegenError::Emit(format!(
                                "internal error: `{symbol}` was resolved for `->` but not declared"
                            ))
                        })?;
                    self.declare_external(&symbol, &qualified, span)?
                }
            };
            let address = match value {
                BasicValueEnum::PointerValue(pointer) => pointer,
                other => {
                    let storage = builder
                        .build_alloca(other.get_type(), "receiver")
                        .map_err(|error| CodegenError::Emit(error.to_string()))?;
                    builder
                        .build_store(storage, other)
                        .map_err(|error| CodegenError::Emit(error.to_string()))?;
                    storage
                }
            };
            let call = builder
                .build_call(function, &[address.into()], "pointer")
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
            value = call.try_as_basic_value().basic().ok_or_else(|| {
                CodegenError::waiting_for_spec("`__pointer__` returning nothing", span)
            })?;
        }

        match value {
            BasicValueEnum::PointerValue(address) => Ok(address),
            _ => Err(CodegenError::waiting_for_spec(
                "member access through this type",
                span,
            )),
        }
    }

    /// Returns the type a chain of `__pointer__` calls ends at.
    ///
    /// The checker resolved the chain, so what the field is reached through is
    /// the pointee of the type the last `__pointer__` produced. It is taken
    /// from the recorded call rather than re-derived, because only the checker
    /// knows which type each step landed on.
    pub(super) fn chain_pointee(&self, span: Span) -> Option<fallc_sema::Type> {
        // The checker walked the chain and recorded what it ended at, which is
        // the type the field belongs to. The object's own type is the *start*
        // of the chain and says nothing about where it finishes.
        self.pointer_targets.get(&span).cloned()
    }

    /// Returns the address of a field reached through an address already known.
    pub(super) fn field_on_address(
        &mut self,
        builder: &Builder<'static>,
        field: &FieldExpr,
        address: inkwell::values::PointerValue<'static>,
        owner: &fallc_sema::Type,
    ) -> Result<
        Option<(
            inkwell::values::PointerValue<'static>,
            inkwell::types::BasicTypeEnum<'static>,
        )>,
        CodegenError,
    > {
        let Some(nominal) = owner.as_nominal() else {
            return Ok(None);
        };
        let nominal = if nominal.is_incomplete {
            self.nominal_types
                .get(&nominal.name)
                .cloned()
                .unwrap_or_else(|| nominal.clone())
        } else {
            nominal.clone()
        };
        let owner = &fallc_sema::Type::Nominal(nominal.clone());
        let mapper = TypeMapper::new(self.backend.context());
        let Some(struct_llvm) = mapper.map(owner) else {
            return Ok(None);
        };
        let Some((index, field_ty)) = nominal.field(&field.field) else {
            return Ok(None);
        };
        let Some(field_llvm) = mapper.map(field_ty) else {
            return Ok(None);
        };
        let pointer = build_field_gep(
            builder,
            struct_llvm,
            address,
            llvm_member_index(&nominal, index),
        )?;
        Ok(Some((pointer, field_llvm)))
    }

    pub(super) fn lower_field(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        field: &FieldExpr,
        span: fallc_span::Span,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        let (storage, field_type) = self
            .field_storage(ast, types, builder, field, span)?
            .ok_or_else(|| CodegenError::waiting_for_spec("this field access", span))?;
        builder
            .build_load(field_type, storage, &field.field)
            .map(Some)
            .map_err(|error| CodegenError::Emit(error.to_string()))
    }

    /// Resolves a member access to the address of the field and its type.
    ///
    /// Shared by reading a field and assigning to one, since both need the same
    /// address and the difference is only what happens next.
    pub(super) fn field_storage(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        field: &FieldExpr,
        span: fallc_span::Span,
    ) -> Result<
        Option<(
            inkwell::values::PointerValue<'static>,
            inkwell::types::BasicTypeEnum<'static>,
        )>,
        CodegenError,
    > {
        // The object's own type, which the checker recorded.
        let object_ty = types
            .get(field.object)
            .cloned()
            .ok_or_else(|| CodegenError::waiting_for_spec("this field's object", span))?;
        let _ = ast;

        // A pointer receiver is followed: the object's *value* is the address.
        //
        // Any other type on the arrow form is asked for `__pointer__`, and its
        // result is followed the same way -- `a->b->c` may take several steps,
        // each supplied by that method. The checker has already established
        // which, and recorded the calls.
        if field.through_pointer && object_ty.pointee().is_none() {
            let pointer = self.lower_pointer_chain(ast, types, builder, field, span)?;
            let Some(pointee) = self.chain_pointee(span) else {
                return Ok(None);
            };
            return self.field_on_address(builder, field, pointer, &pointee);
        }

        let (address, nominal) = if let Some(pointee) = object_ty.pointee() {
            // Where the address comes from depends on what the object is.
            //
            // A *pointer* is the address, so its value is what to index.
            // Loading it would index the slot holding the pointer, which is
            // what produced garbage: the GEP walked the stack address of the
            // parameter instead of the struct it points at.
            //
            // A *pointer* is the address, so its value is what to index. A
            // *reference* is an address too, and it lives in storage: the
            // reference's own storage holds the address, so the value is what
            // to index. Both come to the same thing -- read the expression's
            // value -- and only the lvalue path below is different, for an
            // object that is a *value* and has no address yet.
            let address = if object_ty.is_pointer() {
                let pointer = self
                    .lower_expr(ast, types, builder, field.object)?
                    .ok_or_else(|| CodegenError::waiting_for_spec("this field's object", span))?;
                let BasicValueEnum::PointerValue(address) = pointer else {
                    return Err(CodegenError::waiting_for_spec(
                        "member access on this value",
                        span,
                    ));
                };
                address
            } else if object_ty.is_reference() {
                // A reference holds an address, and *where* that address is
                // depends on what the expression denoting it is:
                //
                //   * `*p` names the storage `p` points at, so its lvalue --
                //     the pointer's own value -- is the address. `a[i]` is the
                //     same thing through a type's `__getitem__`.
                //   * Every other reference expression is a value that *is*
                //     the address: a `this` parameter is a local holding what
                //     it was passed, and a call's result is a register.
                //
                // So those forms go through the lvalue path; the rest are
                // lowered as values.
                // An index is the other expression that names storage: a type's
                // `__getitem__` returns a reference, so `a[i]` is the element
                // itself and `a[i].field` is a field of it, not of a copy.
                let names_storage = match &ast.expr(field.object).kind {
                    ExprKind::Unary(unary) => unary.op == UnaryOp::Deref,
                    ExprKind::Index(_) => true,
                    _ => false,
                };
                if names_storage {
                    match self.lower_lvalue(ast, types, builder, field.object) {
                        Some(binding) => binding.storage,
                        None => return Ok(None),
                    }
                } else {
                    let lowered = self
                        .lower_expr(ast, types, builder, field.object)?
                        .ok_or_else(|| {
                            CodegenError::waiting_for_spec("this field's object", span)
                        })?;
                    let BasicValueEnum::PointerValue(address) = lowered else {
                        return Err(CodegenError::waiting_for_spec(
                            "member access on this value",
                            span,
                        ));
                    };
                    address
                }
            } else {
                match self.lower_lvalue(ast, types, builder, field.object) {
                    Some(binding) => binding.storage,
                    None => return Ok(None),
                }
            };
            (address, pointee.clone())
        } else if matches!(&ast.expr(field.object).kind, ExprKind::Call(_)) {
            // A call's result has storage of its own, and the call is lowered
            // exactly once: asking for its address by lowering it again would
            // run it twice. The temporary the call recorded is used when there
            // is one -- an aggregate result -- and a value that has none is
            // stored here, which is what makes `r.get().field` work when the
            // checker recorded the *referent* as the call's type.
            let lowered = self.lower_expr(ast, types, builder, field.object)?;
            match self.temporaries.get(&ast.expr(field.object).span).copied() {
                Some(binding) => (binding.storage, object_ty),
                None => {
                    let Some(value) = lowered else {
                        return Ok(None);
                    };
                    let storage = self.store_temporary(builder, value, "field.value")?;
                    (storage, object_ty)
                }
            }
        } else {
            // A value receiver needs its address, so it must be an lvalue.
            let binding = self
                .lower_lvalue(ast, types, builder, field.object)
                .ok_or_else(|| {
                    CodegenError::waiting_for_spec(
                        "member access on a value that has no address",
                        span,
                    )
                })?;
            (binding.storage, object_ty)
        };

        let Some(nominal) = nominal.as_nominal() else {
            return Err(CodegenError::waiting_for_spec(
                "member access on a non-struct",
                span,
            ));
        };
        // A member of a family that mentions itself carries an incomplete
        // placeholder where it points at itself: the placeholder is what the
        // declaration had when it was being resolved. The finished type is in
        // the table under the same name, and it is the one whose fields and
        // layout are wanted here.
        let nominal = if nominal.is_incomplete {
            self.nominal_types
                .get(&nominal.name)
                .cloned()
                .unwrap_or_else(|| nominal.clone())
        } else {
            nominal.clone()
        };
        let nominal = &nominal;
        let Some((index, field_ty)) = nominal.field(&field.field) else {
            return Err(CodegenError::Emit(format!(
                "internal error: `{}` has no field `{}`",
                nominal.name, field.field
            )));
        };

        let mapper = TypeMapper::new(self.backend.context());
        let field_llvm = mapper
            .map(field_ty)
            .ok_or_else(|| CodegenError::waiting_for_spec("this field's type", span))?;
        let struct_llvm = mapper
            .map(&Type::Nominal(nominal.clone()))
            .ok_or_else(|| CodegenError::waiting_for_spec("this struct type", span))?;

        // The field's address is a struct GEP with the member's index, which is
        // a constant because the layout is fixed. Declaration order is not
        // member order: every field is preceded by an alignment marker.
        let pointer = builder
            .build_struct_gep(
                struct_llvm,
                address,
                llvm_member_index(nominal, index),
                &field.field,
            )
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        Ok(Some((pointer, field_llvm)))
    }

    /// Lowers an assignment.
    ///
    /// The target must denote storage, so it is resolved to a slot rather than
    /// read. A compound assignment reads the slot, applies its operator, and
    /// stores the result, and yields the stored value either way.
    /// Returns the storage a reference-typed place names, if it names one.
    ///
    /// A binding declared `&T` holds an address and is stored in a slot of its
    /// own, so writing to it has to reach through: `r = v` writes the referent,
    /// exactly as `r` reads it. The address is loaded here because code
    /// generation is where the difference between a slot and what it holds is
    /// visible.
    pub(super) fn referent_storage(
        &mut self,
        builder: &Builder<'static>,
        target: ExprId,
        binding: &LocalBinding<'static>,
        types: &TypeTable,
    ) -> Result<Option<LocalBinding<'static>>, CodegenError> {
        let Some(ty) = types.get(target) else {
            return Ok(None);
        };
        let Some(referent) = ty.referent().cloned() else {
            return Ok(None);
        };
        let Some(llvm) = TypeMapper::new(self.backend.context()).map(&referent) else {
            return Ok(None);
        };
        let address = builder
            .build_load(binding.ty, binding.storage, "referent.mut")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        let BasicValueEnum::PointerValue(address) = address else {
            return Ok(None);
        };
        Ok(Some(LocalBinding {
            storage: address,
            ty: llvm,
        }))
    }

    pub(super) fn lower_assign(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        assign: &AssignExpr,
        span: fallc_span::Span,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        let Some(binding) = self.lower_lvalue(ast, types, builder, assign.target) else {
            return Err(CodegenError::waiting_for_spec(
                "assigning to this expression",
                ast.expr(assign.target).span,
            ));
        };
        // A reference names storage, so assigning to one writes the referent:
        // the binding holds an address, and `r = v` stores through it. Without
        // this the address *itself* was overwritten, which is what made
        // `for (x &int : a) { x = x * 2; }` leave the array untouched.
        // Only a *binding* is handled here. A call, an index, or a
        // dereference that yields a reference has already produced the address
        // of the referent, so its storage is the place to write; a binding's
        // slot holds the address, which is one load further out.
        let binding = match (&ast.expr(assign.target).kind, binding.ty) {
            (ExprKind::Name(_), inkwell::types::BasicTypeEnum::PointerType(_)) => {
                match self.referent_storage(builder, assign.target, &binding, types)? {
                    Some(referential) => referential,
                    None => binding,
                }
            }
            _ => binding,
        };

        // A type that defines `__assign__` is assigned by calling it: this is
        // what gives a type ownership, because the method decides what happens
        // to the value already stored. A member-wise copy would duplicate a
        // string's buffer pointer, and freeing it twice is a crash.
        if assign.op.is_none()
            && let Some(symbol) = self.assign_methods.get(&span).cloned()
        {
            let function = self.resolved_function(&symbol, "`__assign__`", span)?;
            let value = self.lower_argument(ast, types, builder, assign.value, span)?;
            let call = builder
                .build_call(function, &[binding.storage.into(), value], "assign")
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
            return self.unwrap_throwing(ast, types, builder, span, call);
        }

        let value_type = binding.ty;
        // Assigning an aggregate copies its bytes. The right-hand side is
        // addressed rather than loaded, for the same reason a return is.
        let aggregate_target = self
            .lvalue_value_type(ast, types, assign.target)
            .is_some_and(|ty| self.aggregate_is_by_address(&ty));
        let stored = match assign.op {
            // Plain `=`: the value stands on its own.
            None => Some(value_type),
            // Compound `op=`: the operator applies to the target's type.
            Some(op) => {
                // A pointer is offset, not combined: `p += n` is `p = p + n`,
                // and the GEP does the scaling. It is handled before the method
                // lookups because a pointer has no methods to find.
                if matches!(binding.ty, inkwell::types::BasicTypeEnum::PointerType(_)) {
                    if matches!(op, BinaryOp::Add | BinaryOp::Sub) {
                        let pointer_ty = self
                            .lvalue_value_type(ast, types, assign.target)
                            .ok_or_else(|| {
                                CodegenError::waiting_for_spec("this pointer's type", span)
                            })?;
                        let pointee = pointer_ty.pointee().cloned().ok_or_else(|| {
                            CodegenError::waiting_for_spec("a compound assignment on a non-pointer", span)
                        })?;
                        let current = builder
                            .build_load(binding.ty, binding.storage, "current")
                            .map_err(|error| CodegenError::Emit(error.to_string()))?;
                        let BasicValueEnum::PointerValue(current) = current else {
                            return Err(CodegenError::waiting_for_spec(
                                "a compound assignment on a non-pointer",
                                span,
                            ));
                        };
                        let offset = self
                            .lower_expr(ast, types, builder, assign.value)?
                            .ok_or_else(|| {
                                CodegenError::waiting_for_spec("this offset", span)
                            })?;
                        let BasicValueEnum::IntValue(offset) = offset else {
                            return Err(CodegenError::waiting_for_spec(
                                "a non-integer pointer offset",
                                span,
                            ));
                        };
                        let index = self.widen_to_pointer_size(builder, offset)?;
                        let index = if op == BinaryOp::Sub {
                            builder
                                .build_int_neg(index, "neg")
                                .map_err(|error| CodegenError::Emit(error.to_string()))?
                        } else {
                            index
                        };
                        let updated =
                            self.build_pointer_offset(builder, current, index, &pointee, span)?;
                        let updated = updated.as_basic_value_enum();
                        builder
                            .build_store(binding.storage, updated)
                            .map_err(|error| CodegenError::Emit(error.to_string()))?;
                        return Ok(Some(updated));
                    }
                    return Err(CodegenError::waiting_for_spec(
                        format!("`{}=` on a pointer", op.spelling()),
                        span,
                    ));
                }
                // The value read from the target is what the operator applies
                // to, so a target whose `__getitem__` returns a *reference* is
                // read first -- the checker recorded where that is wanted.

                // A type that defines `__iadd__` is modified in place by a call
                // to it: `a += b` is `__iadd__(&a, b)`, and the method's result
                // is the new value of `a`.
                if let Some(symbol) = self.in_place_methods.get(&span).cloned() {
                    // The operand is passed as the method's parameter says: a
                    // pointer parameter takes an address, which is what makes
                    // `s += "text"` work.
                    let value: inkwell::values::BasicMetadataValueEnum<'static> =
                        if self.in_place_by_address.get(&span).copied().unwrap_or(false) {
                            // A string literal is constructed into the type the
                            // method expects, and it is that temporary's address
                            // the parameter wants -- not the character array the
                            // literal itself is.
                            if let Some(symbol) = self
                                .literal_constructions
                                .get(&ast.expr(assign.value).span)
                                .cloned()
                            {
                                self.lower_literal_construction(
                                    ast,
                                    types,
                                    builder,
                                    assign.value,
                                    &symbol,
                                    span,
                                )?
                                .as_basic_value_enum()
                                .into()
                            } else {
                                let lowered = self
                                    .lower_expr(ast, types, builder, assign.value)?
                                    .ok_or_else(|| {
                                        CodegenError::waiting_for_spec("this operand", span)
                                    })?;
                                match lowered {
                                    BasicValueEnum::PointerValue(pointer) => {
                                        pointer.as_basic_value_enum().into()
                                    }
                                    value => self
                                        .store_temporary(builder, value, "operand")?
                                        .as_basic_value_enum()
                                        .into(),
                                }
                            }
                        } else {
                            self.lower_adopted_argument(ast, types, builder, assign.value, span)?
                        };
                    let function = match self.backend.module().get_function(&symbol) {
                        Some(function) => function,
                        None => {
                            // The method belongs to an imported type, so it is
                            // declared here from the exported signature.
                            let qualified = self
                                .external_symbols
                                .iter()
                                .find(|(_, signature)| signature.symbol == symbol)
                                .map(|(qualified, _)| qualified.clone())
                                .ok_or_else(|| {
                                    CodegenError::Emit(format!(
                                        "internal error: `{symbol}` was resolved as an operator \
                                         but is neither defined nor imported"
                                    ))
                                })?;
                            self.declare_external(&symbol, &qualified, span)?
                        }
                    };
                    let call = builder
                        .build_call(
                            function,
                            &[binding.storage.into(), value],
                            "inplace",
                        )
                        .map_err(|error| CodegenError::Emit(error.to_string()))?;
                    let result = self.unwrap_throwing(ast, types, builder, span, call)?;
                    // The method's result is stored back into the target, so
                    // that `a += b` leaves `a` holding what the method returned.
                    // The method is free to return something other than its
                    // receiver -- `__iadd__` returning a new value is exactly
                    // what an immutable type does -- so the store is required
                    // rather than optional.
                    if let Some(value) = result {
                        builder
                            .build_store(binding.storage, value)
                            .map_err(|error| CodegenError::Emit(error.to_string()))?;
                    }
                    let _ = op;
                    return Ok(result);
                }
                if !self.type_is_numeric(binding.ty) {
                    // No `__iadd__`, so `a += b` is `a = a + b`: the plain
                    // operator applies and its result is stored back. Which
                    // method that is was resolved with the statement and keyed
                    // by its span, so it is read rather than searched for.
                    //
                    // The receiver is the address already in hand, not the
                    // target re-lowered: lowering `assign.target` again would
                    // evaluate it twice.
                    let Some(method) = self.operator_methods.get(&span).cloned() else {
                        return Err(CodegenError::waiting_for_spec(
                            format!("`{}=` on this type", op.spelling()),
                            span,
                        ));
                    };
                    // The reflected form puts the right operand in the receiver
                    // position, which would need its address rather than its
                    // value. It is not reached by `a += b` on the target's own
                    // type, so it says so instead of passing the wrong thing.
                    if method.reflected {
                        return Err(CodegenError::waiting_for_spec(
                            format!("reflected `{}=` on this type", op.spelling()),
                            span,
                        ));
                    }
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
                                        "internal error: `{}` was resolved as an operator but \
                                         is neither defined nor imported",
                                        method.symbol
                                    ))
                                })?;
                            self.declare_external(&method.symbol, &qualified, span)?
                        }
                    };
                    // The other operand is passed as the method's parameter
                    // says: a value takes the value, a pointer takes an address.
                    let rhs = if method.argument_by_address {
                        let value = self
                            .lower_expr(ast, types, builder, assign.value)?
                            .ok_or_else(|| {
                                CodegenError::waiting_for_spec("this operand", span)
                            })?;
                        match value {
                            BasicValueEnum::PointerValue(pointer) => {
                                pointer.as_basic_value_enum().into()
                            }
                            value => self
                                .store_temporary(builder, value, "operand")?
                                .as_basic_value_enum()
                                .into(),
                        }
                    } else {
                        self.lower_argument(ast, types, builder, assign.value, span)?
                    };
                    let call = builder
                        .build_call(function, &[binding.storage.into(), rhs], "compound")
                        .map_err(|error| CodegenError::Emit(error.to_string()))?;
                    let result = self.unwrap_throwing(ast, types, builder, span, call)?;
                    if let Some(value) = result {
                        builder
                            .build_store(binding.storage, value)
                            .map_err(|error| CodegenError::Emit(error.to_string()))?;
                    }
                    return Ok(result);
                }
                Some(value_type)
            }
        };
        let _ = stored;

        let new_value = match assign.op {
            None if aggregate_target => self
                .lower_aggregate_source(ast, types, builder, assign.value, span)?
                .as_basic_value_enum(),
            None => {
                let value = self
                    .lower_expr(ast, types, builder, assign.value)?
                    .ok_or_else(|| CodegenError::waiting_for_spec("this value", span))?;
                // A lossless integer promotion of the right-hand side is a
                // conversion the checker recorded, not one the source wrote.
                let value = self.convert_adopted(
                    ast,
                    types,
                    builder,
                    assign.value,
                    value,
                    ast.expr(assign.value).span,
                )?;
                self.convert_implicit(
                    ast,
                    types,
                    builder,
                    assign.value,
                    value,
                    ast.expr(assign.value).span,
                )?
            }
            Some(op) => {
                // A target whose `__getitem__` returns a *reference* holds an
                // address, and the operator applies to the value in it. The
                // checker recorded that a value is wanted here, so the address
                // is read before the operation rather than after it.
                let current = match self
                    .deref_reads
                    .get(&ast.expr(assign.target).span)
                    .cloned()
                    .and_then(|referent| {
                        TypeMapper::new(self.backend.context()).map(&referent)
                    }) {
                    Some(referent_llvm) => builder
                        .build_load(referent_llvm, binding.storage, "current")
                        .map_err(|error| CodegenError::Emit(error.to_string()))?,
                    None => builder
                        .build_load(binding.ty, binding.storage, "current")
                        .map_err(|error| CodegenError::Emit(error.to_string()))?,
                };
                let rhs = self
                    .lower_expr(ast, types, builder, assign.value)?
                    .ok_or_else(|| CodegenError::waiting_for_spec("this value", span))?;
                let operand_ty = match binding.ty {
                    inkwell::types::BasicTypeEnum::IntType(int) => {
                        if int.get_bit_width() == 1 {
                            Type::BOOL
                        } else {
                            Type::unspecified("", span)
                        }
                    }
                    _ => Type::unspecified("", span),
                };
                let _ = operand_ty;
                self.build_compound(builder, current, rhs, op, span)?
            }
        };

        if aggregate_target
            && let BasicValueEnum::PointerValue(source) = new_value
        {
            let size = binding.ty.size_of().ok_or_else(|| {
                CodegenError::waiting_for_spec("an aggregate of unknown size", span)
            })?;
            builder
                .build_memcpy(binding.storage, 1, source, 1, size)
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
        } else {
            builder
                .build_store(binding.storage, new_value)
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
        }
        Ok(Some(new_value))
    }

    /// Applies a compound-assignment operator to two already-lowered values.
    pub(super) fn build_compound(
        &mut self,
        builder: &Builder<'static>,
        lhs: BasicValueEnum<'static>,
        rhs: BasicValueEnum<'static>,
        op: BinaryOp,
        span: fallc_span::Span,
    ) -> Result<BasicValueEnum<'static>, CodegenError> {
        Ok(match (lhs, rhs) {
            (BasicValueEnum::IntValue(lhs), BasicValueEnum::IntValue(rhs)) => match op {
                BinaryOp::Add => emitted(builder.build_int_add(lhs, rhs, "add"))?,
                BinaryOp::Sub => emitted(builder.build_int_sub(lhs, rhs, "sub"))?,
                BinaryOp::Mul => emitted(builder.build_int_mul(lhs, rhs, "mul"))?,
                BinaryOp::Div => emitted(builder.build_int_signed_div(lhs, rhs, "div"))?,
                BinaryOp::Rem => emitted(builder.build_int_signed_rem(lhs, rhs, "rem"))?,
                BinaryOp::BitAnd => emitted(builder.build_and(lhs, rhs, "and"))?,
                BinaryOp::BitOr => emitted(builder.build_or(lhs, rhs, "or"))?,
                BinaryOp::BitXor => emitted(builder.build_xor(lhs, rhs, "xor"))?,
                BinaryOp::Shl => emitted(builder.build_left_shift(lhs, rhs, "shl"))?,
                BinaryOp::Shr => emitted(builder.build_right_shift(lhs, rhs, true, "shr"))?,
                BinaryOp::Pow => self.build_integer_power(builder, lhs, rhs, "pow")?,
                _ => {
                    return Err(CodegenError::waiting_for_spec(
                        "this compound assignment operator",
                        span,
                    ));
                }
            },
            (BasicValueEnum::FloatValue(lhs), BasicValueEnum::FloatValue(rhs)) => match op {
                BinaryOp::Add => emitted(builder.build_float_add(lhs, rhs, "add"))?,
                BinaryOp::Sub => emitted(builder.build_float_sub(lhs, rhs, "sub"))?,
                BinaryOp::Mul => emitted(builder.build_float_mul(lhs, rhs, "mul"))?,
                BinaryOp::Div => emitted(builder.build_float_div(lhs, rhs, "div"))?,
                _ => {
                    return Err(CodegenError::waiting_for_spec(
                        "this compound assignment operator",
                        span,
                    ));
                }
            },
            _ => {
                return Err(CodegenError::waiting_for_spec(
                    "a compound assignment on these operands",
                    span,
                ));
            }
        })
    }

    /// Lowers `++` and `--`, in either position.
    ///
    /// The operand is read, adjusted, and written back. The value the
    /// expression yields is the *old* value for a postfix operator and the
    /// *new* one for a prefix operator, which is the only difference between
    /// the two forms.
    /// Lowers `++v`, `v++`, `--v`, or `v--` on a type that defines the method.
    ///
    /// The call modifies the receiver in place. What differs between the two
    /// positions is the value the *expression* has, so the compiler takes a
    /// copy of the old value first when the postfix form was written.
    pub(super) fn lower_incdec_method(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        incdec: &IncDecExpr,
        symbol: &str,
        span: fallc_span::Span,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        let binding = self
            .lower_lvalue(ast, types, builder, incdec.operand)
            .ok_or_else(|| CodegenError::waiting_for_spec("incrementing this expression", span))?;

        // The postfix form yields the value as it was, so it is read before the
        // call. The prefix form yields the receiver, which the call has already
        // modified, so the load happens after.
        let saved = if incdec.postfix {
            Some(
                builder
                    .build_load(binding.ty, binding.storage, "old")
                    .map_err(|error| CodegenError::Emit(error.to_string()))?,
            )
        } else {
            None
        };

        let function = self.resolved_function(symbol, "`__inc__`", span)?;
        let call = builder
            .build_call(function, &[binding.storage.into()], "incdec")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        // The method wrote the new value into the binding itself, so nothing
        // here needs its result -- but a raise must escape before the value is
        // read back.
        let _ = self.unwrap_throwing(ast, types, builder, span, call)?;

        let value = match saved {
            Some(saved) => saved,
            None => builder
                .build_load(binding.ty, binding.storage, "new")
                .map_err(|error| CodegenError::Emit(error.to_string()))?,
        };
        // The result is kept, so a member of it can be reached: `(c++).value()`
        // asks the value from before the increment for its address, and a
        // register has none.
        let storage = self.store_temporary(builder, value, "incdec.value")?;
        self.temporaries.insert(
            span,
            LocalBinding {
                storage,
                ty: value.get_type(),
            },
        );
        Ok(Some(value))
    }

    pub(super) fn lower_incdec(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        incdec: &IncDecExpr,
        span: fallc_span::Span,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        // A type with `__inc__` is modified by calling it. The two written
        // forms differ only in what the expression *yields*, and that is
        // generated here rather than being the type's business: `++v` yields
        // the receiver, `v++` yields a copy taken before the call.
        if let Some(symbol) = self.unary_methods.get(&span).cloned() {
            return self.lower_incdec_method(ast, types, builder, incdec, &symbol, span);
        }

        let binding = self
            .lower_lvalue(ast, types, builder, incdec.operand)
            .ok_or_else(|| CodegenError::waiting_for_spec("incrementing this expression", span))?;

        let previous = builder
            .build_load(binding.ty, binding.storage, "old")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;

        // A pointer moves by one element, which is a GEP with an index of one:
        // the scaling is the instruction's, so `++p` on a `*Pair` moves by
        // `sizeof(Pair)` rather than by a byte.
        if let BasicValueEnum::PointerValue(old) = previous {
            let pointer_ty = self
                .lvalue_value_type(ast, types, incdec.operand)
                .ok_or_else(|| CodegenError::waiting_for_spec("this pointer's type", span))?;
            let pointee = pointer_ty.pointee().cloned().ok_or_else(|| {
                CodegenError::waiting_for_spec("incrementing a non-pointer", span)
            })?;
            let target_data = self.target_data()?;
            let index = self
                .backend
                .context()
                .ptr_sized_int_type(&target_data, None)
                .const_int(1, false);
            let index = if incdec.op == IncDecOp::Decrement {
                builder
                    .build_int_neg(index, "neg")
                    .map_err(|error| CodegenError::Emit(error.to_string()))?
            } else {
                index
            };
            let updated = self.build_pointer_offset(builder, old, index, &pointee, span)?;
            let updated = updated.as_basic_value_enum();
            builder
                .build_store(binding.storage, updated)
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
            return Ok(Some(if incdec.postfix { previous } else { updated }));
        }

        let one: BasicValueEnum<'static> = match (binding.ty, incdec.op) {
            (inkwell::types::BasicTypeEnum::IntType(int), _) => {
                int.const_int(1, false).as_basic_value_enum()
            }
            (inkwell::types::BasicTypeEnum::FloatType(float), _) => {
                float.const_float(1.0).as_basic_value_enum()
            }
            _ => {
                return Err(CodegenError::waiting_for_spec(
                    "incrementing this type",
                    span,
                ));
            }
        };

        let updated = match (previous, one, incdec.op) {
            (BasicValueEnum::IntValue(lhs), BasicValueEnum::IntValue(rhs), IncDecOp::Increment) => {
                emitted(builder.build_int_add(lhs, rhs, "inc"))?
            }
            (BasicValueEnum::IntValue(lhs), BasicValueEnum::IntValue(rhs), IncDecOp::Decrement) => {
                emitted(builder.build_int_sub(lhs, rhs, "dec"))?
            }
            (
                BasicValueEnum::FloatValue(lhs),
                BasicValueEnum::FloatValue(rhs),
                IncDecOp::Increment,
            ) => emitted(builder.build_float_add(lhs, rhs, "inc"))?,
            (
                BasicValueEnum::FloatValue(lhs),
                BasicValueEnum::FloatValue(rhs),
                IncDecOp::Decrement,
            ) => emitted(builder.build_float_sub(lhs, rhs, "dec"))?,
            _ => {
                return Err(CodegenError::waiting_for_spec(
                    "incrementing this type",
                    span,
                ));
            }
        };

        builder
            .build_store(binding.storage, updated)
            .map_err(|error| CodegenError::Emit(error.to_string()))?;

        // A postfix operator yields the value from before the change.
        Ok(Some(if incdec.postfix { previous } else { updated }))
    }

}
