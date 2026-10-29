//! Lambdas: the body an anonymous function becomes, and the calls to it.
//!
//! A lambda is lowered as a function of its own. The checker decided its
//! parameter types -- per call, when a parameter's type was left out -- the
//! names it captures, and what it returns, so this only has to build the
//! declaration, place the arguments, and emit the body.
//!
//! A captured name is passed as the *address* of the variable it names, so a
//! read or a write in the body goes to the caller's variable rather than to a
//! copy. The emitted body binds that address as the variable's storage.
//!
//! The body is emitted through the ordinary function lowering, which is why a
//! block-bodied lambda returns, destroys its locals and raises exactly as a
//! function does: the declaration handed to it is a function item like any
//! other. Its body is emitted after the function that wrote the lambda is
//! finished, because lowering a body needs the lowering state to itself; the
//! call sites only need the declaration, which can be made at any time.

use super::*;

impl<'ctx> Lowerer<'ctx> {
    /// Lowers a call whose callee is a lambda.
    pub(super) fn lower_lambda_call(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        call: &CallExpr,
        span: fallc_span::Span,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        let Some(instance) = self.lambda_calls.get(&span).cloned() else {
            return Err(CodegenError::waiting_for_spec("this lambda call", span));
        };
        let ExprKind::Lambda(lambda) = &ast.expr(call.callee).kind else {
            return Err(CodegenError::Emit(
                "internal error: a lambda call whose callee is not a lambda".to_owned(),
            ));
        };
        let lambda = lambda.clone();
        let function = self.declare_lambda(ast, &lambda, &instance)?;

        // The captured variables come first, as their addresses: that is the
        // environment the body was written against.
        let mut arguments: Vec<inkwell::values::BasicMetadataValueEnum<'static>> =
            Vec::with_capacity(instance.captures.len() + call.args.len());
        for capture in &instance.captures {
            let name = &capture.name;
            let binding = self.lookup_binding(name).ok_or_else(|| {
                CodegenError::waiting_for_spec(
                    format!("the captured `{name}` at this call"),
                    span,
                )
            })?;
            arguments.push(binding.storage.into());
        }

        self.lower_lambda_arguments(ast, types, builder, call, &instance, span, &mut arguments)?;
        self.build_value_call(builder, function, &instance.symbol, arguments, span)
    }

    /// Lowers a lambda used as a value: the address of the function it names,
    /// or, when the value is a closure, the closure itself.
    pub(super) fn lower_lambda_value(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        expr: ExprId,
        lambda: &fallc_ast::LambdaExpr,
        span: fallc_span::Span,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        // A capturing lambda is a value whose fields are the addresses of what
        // it captured; the checker gave that type, and the field names are the
        // captured names.
        if let Some(Type::Nominal(nominal)) = types.get(expr).cloned() {
            let is_closure = nominal.name.starts_with("__lambda.");
            if is_closure {
                return self.lower_closure_value(ast, builder, &nominal, span);
            }
        }
        self.lower_lambda_function_value(ast, lambda, span)
    }

    /// Builds a closure value: one field per captured variable's address.
    fn lower_closure_value(
        &mut self,
        ast: &Ast,
        builder: &Builder<'static>,
        nominal: &fallc_sema::NominalType,
        span: fallc_span::Span,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        let _ = ast;
        let context = self.backend.context();
        let mapper = TypeMapper::new(context);
        let Some(inkwell::types::BasicTypeEnum::StructType(struct_ty)) =
            mapper.map(&Type::Nominal(nominal.clone()))
        else {
            return Err(CodegenError::waiting_for_spec(
                "a closure value with no layout",
                span,
            ));
        };
        let mut aggregate = struct_ty.const_zero();
        for (index, (captured, _)) in nominal.fields.iter().enumerate() {
            let Some(binding) = self.lookup_binding(captured) else {
                return Err(CodegenError::waiting_for_spec(
                    format!("the captured `{captured}`"),
                    span,
                ));
            };
            aggregate = builder
                .build_insert_value(aggregate, binding.storage, index as u32, "closure")
                .map_err(|error| CodegenError::Emit(error.to_string()))?
                .into_struct_value();
        }
        Ok(Some(aggregate.as_basic_value_enum()))
    }

    /// Lowers a lambda that became a function value: the address of its body.
    fn lower_lambda_function_value(
        &mut self,
        ast: &Ast,
        lambda: &fallc_ast::LambdaExpr,
        span: fallc_span::Span,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        let Some(instance) = self.lambda_values.get(&span).cloned() else {
            return Err(CodegenError::waiting_for_spec("this lambda", span));
        };
        let function = self.declare_lambda(ast, lambda, &instance)?;
        Ok(Some(
            function
                .as_global_value()
                .as_pointer_value()
                .as_basic_value_enum(),
        ))
    }

    /// Lowers a call of a closure value.
    ///
    /// The value's fields are the captured addresses, so the call passes them
    /// as the body's leading parameters -- the same shape an immediate call of
    /// the lambda produces.
    pub(super) fn lower_closure_call(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        call: &CallExpr,
        info: &fallc_sema::ClosureCall,
        span: fallc_span::Span,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        let context = self.backend.context();
        let pointer = context.ptr_type(inkwell::AddressSpace::default());
        let mapper = TypeMapper::new(context);
        let function = self.declare_lambda(&info.tree, &info.instance.lambda, &info.instance)?;

        let Some(inkwell::types::BasicTypeEnum::StructType(struct_ty)) =
            mapper.map(&info.ty)
        else {
            return Err(CodegenError::waiting_for_spec(
                "a closure value with no layout",
                span,
            ));
        };
        let value = self
            .lower_expr(ast, types, builder, info.value)?
            .ok_or_else(|| CodegenError::waiting_for_spec("this closure", span))?;
        let address = match value {
            BasicValueEnum::PointerValue(address) => address,
            other => self.store_temporary(builder, other, "closure")?,
        };
        let mut arguments: Vec<inkwell::values::BasicMetadataValueEnum<'static>> = Vec::new();
        for index in 0..struct_ty.count_fields() {
            let field = build_field_gep(builder, struct_ty.into(), address, index)?;
            let captured = builder
                .build_load(pointer, field, "capture")
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
            arguments.push(captured.into());
        }
        self.lower_lambda_arguments(
            ast,
            types,
            builder,
            call,
            &info.instance,
            span,
            &mut arguments,
        )?;
        self.build_value_call(builder, function, &info.instance.symbol, arguments, span)
    }

    /// Lowers a lambda call's written arguments into their parameter order.
    #[allow(clippy::too_many_arguments)]
    fn lower_lambda_arguments(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        call: &CallExpr,
        instance: &fallc_sema::LambdaInstance,
        span: fallc_span::Span,
        arguments: &mut Vec<inkwell::values::BasicMetadataValueEnum<'static>>,
    ) -> Result<(), CodegenError> {
        let mut slots: Vec<Option<inkwell::values::BasicMetadataValueEnum<'static>>> =
            vec![None; instance.parameters.len()];
        for (index, &argument) in call.args.iter().enumerate() {
            let value = self.lower_adopted_argument(ast, types, builder, argument, span)?;
            if let Some(slot) = slots.get_mut(index) {
                *slot = Some(value);
            }
        }
        for named in &call.named {
            let Some(index) = instance
                .lambda
                .parameters
                .iter()
                .position(|parameter| parameter.name == named.name)
            else {
                return Err(CodegenError::Emit(format!(
                    "internal error: this lambda has no parameter `{}`",
                    named.name
                )));
            };
            let value = self.lower_adopted_argument(ast, types, builder, named.value, span)?;
            slots[index] = Some(value);
        }
        for slot in slots {
            let Some(value) = slot else {
                return Err(CodegenError::waiting_for_spec(
                    "an argument this lambda was not given",
                    span,
                ));
            };
            arguments.push(value);
        }
        Ok(())
    }

    /// Declares a lambda's function and queues its body.
    ///
    /// The declaration is the part a call site needs: the symbol, the
    /// signature, and the return convention. The body cannot be lowered here,
    /// because a call site is reached *inside* another function's lowering, and
    /// that state is that function's.
    fn declare_lambda(
        &mut self,
        ast: &Ast,
        lambda: &fallc_ast::LambdaExpr,
        instance: &fallc_sema::LambdaInstance,
    ) -> Result<inkwell::values::FunctionValue<'static>, CodegenError> {
        if let Some(function) = self.lambda_functions.get(&instance.symbol) {
            return Ok(*function);
        }
        let (llvm_function, _) = self.lambda_signature(instance)?;
        // The body is emitted after the function that wrote the lambda is
        // finished. Recording it here, rather than emitting it, keeps the two
        // kinds of work apart.
        if !self
            .pending_lambdas
            .iter()
            .any(|pending| pending.instance.symbol == instance.symbol)
        {
            self.pending_lambdas.push(PendingLambda {
                tree: ast.clone(),
                lambda: lambda.clone(),
                instance: instance.clone(),
            });
        }
        self.lambda_functions
            .insert(instance.symbol.clone(), llvm_function);
        Ok(llvm_function)
    }

    /// Builds the LLVM declaration of a lambda without a body.
    fn lambda_signature(
        &mut self,
        instance: &fallc_sema::LambdaInstance,
    ) -> Result<
        (
            inkwell::values::FunctionValue<'static>,
            inkwell::types::FunctionType<'static>,
        ),
        CodegenError,
    > {
        let context = self.backend.context();
        let mapper = TypeMapper::new(context);
        let return_llvm = if instance.returns.is_void() {
            None
        } else {
            mapper.map(&instance.returns)
        };
        // A body that raises returns its result struct, tag included, so the
        // hidden slot is not used: the two shapes are alternatives.
        let out_of_line = !instance.returns.is_void()
            && !instance.throws
            && self.aggregate_is_by_address(&instance.returns);
        // A body that can raise returns the exception result struct, exactly as
        // a written `except` function does.
        let return_llvm = if instance.throws {
            self.throwing_result_type(return_llvm)
        } else {
            return_llvm
        };
        let mut param_types = Vec::with_capacity(instance.captures.len() + instance.parameters.len());
        if out_of_line {
            param_types.push(context.ptr_type(inkwell::AddressSpace::default()).into());
        }
        // A capture is an address, whatever the captured variable's type is.
        for _ in &instance.captures {
            param_types.push(context.ptr_type(inkwell::AddressSpace::default()).into());
        }
        for ty in &instance.parameters {
            let Some(llvm) = mapper.map(ty) else {
                continue;
            };
            let llvm = if self.aggregate_is_by_address(ty) {
                context.ptr_type(inkwell::AddressSpace::default()).into()
            } else {
                llvm
            };
            param_types.push(llvm.into());
        }
        let fn_type = match return_llvm.filter(|_| !out_of_line) {
            Some(return_llvm) => {
                inkwell::types::BasicType::fn_type(&return_llvm, &param_types, false)
            }
            None => context.void_type().fn_type(&param_types, false),
        };
        let function = self
            .backend
            .module()
            .get_function(&instance.symbol)
            .unwrap_or_else(|| {
                self.backend
                    .module()
                    .add_function(&instance.symbol, fn_type, None)
            });
        self.return_kinds.insert(
            instance.symbol.clone(),
            if out_of_line {
                ReturnKind::OutOfLine(instance.returns.clone())
            } else {
                ReturnKind::Value
            },
        );
        Ok((function, fn_type))
    }

    /// Emits the bodies of every lambda declared so far.
    ///
    /// Called when no other function is being lowered, so the lowering state is
    /// free. A body may call another lambda, which declares and queues that
    /// one; the loop continues until nothing is left.
    pub(super) fn lower_pending_lambdas(&mut self) {
        while let Some(pending) = self.pending_lambdas.pop() {
            if self.lambda_emitted.contains(&pending.instance.symbol) {
                continue;
            }
            self.emit_lambda_body(&pending);
        }
    }

    /// Emits one lambda body, reusing the ordinary function lowering.
    fn emit_lambda_body(&mut self, pending: &PendingLambda) {
        let PendingLambda {
            tree,
            lambda,
            instance,
        } = pending;
        let span = lambda.span;
        // The body, and the `return` that an expression-bodied lambda needs,
        // must be items of the tree the lowering walks. The tree is shared, so
        // it is cloned and the items are added to the clone.
        let mut tree = tree.clone();
        let body = match &lambda.body {
            fallc_ast::LambdaBody::Expression(value) => {
                let statement = tree.builder().alloc_stmt(
                    fallc_ast::StmtKind::Return(fallc_ast::ReturnStmt {
                        value: Some(*value),
                    }),
                    span,
                );
                let block = tree.builder().alloc_block(vec![statement], span);
                Some(block)
            }
            fallc_ast::LambdaBody::Block(block) => Some(*block),
        };

        // A capture is a parameter whose type is a pointer to the captured
        // variable; the body binds the incoming address as that variable's
        // storage, which is what makes the capture a reference rather than a
        // copy.
        let mut params = Vec::with_capacity(instance.captures.len() + lambda.parameters.len());
        let mut resolved = Vec::with_capacity(params.capacity());
        for capture in &instance.captures {
            params.push(fallc_ast::Param {
                name: capture.name.clone(),
                name_span: fallc_span::Span::SYNTHETIC,
                ty: fallc_ast::TypeSyntax::Auto(fallc_span::Span::SYNTHETIC),
                default: None,
                is_pack: false,
                span,
            });
            resolved.push(fallc_sema::Type::Pointer {
                pointee: Box::new(capture.ty.clone()),
                pointee_is_const: false,
                pointer_is_const: false,
            });
        }
        for (parameter, ty) in lambda.parameters.iter().zip(&instance.parameters) {
            params.push(fallc_ast::Param {
                name: parameter.name.clone(),
                name_span: parameter.name_span,
                ty: fallc_ast::TypeSyntax::Auto(fallc_span::Span::SYNTHETIC),
                default: None,
                is_pack: false,
                span: parameter.span,
            });
            resolved.push(ty.clone());
        }
        let function = fallc_ast::FunctionItem {
            abi: None,
            is_constexpr: false,
            exported: false,
            throws: instance.throws.then_some(fallc_ast::Throws {
                condition: None,
                is_auto: false,
                span,
            }),
            is_external: false,
            variadic: false,
            template_params: Vec::new(),
            name: instance.symbol.clone(),
            overloaded: false,
            name_span: span,
            params,
            return_type: fallc_ast::TypeSyntax::Auto(fallc_span::Span::SYNTHETIC),
            body,
        };

        let saved = self.install_records(&instance.records);
        self.pre_resolved_params = resolved;
        self.reference_params = instance.captures.len();
        self.pre_resolved_return = Some(instance.returns.clone());
        let previous_symbol = self.current_symbol.replace(instance.symbol.clone());
        // The locals the body declares were typed by the check of *this*
        // instantiation, so the table travels with it: a body that binds a
        // local would otherwise have no type to lower the binding with.
        self.lower_function(&tree, &instance.types, &instance.bindings, &function);
        self.pre_resolved_params.clear();
        self.reference_params = 0;
        self.pre_resolved_return = None;
        self.current_symbol = previous_symbol;
        self.restore_records(saved);
        self.lambda_emitted.insert(instance.symbol.clone());
    }
}
