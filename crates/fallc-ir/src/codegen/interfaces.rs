//! Interfaces: boxing a value, and calling through its table.
//!
//! A value of an interface type is a heap copy of a concrete value plus a
//! pointer to a table of functions. Boxing happens where the value is used as
//! the interface: the value is lowered as it normally would be, copied onto the
//! heap, and paired with the table for its type. A call through the interface
//! loads the entry the checker chose and calls it with the copy's address as
//! the receiver -- the concrete method's own `this`.

use super::*;

impl<'ctx> Lowerer<'ctx> {
    /// Boxes a value into an interface: heap copy plus table.
    pub(super) fn lower_interface_conversion(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        expr: ExprId,
        conversion: &fallc_sema::InterfaceConversion,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        // An interface used as another interface keeps its object and gets a
        // table of the target's methods, picked from the source's.
        if let Some(source) = &conversion.source {
            return self.lower_interface_rebox(ast, types, builder, expr, conversion, source);
        }
        let context = self.backend.context();
        let span = ast.expr(expr).span;
        let mapper = TypeMapper::new(context);
        let Some(concrete) = mapper.map(&conversion.concrete) else {
            return Err(CodegenError::waiting_for_spec(
                "a value of this type used as an interface",
                span,
            ));
        };

        // The value itself, lowered as it normally would be. Taking the
        // conversion out first keeps this call from finding it again.
        let saved = self.interface_conversions.remove(&span);
        let value = self.lower_expr(ast, types, builder, expr);
        if let Some(saved) = saved {
            self.interface_conversions.insert(span, saved);
        }
        let Some(value) = value? else {
            return Ok(None);
        };
        let source = if matches!(value, BasicValueEnum::PointerValue(_))
            && self.aggregate_is_by_address(&conversion.concrete)
        {
            value.into_pointer_value()
        } else {
            self.store_temporary(builder, value, "boxed")?
        };

        // The copy: `malloc(size)` and a `memcpy` from the value's storage. The
        // heap is what the settled rule asks for, and it is also what makes the
        // copy outlive the expression that produced it.
        // The size comes from the target, not from LLVM's idea of the type:
        // a struct's layout is what the target machine decided.
        let target_data = self.target_data()?;
        let size = context
            .i64_type()
            .const_int(target_data.get_abi_size(&concrete), false);
        let malloc = self.declare_malloc();
        let call = builder
            .build_call(malloc, &[size.into()], "box")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        let Some(BasicValueEnum::PointerValue(data)) = call.try_as_basic_value().basic() else {
            return Err(CodegenError::Emit(
                "internal error: `malloc` did not return an address".to_owned(),
            ));
        };
        builder
            .build_memcpy(data, 1, source, 1, size)
            .map_err(|error| CodegenError::Emit(error.to_string()))?;

        let table = self.interface_vtable(conversion)?;
        let Some(inkwell::types::BasicTypeEnum::StructType(struct_ty)) =
            mapper.map(&conversion.interface_type)
        else {
            return Err(CodegenError::waiting_for_spec(
                "an interface value with no layout",
                span,
            ));
        };
        let mut aggregate = struct_ty.const_zero();
        aggregate = builder
            .build_insert_value(aggregate, data, 0, "interface")
            .map_err(|error| CodegenError::Emit(error.to_string()))?
            .into_struct_value();
        aggregate = builder
            .build_insert_value(aggregate, table, 1, "interface")
            .map_err(|error| CodegenError::Emit(error.to_string()))?
            .into_struct_value();
        Ok(Some(aggregate.as_basic_value_enum()))
    }

    /// Rebuilds an interface value as another interface's.
    ///
    /// The object and the table are both in the source value already; what the
    /// target needs is a table of its own methods, whose entries are the
    /// source's at the indices the checker matched by name.
    fn lower_interface_rebox(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        expr: ExprId,
        conversion: &fallc_sema::InterfaceConversion,
        source: &fallc_sema::InterfaceSource,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        let context = self.backend.context();
        let pointer = context.ptr_type(inkwell::AddressSpace::default());
        let span = ast.expr(expr).span;
        let mapper = TypeMapper::new(context);

        let saved = self.interface_conversions.remove(&span);
        let value = self.lower_expr(ast, types, builder, expr);
        if let Some(saved) = saved {
            self.interface_conversions.insert(span, saved);
        }
        let Some(value) = value? else {
            return Ok(None);
        };
        let Some(inkwell::types::BasicTypeEnum::StructType(source_ty)) =
            mapper.map(&conversion.concrete)
        else {
            return Err(CodegenError::waiting_for_spec(
                "an interface value with no layout",
                span,
            ));
        };
        let (data, table) = match value {
            BasicValueEnum::StructValue(aggregate) => (
                builder
                    .build_extract_value(aggregate, 0, "data")
                    .map_err(|error| CodegenError::Emit(error.to_string()))?,
                builder
                    .build_extract_value(aggregate, 1, "table")
                    .map_err(|error| CodegenError::Emit(error.to_string()))?,
            ),
            BasicValueEnum::PointerValue(address) => {
                let data_field = build_field_gep(builder, source_ty.into(), address, 0)?;
                let table_field = build_field_gep(builder, source_ty.into(), address, 1)?;
                (
                    builder
                        .build_load(pointer, data_field, "data")
                        .map_err(|error| CodegenError::Emit(error.to_string()))?,
                    builder
                        .build_load(pointer, table_field, "table")
                        .map_err(|error| CodegenError::Emit(error.to_string()))?,
                )
            }
            _ => {
                return Err(CodegenError::waiting_for_spec(
                    "an interface value as a receiver",
                    span,
                ));
            }
        };
        let BasicValueEnum::PointerValue(table) = table else {
            return Err(CodegenError::waiting_for_spec(
                "an interface without a method table",
                span,
            ));
        };

        // The new table lives on the heap: the value may outlive this call, and
        // a stack array would not.
        let count = u64::try_from(source.entries.len()).unwrap_or(0);
        let target_data = self.target_data()?;
        let slot_size = target_data.get_abi_size(&pointer);
        let malloc = self.declare_malloc();
        let bytes = context
            .i64_type()
            .const_int(slot_size.saturating_mul(count), false);
        let call = builder
            .build_call(malloc, &[bytes.into()], "table")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        let Some(BasicValueEnum::PointerValue(new_table)) = call.try_as_basic_value().basic()
        else {
            return Err(CodegenError::Emit(
                "internal error: `malloc` did not return an address".to_owned(),
            ));
        };
        for (target_index, source_index) in source.entries.iter().enumerate() {
            let source_slot = build_index_gep(
                builder,
                pointer.into(),
                table,
                context
                    .i32_type()
                    .const_int(u64::try_from(*source_index).unwrap_or(0), false),
            )?;
            let entry = builder
                .build_load(pointer, source_slot, "entry")
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
            let target_slot = build_index_gep(
                builder,
                pointer.into(),
                new_table,
                context
                    .i32_type()
                    .const_int(u64::try_from(target_index).unwrap_or(0), false),
            )?;
            builder
                .build_store(target_slot, entry)
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
        }

        let Some(inkwell::types::BasicTypeEnum::StructType(target_ty)) =
            mapper.map(&conversion.interface_type)
        else {
            return Err(CodegenError::waiting_for_spec(
                "an interface value with no layout",
                span,
            ));
        };
        let mut aggregate = target_ty.const_zero();
        aggregate = builder
            .build_insert_value(aggregate, data, 0, "interface")
            .map_err(|error| CodegenError::Emit(error.to_string()))?
            .into_struct_value();
        aggregate = builder
            .build_insert_value(aggregate, new_table, 1, "interface")
            .map_err(|error| CodegenError::Emit(error.to_string()))?
            .into_struct_value();
        Ok(Some(aggregate.as_basic_value_enum()))
    }

    /// Returns the table of methods for one concrete type, emitting it once.
    fn interface_vtable(
        &mut self,
        conversion: &fallc_sema::InterfaceConversion,
    ) -> Result<inkwell::values::PointerValue<'static>, CodegenError> {
        let key = format!(
            "{}.{}",
            conversion.interface,
            conversion.concrete.spelling()
        );
        if let Some(global) = self.interface_vtables.get(&key) {
            return Ok(*global);
        }
        let context = self.backend.context();
        let pointer = context.ptr_type(inkwell::AddressSpace::default());
        let mut entries = Vec::with_capacity(conversion.methods.len());
        for symbol in &conversion.methods {
            let function = match self.backend.module().get_function(symbol) {
                Some(function) => function,
                // The method's body may be in a module that is lowered after
                // this one. Its export carries the symbol and the signature, so
                // a declaration is enough here; the definition arrives with
                // that module.
                None => {
                    let qualified = self
                        .external_symbols
                        .iter()
                        .find(|(_, signature)| signature.symbol == *symbol)
                        .map(|(qualified, _)| qualified.clone());
                    let Some(qualified) = qualified else {
                        return Err(CodegenError::waiting_for_spec(
                            format!("the concrete method `{symbol}` of this interface"),
                            fallc_span::Span::SYNTHETIC,
                        ));
                    };
                    self.declare_external(symbol, &qualified, fallc_span::Span::SYNTHETIC)?
                }
            };
            entries.push(
                function
                    .as_global_value()
                    .as_pointer_value()
                    .as_basic_value_enum(),
            );
        }
        let array_ty = pointer.array_type(
            u32::try_from(entries.len())
                .map_err(|_| CodegenError::Emit("internal error: too many methods".to_owned()))?,
        );
        let Some(initializer) = constant_array(pointer.into(), entries) else {
            return Err(CodegenError::Emit(
                "internal error: the method table is not a constant array".to_owned(),
            ));
        };
        let initializer = match initializer {
            BasicValueEnum::ArrayValue(array) => array,
            _ => {
                return Err(CodegenError::Emit(
                    "internal error: the method table is not an array".to_owned(),
                ));
            }
        };
        let global = self
            .backend
            .module()
            .add_global(array_ty, None, &format!(".vtable.{key}"));
        global.set_initializer(&initializer);
        global.set_constant(true);
        global.set_linkage(inkwell::module::Linkage::Private);
        let address = global.as_pointer_value();
        self.interface_vtables.insert(key, address);
        Ok(address)
    }

    /// Lowers a call of an interface method: load the entry, call it.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn lower_interface_call(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        field: &FieldExpr,
        call: &CallExpr,
        info: &fallc_sema::InterfaceCall,
        span: fallc_span::Span,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        let context = self.backend.context();
        let pointer = context.ptr_type(inkwell::AddressSpace::default());
        let mapper = TypeMapper::new(context);

        // The receiver is the interface value: its two fields are the object
        // and its table.
        let receiver_ty = types
            .get(field.object)
            .cloned()
            .ok_or_else(|| CodegenError::waiting_for_spec("this receiver", span))?;
        // A reference to an interface is an address of the box, not the box:
        // the layout is the referent's, and the fields are read from the
        // address the reference holds.
        let behind = behind_references(&receiver_ty);
        let Some(inkwell::types::BasicTypeEnum::StructType(struct_ty)) = mapper.map(behind) else {
            return Err(CodegenError::waiting_for_spec(
                "an interface value with no layout",
                span,
            ));
        };
        let (data, table) = if receiver_ty.is_reference() {
            let address = match self.lower_expr(ast, types, builder, field.object)? {
                Some(value) => value,
                None => {
                    let binding = self
                        .lower_lvalue(ast, types, builder, field.object)
                        .ok_or_else(|| {
                            CodegenError::waiting_for_spec("this interface reference", span)
                        })?;
                    builder
                        .build_load(pointer, binding.storage, "interface")
                        .map_err(|error| CodegenError::Emit(error.to_string()))?
                }
            };
            let data_field =
                build_field_gep(builder, struct_ty.into(), address.into_pointer_value(), 0)?;
            let table_field =
                build_field_gep(builder, struct_ty.into(), address.into_pointer_value(), 1)?;
            (
                builder
                    .build_load(pointer, data_field, "data")
                    .map_err(|error| CodegenError::Emit(error.to_string()))?,
                builder
                    .build_load(pointer, table_field, "table")
                    .map_err(|error| CodegenError::Emit(error.to_string()))?,
            )
        } else if let Some(binding) = self.lower_lvalue(ast, types, builder, field.object) {
            let data_field = build_field_gep(builder, struct_ty.into(), binding.storage, 0)?;
            let table_field = build_field_gep(builder, struct_ty.into(), binding.storage, 1)?;
            (
                builder
                    .build_load(pointer, data_field, "data")
                    .map_err(|error| CodegenError::Emit(error.to_string()))?,
                builder
                    .build_load(pointer, table_field, "table")
                    .map_err(|error| CodegenError::Emit(error.to_string()))?,
            )
        } else {
            let value = self
                .lower_expr(ast, types, builder, field.object)?
                .ok_or_else(|| CodegenError::waiting_for_spec("this receiver", span))?;
            let BasicValueEnum::StructValue(aggregate) = value else {
                return Err(CodegenError::waiting_for_spec(
                    "an interface value as a receiver",
                    span,
                ));
            };
            (
                builder
                    .build_extract_value(aggregate, 0, "data")
                    .map_err(|error| CodegenError::Emit(error.to_string()))?,
                builder
                    .build_extract_value(aggregate, 1, "table")
                    .map_err(|error| CodegenError::Emit(error.to_string()))?,
            )
        };

        // The entry the checker chose, then the call through it.
        let BasicValueEnum::PointerValue(table) = table else {
            return Err(CodegenError::waiting_for_spec(
                "an interface without a method table",
                span,
            ));
        };
        let index = context
            .i32_type()
            .const_int(u64::try_from(info.index).unwrap_or(0), false);
        let slot = build_index_gep(builder, pointer.into(), table, index)?;
        let callee = builder
            .build_load(pointer, slot, "method")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;

        // A method that may raise returns its result through the exception ABI,
        // which carries the value inline -- so that shape and the hidden slot
        // are alternatives, exactly as they are for a written function.
        let throwing = info.throws;
        let throwing_result = if throwing {
            let value = if info.returns.is_void() {
                None
            } else {
                let Some(llvm) = mapper.map(&info.returns) else {
                    return Err(CodegenError::waiting_for_spec(
                        "an interface method returning this type",
                        span,
                    ));
                };
                Some(llvm)
            };
            let Some(result) = self.throwing_result_type(value) else {
                return Err(CodegenError::waiting_for_spec(
                    "an interface method that may raise",
                    span,
                ));
            };
            Some(result)
        } else {
            None
        };
        // A large aggregate comes back through a hidden first pointer, exactly
        // as a written function's does: the caller allocates the slot, the
        // method writes it, and the call's value is the slot's address.
        let out_of_line =
            !throwing && !info.returns.is_void() && self.aggregate_is_by_address(&info.returns);
        let sret_slot = if out_of_line {
            let Some(llvm) = mapper.map(&info.returns) else {
                return Err(CodegenError::waiting_for_spec(
                    "an interface method returning this type",
                    span,
                ));
            };
            let entry = self
                .current_function
                .and_then(|function| function.get_first_basic_block())
                .ok_or_else(|| {
                    CodegenError::Emit("internal error: no function is being lowered".to_owned())
                })?;
            Some(self.create_entry_alloca(builder, entry, "virtual.result", llvm))
        } else {
            None
        };
        let mut parameters: Vec<inkwell::types::BasicMetadataTypeEnum<'static>> = Vec::new();
        if sret_slot.is_some() {
            parameters.push(pointer.into());
        }
        parameters.push(pointer.into());
        for ty in &info.parameters {
            let Some(llvm) = mapper.map(ty) else {
                return Err(CodegenError::waiting_for_spec(
                    "an interface method parameter of this type",
                    span,
                ));
            };
            if self.aggregate_is_by_address(ty) {
                parameters.push(pointer.into());
            } else {
                parameters.push(llvm.into());
            }
        }
        let function_type = if let Some(result) = throwing_result {
            inkwell::types::BasicType::fn_type(&result, &parameters, false)
        } else if out_of_line || info.returns.is_void() {
            context.void_type().fn_type(&parameters, false)
        } else {
            let Some(returns) = mapper.map(&info.returns) else {
                return Err(CodegenError::waiting_for_spec(
                    "an interface method returning this type",
                    span,
                ));
            };
            returns.fn_type(&parameters, false)
        };

        let mut arguments: Vec<inkwell::values::BasicMetadataValueEnum<'static>> = Vec::new();
        if let Some(slot) = sret_slot {
            arguments.push(slot.as_basic_value_enum().into());
        }
        arguments.push(data.into());
        for &argument in &call.args {
            arguments.push(self.lower_argument(ast, types, builder, argument, span)?);
        }
        let site = builder
            .build_indirect_call(
                function_type,
                callee.into_pointer_value(),
                &arguments,
                "virtual",
            )
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        if let Some(slot) = sret_slot {
            return Ok(Some(slot.as_basic_value_enum()));
        }
        // The tag is read by the caller of this lowering, from the shape of the
        // value, so a throwing method's result is returned whole.
        Ok(match site.try_as_basic_value() {
            inkwell::values::ValueKind::Basic(value) => Some(value),
            inkwell::values::ValueKind::Instruction(_) => None,
        })
    }

    /// Declares `malloc`, which boxing needs and the language does not declare.
    fn declare_malloc(&mut self) -> inkwell::values::FunctionValue<'static> {
        if let Some(function) = self.backend.module().get_function("malloc") {
            return function;
        }
        let context = self.backend.context();
        let pointer = context.ptr_type(inkwell::AddressSpace::default());
        let signature = pointer.fn_type(&[context.i64_type().into()], false);
        self.backend
            .module()
            .add_function("malloc", signature, None)
    }
}

/// Peels references and `const` to reach the type a value names.
///
/// `&ICount` is an address, so the layout of what it points at is `ICount`'s:
/// a reference around an interface changes where the box is read from, not
/// what its two fields are.
fn behind_references(ty: &fallc_sema::Type) -> &fallc_sema::Type {
    let mut ty = ty;
    loop {
        match ty {
            fallc_sema::Type::Reference { referent, .. } => ty = referent,
            fallc_sema::Type::Const(inner) => ty = inner,
            other => return other,
        }
    }
}
