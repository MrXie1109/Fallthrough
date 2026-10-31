//! Statements, conditions and bindings.
//!
//! The body of a function: blocks, drops, `if`/`while`/`for`/`switch`, the
//! bindings a body introduces -- including the static and aligned ones -- and
//! the allocas that back them.

use super::*;

impl<'ctx> Lowerer<'ctx> {
    /// Lowers a block, giving its bindings their own scope.
    ///
    /// Bindings are pushed onto a scope stack on entry and popped on exit, so a
    /// name declared inside a block stops being visible when the block ends.
    /// That is what makes shadowing correct: an inner `x` hides an outer `x`
    /// only for the duration of the inner block, and the outer one is visible
    /// again afterwards.
    pub(super) fn lower_block(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        bindings: &BindingTypes,
        builder: &Builder<'static>,
        block: BlockId,
    ) -> Result<(), CodegenError> {
        self.binding_scopes.push(HashMap::new());
        self.drop_scopes.push(Vec::new());
        self.defer_scopes.push(Vec::new());
        let result = (|| {
            for &stmt in &ast.block(block).statements {
                // Once a block is terminated — by `return`, `break`, or the
                // end of a `switch` arm — nothing after it can execute. LLVM
                // rejects instructions after a terminator, so lowering stops
                // here rather than emitting unreachable code.
                if Self::is_terminated(builder) {
                    break;
                }
                self.lower_stmt(ast, types, bindings, builder, stmt)?;
            }
            // The scope's values are destroyed here, in reverse order of
            // creation, which is the order a stack unwinds in: a value built
            // from another is destroyed first, so the thing it referred to is
            // still alive when it goes.
            //
            // Nothing is destroyed when the block is already terminated: a
            // `return` has taken the values, and LLVM rejects instructions
            // after a terminator.
            if !Self::is_terminated(builder) {
                self.emit_drops(ast, types, bindings, builder)?;
            }
            Ok(())
        })();
        self.drop_scopes.pop();
        self.defer_scopes.pop();
        self.binding_scopes.pop();
        result
    }

    /// Emits the deferred blocks and destructor calls for the scopes inside
    /// `depth`.
    ///
    /// Used where control leaves some scopes but not the function: `break` and
    /// `continue` leave the loop body's scopes, and `return` leaves all of
    /// them, which is why both are expressed through this.
    pub(super) fn emit_drops_to(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        bindings: &BindingTypes,
        builder: &Builder<'static>,
        depth: usize,
    ) -> Result<(), CodegenError> {
        let defers: Vec<Vec<BlockId>> = self.defer_scopes.iter().skip(depth).cloned().collect();
        let scopes: Vec<Vec<DropEntry<'static>>> =
            self.drop_scopes.iter().skip(depth).cloned().collect();
        for (scope, defers) in scopes.iter().rev().zip(defers.iter().rev()) {
            for block in defers.iter().rev() {
                self.lower_deferred(ast, types, bindings, builder, *block)?;
            }
            for entry in scope.iter().rev() {
                self.emit_drop(builder, entry)?;
            }
        }
        Ok(())
    }

    /// Emits the deferred blocks and destructor calls for every live scope,
    /// innermost first.
    ///
    /// Used where control leaves the whole function at once -- a `return` --
    /// rather than one scope at a time.
    ///
    /// # The value being returned
    ///
    /// `return s;` moves `s` out of the function, so `s` must *not* be
    /// destroyed here: its storage is the caller's now. Destroying it anyway
    /// freed the buffer the caller had just been handed, and the caller read
    /// whatever the allocator left there -- which is how `a + b` came out as
    /// two bytes of rubbish with the correct length.
    pub(super) fn emit_all_drops(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        bindings: &BindingTypes,
        builder: &Builder<'static>,
        moved_out: Option<inkwell::values::PointerValue<'static>>,
    ) -> Result<(), CodegenError> {
        let defers: Vec<Vec<BlockId>> = self.defer_scopes.clone();
        let scopes: Vec<Vec<DropEntry<'static>>> = self.drop_scopes.clone();
        for (scope, defers) in scopes.iter().rev().zip(defers.iter().rev()) {
            for block in defers.iter().rev() {
                self.lower_deferred(ast, types, bindings, builder, *block)?;
            }
            for entry in scope.iter().rev() {
                // The moved-out binding is skipped, and so is everything
                // *inside* it: a field of a returned value leaves with it.
                if let Some(storage) = moved_out
                    && entry.storage == storage
                {
                    continue;
                }
                self.emit_drop(builder, entry)?;
            }
        }
        Ok(())
    }

    /// Emits the deferred blocks and destructor calls for the innermost scope.
    pub(super) fn emit_drops(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        bindings: &BindingTypes,
        builder: &Builder<'static>,
    ) -> Result<(), CodegenError> {
        let Some(scope) = self.drop_scopes.last().cloned() else {
            return Ok(());
        };
        let defers = self.defer_scopes.last().cloned().unwrap_or_default();
        for block in defers.iter().rev() {
            self.lower_deferred(ast, types, bindings, builder, *block)?;
        }
        for entry in scope.iter().rev() {
            self.emit_drop(builder, entry)?;
        }
        Ok(())
    }

    /// Runs one deferred block where its scope is ending.
    ///
    /// Lowering a block pushes a scope of its own, so a `defer` block may
    /// declare values and defer again; those run when the block itself ends.
    fn lower_deferred(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        bindings: &BindingTypes,
        builder: &Builder<'static>,
        block: BlockId,
    ) -> Result<(), CodegenError> {
        if Self::is_terminated(builder) {
            return Ok(());
        }
        self.lower_block(ast, types, bindings, builder, block)
    }

    /// Emits one destructor call.
    pub(super) fn emit_drop(
        &mut self,
        builder: &Builder<'static>,
        entry: &DropEntry<'static>,
    ) -> Result<(), CodegenError> {
        // A type's own destructor runs first, then its fields are destroyed.
        // That is the order the type asked for: its `__drop__` may use the
        // fields, so they have to still be alive while it runs.
        if let Some(symbol) = &entry.symbol {
            // `resolved_function` declares an imported destructor if this
            // module has not been given one yet, which is the usual case: the
            // type comes from a module emitted after this one.
            let function =
                self.resolved_function(symbol, "a destructor", fallc_span::Span::SYNTHETIC)?;
            builder
                .build_call(function, &[entry.storage.into()], "drop")
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
            // A type that declares `__drop__` decides what its destructor does,
            // and that includes its fields: `Optional<T>` releases the value it
            // holds, and `String` releases its buffer. Destroying the fields as
            // well freed the same buffer twice, which read as
            // `munmap_chunk(): invalid pointer` -- and the field-destroying pass
            // is only for a type that has no destructor of its own but whose
            // fields do.
            return Ok(());
        }
        self.emit_field_drops(builder, entry.storage, &entry.ty)
    }

    /// Destroys the fields of a value that need it, in reverse declaration
    /// order.
    pub(super) fn emit_field_drops(
        &mut self,
        builder: &Builder<'static>,
        storage: inkwell::values::PointerValue<'static>,
        ty: &Type,
    ) -> Result<(), CodegenError> {
        let Some(nominal) = ty.as_nominal() else {
            return Ok(());
        };
        let fields = nominal.fields.clone();
        let Some(struct_ty) = TypeMapper::new(self.backend.context()).map(ty) else {
            return Ok(());
        };
        for (index, (_, field_ty)) in fields.iter().enumerate().rev() {
            if !self.type_needs_drop(field_ty) {
                continue;
            }
            // The field's address, which is what the destructor takes.
            let address = build_field_gep(
                builder,
                struct_ty,
                storage,
                llvm_member_index(nominal, index),
            )?;
            let symbol = self.drop_symbol_for(field_ty);
            let entry = DropEntry {
                storage: address,
                symbol,
                ty: field_ty.clone(),
            };
            self.emit_drop(builder, &entry)?;
        }
        Ok(())
    }

    /// Returns `true` if the builder's current block already ends in a
    /// terminator.
    pub(super) fn is_terminated(builder: &Builder<'static>) -> bool {
        builder
            .get_insert_block()
            .is_some_and(|block| block.get_terminator().is_some())
    }

    /// Lowers one statement.
    pub(super) fn lower_stmt(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        bindings: &BindingTypes,
        builder: &Builder<'static>,
        stmt: StmtId,
    ) -> Result<(), CodegenError> {
        match &ast.stmt(stmt).kind {
            // An assertion produces no code: the checker has already decided it.
            StmtKind::StaticAssert(_) => Ok(()),
            StmtKind::Expr(node) => {
                // The value is discarded; the effects are not.
                let _ = self.lower_expr(ast, types, builder, node.expr)?;
                Ok(())
            }
            StmtKind::Bind(node) => self.lower_bind(ast, types, bindings, builder, node),
            StmtKind::Return(node) => self.lower_return(ast, types, bindings, builder, node),
            StmtKind::If(node) => self.lower_if(ast, types, bindings, builder, node),
            // A block in statement position: its own scope, so the values it
            // declares are destroyed at the closing brace.
            StmtKind::Block(block) => {
                self.drop_scopes.push(Vec::new());
                self.defer_scopes.push(Vec::new());
                let result = self.lower_block(ast, types, bindings, builder, *block);
                self.emit_drops(ast, types, bindings, builder)?;
                self.drop_scopes.pop();
                self.defer_scopes.pop();
                result
            }
            // A `defer` emits nothing where it is written: the block is
            // recorded in its scope and runs when the scope exits. The parser
            // has already given it its own block, so this is just the record.
            StmtKind::Defer(block) => {
                if let Some(scope) = self.defer_scopes.last_mut() {
                    scope.push(*block);
                }
                Ok(())
            }
            StmtKind::While(node) => self.lower_while(ast, types, bindings, builder, node),
            StmtKind::ForC(node) => self.lower_for_c(ast, types, bindings, builder, node),
            StmtKind::Switch(node) => self.lower_switch(ast, types, bindings, builder, node),
            StmtKind::Break => self.lower_break(ast, types, bindings, builder, stmt),
            StmtKind::Continue => self.lower_continue(ast, types, bindings, builder, stmt),
            StmtKind::Fallthrough => Err(CodegenError::waiting_for_spec(
                "`fallthrough` outside a `switch` arm",
                ast.stmt(stmt).span,
            )),
            StmtKind::Try(node) => self.lower_try(ast, types, bindings, builder, node),
            StmtKind::Raise(node) => self.lower_raise(ast, types, bindings, builder, node),

            StmtKind::Unresolved(_) => Err(CodegenError::waiting_for_spec(
                "this statement",
                ast.stmt(stmt).span,
            )),
        }
    }

    /// Lowers the condition of an `if` or a loop to an `i1`.
    ///
    /// # Truthiness
    ///
    /// This is the **only** place in the compiler where a value may be used at
    /// a type it does not have. Everywhere else a value must already have the
    /// required type, because the language has no implicit conversions.
    ///
    /// The rule is that a numeric value is true when it is non-zero. LLVM's
    /// `br` needs an `i1`, so the conversion is mechanical:
    ///
    /// * `bool` is already an `i1` and passes through;
    /// * an integer becomes `x != 0`;
    /// * a float becomes `x != 0.0`, using an ordered comparison so that NaN is
    ///   true rather than false — NaN is not zero, and the rule is stated in
    ///   terms of non-zeroness, not of equality with zero.
    ///
    /// Lowers a condition by calling the type's `__bool__`.
    ///
    /// The result is an `i1`, which is what a condition needs. A `__bool__`
    /// that returns something other than `bool` is compared against zero, so
    /// the method is free to return an integer.
    pub(super) fn lower_bool_method(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        condition: ExprId,
        symbol: &str,
        span: fallc_span::Span,
    ) -> Result<inkwell::values::IntValue<'static>, CodegenError> {
        let function = self.resolved_function(symbol, "`__bool__`", span)?;
        let value = self
            .lower_expr(ast, types, builder, condition)?
            .ok_or_else(|| CodegenError::waiting_for_spec("this condition", span))?;
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
        let call = builder
            .build_call(function, &[receiver.into()], "bool")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        let returned = match self.unwrap_throwing(ast, types, builder, span, call)? {
            Some(BasicValueEnum::IntValue(value)) => value,
            _ => {
                return Err(CodegenError::waiting_for_spec(
                    "a `__bool__` that does not return a value",
                    span,
                ));
            }
        };
        // Already an `i1` when the method returns `bool`.
        if returned.get_type().get_bit_width() == 1 {
            return Ok(returned);
        }
        builder
            .build_int_compare(
                inkwell::IntPredicate::NE,
                returned,
                returned.get_type().const_zero(),
                "truthy",
            )
            .map_err(|error| CodegenError::Emit(error.to_string()))
    }

    pub(super) fn lower_condition(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        condition: ExprId,
    ) -> Result<inkwell::values::IntValue<'static>, CodegenError> {
        let span = ast.expr(condition).span;
        let value = self
            .lower_expr(ast, types, builder, condition)?
            .ok_or_else(|| CodegenError::waiting_for_spec("this condition", span))?;

        // A type that defines `__bool__` is tested by that call; the checker
        // recorded which, so nothing is looked up here.
        if let Some(symbol) = self.bool_methods.get(&span).cloned() {
            return self.lower_bool_method(ast, types, builder, condition, &symbol, span);
        }

        let ty = types
            .get(condition)
            .cloned()
            .ok_or_else(|| CodegenError::waiting_for_spec("this condition's type", span))?;
        if !ty.is_truth_testable() {
            return Err(CodegenError::waiting_for_spec(
                format!(
                    "a `{}` condition (only numbers and `bool` have truthiness)",
                    ty.spelling()
                ),
                span,
            ));
        }

        let context = self.backend.context();
        match value {
            // Already a boolean.
            BasicValueEnum::IntValue(int) if int.get_type().get_bit_width() == 1 => Ok(int),
            BasicValueEnum::IntValue(int) => {
                let zero = int.get_type().const_zero();
                builder
                    .build_int_compare(inkwell::IntPredicate::NE, int, zero, "truthy")
                    .map_err(|error| CodegenError::Emit(error.to_string()))
            }
            BasicValueEnum::FloatValue(float) => {
                let zero = float.get_type().const_zero();
                // `ONE` is the *ordered* not-equal predicate. It reports NaN as
                // true, which matches "non-zero is true" because NaN is not
                // zero. The unordered form would report NaN as false.
                builder
                    .build_float_compare(inkwell::FloatPredicate::ONE, float, zero, "truthy")
                    .map_err(|error| CodegenError::Emit(error.to_string()))
            }
            // An array is always true: it is viewed as a pointer, and the
            // address of an array is never null. There is nothing to compare,
            // so the answer is a constant.
            BasicValueEnum::ArrayValue(_) => Ok(context.bool_type().const_int(1, false)),
            // A pointer is true when it is not null -- `nullptr` is false, and
            // any other address is true. An array is always true, because it is
            // viewed as a pointer and the address of an array is never null;
            // the checker accepted it, and the value here is that address.
            BasicValueEnum::PointerValue(pointer) => {
                let null = pointer.get_type().const_null();
                builder
                    .build_int_compare(inkwell::IntPredicate::NE, pointer, null, "truthy")
                    .or_else(|_| {
                        // `build_int_compare` wants integers; pointers compare
                        // by address with the same predicate through a cast.
                        let as_int = builder
                            .build_ptr_to_int(pointer, context.i64_type(), "truthy.ptr")
                            .map_err(|error| CodegenError::Emit(error.to_string()))?;
                        builder
                            .build_int_compare(
                                inkwell::IntPredicate::NE,
                                as_int,
                                context.i64_type().const_zero(),
                                "truthy",
                            )
                            .map_err(|error| CodegenError::Emit(error.to_string()))
                    })
            }
            other => {
                let _ = context;
                Err(CodegenError::waiting_for_spec(
                    format!("a condition of LLVM type {}", other.get_type()),
                    span,
                ))
            }
        }
    }

    /// Lowers `if (cond) { .. } else { .. }`.
    ///
    /// Creates three blocks — `then`, `else`, and `join` — and branches between
    /// them. An `else if` chain is just a nested `if` lowered into the else
    /// block, so no special handling is needed.
    pub(super) fn lower_if(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        bindings: &BindingTypes,
        builder: &Builder<'static>,
        node: &IfStmt,
    ) -> Result<(), CodegenError> {
        // A condition the checker folded is decided here, and only the live
        // branch is emitted. The other was never checked, so its expressions
        // have no types -- emitting it would find none, and there is nothing to
        // emit anyway, since the answer is already known.
        if let Some(taken) = self
            .compile_time_ifs
            .get(&ast.expr(node.condition).span)
            .copied()
        {
            return if taken {
                self.lower_block(ast, types, bindings, builder, node.then_block)
            } else {
                match &node.else_branch {
                    Some(fallc_ast::ElseBranch::Block(block)) => {
                        self.lower_block(ast, types, bindings, builder, *block)
                    }
                    Some(fallc_ast::ElseBranch::If(inner)) => {
                        self.lower_stmt(ast, types, bindings, builder, *inner)
                    }
                    None => Ok(()),
                }
            };
        }
        let condition = self.lower_condition(ast, types, builder, node.condition)?;
        let function = self.current_function.expect("a function is being lowered");

        let then_block = self
            .backend
            .context()
            .append_basic_block(function, "if.then");
        let else_block = self
            .backend
            .context()
            .append_basic_block(function, "if.else");
        let join_block = self
            .backend
            .context()
            .append_basic_block(function, "if.end");

        builder
            .build_conditional_branch(condition, then_block, else_block)
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        for target in [then_block, else_block] {
            if !self.branched_to.contains(&target) {
                self.branched_to.push(target);
            }
        }

        // `then`
        builder.position_at_end(then_block);
        self.lower_block(ast, types, bindings, builder, node.then_block)?;
        if !Self::is_terminated(builder) {
            self.branch_to(builder, join_block)?;
        }

        // `else`
        builder.position_at_end(else_block);
        match &node.else_branch {
            Some(ElseBranch::Block(block)) => {
                self.lower_block(ast, types, bindings, builder, *block)?;
            }
            Some(ElseBranch::If(nested)) => {
                self.lower_stmt(ast, types, bindings, builder, *nested)?;
            }
            None => {}
        }
        if !Self::is_terminated(builder) {
            self.branch_to(builder, join_block)?;
        }

        // `join` is unterminated when neither arm branched to it, which happens
        // when both arms terminated. An unterminated block is invalid IR, so it
        // is closed with `unreachable`.
        //
        // Reachability is tracked by asking whether the block has any uses as a
        // branch target, not by whether the arms "looked" terminated: one arm
        // can `return` while the other falls through, and a nested `if` can
        // branch here from deep inside. Both cases make the join reachable.
        builder.position_at_end(join_block);
        if !self.is_reachable(join_block) {
            builder
                .build_unreachable()
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
        }
        Ok(())
    }

    /// Lowers `while (cond) { .. } else { .. }`.
    ///
    /// # The `else` clause
    ///
    /// A loop's `else` runs when the loop ends **normally** — the condition
    /// becoming false — and is skipped by `break`, `return`, or `raise`.
    /// `continue` is not an interruption.
    ///
    /// That maps exactly onto LLVM blocks: the normal exit branches to
    /// `loop.else`, while `break` branches to `loop.end`. So the distinction is
    /// structural rather than something that needs a flag.
    ///
    /// ```text
    ///        br cond
    ///   cond --true--> body --> continue --> cond
    ///     |false
    ///     v
    ///   else --> end        break ---------> end
    /// ```
    pub(super) fn lower_while(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        bindings: &BindingTypes,
        builder: &Builder<'static>,
        node: &LoopStmt,
    ) -> Result<(), CodegenError> {
        let function = self.current_function.expect("a function is being lowered");
        let context = self.backend.context();

        let cond_block = context.append_basic_block(function, "while.cond");
        let body_block = context.append_basic_block(function, "while.body");
        let else_block = context.append_basic_block(function, "while.else");
        let end_block = context.append_basic_block(function, "while.end");

        self.branch_to(builder, cond_block)?;

        // The loop's exit is `end`; `continue` targets the condition.
        self.loop_stack.push(LoopTargets {
            continue_to: cond_block,
            break_to: end_block,
            // The body's own scope, which `break` and `continue` leave.
            scope_depth: self.drop_scopes.len(),
        });

        builder.position_at_end(cond_block);
        let condition = self.lower_condition(ast, types, builder, node.condition)?;
        builder
            .build_conditional_branch(condition, body_block, else_block)
            .map_err(|error| CodegenError::Emit(error.to_string()))?;

        builder.position_at_end(body_block);
        self.lower_block(ast, types, bindings, builder, node.body)?;
        if !Self::is_terminated(builder) {
            self.branch_to(builder, cond_block)?;
        }

        self.loop_stack.pop();

        // The `else` clause is part of the loop for `break`: a `break` inside
        // it would be a `break` with no enclosing loop, which the resolver
        // should have rejected. It is popped before lowering `else` so that is
        // true here too.
        builder.position_at_end(else_block);
        if let Some(block) = node.else_block {
            self.lower_block(ast, types, bindings, builder, block)?;
        }
        if !Self::is_terminated(builder) {
            self.branch_to(builder, end_block)?;
        }

        builder.position_at_end(end_block);
        Ok(())
    }

    /// Lowers a C-style `for (init; cond; update) { .. } else { .. }`.
    ///
    /// `init` runs once, then the condition is tested each iteration and
    /// `update` runs at the end of each body execution. A missing condition
    /// means "always true", so the loop only exits via `break`. A binding
    /// declared in `init` is scoped to the loop: it is declared in a scope that
    /// is pushed before `init` and popped after the loop ends.
    pub(super) fn lower_for_c(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        bindings: &BindingTypes,
        builder: &Builder<'static>,
        node: &ForCStmt,
    ) -> Result<(), CodegenError> {
        let function = self.current_function.expect("a function is being lowered");
        let context = self.backend.context();

        // The `init` clause is scoped to the loop, so its bindings are pushed
        // here and popped after the loop ends.
        self.binding_scopes.push(HashMap::new());

        if let Some(init) = node.init {
            self.lower_stmt(ast, types, bindings, builder, init)?;
        }

        let cond_block = context.append_basic_block(function, "for.cond");
        let body_block = context.append_basic_block(function, "for.body");
        let update_block = context.append_basic_block(function, "for.update");
        let else_block = context.append_basic_block(function, "for.else");
        let end_block = context.append_basic_block(function, "for.end");

        builder
            .build_unconditional_branch(cond_block)
            .map_err(|error| CodegenError::Emit(error.to_string()))?;

        // `continue` runs the update clause before re-testing, which is what
        // makes `continue` behave like reaching the end of the body.
        self.loop_stack.push(LoopTargets {
            continue_to: update_block,
            break_to: end_block,
            scope_depth: self.drop_scopes.len(),
        });

        builder.position_at_end(cond_block);
        match node.condition {
            Some(condition) => {
                let value = self.lower_condition(ast, types, builder, condition)?;
                builder
                    .build_conditional_branch(value, body_block, else_block)
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
            }
            None => {
                builder
                    .build_unconditional_branch(body_block)
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
            }
        }

        builder.position_at_end(body_block);
        self.lower_block(ast, types, bindings, builder, node.body)?;
        if !Self::is_terminated(builder) {
            builder
                .build_unconditional_branch(update_block)
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
        }

        builder.position_at_end(update_block);
        if let Some(update) = node.update {
            let _ = self.lower_expr(ast, types, builder, update)?;
        }
        builder
            .build_unconditional_branch(cond_block)
            .map_err(|error| CodegenError::Emit(error.to_string()))?;

        self.loop_stack.pop();

        builder.position_at_end(else_block);
        if let Some(block) = node.else_block {
            self.lower_block(ast, types, bindings, builder, block)?;
        }
        if !Self::is_terminated(builder) {
            self.branch_to(builder, end_block)?;
        }

        builder.position_at_end(end_block);
        self.binding_scopes.pop();
        Ok(())
    }

    /// Lowers `switch (scrutinee) { case ..: .. }`.
    ///
    /// # Fall-through and automatic break
    ///
    /// Cases do **not** fall through by default: each arm ends by branching to
    /// the join block. An arm ending in `fallthrough;` instead branches to the
    /// next arm's block -- `default`'s block when that is the next arm -- and to
    /// the join block after the last arm.
    ///
    /// # Labels
    ///
    /// Labels are constants, so each arm becomes a `switch` case value. A
    /// non-constant label is rejected by the checker (S0161), because a value
    /// computed at run time would have nothing to compare against. `default`
    /// may appear in any position.
    ///
    /// A duplicate label is not detected: both cases are emitted as written,
    /// and LLVM's verifier is what refuses the module, as an internal error
    /// rather than a source diagnostic.
    pub(super) fn lower_switch(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        bindings: &BindingTypes,
        builder: &Builder<'static>,
        node: &SwitchStmt,
    ) -> Result<(), CodegenError> {
        let function = self.current_function.expect("a function is being lowered");
        let context = self.backend.context();

        let scrutinee = self
            .lower_expr(ast, types, builder, node.scrutinee)?
            .ok_or_else(|| {
                CodegenError::waiting_for_spec("this switch value", ast.expr(node.scrutinee).span)
            })?;
        let BasicValueEnum::IntValue(scrutinee) = scrutinee else {
            return Err(CodegenError::waiting_for_spec(
                "a non-integer switch value",
                ast.expr(node.scrutinee).span,
            ));
        };

        // One block per arm, plus a join block.
        let arm_blocks: Vec<_> = node
            .arms
            .iter()
            .enumerate()
            .map(|(index, _)| context.append_basic_block(function, &format!("case.{index}")))
            .collect();
        let end_block = context.append_basic_block(function, "switch.end");

        // Collect case values, and find the default arm's block.
        let mut cases = Vec::new();
        let mut default_block = None;
        for (index, arm) in node.arms.iter().enumerate() {
            match &arm.labels {
                Some(labels) => {
                    for &label in labels {
                        let value =
                            self.lower_expr(ast, types, builder, label)?
                                .ok_or_else(|| {
                                    CodegenError::waiting_for_spec(
                                        "this case label",
                                        ast.expr(label).span,
                                    )
                                })?;
                        let BasicValueEnum::IntValue(value) = value else {
                            return Err(CodegenError::waiting_for_spec(
                                "a non-integer case label",
                                ast.expr(label).span,
                            ));
                        };
                        cases.push((value, arm_blocks[index]));
                    }
                }
                None => default_block = Some(arm_blocks[index]),
            }
        }

        // No `default` means the switch does nothing when nothing matches, so
        // the fallback is the join block.
        let fallback = default_block.unwrap_or(end_block);
        builder
            .build_switch(scrutinee, fallback, &cases)
            .map_err(|error| CodegenError::Emit(error.to_string()))?;

        // A `break` inside an arm leaves the switch, so an arm behaves like a
        // one-iteration loop body for that purpose. `continue` is not valid
        // inside a switch unless a loop encloses it, so the inherited target is
        // kept for `continue` and only `break` is rebound.
        let inherited_continue = self.loop_stack.last().map(|targets| targets.continue_to);
        self.loop_stack.push(LoopTargets {
            // `continue` inside a switch arm belongs to the enclosing loop, if
            // there is one. Falling back to the join block makes a stray
            // `continue` terminate the arm rather than produce invalid IR; the
            // resolver rejects it separately.
            continue_to: inherited_continue.unwrap_or(end_block),
            break_to: end_block,
            scope_depth: self.drop_scopes.len(),
        });

        for (index, arm) in node.arms.iter().enumerate() {
            builder.position_at_end(arm_blocks[index]);

            // An arm's body is a statement list, so it gets its own scope.
            self.binding_scopes.push(HashMap::new());
            for &stmt in &arm.body {
                if Self::is_terminated(builder) {
                    break;
                }
                // `fallthrough` is handled here rather than in `lower_stmt`,
                // because it needs to know the next arm.
                if matches!(ast.stmt(stmt).kind, StmtKind::Fallthrough) {
                    let next = arm_blocks.get(index + 1).copied().unwrap_or(end_block);
                    builder
                        .build_unconditional_branch(next)
                        .map_err(|error| CodegenError::Emit(error.to_string()))?;
                    break;
                }
                self.lower_stmt(ast, types, bindings, builder, stmt)?;
            }
            self.binding_scopes.pop();

            if !Self::is_terminated(builder) {
                builder
                    .build_unconditional_branch(end_block)
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
            }
        }

        self.loop_stack.pop();
        builder.position_at_end(end_block);
        Ok(())
    }

    /// Lowers `break`.
    pub(super) fn lower_break(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        bindings: &BindingTypes,
        builder: &Builder<'static>,
        stmt: StmtId,
    ) -> Result<(), CodegenError> {
        match self.loop_stack.last() {
            Some(targets) => {
                let target = targets.break_to;
                let depth = targets.scope_depth;
                self.emit_drops_to(ast, types, bindings, builder, depth)?;
                builder
                    .build_unconditional_branch(target)
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
                Ok(())
            }
            None => Err(CodegenError::waiting_for_spec(
                "`break` outside a loop or switch",
                ast.stmt(stmt).span,
            )),
        }
    }

    /// Records a binding and, when its type needs one, its destructor.
    ///
    /// # Why one helper
    ///
    /// Every binding must be registered the same way, and one that is registered
    /// without its destructor is a leak: the value is never destroyed, and
    /// nothing says so. There are three places a binding is created -- a
    /// parameter, a plain binding, and one initialised by `__init__` -- and they
    /// used to register themselves separately.
    ///
    /// The destructor is looked up from the type's own `__drop__`, so a type
    /// that defines none costs nothing.
    pub(super) fn register_binding(
        &mut self,
        name: &str,
        ty: &Type,
        storage: inkwell::values::PointerValue<'static>,
        llvm_ty: inkwell::types::BasicTypeEnum<'static>,
    ) {
        self.current_scope_mut().insert(
            name.to_owned(),
            LocalBinding {
                storage,
                ty: llvm_ty,
            },
        );
        // A value needs destroying when it has a destructor of its own *or*
        // when any field of it does: `Outer` above holds two `Inner`s, and
        // without this the two were leaked, because `Outer` itself defines no
        // `__drop__` and nothing looked inside it.
        // The symbol is resolved before the scope is borrowed mutably, since
        // both borrow `self`.
        let symbol = self.drop_symbol_for(ty);
        if symbol.is_some() || self.type_needs_drop(ty) {
            if let Some(scope) = self.drop_scopes.last_mut() {
                scope.push(DropEntry {
                    storage,
                    symbol,
                    ty: ty.clone(),
                });
            }
        }
    }

    /// Returns `true` if a value of this type must be destroyed.
    ///
    /// True when the type defines `__drop__`, or when any field of it would
    /// itself need destroying. A nominal type's fields are known, so this
    /// recurses into them; an array or pointer has no destructor of its own.
    pub(super) fn type_needs_drop(&self, ty: &Type) -> bool {
        if self.drop_symbol_for(ty).is_some() {
            return true;
        }
        let Some(nominal) = ty.as_nominal() else {
            return false;
        };
        nominal
            .fields
            .iter()
            .any(|(_, field)| self.type_needs_drop(field))
    }

    /// Returns the symbol of a type's `__drop__`, if it defines one.
    ///
    /// The receiver is a pointer, so every pointer form is tried; a type that
    /// declares only `*const T` and one that declares `*T` are both found.
    pub(super) fn drop_symbol_for(&self, ty: &Type) -> Option<String> {
        // A reference or a pointer does not own what it names, so a binding of
        // one is never destroyed here: `r &Array<int> = *p;` drops nothing, and
        // the array is dropped where it was declared. Destroying through the
        // reference passed the *slot holding the address* to `__drop__`, which
        // read the pointer's bytes as the container and freed garbage.
        if ty.is_indirection() {
            return None;
        }
        let written = fallc_ast::special::SpecialMethod::Drop.name();
        for receiver in drop_receiver_forms(ty) {
            let params = [fallc_sema::mangle_type(&receiver)];
            let symbol = fallc_ast::special::symbol_for(written, &params);
            // A destructor of an *imported* type is not in this module yet:
            // the importing unit emits its own functions first, so the
            // defining module's arrive later. What is known now is its symbol,
            // and that is enough to record the drop -- the declaration is made
            // when the call is written. Looking only at this module left every
            // `std.String` leaking, silently and with no diagnostic.
            let defined_here = self.backend.module().get_function(&symbol).is_some();
            let imported = self
                .external_symbols
                .values()
                .any(|signature| signature.symbol == symbol);
            if defined_here || imported {
                return Some(symbol);
            }
        }
        None
    }

    /// Lowers `continue`.
    pub(super) fn lower_continue(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        bindings: &BindingTypes,
        builder: &Builder<'static>,
        stmt: StmtId,
    ) -> Result<(), CodegenError> {
        match self.loop_stack.last() {
            Some(targets) => {
                let target = targets.continue_to;
                let depth = targets.scope_depth;
                self.emit_drops_to(ast, types, bindings, builder, depth)?;
                builder
                    .build_unconditional_branch(target)
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
                Ok(())
            }
            None => Err(CodegenError::waiting_for_spec(
                "`continue` outside a loop",
                ast.stmt(stmt).span,
            )),
        }
    }

    /// Lowers a variable binding to an `alloca` plus a store.
    ///
    /// # Zero initialisation
    ///
    /// `name type;` with no initialiser is zero-initialised, so it lowers to a
    /// store of LLVM's zero for the type rather than to an uninitialised
    /// `alloca`.
    ///
    /// # Naming
    ///
    /// The alloca is named after the source variable, with a numeric suffix on
    /// collision. LLVM would rename duplicates itself, but doing it here keeps
    /// the emitted IR readable and stable.
    pub(super) fn lower_bind(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        bindings: &BindingTypes,
        builder: &Builder<'static>,
        node: &BindStmt,
    ) -> Result<(), CodegenError> {
        let ty = bindings.get(node.name_span).cloned().ok_or_else(|| {
            // The type checker records every binding's type, so a miss here is
            // a compiler bug rather than a user error.
            CodegenError::Emit(format!(
                "internal error: no type recorded for binding `{}`",
                node.name
            ))
        })?;

        let mapper = TypeMapper::new(self.backend.context());
        let llvm_ty = mapper.map(&ty).ok_or_else(|| {
            CodegenError::waiting_for_spec("a binding of this type", node.name_span)
        })?;

        // A coroutine binding whose initialiser suspends takes the order the
        // suspension requires: the value is produced first, and the slot's
        // address is taken afterwards, where the store happens. Taking it first
        // put the `getelementptr` before a `yield` and left it dominating a
        // store it does not reach.
        if self.coroutine.is_some()
            && let Some(fallc_ast::Initializer::Expr(expr)) = &node.init
            && super::coroutines::contains_yield(ast, *expr)
        {
            let field = self.coroutine_slot(node.name_span).map(|(field, _)| field);
            let value = self
                .lower_expr(ast, types, builder, *expr)?
                .ok_or_else(|| {
                    CodegenError::waiting_for_spec("this initialiser", node.name_span)
                })?;
            let storage = match field {
                Some(field) => {
                    self.note_coroutine_slot_name(&node.name, field);
                    self.coroutine_slot_pointer(builder, field, &node.name)?
                }
                None => {
                    let entry = self
                        .current_function
                        .and_then(|function| function.get_first_basic_block())
                        .ok_or_else(|| {
                            CodegenError::Emit(
                                "internal error: no function is being lowered".to_owned(),
                            )
                        })?;
                    self.create_entry_alloca(builder, entry, &node.name, llvm_ty)
                }
            };
            // A value that owns something is constructed into its slot: the
            // expression's own value is still owned by whatever produced it.
            if (self.type_needs_drop(&ty) || self.drop_symbol_for(&ty).is_some())
                && let Some(constructor) = self.parameter_copy_constructor(&ty)
            {
                let source = builder
                    .build_alloca(llvm_ty, "init.source")
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
                builder
                    .build_store(source, value)
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
                builder
                    .build_call(constructor, &[storage.into(), source.into()], "init.copy")
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
            } else {
                builder
                    .build_store(storage, value)
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
            }
            self.register_binding(&node.name, &ty, storage, llvm_ty);
            return Ok(());
        }

        // A `static` local is one object for the whole run, not one per call.
        // Its initialiser is therefore part of the program image, which is why
        // the checker requires it to be constant. The object is an internal
        // global and the binding's storage is its address.
        if node.is_static {
            let constant = match &node.init {
                None => llvm_ty.const_zero(),
                Some(init) => match self.constant_initializer(ast, &ty, llvm_ty, init) {
                    Some(value) => value,
                    None => {
                        let span = match init {
                            fallc_ast::Initializer::Expr(expr) => ast.expr(*expr).span,
                            fallc_ast::Initializer::Designated(designated) => designated.span,
                        };
                        self.report_waiting_for_spec(span, "a `static` initialiser of this form");
                        llvm_ty.const_zero()
                    }
                },
            };
            let symbol = format!("__ft_static.{}.{}", self.static_count, node.name);
            self.static_count += 1;
            let global = self.backend.module().add_global(llvm_ty, None, &symbol);
            global.set_initializer(&constant);
            global.set_constant(false);
            global.set_thread_local(node.is_thread_local);
            global.set_linkage(inkwell::module::Linkage::Internal);
            self.apply_global_alignment(&node.name_span, global);
            // Registered without a drop entry on purpose: one object for the
            // whole run cannot be destroyed at the end of a call. Whether a
            // static owner is destroyed at program exit is a language question
            // this stage has no answer for, so nothing is emitted.
            self.current_scope_mut().insert(
                node.name.clone(),
                LocalBinding {
                    storage: global.as_pointer_value(),
                    ty: llvm_ty,
                },
            );
            return Ok(());
        }

        // An alloca in the entry block, so that the storage lives for the whole
        // function regardless of where the binding is declared.
        let entry = self
            .current_function
            .and_then(|function| function.get_first_basic_block())
            .ok_or_else(|| {
                CodegenError::Emit("internal error: no function is being lowered".to_owned())
            })?;

        // A binding inside a `@coroutine` body lives in the coroutine's frame,
        // not in an `alloca`: it has to survive a suspension, and an `alloca`
        // does not outlive the activation of the body that suspended.
        let alloca = match self.coroutine_slot(node.name_span) {
            Some((field, _)) => {
                self.note_coroutine_slot_name(&node.name, field);
                self.coroutine_slot_pointer(builder, field, &node.name)?
            }
            None => self.create_entry_alloca(builder, entry, &node.name, llvm_ty),
        };
        self.apply_alloca_alignment(&node.name_span, alloca);
        // `@uninitialized` says the storage is deliberately untouched: no
        // empty constructor, no zero fill. Reading it before a write is the
        // program's own business.
        if node.is_uninitialized {
            self.register_binding(&node.name, &ty, alloca, llvm_ty);
            return Ok(());
        }

        // `T x;` calls the type's empty constructor when it has one. Leaving
        // the storage zeroed is only right for a type whose empty value *is*
        // zero bytes; a table would have no buckets, and a device would think a
        // character was already pushed.
        if node.init.is_none()
            && let Some(symbol) = self.init_calls.get(&node.name_span).cloned()
        {
            let qualified = self
                .external_symbols
                .iter()
                .find(|(_, signature)| signature.symbol == symbol)
                .map(|(qualified, _)| qualified.clone());
            let callee = match self.backend.module().get_function(&symbol) {
                Some(function) => function,
                None => match qualified {
                    Some(qualified) => {
                        self.declare_external(&symbol, &qualified, node.name_span)?
                    }
                    None => {
                        return Err(CodegenError::Emit(format!(
                            "internal error: `{symbol}`, the constructor for `{}`, was not emitted",
                            ty.spelling()
                        )));
                    }
                },
            };
            let call_site = builder
                .build_call(callee, &[alloca.into()], "")
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
            let _ = self.unwrap_throwing(ast, types, builder, node.name_span, call_site)?;
            self.register_binding(&node.name, &ty, alloca, llvm_ty);
            return Ok(());
        }

        // A large aggregate is zeroed with `memset`, not with a store of a
        // zeroinitializer aggregate. The store is a value of exactly the shape
        // LLVM's instruction selection cannot handle, so `m [256][256]double;`
        // crashed the same way the returned value did.
        if node.init.is_none() && self.aggregate_is_by_address(&ty) {
            let size = llvm_ty.size_of().ok_or_else(|| {
                CodegenError::waiting_for_spec("an aggregate of unknown size", node.name_span)
            })?;
            let zero = self.backend.context().i8_type().const_zero();
            builder
                .build_memset(alloca, 1, zero, size)
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
            self.register_binding(&node.name, &ty, alloca, llvm_ty);
            return Ok(());
        }

        let value = match &node.init {
            None => zero_value(llvm_ty).ok_or_else(|| {
                CodegenError::waiting_for_spec("zero-initialising this type", node.name_span)
            })?,
            // A constructor the checker chose takes the initialiser as its
            // argument: `String s = "hi";` is `__init__(&s, "hi")`, so the
            // binding's storage is passed rather than a value being stored.
            Some(Initializer::Expr(expr))
                if self.init_calls.contains_key(&initialiser_span(ast, *expr)) =>
            {
                let symbol = self.init_calls[&initialiser_span(ast, *expr)].clone();
                // `T x = arg??;` puts the propagation marker on the initialiser,
                // because a construction has no call to write it on. The
                // argument is what the constructor takes, so the marker is not
                // lowered -- only the value under it.
                let expr = match &ast.expr(*expr).kind {
                    ExprKind::Propagate(inner) => &inner.value,
                    _ => expr,
                };
                // The constructor may be defined in another module, in which
                // case it is declared here on first use: the module compiled
                // its own definition, and this is the call to it.
                let function = match self.backend.module().get_function(&symbol) {
                    Some(function) => function,
                    None => {
                        let qualified = self
                            .external_symbols
                            .iter()
                            .find(|(_, signature)| signature.symbol == symbol)
                            .map(|(qualified, _)| qualified.clone());
                        match qualified {
                            Some(qualified) => {
                                self.declare_external(&symbol, &qualified, node.name_span)?
                            }
                            None => {
                                return Err(CodegenError::Emit(format!(
                                    "internal error: `{symbol}` was resolved as a constructor \
                                     but not declared"
                                )));
                            }
                        }
                    }
                };
                let argument = self.lower_argument(ast, types, builder, *expr, node.name_span)?;
                let call = builder
                    .build_call(function, &[alloca.into(), argument], "")
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
                // A constructor that may raise returns the exception instead of
                // finishing, so the call is unwrapped before the binding is
                // registered: a failed construction leaves nothing to name.
                let _ = self.unwrap_throwing(ast, types, builder, ast.expr(*expr).span, call)?;
                self.register_binding(&node.name, &ty, alloca, llvm_ty);
                return Ok(());
            }
            // An aggregate binding copies its initialiser's bytes. The source is
            // addressed rather than loaded: a call has already written into a
            // temporary, and a place -- `a [N][N]double = b;` -- has storage of
            // its own. There is no aggregate value to store.
            Some(Initializer::Expr(expr)) if self.aggregate_is_by_address(&ty) => {
                let source =
                    self.lower_aggregate_source(ast, types, builder, *expr, ast.expr(*expr).span)?;
                let size = llvm_ty.size_of().ok_or_else(|| {
                    CodegenError::waiting_for_spec("an aggregate of unknown size", node.name_span)
                })?;
                builder
                    .build_memcpy(alloca, 1, source, 1, size)
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
                self.register_binding(&node.name, &ty, alloca, llvm_ty);
                return Ok(());
            }
            Some(Initializer::Expr(expr)) => {
                let value = self
                    .lower_expr(ast, types, builder, *expr)?
                    .ok_or_else(|| {
                        CodegenError::waiting_for_spec("this initialiser", node.name_span)
                    })?;
                // A lossless integer promotion is recorded by the checker and
                // has to be performed here; the source wrote no `as`.
                let value =
                    self.convert_adopted(ast, types, builder, *expr, value, node.name_span)?;
                self.convert_implicit(ast, types, builder, *expr, value, node.name_span)?
            }
            // `name type = .{ .. };`
            //
            // The elements are evaluated in order and combined into a struct
            // value, which is then stored. The struct's type is the binding's,
            // because the elements' types come from its fields.
            Some(Initializer::Designated(designated)) => {
                // A type that defines `__init__` is initialised by calling it,
                // with the binding's address as the receiver. The fields are
                // not written directly: the constructor is the only path, so
                // that it can do more than assign.
                //
                // A type without one has its fields filled directly.
                // The values in declaration order, whether they were written
                // by position or by name: a constructor takes them positionally,
                // and the fields are inserted in declaration order.
                let ordered: Vec<fallc_ast::ExprId> = match ty.as_nominal() {
                    Some(nominal) => Self::init_field_values(designated, &nominal.fields)
                        .into_iter()
                        .flatten()
                        .collect(),
                    None => designated.elements.clone(),
                };
                if self.type_has_init(&ty) {
                    let receiver = alloca.as_basic_value_enum();
                    let arguments: Vec<Type> = ordered
                        .iter()
                        .filter_map(|element| types.get(*element).cloned())
                        .collect();
                    let symbol = self.init_symbol_for(&ty, &arguments).ok_or_else(|| {
                        CodegenError::Emit(format!(
                            "internal error: `__init__` for `{}` was not emitted",
                            ty.spelling()
                        ))
                    })?;
                    let callee = self
                        .backend
                        .module()
                        .get_function(&symbol)
                        .expect("the symbol was just found");
                    let mut args: Vec<inkwell::values::BasicMetadataValueEnum<'static>> =
                        Vec::with_capacity(ordered.len() + 1);
                    args.push(receiver.into());
                    for &element in &ordered {
                        let value =
                            self.lower_expr(ast, types, builder, element)?
                                .ok_or_else(|| {
                                    CodegenError::waiting_for_spec(
                                        "this initialiser element",
                                        designated.span,
                                    )
                                })?;
                        let value = self.convert_adopted(
                            ast,
                            types,
                            builder,
                            element,
                            value,
                            ast.expr(element).span,
                        )?;
                        let value = self.convert_implicit(
                            ast,
                            types,
                            builder,
                            element,
                            value,
                            ast.expr(element).span,
                        )?;
                        args.push(value.into());
                    }
                    let call_site = builder
                        .build_call(callee, &args, "")
                        .map_err(|error| CodegenError::Emit(error.to_string()))?;
                    let _ =
                        self.unwrap_throwing(ast, types, builder, designated.span, call_site)?;
                    // The constructor wrote the fields, so the binding is
                    // initialised and nothing more is stored.
                    self.register_binding(&node.name, &ty, alloca, llvm_ty);
                    return Ok(());
                }

                // An array is initialised by position: each value is stored
                // into the slot at its index, in the order written.
                if let fallc_sema::Type::Array { element, len } = ty.unqualified() {
                    let mapper = TypeMapper::new(self.backend.context());
                    let element_ty = mapper.map(element).ok_or_else(|| {
                        CodegenError::waiting_for_spec("this array's element", designated.span)
                    })?;
                    for (index, &value) in designated.elements.iter().enumerate() {
                        if index as u64 >= *len {
                            break;
                        }
                        let Some(lowered) = self.lower_expr(ast, types, builder, value)? else {
                            continue;
                        };
                        let subscript = self
                            .backend
                            .context()
                            .i32_type()
                            .const_int(index as u64, false);
                        let slot = build_index_gep(builder, element_ty, alloca, subscript)?;
                        builder
                            .build_store(slot, lowered)
                            .map_err(|error| CodegenError::Emit(error.to_string()))?;
                    }
                    self.register_binding(&node.name, &ty, alloca, llvm_ty);
                    return Ok(());
                }
                let Some(nominal) = ty.as_nominal() else {
                    return Err(CodegenError::waiting_for_spec(
                        "a designated initialiser for this type",
                        designated.span,
                    ));
                };
                let mapper = TypeMapper::new(self.backend.context());
                let struct_ty = mapper.map(&ty).ok_or_else(|| {
                    CodegenError::waiting_for_spec("this struct type", designated.span)
                })?;
                let inkwell::types::BasicTypeEnum::StructType(struct_ty) = struct_ty else {
                    return Err(CodegenError::Emit(
                        "internal error: a nominal type is not an LLVM struct".to_owned(),
                    ));
                };

                let mut values = Vec::with_capacity(ordered.len());
                for &element in &ordered {
                    let value =
                        self.lower_expr(ast, types, builder, element)?
                            .ok_or_else(|| {
                                CodegenError::waiting_for_spec(
                                    "this initialiser element",
                                    designated.span,
                                )
                            })?;
                    let value = self.convert_adopted(
                        ast,
                        types,
                        builder,
                        element,
                        value,
                        ast.expr(element).span,
                    )?;
                    let value = self.convert_implicit(
                        ast,
                        types,
                        builder,
                        element,
                        value,
                        ast.expr(element).span,
                    )?;
                    values.push(value);
                }
                // Build the struct by inserting each element in turn, starting
                // from an undef value. A single `const_struct` would need all
                // elements to be constants, which they need not be.
                // `build_insert_value` wants the concrete `StructValue`, not a
                // `BasicValueEnum`, so the aggregate stays in that type until it
                // is complete.
                //
                // The elements are given in declaration order, and the member
                // index is translated: the representation carries an alignment
                // marker before every field.
                let mut aggregate = struct_ty.get_undef();
                for (index, value) in values.into_iter().enumerate() {
                    aggregate = builder
                        .build_insert_value(
                            aggregate,
                            value,
                            llvm_member_index(nominal, index),
                            "field",
                        )
                        .map_err(|error| CodegenError::Emit(error.to_string()))?
                        .into_struct_value();
                }
                aggregate.as_basic_value_enum()
            }
        };

        // An out-of-line aggregate is copied from its address: the value of
        // such an expression is the slot it lives in, not a value to store.
        // Storing the pointer instead wrote eight bytes into a half-megabyte
        // slot and corrupted the stack.
        if let BasicValueEnum::PointerValue(source) = value
            && self.aggregate_is_by_address(&ty)
        {
            let size = llvm_ty.size_of().ok_or_else(|| {
                CodegenError::waiting_for_spec("an aggregate of unknown size", node.name_span)
            })?;
            builder
                .build_memcpy(alloca, 1, source, 1, size)
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
        } else {
            builder
                .build_store(alloca, value)
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
        }

        // Record the slot and its type in the innermost scope, so the name
        // disappears when its block ends.
        self.register_binding(&node.name, &ty, alloca, llvm_ty);
        Ok(())
    }

    /// Emits an `alloca` at the top of `entry`, positioned before the first
    /// existing instruction so that allocas stay grouped.
    pub(super) fn create_entry_alloca(
        &self,
        builder: &Builder<'static>,
        entry: inkwell::basic_block::BasicBlock<'static>,
        name: &str,
        ty: inkwell::types::BasicTypeEnum<'static>,
    ) -> inkwell::values::PointerValue<'static> {
        // `Builder::build_alloca` appends at the builder's position, so the
        // builder is temporarily moved to the entry block and positioned before
        // its first instruction, then returned to where the caller had it.
        let previous = builder.get_insert_block();
        builder.position_at_end(entry);
        if let Some(first) = entry.get_first_instruction() {
            builder.position_before(&first);
        }
        let alloca = builder
            .build_alloca(ty, &self.unique_name(name))
            .expect("allocating storage cannot fail; the type is sized and the position is set");
        if let Some(previous) = previous {
            builder.position_at_end(previous);
        }
        alloca
    }

    /// Applies a binding's `alignas` request to an alloca.
    ///
    /// An alloca's alignment is an instruction attribute. The checker validated
    /// the value as a power of two no weaker than the type's own, which is why a
    /// failure here is a compiler bug rather than a user error.
    pub(super) fn apply_alloca_alignment(
        &mut self,
        name_span: &Span,
        alloca: inkwell::values::PointerValue<'static>,
    ) {
        let Some(requested) = self.binding_alignments.get(name_span).copied() else {
            return;
        };
        let Ok(requested) = u32::try_from(requested) else {
            self.report_error("internal error: an alignment larger than u32");
            return;
        };
        let Some(instruction) = alloca.as_instruction() else {
            self.report_error("internal error: an aligned binding is not an alloca");
            return;
        };
        if let Err(error) = instruction.set_alignment(requested) {
            self.report_error(&error.to_string());
        }
    }

    /// Applies a binding's `alignas` request to a module-level global.
    pub(super) fn apply_global_alignment(
        &mut self,
        name_span: &Span,
        global: inkwell::values::GlobalValue<'static>,
    ) {
        let Some(requested) = self.binding_alignments.get(name_span).copied() else {
            return;
        };
        let Ok(requested) = u32::try_from(requested) else {
            self.report_error("internal error: an alignment larger than u32");
            return;
        };
        global.set_alignment(requested);
    }

    /// Branches to `target` unconditionally, recording that `target` is
    /// reachable.
    pub(super) fn branch_to(
        &mut self,
        builder: &Builder<'static>,
        target: inkwell::basic_block::BasicBlock<'static>,
    ) -> Result<(), CodegenError> {
        builder
            .build_unconditional_branch(target)
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        if !self.branched_to.contains(&target) {
            self.branched_to.push(target);
        }
        Ok(())
    }

    /// Returns `true` if anything branches to `target`.
    pub(super) fn is_reachable(&self, target: inkwell::basic_block::BasicBlock<'static>) -> bool {
        self.branched_to.contains(&target)
    }

    /// Returns the innermost scope, creating one if the function body has not
    /// pushed any yet.
    pub(super) fn current_scope_mut(&mut self) -> &mut HashMap<String, LocalBinding<'static>> {
        if self.binding_scopes.is_empty() {
            self.binding_scopes.push(HashMap::new());
        }
        self.binding_scopes
            .last_mut()
            .expect("a scope was just ensured")
    }

    /// Looks a name up from the innermost scope outward.
    pub(super) fn lookup_binding(&self, name: &str) -> Option<LocalBinding<'static>> {
        self.binding_scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
    }

    /// Returns `name`, or `name.N` if that name is already used.
    pub(super) fn unique_name(&self, name: &str) -> String {
        let mut candidate = name.to_owned();
        let mut counter = 1u32;
        while self.used_binding_names.contains(&candidate) {
            candidate = format!("{name}.{counter}");
            counter += 1;
        }
        candidate
    }
}
