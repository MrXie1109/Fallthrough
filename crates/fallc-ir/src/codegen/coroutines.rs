//! Coroutines: a `@coroutine` function lowered as a state machine.
//!
//! # The shape
//!
//! A coroutine is written like a function and run like two. `echo()` creates
//! one, `c.resume(v)` runs it as far as its next `yield`, and `c.result()`
//! answers what it returned. The coroutine has no stack of its own, so the
//! whole of it is one LLVM function whose starting point is a number held in a
//! frame -- a *state machine*.
//!
//! ```text
//!   fn echo(n int) Coroutine<int,int,int>      the entry, state 0
//!         |  frame = malloc(size); store n into the frame
//!         v
//!   fn echo__body(frame *Frame)                the machine
//!         |  switch frame.state { 1 -> after the first yield; ... }
//!         v
//!      yield: store the value, state = k, return to the resumer
//! ```
//!
//! # The frame
//!
//! One record, one heap allocation: the `Send` a resume hands over, the
//! `Yield`/`Return` payload, a tag saying which of the two the payload holds,
//! the resume state, and a slot for every binding the body declares.
//!
//! A binding lives in a frame slot rather than in an `alloca`, because it has
//! to outlive one activation of the body: `i` in `echo` is live across every
//! `yield`, and a stack slot would be gone the moment the body returned to its
//! resumer.
//!
//! # Where the frame lives
//!
//! On the heap, and freed by the coroutine object's destructor. A frame in the
//! caller's stack would be invalidated by a `return` the caller need not have
//! seen, and nothing in the type system would say so. The cost is one
//! allocation per coroutine, which is the price of a coroutine whose lifetime
//! the program decides rather than the stack's.
//!
//! # Which types travel
//!
//! `Send`, `Yield`, `Return` and every binding must have a layout with no
//! destructor of its own. The frame is reached only through fields of that
//! layout, and a type that owns something would need its construction and its
//! destruction to run at a moment the frame's lifetime makes clear. The checker
//! refuses such a coroutine rather than letting the code generator guess.
//!
//! # The three types
//!
//! `std.Coroutine<S, Y, R>` is the answer the function gives: `S` is what a
//! `yield` produces, `Y` is what it hands over, and `R` is what the body
//! returns. The tag in the frame says which of `Y` and `R` the payload holds:
//! [`PAYLOAD_YIELDED`] means the coroutine is suspended with a `Y` waiting,
//! [`PAYLOAD_RETURNED`] means it finished with its `R`.

use super::*;

/// What one coroutine entry built, recorded so its members can be generated.
///
/// The entry already has the machine and the frame's layout in hand; `resume`
/// and `result` only ever touch the frame's fixed head, so one implementation
/// serves every coroutine of the same type. This is what carries the head's
/// field indices and the machine's address from the entry to the generator.
#[derive(Clone, Debug)]
pub(super) struct CoroutineFrame {
    /// The index of the field holding the resume state.
    pub state_field: u32,
    /// The frame's LLVM type, kept as the layout to GEP through.
    pub frame_ty: inkwell::types::StructType<'static>,
    /// The index of the field holding the payload tag.
    pub tag_field: u32,
    /// The index of the field holding the payload.
    pub payload_field: u32,
    /// The index of the field holding the `Send`.
    pub send_field: u32,
    /// The index of the field holding the exception the body raised, if any.
    pub exception_field: u32,
    /// The payload slot's width.
    pub payload_llvm: inkwell::types::BasicTypeEnum<'static>,
    /// The LLVM type of the coroutine object.
    pub coroutine_llvm: Option<inkwell::types::BasicTypeEnum<'static>>,
    /// The LLVM type of a `Yield`, for narrowing the payload back to it.
    pub yield_llvm: Option<inkwell::types::BasicTypeEnum<'static>>,
    /// Whether the coroutine takes a value.
    pub takes_value: bool,
    /// What a `resume` hands in.
    pub send_ty: fallc_sema::Type,
    /// What the body returns.
    pub return_ty: fallc_sema::Type,
    /// The symbol `resume` is emitted under, when the program calls it.
    pub resume_symbol: Option<String>,
    /// The type `resume` answers, taken from the instantiation the checker
    /// created for the call: `?Yield`.
    pub resume_returns: Option<fallc_sema::Type>,
    /// The symbol `result` is emitted under, when the program calls it.
    pub result_symbol: Option<String>,
}

/// How many suspension points one coroutine body may have.
///
/// `yield` is a state, and the machine's dispatch has one block per state. The
/// bound is a limit on a program rather than a property of the design; it exists
/// so that the continuation blocks can be created before the body is lowered,
/// since a `yield` has to know the block it resumes at.
pub(super) const MAX_COROUTINE_STATES: u32 = 1024;

/// The tag a payload carries while the coroutine is suspended.
const PAYLOAD_YIELDED: u64 = 0;
/// The tag a payload carries once the body has returned.
const PAYLOAD_RETURNED: u64 = 1;
/// The tag a frame starts with: the coroutine exists but has not suspended, so
/// there is no value to hand back.
const PAYLOAD_STARTED: u64 = 2;

/// The body ended by raising: the frame's exception field holds the failure, and
/// `result()` re-raises it instead of answering a `Return`.
pub(super) const PAYLOAD_RAISED: u64 = 3;

/// What the lowering of a coroutine body needs to know about its machine.
pub(super) struct CoroutineContext<'ctx> {
    /// The address of the frame: the body's only parameter.
    pub frame: inkwell::values::PointerValue<'static>,
    /// The frame's LLVM type.
    pub frame_ty: inkwell::types::StructType<'static>,
    /// The index of the field holding the resume state.
    pub state_field: u32,
    /// The index of the field holding the payload tag.
    pub tag_field: u32,
    /// The index of the field holding the `Yield`/`Return` payload.
    pub payload_field: u32,
    /// The index of the field holding the `Send` a resume handed over.
    pub send_field: u32,
    /// The width of the payload slot.
    pub payload_llvm: inkwell::types::BasicTypeEnum<'static>,
    /// The number of the next resume state to hand out.
    pub next_state: u32,
    /// The block the body returns to the resumer from.
    pub suspend_block: inkwell::basic_block::BasicBlock<'ctx>,
    /// The block a resume at state `n` starts at, at index `n - 1`.
    pub resume_blocks: Vec<inkwell::basic_block::BasicBlock<'ctx>>,
    /// Whether the coroutine takes a value: `Send` is not `void`.
    pub takes_value: bool,
    /// What `yield` produces -- the `Send` type.
    pub send_ty: fallc_sema::Type,
    /// What `yield` hands over -- the `Yield` type.
    #[allow(dead_code)]
    pub yield_ty: fallc_sema::Type,
    /// The index of the frame slot backing each binding's storage, keyed by the
    /// span of the declaration that introduces it.
    ///
    /// The span is the key rather than the name, because two nested blocks may
    /// each declare a `t` and each needs its own storage; the span is one
    /// occurrence, so it cannot be ambiguous. A parameter appears here too: it
    /// is copied into its slot by the entry, and the body reads it from there
    /// however many times it has been resumed.
    pub slots: HashMap<Span, (u32, inkwell::types::BasicTypeEnum<'static>)>,
    /// The same slots, keyed by the name the binding is written with.
    ///
    /// A resumed block knows only names -- the span of a declaration is not
    /// what an expression carries -- and a name is unique within the scopes
    /// that are live at one point, so it is what a rebind looks up.
    pub slot_names: HashMap<String, u32>,
}

impl<'ctx> Lowerer<'ctx> {
    /// The address of one field of the frame being lowered.
    fn frame_field(
        &mut self,
        builder: &Builder<'static>,
        index: u32,
        name: &str,
    ) -> Result<inkwell::values::PointerValue<'static>, CodegenError> {
        let Some(coroutine) = &self.coroutine else {
            return Err(CodegenError::Emit(
                "internal error: a coroutine field outside a coroutine".to_owned(),
            ));
        };
        builder
            .build_struct_gep(coroutine.frame_ty, coroutine.frame, index, name)
            .map_err(|error| CodegenError::Emit(error.to_string()))
    }

    /// Stores a value into the payload slot, widened to the slot's width.
    fn store_payload(
        &mut self,
        builder: &Builder<'static>,
        value: BasicValueEnum<'static>,
    ) -> Result<(), CodegenError> {
        let Some(coroutine) = &self.coroutine else {
            return Ok(());
        };
        let payload_field = coroutine.payload_field;
        let payload_llvm = coroutine.payload_llvm;
        let slot = self.frame_field(builder, payload_field, "payload.slot")?;
        let yield_ty = self
            .coroutine
            .as_ref()
            .map(|coroutine| coroutine.yield_ty.clone());
        // A value that owns something is copied into the payload: the body keeps
        // its own variable, and the resumer receives a value of its own. Storing
        // the bytes would give two owners one buffer.
        if let Some(yield_ty) = yield_ty
            && (self.type_needs_drop(&yield_ty) || self.drop_symbol_for(&yield_ty).is_some())
            && let Some(constructor) = self.parameter_copy_constructor(&yield_ty)
        {
            // The slot may already hold a value from the previous suspension.
            // Constructing over it would leak that value's buffer -- and, for a
            // type whose destructor frees, leak it in a way the caller cannot
            // see. The old one is released first, unless this is the first
            // suspension, where the slot is still the zero fill of `malloc`.
            if let Some(destructor) = self.drop_symbol_for(&yield_ty) {
                let previous_state = self
                    .coroutine
                    .as_ref()
                    .map(|coroutine| (coroutine.frame, coroutine.frame_ty, coroutine.state_field));
                if let Some((frame, frame_ty, state_field)) = previous_state {
                    let started = builder
                        .build_struct_gep(frame_ty, frame, state_field, "state.before")
                        .ok()
                        .and_then(|slot| {
                            builder
                                .build_load(self.backend.context().i32_type(), slot, "state.value")
                                .ok()
                        })
                        .and_then(|value| match value {
                            BasicValueEnum::IntValue(int) => Some(int),
                            _ => None,
                        })
                        .and_then(|state| {
                            builder
                                .build_int_compare(
                                    inkwell::IntPredicate::NE,
                                    state,
                                    self.backend.context().i32_type().const_zero(),
                                    "payload.live",
                                )
                                .ok()
                        });
                    if let Some(started) = started {
                        let function = self.current_function.expect("a body is being lowered");
                        let release = self
                            .backend
                            .context()
                            .append_basic_block(function, "payload.release");
                        let store_block = self
                            .backend
                            .context()
                            .append_basic_block(function, "payload.store");
                        builder
                            .build_conditional_branch(started, release, store_block)
                            .map_err(|error| CodegenError::Emit(error.to_string()))?;
                        builder.position_at_end(release);
                        builder
                            .build_call(
                                self.resolved_function(
                                    &destructor,
                                    "a destructor",
                                    fallc_span::Span::SYNTHETIC,
                                )?,
                                &[slot.into()],
                                "payload.drop",
                            )
                            .map_err(|error| CodegenError::Emit(error.to_string()))?;
                        builder
                            .build_unconditional_branch(store_block)
                            .map_err(|error| CodegenError::Emit(error.to_string()))?;
                        builder.position_at_end(store_block);
                    }
                }
            }
            let source = builder
                .build_alloca(value.get_type(), "payload.source")
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
            builder
                .build_store(source, value)
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
            builder
                .build_call(constructor, &[slot.into(), source.into()], "payload.copy")
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
            return Ok(());
        }
        let value = resize(builder, value, payload_llvm)?;
        builder
            .build_store(slot, value)
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        Ok(())
    }

    /// Stores the frame's tag.
    pub(super) fn store_tag(
        &mut self,
        builder: &Builder<'static>,
        tag: u64,
    ) -> Result<(), CodegenError> {
        let Some(tag_field) = self.coroutine.as_ref().map(|coroutine| coroutine.tag_field) else {
            return Ok(());
        };
        let slot = self.frame_field(builder, tag_field, "tag.slot")?;
        let value = self
            .backend
            .context()
            .i8_type()
            .const_int(tag, false)
            .as_basic_value_enum();
        builder
            .build_store(slot, value)
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        Ok(())
    }

    /// Stores the state the machine resumes at.
    fn store_state(&mut self, builder: &Builder<'static>, state: u32) -> Result<(), CodegenError> {
        let Some(state_field) = self
            .coroutine
            .as_ref()
            .map(|coroutine| coroutine.state_field)
        else {
            return Ok(());
        };
        let slot = self.frame_field(builder, state_field, "state.slot")?;
        let value = self
            .backend
            .context()
            .i32_type()
            .const_int(u64::from(state), false)
            .as_basic_value_enum();
        builder
            .build_store(slot, value)
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        Ok(())
    }

    /// Lowers `yield expr`: hands the value over, suspends, and reads the next
    /// `Send` back.
    ///
    /// The suspension is a store and a `return`; what the resumer sees is the
    /// `Yield` in the frame and the state that says where to continue. The
    /// value the expression *produces* is read at the continuation, because
    /// that is where the next `resume` puts it.
    pub(super) fn lower_yield(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        node: &fallc_ast::YieldExpr,
        span: Span,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        let Some(coroutine) = &self.coroutine else {
            return Err(CodegenError::waiting_for_spec(
                "`yield` outside a coroutine",
                span,
            ));
        };
        let index = coroutine.next_state as usize;
        let Some(resume_block) = coroutine.resume_blocks.get(index).copied() else {
            return Err(CodegenError::Emit(format!(
                "this coroutine has more than {MAX_COROUTINE_STATES} suspension points, which \
                 is more than the state machine was built for"
            )));
        };
        let suspend_block = coroutine.suspend_block;
        let takes_value = coroutine.takes_value;
        let send_ty = coroutine.send_ty.clone();
        let yield_ty = coroutine.yield_ty.clone();
        let next_state = coroutine.next_state + 1;
        if let Some(coroutine) = &mut self.coroutine {
            coroutine.next_state = next_state;
        }

        // What a `yield` hands over is the coroutine's `Yield` type, and a
        // value written under it is treated exactly as a `return` value is: the
        // type's own constructor answers when the expression is not already the
        // type, and everything else is converted to it.
        let yield_llvm = TypeMapper::new(self.backend.context()).map(&yield_ty);
        let yielded = match node.value {
            Some(value) => {
                // The checker may have chosen a constructor for this value --
                // exactly as it does for a `return` -- because the expression is
                // not already the `Yield` type. The call is emitted here, and
                // the temporary it fills is what the frame stores.
                let lowered = match self.init_calls.get(&initialiser_span(ast, value)).cloned() {
                    Some(symbol) => self.lower_constructed_yield(
                        ast, types, builder, value, &symbol, &yield_ty, span,
                    )?,
                    // An integer literal is rebuilt at the `Yield` type's own
                    // width: the literal's recorded type is a detail of how the
                    // expression around it was checked, and a value the frame
                    // hands over is the coroutine's `Yield` whatever that record
                    // says -- `yield 20` under a `bool` expectation was lowered
                    // as `i1 false`, and widening that gives 0, not 20.
                    None => match (ast.expr(value).kind.clone(), yield_llvm) {
                        (fallc_ast::ExprKind::IntLiteral(literal), Some(llvm)) => {
                            match (llvm, parse_integer_literal(&literal.digits)) {
                                (inkwell::types::BasicTypeEnum::IntType(int_ty), Some(parsed)) => {
                                    Some(int_ty.const_int(parsed, false).as_basic_value_enum())
                                }
                                _ => self.lower_expr(ast, types, builder, value)?,
                            }
                        }
                        _ => self.lower_expr(ast, types, builder, value)?,
                    },
                };
                match lowered {
                    Some(lowered) => {
                        let lowered =
                            self.convert_adopted(ast, types, builder, value, lowered, span)?;
                        let lowered =
                            self.convert_implicit(ast, types, builder, value, lowered, span)?;
                        Some(match yield_llvm {
                            Some(slot) => resize(builder, lowered, slot)?,
                            None => lowered,
                        })
                    }
                    None => None,
                }
            }
            None => None,
        };
        self.store_tag(builder, PAYLOAD_YIELDED)?;
        if let Some(yielded) = yielded {
            // What a `yield` hands over is the coroutine's `Yield` type, so the
            // value is converted to that type before it goes into the payload:
            // the literal's own recorded width is a detail of how it was
            // checked, and the payload's width is what the frame was built
            // with.
            let yielded = match yield_llvm {
                Some(yield_llvm) => resize(builder, yielded, yield_llvm)?,
                None => yielded,
            };
            self.store_payload(builder, yielded)?;
        }
        self.store_state(builder, next_state)?;
        self.branch_to(builder, suspend_block)?;

        builder.position_at_end(resume_block);
        // A resumed block is a new basic block, so every binding's address has
        // to be rebuilt here: the GEP that found it in the block the binding
        // was declared in does not dominate this one. The frame is the same
        // object, so this is a re-address of storage, not a copy.
        self.rebind_coroutine_slots(builder)?;
        if !takes_value {
            return Ok(None);
        }
        let llvm_ty = TypeMapper::new(self.backend.context())
            .map(&send_ty)
            .ok_or_else(|| {
                CodegenError::Emit(format!(
                    "internal error: `{}` has no LLVM form as a `Send` type",
                    send_ty.spelling()
                ))
            })?;
        let send_field = self
            .coroutine
            .as_ref()
            .map(|coroutine| coroutine.send_field)
            .expect("a send slot exists");
        let slot = self.frame_field(builder, send_field, "send.slot")?;
        let sent = builder
            .build_load(llvm_ty, slot, "send")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        Ok(Some(sent))
    }

    /// The frame slot a binding's storage lives in, by its name, when a
    /// coroutine is being lowered and the name has one.
    pub(super) fn coroutine_slot_name(
        &self,
        name: &str,
    ) -> Option<(u32, inkwell::types::BasicTypeEnum<'static>)> {
        let coroutine = self.coroutine.as_ref()?;
        let field = coroutine.slot_names.get(name).copied()?;
        let ty = coroutine
            .slots
            .values()
            .find(|(index, _)| *index == field)
            .map(|(_, ty)| *ty)?;
        Some((field, ty))
    }

    /// Records the frame slot a binding declared in the body lives in, by name.
    ///
    /// Called where the body's declaration is lowered: the frame was built from
    /// a walk of the body, so a declaration always has a slot, and this is what
    /// lets a resumed block find it again by name.
    pub(super) fn note_coroutine_slot_name(&mut self, name: &str, field: u32) {
        if let Some(coroutine) = &mut self.coroutine {
            coroutine.slot_names.insert(name.to_owned(), field);
        }
    }

    /// The frame slot a binding's storage lives in, when one is being lowered.
    pub(super) fn coroutine_slot(
        &self,
        declaration: Span,
    ) -> Option<(u32, inkwell::types::BasicTypeEnum<'static>)> {
        self.coroutine.as_ref()?.slots.get(&declaration).copied()
    }

    /// The address of a binding's frame slot.
    /// Rebuilds every live binding's address at the current position.
    ///
    /// A `yield` resumes in a block of its own, and a `getelementptr` computed
    /// in the block a binding was declared in does not dominate it. The frame
    /// is one object for the whole coroutine, so the slots are the same
    /// storage; only the instruction that names them has to be emitted again,
    /// here, where it dominates its uses.
    fn rebind_coroutine_slots(&mut self, builder: &Builder<'static>) -> Result<(), CodegenError> {
        let mut names: Vec<String> = Vec::new();
        for scope in &self.binding_scopes {
            for name in scope.keys() {
                names.push(name.clone());
            }
        }
        for name in names {
            let Some(field) = self
                .coroutine
                .as_ref()
                .and_then(|coroutine| coroutine.slot_names.get(&name).copied())
            else {
                continue;
            };
            let storage = match self.coroutine_slot_pointer(builder, field, &name) {
                Ok(storage) => storage,
                Err(error) => return Err(error),
            };
            for scope in self.binding_scopes.iter_mut().rev() {
                if let Some(entry) = scope.get_mut(&name) {
                    entry.storage = storage;
                    break;
                }
            }
        }
        Ok(())
    }

    pub(super) fn coroutine_slot_pointer(
        &mut self,
        builder: &Builder<'static>,
        field: u32,
        name: &str,
    ) -> Result<inkwell::values::PointerValue<'static>, CodegenError> {
        self.frame_field(builder, field, name)
    }
}

/// Converts a payload value to the slot's width, in either direction.
///
/// The payload slot is as wide as the larger of `Yield` and `Return`, so a
/// smaller one is stored widened and read back narrowed. Everything else is
/// already the slot's type.
fn resize(
    builder: &Builder<'static>,
    value: BasicValueEnum<'static>,
    target: inkwell::types::BasicTypeEnum<'static>,
) -> Result<BasicValueEnum<'static>, CodegenError> {
    if value.get_type() == target {
        return Ok(value);
    }
    match (value, target) {
        (BasicValueEnum::IntValue(int), inkwell::types::BasicTypeEnum::IntType(target)) => {
            if target.get_bit_width() >= int.get_type().get_bit_width() {
                // A `bool` is the one narrow value that reaches a slot wider
                // than itself: `?bool` and a `bool` payload travel as one bit
                // and are stored in a slot sized for the other half of the
                // pair. Zero-extending keeps false as 0 and true as 1.
                builder
                    .build_int_z_extend(int, target, "payload.wide")
                    .map(BasicValueEnum::IntValue)
                    .map_err(|error| CodegenError::Emit(error.to_string()))
            } else {
                builder
                    .build_int_truncate(int, target, "payload.narrow")
                    .map(BasicValueEnum::IntValue)
                    .map_err(|error| CodegenError::Emit(error.to_string()))
            }
        }
        // The two payloads share a slot only when their widths agree; a record
        // or pointer below a wider integer already has the right shape.
        (value, _) => Ok(value),
    }
}

impl<'ctx> Lowerer<'ctx> {
    /// Lowers one `@coroutine` function into a state machine.
    ///
    /// The function's own symbol keeps the *entry*: it takes the written
    /// parameters, allocates the frame, copies the parameters into it, and runs
    /// the machine at state 0. The machine is a second, private function that
    /// takes the frame and nothing else, so a `resume` from another module
    /// reaches it through the object's function pointer rather than by name.
    ///
    /// - `ast`: the tree the function is declared in
    /// - `types`: the checked types of every expression
    /// - `bindings`: the declared type of every binding
    /// - `function`: the declaration being lowered
    /// - `entry_function`: the LLVM function already declared under the
    ///   function's symbol
    /// - `body`: the block the body was written as
    pub(super) fn lower_coroutine(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        bindings: &BindingTypes,
        function: &FunctionItem,
        entry_function: FunctionValue<'static>,
        body: BlockId,
    ) {
        let context = self.backend.context();
        let mapper = TypeMapper::new(context);
        // The three types the declaration fixed. `@coroutine` on anything else
        // was reported by the checker, and a coroutine is not emitted without
        // them.
        let return_type = self
            .pre_resolved_return
            .take()
            .unwrap_or_else(|| self.resolve_type(&function.return_type));
        let Some(shape) = CoroutineShapeIr::of(&return_type) else {
            self.report_error(
                "internal error: a `@coroutine` function's return type is not a coroutine",
            );
            return;
        };
        // A `void` type has no LLVM form, and it is exactly the case that means
        // "no value travels" -- so it is mapped to `None` rather than treated
        // as a failure.
        let map_or_void = |ty: &fallc_sema::Type| {
            if ty.is_void() { None } else { mapper.map(ty) }
        };
        // A `Return` of `void` is a coroutine that only does something: the
        // payload then carries nothing when it finishes, and `result()` answers
        // nothing. The three positions are independent, so this is the third of
        // the three that may be `void`.
        let result_llvm = map_or_void(&shape.return_ty);
        let takes_value = !shape.send.is_void();
        let yields_value = !shape.yield_ty.is_void();
        // An array's `?T` has no address yet -- `?[N]T` cannot be read back out
        // of its optional -- so a coroutine carrying one is refused rather than
        // compiled into a module that does not verify.
        for (what, ty) in [
            ("`Yield`", &shape.yield_ty),
            ("`Return`", &shape.return_ty),
            ("`Send`", &shape.send),
        ] {
            if ty.is_array() {
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "a coroutine whose {what} is an array is not supported: `?[N]T` \
                         cannot be read back out of its optional yet"
                    ))
                    .with_code(fallc_diag::DiagnosticCode::ir(12))
                    .with_span(function.return_type.span())
                    .with_note(
                        "give the coroutine a named record with the elements as fields, or \
                         pass the elements one at a time",
                    ),
                );
                return;
            }
        }
        let send_llvm = if takes_value {
            match map_or_void(&shape.send) {
                Some(llvm) => llvm,
                None => {
                    self.report_waiting_for_spec(
                        function.return_type.span(),
                        "a coroutine whose `Send` has no LLVM form",
                    );
                    return;
                }
            }
        } else {
            context.i8_type().into()
        };
        let yield_llvm = if yields_value {
            match map_or_void(&shape.yield_ty) {
                Some(llvm) => llvm,
                None => {
                    self.report_waiting_for_spec(
                        function.return_type.span(),
                        "a coroutine whose `Yield` has no LLVM form",
                    );
                    return;
                }
            }
        } else {
            context.i8_type().into()
        };
        // One slot holds the value yielded or the value returned. Its width is
        // the larger of the two, so a smaller one is stored widened and read
        // back narrowed.
        let payload_llvm = payload_shape(
            yields_value.then_some(yield_llvm),
            result_llvm,
            context.i8_type().into(),
        );

        // The frame's field order. The first four are fixed, so the standard
        // library's `resume` and `result` can read them without knowing which
        // coroutine they have; the bindings follow.
        let state_field = 0u32;
        let tag_field = 1u32;
        let payload_field = 2u32;
        let send_field = 3u32;
        // The exception a `raise` inside the body is carrying, if any. A
        // coroutine body may raise like any other function, and the failure has
        // to survive from the `raise` to the `result()` that re-raises it -- so
        // it lives in the frame, beside the payload.
        let exception_field = 4u32;
        let exception_llvm = self
            .exception_type()
            .unwrap_or_else(|| context.i8_type().into());
        let mut field_types: Vec<inkwell::types::BasicTypeEnum<'static>> = vec![
            context.i32_type().into(),
            context.i8_type().into(),
            payload_llvm,
            if takes_value {
                send_llvm
            } else {
                context.i8_type().into()
            },
            exception_llvm,
        ];
        let mut slots: HashMap<Span, (u32, inkwell::types::BasicTypeEnum<'static>)> =
            HashMap::new();
        let mut slot_names: HashMap<String, u32> = HashMap::new();
        let mut parameter_slots: Vec<(Span, u32)> = Vec::new();
        for param in &function.params {
            let ty = self.resolve_type(&param.ty);
            let Some(llvm) = mapper.map(&ty) else {
                self.report_waiting_for_spec(
                    param.ty.span(),
                    &format!("a coroutine parameter of type `{}`", ty.spelling()),
                );
                return;
            };
            let index =
                u32::try_from(field_types.len()).expect("a frame has fewer than u32 fields");
            field_types.push(llvm);
            slots.insert(param.name_span, (index, llvm));
            slot_names.insert(param.name.clone(), index);
            parameter_slots.push((param.name_span, index));
        }
        let mut bindings_found: Vec<(Span, inkwell::types::BasicTypeEnum<'static>)> = Vec::new();
        if !self.collect_coroutine_slots(ast, bindings, body, &mut bindings_found) {
            return;
        }
        for (span, llvm) in bindings_found {
            if slots.contains_key(&span) {
                continue;
            }
            let index =
                u32::try_from(field_types.len()).expect("a frame has fewer than u32 fields");
            field_types.push(llvm);
            slots.insert(span, (index, llvm));
        }
        let frame_ty = context.struct_type(&field_types, false);

        // The machine. Its symbol is derived from the entry's so that two
        // coroutines of one name-and-signature pair -- the members of a family,
        // say -- cannot collide.
        let entry_symbol = entry_function.get_name().to_string_lossy().into_owned();
        let body_symbol = format!("{entry_symbol}__coroutine_body");
        let pointer = context.ptr_type(inkwell::AddressSpace::default());
        let body_ty = context.void_type().fn_type(&[pointer.into()], false);
        let body_function = self
            .backend
            .module()
            .get_function(&body_symbol)
            .unwrap_or_else(|| {
                self.backend
                    .module()
                    .add_function(&body_symbol, body_ty, None)
            });
        // The machine's blocks. The dispatch is created first, so it is the
        // function's entry block, and it is filled in *after* the body is
        // lowered, because only then is it known how many states the body
        // created. Every continuation is created up
        // front, one per state the design allows, because a `yield` has to know
        // the block it resumes at while it is being lowered.
        let dispatch_block = context.append_basic_block(body_function, "coroutine.dispatch");
        let suspend_block = context.append_basic_block(body_function, "coroutine.suspend");
        let start_block = context.append_basic_block(body_function, "coroutine.start");
        let resume_blocks: Vec<_> = (1..=MAX_COROUTINE_STATES)
            .map(|state| {
                context.append_basic_block(body_function, &format!("coroutine.resume.{state}"))
            })
            .collect();
        let frame = body_function
            .get_nth_param(0)
            .expect("the body takes the frame")
            .into_pointer_value();
        let builder = context.create_builder();
        builder.position_at_end(start_block);

        // The machine's own lowering context. The binding scopes start empty:
        // every binding the body declares resolves to a frame slot, which
        // `lower_bind` finds by the declaration's span.
        self.current_function = Some(body_function);
        self.current_return_type = Some(return_type.clone());
        self.current_bindings = bindings.clone();
        self.loop_stack.clear();
        self.binding_scopes.clear();
        self.binding_scopes.push(HashMap::new());
        self.drop_scopes.clear();
        self.drop_scopes.push(Vec::new());
        self.defer_scopes.clear();
        self.defer_scopes.push(Vec::new());
        self.exc_frames.clear();
        // The body may raise like any other function, and the failure has to
        // outlive the `yield`s between the `raise` and the `result()` that
        // re-raises it. So its escape goes into the frame, and `result()` reads
        // it from there -- the same role the tagged result plays in an ordinary
        // `except` function, kept in the coroutine's own storage because the
        // body returns to its resumer rather than to a caller.
        // The address of the escape slot, emitted where the body starts so that
        // every `raise` and every `return` can write it.
        let exception_slot = body_function.get_first_basic_block().and_then(|entry| {
            let builder = context.create_builder();
            builder.position_at_end(entry);
            builder
                .build_struct_gep(frame_ty, frame, exception_field, "exception")
                .ok()
        });
        self.exc_result = exception_slot;
        self.exc_result_type =
            exception_slot.and_then(|_| field_types.get(exception_field as usize).copied());
        self.sret_slot = None;
        self.coroutine = Some(CoroutineContext {
            frame,
            frame_ty,
            state_field,
            tag_field,
            payload_field,
            send_field,
            payload_llvm,
            next_state: 0,
            suspend_block,
            resume_blocks: resume_blocks.clone(),
            takes_value,
            send_ty: shape.send.clone(),
            yield_ty: shape.yield_ty.clone(),
            slot_names: slot_names.clone(),
            slots,
        });

        // The parameters are bindings of the body like any other, and their
        // storage is their frame slot. Registering them here is what makes a
        // read of `n` inside the body a load from the frame.
        for param in &function.params {
            let Some((field, llvm)) = self.coroutine_slot(param.name_span) else {
                continue;
            };
            let Ok(storage) = self.coroutine_slot_pointer(&builder, field, &param.name) else {
                self.report_error("internal error: a coroutine parameter has no frame slot");
                continue;
            };
            let ty = self.resolve_type(&param.ty);
            self.register_binding(&param.name, &ty, storage, llvm);
        }

        // The body itself, starting at state 0.
        if let Err(error) = self.lower_coroutine_block(ast, types, bindings, &builder, body) {
            self.diagnostics.push(error.to_diagnostic());
        }

        // How many states the body created, now that it has been lowered.
        let coroutine_states = self
            .coroutine
            .as_ref()
            .map_or(0, |coroutine| coroutine.next_state);
        self.coroutine = None;
        self.current_function = None;
        self.current_return_type = None;
        self.binding_scopes.clear();
        self.drop_scopes.clear();
        self.defer_scopes.clear();

        // The dispatch, now that the states are known: it goes into the
        // function's first block. Only a state the body actually reached is a
        // case -- a continuation nothing reaches is filled with `unreachable`,
        // and a switch that named it would be a branch *into* it.
        let dispatch = context.create_builder();
        dispatch.position_at_end(dispatch_block);
        let dispatched = (|| -> Result<(), CodegenError> {
            let state_pointer = dispatch
                .build_struct_gep(frame_ty, frame, state_field, "state")
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
            let state = dispatch
                .build_load(context.i32_type(), state_pointer, "state.value")
                .map_err(|error| CodegenError::Emit(error.to_string()))?
                .into_int_value();
            let mut cases: Vec<_> = resume_blocks
                .iter()
                .take(coroutine_states as usize)
                .enumerate()
                .map(|(index, block)| {
                    (
                        context.i32_type().const_int(index as u64 + 1, false),
                        *block,
                    )
                })
                .collect();
            // State 0 is the beginning, and it is a case rather than the
            // default: a state the machine never hands out cannot occur.
            cases.push((context.i32_type().const_zero(), start_block));
            dispatch
                .build_switch(state, suspend_block, &cases)
                .map(|_| ())
                .map_err(|error| CodegenError::Emit(error.to_string()))
        })();
        if let Err(error) = dispatched {
            self.report_error(&error.to_string());
            return;
        }
        // Every continuation the body never reached is unreachable, which is
        // both true and what keeps LLVM's verifier satisfied: an unterminated
        // block is invalid IR.
        for (index, block) in resume_blocks.iter().enumerate() {
            if index >= coroutine_states as usize {
                let filler = context.create_builder();
                filler.position_at_end(*block);
                let _ = filler.build_unreachable();
            }
        }
        // The suspend block is where a `yield` returns to the resumer, and it
        // is the machine's exit: a `yield` branched there and returns.
        let suspend_builder = context.create_builder();
        suspend_builder.position_at_end(suspend_block);
        suspend_builder
            .build_return(None)
            .map_err(|error| error.to_string())
            .ok();
        self.coroutine = None;
        self.current_function = None;
        self.current_return_type = None;
        self.binding_scopes.clear();
        self.drop_scopes.clear();
        self.defer_scopes.clear();

        // What `resume` and `result` will need, recorded before the entry is
        // lowered so that a program which *calls* a member still finds it
        // declared: the generator runs after every entry, and the record is
        // keyed by the type the entry returns because that is what a member's
        // symbol is mangled from.
        // The member symbols the program calls on this type. The checker
        // recorded each one against the call's span, and the symbol itself
        // carries the receiver's mangled type -- so the type is found in the
        // symbol rather than recovered from a signature. A symbol whose
        // receiver is another member of the family does not contain this
        // type's own mangling, which is what keeps two coroutines apart.
        // The two members belong to the coroutine, not to a module: their
        // symbols are built from the coroutine's own type, by the same rule the
        // checker used at every call site. A coroutine whose members are called
        // only from another module -- a task queued by `std.go` -- still needs
        // its `resume` and `result` emitted here, where the frame is known, so
        // the symbols cannot be read off this module's call table.
        // The receiver is the coroutine object itself, mangled exactly as the
        // checker mangled it at the call site -- the value's type, not a
        // reference to it.
        let receiver = fallc_sema::mangle_type(&return_type);
        let mut resume_args = vec![receiver.clone()];
        if !shape.send.is_void() {
            resume_args.push(fallc_sema::mangle_type(&shape.send));
        }
        let resume_symbol = Some(fallc_ast::special::symbol_for("resume", &resume_args));
        let result_symbol = Some(fallc_ast::special::symbol_for("result", &[receiver]));
        // What a `resume` answers is a property of the coroutine's own types,
        // read here rather than looked up: `?Yield` when the `yield` hands a
        // value over, and `void` when it hands nothing. The checker reads the
        // same thing, and the two stages must not disagree about which type a
        // call has.
        let resume_returns = Some(if shape.yield_ty.is_void() {
            fallc_sema::Type::unspecified(String::new(), fallc_span::Span::SYNTHETIC)
        } else {
            // The member the checker built, with the fields the declaration gave
            // it. Building one here from the name would leave every field
            // missing, and the optional's layout would be empty.
            let name = format!("Optional<{}>", shape.yield_ty.spelling());
            match self.nominal_types.get(&name).cloned() {
                Some(nominal) => fallc_sema::Type::Nominal(nominal),
                None => {
                    self.report_waiting_for_spec(
                        fallc_span::Span::SYNTHETIC,
                        "an optional of this coroutine's `Yield`",
                    );
                    return;
                }
            }
        });
        self.coroutine_frames.insert(
            shape.key(),
            CoroutineFrame {
                resume_symbol,
                result_symbol,
                resume_returns,
                frame_ty,
                state_field,
                tag_field,
                payload_field,
                send_field,
                exception_field,
                payload_llvm,
                coroutine_llvm: TypeMapper::new(context).map(&return_type),
                yield_llvm: if yields_value { Some(yield_llvm) } else { None },
                takes_value,
                send_ty: shape.send.clone(),
                return_ty: shape.return_ty.clone(),
            },
        );

        // The entry: allocate the frame, copy the parameters into it, and jump
        // into the machine.
        let entry_block = context.append_basic_block(entry_function, "coroutine.entry");
        let entry_builder = context.create_builder();
        entry_builder.position_at_end(entry_block);
        // The frame's size is what `malloc` is asked for, so it has to be a
        // constant now. `size_of` answers with one only when the module's data
        // layout is set, which happens before lowering begins.
        // `sizeof` as a constant expression rather than a folded number: the
        // frame may hold a pointer, whose size the module's data layout gives
        // and which the layout may not be known for yet. LLVM reduces the
        // expression when it emits the module.
        let Some(size) = frame_ty.size_of() else {
            self.report_error("internal error: a coroutine frame has no size");
            return;
        };
        let frame_memory = self.allocate_coroutine_frame(&entry_builder, size);
        let frame_memory = match frame_memory {
            Ok(memory) => memory,
            Err(error) => {
                self.report_error(&error.to_string());
                return;
            }
        };
        // The frame is zeroed first: `malloc` hands back whatever the allocator
        // had, and the resume state is the machine's first decision -- reading
        // an uninitialised one would dispatch to a state the coroutine never
        // reached.
        let size_for_fill = frame_ty.size_of().ok_or_else(|| {
            CodegenError::Emit("internal error: a coroutine frame has no size".to_owned())
        });
        match size_for_fill {
            Ok(size) => {
                entry_builder
                    .build_memset(frame_memory, 8, context.i8_type().const_zero(), size)
                    .map_err(|error| error.to_string())
                    .ok();
            }
            Err(error) => {
                self.report_error(&error.to_string());
                return;
            }
        }
        // The tag then starts at "created, not suspended": a zero-filled frame
        // would say "a `Yield` is waiting", which is the one wrong answer a
        // fresh coroutine could give -- `resume` would hand back an
        // uninitialised payload.
        let start_tag = entry_builder
            .build_struct_gep(frame_ty, frame_memory, tag_field, "tag.slot")
            .map_err(|error| error.to_string());
        match start_tag {
            Ok(slot) => {
                let started = context
                    .i8_type()
                    .const_int(PAYLOAD_STARTED, false)
                    .as_basic_value_enum();
                entry_builder
                    .build_store(slot, started)
                    .map_err(|error| error.to_string())
                    .ok();
            }
            Err(error) => {
                self.report_error(&error);
                return;
            }
        }
        for (index, (span, field)) in parameter_slots.iter().enumerate() {
            let Some(value) = entry_function.get_nth_param(index_u32(index)) else {
                continue;
            };
            let Ok(slot) = entry_builder.build_struct_gep(frame_ty, frame_memory, *field, "param")
            else {
                self.report_error("internal error: a coroutine parameter slot is out of range");
                return;
            };
            if let Err(error) = entry_builder.build_store(slot, value) {
                self.report_error(&error.to_string());
                return;
            }
            let _ = span;
        }
        // The call creates the coroutine and stops: running it is what
        // `resume` is for, and the first `resume` starts the body. Starting
        // here would make the first `resume` answer the *second* value and lose
        // the first.
        let _ = body_function;
        // The coroutine object. Its first two fields are the frame and the body,
        // which is the whole of what `resume` needs; the object's own store in
        // the standard library is these two fields and nothing else.
        let Some(object_ty) = self.coroutine_object_type() else {
            self.report_error("internal error: `std.Coroutine` has no LLVM layout");
            return;
        };
        let object = self.create_entry_alloca(&entry_builder, entry_block, "coroutine", object_ty);
        let Ok(frame_slot) = entry_builder.build_struct_gep(object_ty, object, 0, "frame") else {
            self.report_error("internal error: a coroutine object has no frame field");
            return;
        };
        entry_builder
            .build_store(frame_slot, frame_memory)
            .map_err(|error| error.to_string())
            .ok();
        let Ok(body_slot) = entry_builder.build_struct_gep(object_ty, object, 1, "body") else {
            self.report_error("internal error: a coroutine object has no body field");
            return;
        };
        entry_builder
            .build_store(
                body_slot,
                body_function.as_global_value().as_pointer_value(),
            )
            .map_err(|error| error.to_string())
            .ok();
        let loaded = entry_builder
            .build_load(object_ty, object, "coroutine.value")
            .expect("loading the object cannot fail");
        entry_builder
            .build_return(Some(&loaded))
            .map_err(|error| error.to_string())
            .ok();
    }

    /// Lowers the written body into the machine.
    ///
    /// The body's own `return` is the coroutine's answer -- the value goes into
    /// the payload, the tag says it is the result, and the machine returns to
    /// the resumer -- which `finish_coroutine` handles.
    fn lower_coroutine_block(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        bindings: &BindingTypes,
        builder: &Builder<'static>,
        body: BlockId,
    ) -> Result<(), CodegenError> {
        self.lower_block(ast, types, bindings, builder, body)?;
        // Reaching the end of the body without a `return` finishes the
        // coroutine with no value, which is what a `Return` of `void` means.
        if !Self::is_terminated(builder) {
            self.finish_coroutine(builder, None)?;
        }
        Ok(())
    }

    /// Finishes the coroutine: stores the result, tags the payload, and returns
    /// to the resumer.
    ///
    /// - `builder`: where the finishing happens
    /// - `value`: the body's `return` value, or `None` for a body that falls off
    ///   its end
    pub(super) fn finish_coroutine(
        &mut self,
        builder: &Builder<'static>,
        value: Option<BasicValueEnum<'static>>,
    ) -> Result<(), CodegenError> {
        let Some(coroutine) = &self.coroutine else {
            return Err(CodegenError::Emit(
                "internal error: finishing a coroutine outside one".to_owned(),
            ));
        };
        let suspend_block = coroutine.suspend_block;
        self.store_tag(builder, PAYLOAD_RETURNED)?;
        if let Some(value) = value {
            self.store_payload(builder, value)?;
        }
        // The state of a finished coroutine is the suspend state: a `resume`
        // after it ends finds the tag saying so and does nothing.
        self.store_state(builder, 0)?;
        self.branch_to(builder, suspend_block)?;
        Ok(())
    }

    /// Allocates the frame on the heap.
    ///
    /// `malloc` rather than the library's allocator: the frame is the
    /// compiler's own bookkeeping, and a call into the standard library here
    /// would make every coroutine depend on the module that owns allocation.
    /// The matching `free` is the coroutine object's destructor.
    fn allocate_coroutine_frame(
        &mut self,
        builder: &Builder<'static>,
        size: inkwell::values::IntValue<'static>,
    ) -> Result<inkwell::values::PointerValue<'static>, CodegenError> {
        let context = self.backend.context();
        let size_ty = context.i64_type();
        let malloc = self.declare_libc(
            "malloc",
            &[size_ty.into()],
            Some(context.ptr_type(inkwell::AddressSpace::default()).into()),
        );
        let allocated = builder
            .build_call(malloc, &[size.into()], "frame")
            .map_err(|error| CodegenError::Emit(error.to_string()))?
            .try_as_basic_value()
            .basic()
            .ok_or_else(|| {
                CodegenError::Emit("internal error: malloc returned nothing".to_owned())
            })?;
        let BasicValueEnum::PointerValue(frame) = allocated else {
            return Err(CodegenError::Emit(
                "internal error: malloc did not return a pointer".to_owned(),
            ));
        };
        Ok(frame)
    }

    /// Collects the frame slots for every binding the body declares.
    ///
    /// The frame's size is fixed before the first instruction is emitted, so
    /// every binding the body can create has to be known in advance. The walk
    /// covers the body and every block nested in it -- an `if`, a loop body, a
    /// `switch` arm -- and records each declaration by its span, which is one
    /// occurrence and so cannot be confused with another block's `t`.
    ///
    /// Returns `false` when a binding cannot live in the frame, which is
    /// reported.
    fn collect_coroutine_slots(
        &mut self,
        ast: &Ast,
        bindings: &BindingTypes,
        body: BlockId,
        found: &mut Vec<(Span, inkwell::types::BasicTypeEnum<'static>)>,
    ) -> bool {
        let mapper = TypeMapper::new(self.backend.context());
        let mut pending: Vec<BlockId> = vec![body];
        let mut seen: std::collections::HashSet<BlockId> = std::collections::HashSet::new();
        let mut ok = true;
        while let Some(block) = pending.pop() {
            if !seen.insert(block) {
                continue;
            }
            for &stmt in &ast.block(block).statements {
                collect_binding(ast, bindings, &mapper, stmt, found, &mut pending, &mut ok);
            }
        }
        ok
    }

    /// The LLVM type of the coroutine object: the frame and the machine.
    fn coroutine_object_type(&self) -> Option<inkwell::types::BasicTypeEnum<'static>> {
        let context = self.backend.context();
        let pointer = context.ptr_type(inkwell::AddressSpace::default());
        Some(
            context
                .struct_type(&[pointer.into(), pointer.into()], false)
                .into(),
        )
    }
}

/// Records the frame slot a statement's binding needs, and queues the blocks
/// nested in it.
///
/// - `ast`: the tree the statement is in
/// - `bindings_types`: the declared type of every binding, keyed by its span
/// - `mapper`: turns a checked type into the LLVM type the slot holds
/// - `stmt`: the statement to look at
/// - `found`: the slot width recorded for each binding, appended to
/// - `pending`: the nested blocks to look at next
/// - `ok`: set to `false` when a binding cannot live in a frame
fn collect_binding(
    ast: &Ast,
    bindings_types: &BindingTypes,
    mapper: &TypeMapper<'static>,
    stmt: StmtId,
    found: &mut Vec<(Span, inkwell::types::BasicTypeEnum<'static>)>,
    pending: &mut Vec<BlockId>,
    ok: &mut bool,
) {
    match &ast.stmt(stmt).kind {
        StmtKind::Bind(node) => {
            // A `static` local is one object for the whole run and is not part
            // of the frame: it lives in a global, and `lower_bind` emits it
            // there however many times the body is entered.
            if node.is_static {
                return;
            }
            let Some(ty) = bindings_types.get(node.name_span).cloned() else {
                *ok = false;
                return;
            };
            let Some(llvm) = mapper.map(&ty) else {
                *ok = false;
                return;
            };
            found.push((node.name_span, llvm));
        }
        StmtKind::Block(block) | StmtKind::Defer(block) => pending.push(*block),
        StmtKind::If(node) => {
            pending.push(node.then_block);
            match &node.else_branch {
                Some(fallc_ast::ElseBranch::Block(block)) => pending.push(*block),
                Some(fallc_ast::ElseBranch::If(other)) => {
                    collect_binding(ast, bindings_types, mapper, *other, found, pending, ok);
                }
                None => {}
            }
        }
        StmtKind::While(node) => {
            pending.push(node.body);
            if let Some(block) = node.else_block {
                pending.push(block);
            }
        }
        StmtKind::ForC(node) => {
            pending.push(node.body);
            if let Some(block) = node.else_block {
                pending.push(block);
            }
        }
        StmtKind::Switch(node) => {
            for arm in &node.arms {
                for &inner in &arm.body {
                    collect_binding(ast, bindings_types, mapper, inner, found, pending, ok);
                }
            }
        }
        _ => {}
    }
}

/// The width one payload slot needs for the values it may hold.
fn payload_shape(
    yielded: Option<inkwell::types::BasicTypeEnum<'static>>,
    returned: Option<inkwell::types::BasicTypeEnum<'static>>,
    fallback: inkwell::types::BasicTypeEnum<'static>,
) -> inkwell::types::BasicTypeEnum<'static> {
    match (yielded, returned) {
        (Some(yielded), Some(returned)) => wider(yielded, returned),
        (Some(ty), None) | (None, Some(ty)) => ty,
        (None, None) => fallback,
    }
}

/// The wider of two payload types, for the one slot they share.
fn wider(
    left: inkwell::types::BasicTypeEnum<'static>,
    right: inkwell::types::BasicTypeEnum<'static>,
) -> inkwell::types::BasicTypeEnum<'static> {
    match (left, right) {
        (
            inkwell::types::BasicTypeEnum::IntType(left),
            inkwell::types::BasicTypeEnum::IntType(right),
        ) => {
            if left.get_bit_width() >= right.get_bit_width() {
                left.into()
            } else {
                right.into()
            }
        }
        // A `Yield` and a `Return` of different kinds share the slot by size:
        // the wider one is the layout, and the value is converted when it is
        // read back out.
        (left, right) => {
            let left_size = left
                .size_of()
                .and_then(|size| size.get_zero_extended_constant());
            let right_size = right
                .size_of()
                .and_then(|size| size.get_zero_extended_constant());
            if right_size.unwrap_or(0) > left_size.unwrap_or(0) {
                right
            } else {
                left
            }
        }
    }
}

/// An index as the `u32` LLVM parameters are numbered by.
fn index_u32(index: usize) -> u32 {
    u32::try_from(index).expect("a parameter list has fewer than u32 entries")
}

/// The `Send`, `Yield` and `Return` types of a coroutine's return type.
///
/// The same three the checker read, read again from the type because the code
/// generator is handed the type and not the checker's table. The name is
/// `std.Coroutine`, so the family's arguments are the three types in order.
#[derive(Clone, Debug)]
struct CoroutineShapeIr {
    /// What a `resume` hands over.
    send: fallc_sema::Type,
    /// What a `yield` hands over.
    yield_ty: fallc_sema::Type,
    /// What the body returns.
    return_ty: fallc_sema::Type,
}

impl std::fmt::Debug for CoroutineContext<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("CoroutineContext")
    }
}

impl CoroutineShapeIr {
    /// The key two coroutines of this shape share: the type's own spelling.
    ///
    /// The spelling is what a member's symbol is mangled from, so two entries
    /// that return `Coroutine<i32, i32, i32>` produce the same key and share one
    /// `resume`.
    fn key(&self) -> String {
        format!(
            "Coroutine<{}, {}, {}>",
            self.send.spelling(),
            self.yield_ty.spelling(),
            self.return_ty.spelling()
        )
    }

    /// Reads the three types off a `std.Coroutine<S, Y, R>` type.
    fn of(return_type: &fallc_sema::Type) -> Option<Self> {
        let fallc_sema::Type::Nominal(nominal) = return_type.unqualified() else {
            return None;
        };
        let (family, arguments) = nominal.template_arguments.as_ref()?;
        let family = family.clone();
        let _ = &family;
        // The family is recorded under the name the declaration was written
        // with, so `std.Coroutine` and a bare `Coroutine` both end in the same
        // word. The last segment is what is compared.
        let bare = family
            .rsplit_once('.')
            .map_or(family.as_str(), |(_, last)| last);
        if bare != "Coroutine" || arguments.0.len() != 3 {
            return None;
        }
        let mut types = Vec::with_capacity(3);
        for argument in &arguments.0 {
            let fallc_sema::template::TemplateArgument::Type(ty) = argument else {
                return None;
            };
            types.push(ty.clone());
        }
        Some(Self {
            send: types[0].clone(),
            yield_ty: types[1].clone(),
            return_ty: types[2].clone(),
        })
    }
}

impl<'ctx> Lowerer<'ctx> {
    /// Lowers a `return` inside a `@coroutine` body: finishes the coroutine with
    /// the value instead of returning from the machine.
    ///
    /// - `ast`: the tree the statement is in
    /// - `types`: the checked types
    /// - `bindings`: the declared type of every binding
    /// - `builder`: where the finishing is emitted
    /// - `node`: the `return` statement
    pub(super) fn lower_coroutine_return(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        bindings: &BindingTypes,
        builder: &Builder<'static>,
        node: &ReturnStmt,
    ) -> Result<(), CodegenError> {
        let value = match node.value {
            Some(value) => Some(
                self.lower_expr(ast, types, builder, value)?
                    .ok_or_else(|| {
                        CodegenError::waiting_for_spec(
                            "this coroutine's return value",
                            ast.expr(value).span,
                        )
                    })?,
            ),
            None => None,
        };
        // The body's own values are not destroyed on the way out: they live in
        // the frame, and the frame is freed by the coroutine object rather than
        // by this `return`. A `defer` in the body *does* run, because it is a
        // statement that runs when a scope exits and a `return` exits every
        // scope it is inside.
        let defers: Vec<BlockId> = self
            .defer_scopes
            .iter()
            .rev()
            .flat_map(|scope| scope.iter().rev().copied())
            .collect();
        for block in defers {
            if Self::is_terminated(builder) {
                break;
            }
            self.lower_block(ast, types, bindings, builder, block)?;
        }
        self.finish_coroutine(builder, value)
    }
}

/// Whether an expression contains a `yield` anywhere inside it.
///
/// The answer decides an ordering: a `yield` splits the machine into blocks, and
/// an operation that spans the split -- `i += yield i` reads `i` before the
/// suspension and writes it after -- has to be lowered in the order that
/// survives one.
pub(super) fn contains_yield(ast: &Ast, expr: fallc_ast::ExprId) -> bool {
    struct Finder {
        found: bool,
    }
    impl fallc_ast::visit::Visitor for Finder {
        fn visit_expr(&mut self, ast: &Ast, id: fallc_ast::ExprId) {
            if matches!(ast.expr(id).kind, fallc_ast::ExprKind::Yield(_)) {
                self.found = true;
            }
        }
    }
    let mut finder = Finder { found: false };
    fallc_ast::visit::walk_expr(ast, expr, &mut finder);
    finder.found
}

impl<'ctx> Lowerer<'ctx> {
    /// Emits `resume` and `result` for every coroutine type a program uses.
    ///
    /// Both are declared in `std.coroutine` and implemented here, once per
    /// `Coroutine<S, Y, R>`: a `resume` writes the `Send` into the frame, calls
    /// the machine, and reads the payload back, and every coroutine with the
    /// same three types has the same frame head. Per *function* would be copies
    /// of one rule.
    pub(super) fn emit_coroutine_members(&mut self) {
        if self.coroutine_frames.is_empty() {
            return;
        }
        let frames: Vec<CoroutineFrame> = self.coroutine_frames.values().cloned().collect();
        for frame in frames {
            if frame.resume_symbol.is_some()
                && let Err(error) = self.emit_coroutine_resume(&frame)
            {
                self.diagnostics.push(error.to_diagnostic());
            }
            if frame.result_symbol.is_some()
                && let Err(error) = self.emit_coroutine_result(&frame)
            {
                self.diagnostics.push(error.to_diagnostic());
            }
        }
    }

    /// Emits the `resume` for one coroutine type.
    ///
    /// - `frame`: what the entry recorded about the machine and the frame
    fn emit_coroutine_resume(&mut self, frame: &CoroutineFrame) -> Result<(), CodegenError> {
        let context = self.backend.context();
        let mapper = TypeMapper::new(context);
        let pointer = context.ptr_type(inkwell::AddressSpace::default());
        let Some(coroutine_llvm) = frame.coroutine_llvm else {
            return Ok(());
        };
        // The declaration's own answer: `?Yield`, without the exception result
        // struct, because `resume` neither raises nor reports one.
        let Some(optional_ty) = frame.resume_returns.clone() else {
            return Ok(());
        };
        // A `Yield` of `void` has no optional -- `?void` would hold a value that
        // has no representation -- so that spelling answers nothing at all and
        // the declaration says `void`. What a `resume` *takes* is decided by
        // `Send` and what it *gives* by `Yield`; the two are independent, so a
        // coroutine may take nothing in and hand nothing back, or one of each.
        let yields_value = frame.yield_llvm.is_some();
        let optional_llvm: Option<inkwell::types::BasicTypeEnum<'static>> = if yields_value {
            let Some(mapped) = mapper.map(&optional_ty) else {
                // The answer's layout comes from the declaration's type, and a
                // type whose member is an array cannot be rebuilt from the name
                // alone. Saying so is what keeps a wrong guess out of the
                // module.
                self.report_waiting_for_spec(
                    fallc_span::Span::SYNTHETIC,
                    "a coroutine whose `Yield` has no named layout of its own",
                );
                return Ok(());
            };
            Some(mapped)
        } else {
            // `?Yield` degrades to `bool` when `Yield` is `void`: whether the
            // coroutine suspended again.
            Some(context.bool_type().into())
        };
        // `resume` never raises: its answer is the optional and nothing else.
        let Some(symbol) = frame.resume_symbol.clone() else {
            return Ok(());
        };
        let mut params: Vec<inkwell::types::BasicMetadataTypeEnum<'static>> = vec![pointer.into()];
        if frame.takes_value
            && let Some(send_llvm) = mapper.map(&frame.send_ty)
        {
            params.push(send_llvm.into());
        }
        // The declaration the call site already made is the authority on the
        // function's shape: it was built from the same signature, and giving
        // the same symbol a second, differently-shaped declaration is what
        // makes a module fail to verify. A program that never calls `resume`
        // has no declaration, so one is built from the same types.
        let fn_type = match self.backend.module().get_function(&symbol) {
            Some(existing) => existing.get_type(),
            None => match optional_llvm {
                Some(answered) => answered.fn_type(&params, false),
                // `void`: nothing is returned, and `void` is not one of the
                // basic value types a `fn_type` builder starts from.
                None => context.void_type().fn_type(&params, false),
            },
        };
        let function = self
            .backend
            .module()
            .get_function(&symbol)
            .unwrap_or_else(|| self.backend.module().add_function(&symbol, fn_type, None));
        // Weak, because the symbol is determined by the coroutine's three
        // types and not by which function was declared: the library carries a
        // coroutine of its own so that the executor links on its own, and a
        // program that declares one is the definition that should run. Both read
        // the same fixed head of the frame -- state, tag, payload, send,
        // exception -- so either implementation is correct for either coroutine.
        function.set_linkage(inkwell::module::Linkage::WeakAny);
        // The body is added to the declaration the call site already emitted,
        // and a declaration cannot be given a body twice.
        if function.count_basic_blocks() > 0 {
            return Ok(());
        }

        let entry = context.append_basic_block(function, "resume.entry");
        let builder = context.create_builder();
        builder.position_at_end(entry);
        let object = function
            .get_nth_param(0)
            .expect("resume takes its receiver")
            .into_pointer_value();
        let frame_slot = build_field_gep(&builder, coroutine_llvm, object, 0)?;
        let body_slot = build_field_gep(&builder, coroutine_llvm, object, 1)?;
        let frame_address = builder
            .build_load(pointer, frame_slot, "frame")
            .map_err(|error| CodegenError::Emit(error.to_string()))?
            .into_pointer_value();
        let body_address = builder
            .build_load(pointer, body_slot, "body")
            .map_err(|error| CodegenError::Emit(error.to_string()))?
            .into_pointer_value();

        // The machine runs on every resumption, from whatever state the frame
        // records -- state 0 starts the body, and any other state continues it
        // after the suspension it stopped at. What the call answers is the
        // value the machine leaves behind, which is why the run comes first and
        // the reading second.
        let machine_ty = context.void_type().fn_type(&[pointer.into()], false);
        builder
            .build_indirect_call(machine_ty, body_address, &[frame_address.into()], "")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;

        // The value handed in is stored *after* the machine runs: the
        // continuation a suspension jumps to consumes the value the *previous*
        // resume brought, so writing this call's before the run would make it
        // overwrite the one still waiting to be used.
        if frame.takes_value
            && let Some(sent) = function.get_nth_param(1)
        {
            let send_slot = builder
                .build_struct_gep(frame.frame_ty, frame_address, frame.send_field, "send")
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
            // A `Send` that owns something is *constructed* into the frame by
            // its copy constructor: storing its bytes would make the frame and
            // the caller own one buffer, and both would free it.
            let owned = self.type_needs_drop(&frame.send_ty)
                || self.drop_symbol_for(&frame.send_ty).is_some();
            // The slot may already hold a value the body has not taken yet --
            // the machine runs before this store, so a resumed body has consumed
            // the previous one, but a coroutine that has not started has not.
            // Writing a constructed value over an unreleased one would leak the
            // buffer it owned.
            let earlier_state = builder
                .build_struct_gep(
                    frame.frame_ty,
                    frame_address,
                    frame.state_field,
                    "state.before",
                )
                .ok()
                .and_then(|slot| {
                    builder
                        .build_load(context.i32_type(), slot, "state.before")
                        .ok()
                });
            let earlier_state: Option<inkwell::values::IntValue<'static>> = earlier_state.and_then(
                |value: inkwell::values::BasicValueEnum<'static>| match value {
                    inkwell::values::BasicValueEnum::IntValue(int) => Some(int),
                    _ => None,
                },
            );
            if owned
                && let Some(destructor) = self.drop_symbol_for(&frame.send_ty)
                && let Some(earlier_state) = earlier_state
            {
                let started = builder
                    .build_int_compare(
                        inkwell::IntPredicate::NE,
                        earlier_state,
                        context.i32_type().const_zero(),
                        "send.used",
                    )
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
                let release = context.append_basic_block(function, "send.release");
                let store_block = context.append_basic_block(function, "send.store");
                builder
                    .build_conditional_branch(started, release, store_block)
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
                builder.position_at_end(release);
                builder
                    .build_call(
                        self.resolved_function(
                            &destructor,
                            "a destructor",
                            fallc_span::Span::SYNTHETIC,
                        )?,
                        &[send_slot.into()],
                        "send.drop",
                    )
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
                builder
                    .build_unconditional_branch(store_block)
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
                builder.position_at_end(store_block);
            }
            if owned && let Some(constructor) = self.parameter_copy_constructor(&frame.send_ty) {
                let source = builder
                    .build_alloca(sent.get_type(), "send.source")
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
                builder
                    .build_store(source, sent)
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
                builder
                    .build_call(constructor, &[send_slot.into(), source.into()], "send.copy")
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
            } else {
                builder
                    .build_store(send_slot, sent)
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
            }
        }

        // What came back: the tag says whether the machine suspended with a
        // value or finished, and only a suspension is a value to hand over.
        let tag_slot = builder
            .build_struct_gep(frame.frame_ty, frame_address, frame.tag_field, "tag")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        let tag = builder
            .build_load(context.i8_type(), tag_slot, "tag.value")
            .map_err(|error| CodegenError::Emit(error.to_string()))?
            .into_int_value();
        let suspended = builder
            .build_int_compare(
                inkwell::IntPredicate::EQ,
                tag,
                context.i8_type().const_int(PAYLOAD_YIELDED, false),
                "suspended",
            )
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        let present_block = context.append_basic_block(function, "resume.present");
        let absent_block = context.append_basic_block(function, "resume.absent");
        builder
            .build_conditional_branch(suspended, present_block, absent_block)
            .map_err(|error| CodegenError::Emit(error.to_string()))?;

        builder.position_at_end(present_block);
        if !yields_value {
            // A bare `yield` still suspends, so the answer is whether it did:
            // `true` is `?Yield`'s `Some(())`, `false` is its `none`. This is
            // what lets a queue run a coroutine in a loop -- `while
            // (task.resume())` -- without asking `result()` whether it has
            // finished.
            let yes = context
                .bool_type()
                .const_int(1, false)
                .as_basic_value_enum();
            builder
                .build_return(Some(&yes))
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
            builder.position_at_end(absent_block);
            let no = context
                .bool_type()
                .const_int(0, false)
                .as_basic_value_enum();
            builder
                .build_return(Some(&no))
                .map_err(|error| CodegenError::Emit(error.to_string()))?;
            return Ok(());
        }
        let payload_ty = frame.yield_llvm.ok_or_else(|| {
            CodegenError::Emit("internal error: a coroutine has no `Yield` type".to_owned())
        })?;
        let payload_slot = builder
            .build_struct_gep(
                frame.frame_ty,
                frame_address,
                frame.payload_field,
                "payload",
            )
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        let wide = builder
            .build_load(frame.payload_llvm, payload_slot, "payload.wide")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        let yielded = narrow_payload(&builder, wide, payload_ty)?;
        // The answer is a value of the caller's own: a payload that owns
        // something is copied out, so the frame keeps its copy and the caller
        // keeps a different buffer. Handing over the frame's bytes would give
        // two owners one pointer, and both would free it.
        // The optional is *built*, not assembled: its own constructors own the
        // value they put in, and its destructor releases it. Writing the fields
        // here would make the caller's optional a copy of a value the frame
        // still owned -- two owners of one buffer.
        // Reaching here means `Yield` is a real type, so the answer's layout is
        // the optional's.
        let answered_llvm = optional_llvm.ok_or_else(|| {
            CodegenError::Emit("internal error: a coroutine has no answer layout".to_owned())
        })?;
        let present = build_optional(&builder, answered_llvm, Some(yielded))?;
        builder
            .build_return(Some(&present))
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        builder.position_at_end(absent_block);
        let absent = build_optional(&builder, answered_llvm, None)?;
        builder
            .build_return(Some(&absent))
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        Ok(())
    }

    /// Emits the `result` for one coroutine type.
    ///
    /// - `frame`: what the entry recorded about the machine and the frame
    fn emit_coroutine_result(&mut self, frame: &CoroutineFrame) -> Result<(), CodegenError> {
        let context = self.backend.context();
        let mapper = TypeMapper::new(context);
        let pointer = context.ptr_type(inkwell::AddressSpace::default());
        let Some(coroutine_llvm) = frame.coroutine_llvm else {
            return Ok(());
        };
        // A `Return` of `void` answers nothing: the result still carries
        // whether asking succeeded, and no value beside it.
        let value_llvm = if frame.return_ty.is_void() {
            None
        } else {
            let Some(mapped) = mapper.map(&frame.return_ty) else {
                return Ok(());
            };
            Some(mapped)
        };
        let Some(result_llvm) = self.throwing_result_type(value_llvm) else {
            return Ok(());
        };
        let Some(symbol) = frame.result_symbol.clone() else {
            return Ok(());
        };
        let fn_type = result_llvm.fn_type(&[pointer.into()], false);
        let function = self
            .backend
            .module()
            .get_function(&symbol)
            .unwrap_or_else(|| self.backend.module().add_function(&symbol, fn_type, None));
        // Weak, because the symbol is determined by the coroutine's three
        // types and not by which function was declared: the library carries a
        // coroutine of its own so that the executor links on its own, and a
        // program that declares one is the definition that should run. Both read
        // the same fixed head of the frame -- state, tag, payload, send,
        // exception -- so either implementation is correct for either coroutine.
        function.set_linkage(inkwell::module::Linkage::WeakAny);
        if function.count_basic_blocks() > 0 {
            return Ok(());
        }

        let entry = context.append_basic_block(function, "result.entry");
        let builder = context.create_builder();
        builder.position_at_end(entry);
        let object = function
            .get_nth_param(0)
            .expect("result takes its receiver")
            .into_pointer_value();
        let frame_slot = build_field_gep(&builder, coroutine_llvm, object, 0)?;
        let frame_address = builder
            .build_load(pointer, frame_slot, "frame")
            .map_err(|error| CodegenError::Emit(error.to_string()))?
            .into_pointer_value();
        let tag_slot = builder
            .build_struct_gep(frame.frame_ty, frame_address, frame.tag_field, "tag")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        let tag = builder
            .build_load(context.i8_type(), tag_slot, "tag.value")
            .map_err(|error| CodegenError::Emit(error.to_string()))?
            .into_int_value();
        let finished = builder
            .build_int_compare(
                inkwell::IntPredicate::EQ,
                tag,
                context.i8_type().const_int(PAYLOAD_RETURNED, false),
                "finished",
            )
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        let value_block = context.append_basic_block(function, "result.value");
        let check_raised = context.append_basic_block(function, "result.check");
        let failed_block = context.append_basic_block(function, "result.failed");
        let raise_block = context.append_basic_block(function, "result.unfinished");
        // Three ways the body can have ended: it returned a value, it raised, or
        // it has not finished at all.
        builder
            .build_conditional_branch(finished, value_block, check_raised)
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        builder.position_at_end(check_raised);
        let raised = builder
            .build_int_compare(
                inkwell::IntPredicate::EQ,
                tag,
                context.i8_type().const_int(PAYLOAD_RAISED, false),
                "raised",
            )
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        builder
            .build_conditional_branch(raised, failed_block, raise_block)
            .map_err(|error| CodegenError::Emit(error.to_string()))?;

        // The body raised: answer with that exception rather than with a value.
        builder.position_at_end(failed_block);
        let exception_slot = builder
            .build_struct_gep(
                frame.frame_ty,
                frame_address,
                frame.exception_field,
                "exception",
            )
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        let Some(exception_llvm) = self.exception_type() else {
            return Ok(());
        };
        let waiting = builder
            .build_load(exception_llvm, exception_slot, "exception.value")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        let not_ok = context
            .bool_type()
            .const_int(0, false)
            .as_basic_value_enum();
        let failed = build_throwing_result(&builder, result_llvm, not_ok, None, Some(waiting))?;
        builder
            .build_return(Some(&failed))
            .map_err(|error| CodegenError::Emit(error.to_string()))?;

        builder.position_at_end(value_block);
        // A `Return` of `void` has nothing to read out of the payload: the
        // answer says only that the body has finished.
        let returned = match value_llvm {
            Some(value_llvm) => {
                let payload_slot = builder
                    .build_struct_gep(
                        frame.frame_ty,
                        frame_address,
                        frame.payload_field,
                        "payload",
                    )
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
                let wide = builder
                    .build_load(frame.payload_llvm, payload_slot, "payload.wide")
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
                Some(narrow_payload(&builder, wide, value_llvm)?)
            }
            None => None,
        };
        let ok = context
            .bool_type()
            .const_int(1, false)
            .as_basic_value_enum();
        let done = build_throwing_result(&builder, result_llvm, ok, returned, None)?;
        builder
            .build_return(Some(&done))
            .map_err(|error| CodegenError::Emit(error.to_string()))?;

        // Asking before the coroutine has finished has no answer, so it raises
        // `NoneError` -- the exception an absent `?T` raises, which is what
        // "there is no value yet" means everywhere else in the library.
        builder.position_at_end(raise_block);
        let text_type = fallc_sema::Type::Pointer {
            pointee: Box::new(fallc_sema::Type::C8),
            pointee_is_const: true,
            pointer_is_const: false,
        };
        let name = self.text_constant_pointer("NoneError");
        let message = self.text_constant_pointer("the coroutine has not finished");
        let build = self.std_function(
            "__ft_exception",
            &[text_type.clone(), text_type],
            fallc_span::Span::SYNTHETIC,
        )?;
        let raised = builder
            .build_call(build, &[name.into(), message.into()], "exception")
            .map_err(|error| CodegenError::Emit(error.to_string()))?
            .try_as_basic_value()
            .basic()
            .ok_or_else(|| {
                CodegenError::Emit("internal error: `__ft_exception` returns no value".to_owned())
            })?;
        let not_ok = context
            .bool_type()
            .const_int(0, false)
            .as_basic_value_enum();
        let failed = build_throwing_result(&builder, result_llvm, not_ok, None, Some(raised))?;
        builder
            .build_return(Some(&failed))
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        Ok(())
    }
}

/// Inserts one value into an aggregate's field, keeping the aggregate's type.
///
/// - `builder`: where the instruction is emitted
/// - `aggregate`: the aggregate being built
/// - `value`: the field's value
/// Inserts one value into an aggregate's field, keeping the aggregate's type.
///
/// - `builder`: where the instruction is emitted
/// - `aggregate`: the aggregate being built
/// - `value`: the field's value
/// - `index`: the field's position
/// - `name`: the instruction's name
fn insert_field(
    builder: &Builder<'static>,
    aggregate: inkwell::values::StructValue<'static>,
    value: BasicValueEnum<'static>,
    index: u32,
    name: &str,
) -> Result<inkwell::values::StructValue<'static>, CodegenError> {
    match builder
        .build_insert_value(aggregate, value, index, name)
        .map_err(|error| CodegenError::Emit(error.to_string()))?
    {
        inkwell::values::AggregateValueEnum::StructValue(field) => Ok(field),
        _ => Err(CodegenError::Emit(format!(
            "internal error: `{name}` did not produce an aggregate"
        ))),
    }
}

/// Builds an `Optional<T>` value with or without its payload.
///
/// - `builder`: where the value is built
/// - `optional_llvm`: the optional's own layout
/// - `value`: the payload, or `None` for an absent optional
fn build_optional(
    builder: &Builder<'static>,
    optional_llvm: inkwell::types::BasicTypeEnum<'static>,
    value: Option<BasicValueEnum<'static>>,
) -> Result<BasicValueEnum<'static>, CodegenError> {
    let inkwell::types::BasicTypeEnum::StructType(struct_ty) = optional_llvm else {
        return Err(CodegenError::Emit(
            "internal error: an optional is not a struct".to_owned(),
        ));
    };
    let mut aggregate = struct_ty.get_undef();
    let present = struct_ty.get_field_types()[0]
        .into_int_type()
        .const_int(u64::from(value.is_some()), false);
    aggregate = insert_field(builder, aggregate, present.into(), 0, "present")?;
    if let Some(value) = value {
        aggregate = insert_field(builder, aggregate, value, 1, "value")?;
    }
    Ok(aggregate.as_basic_value_enum())
}

/// Builds a `??` function's result: the tag, the value, and the exception.
///
/// - `builder`: where the value is built
/// - `result_llvm`: the result struct's layout
/// - `ok`: whether the answer is a value rather than an exception
/// - `value`: the value, when the answer is one
/// - `error`: the exception, when the answer is one
pub(super) fn build_throwing_result(
    builder: &Builder<'static>,
    result_llvm: inkwell::types::BasicTypeEnum<'static>,
    ok: BasicValueEnum<'static>,
    value: Option<BasicValueEnum<'static>>,
    error: Option<BasicValueEnum<'static>>,
) -> Result<BasicValueEnum<'static>, CodegenError> {
    let inkwell::types::BasicTypeEnum::StructType(struct_ty) = result_llvm else {
        return Err(CodegenError::Emit(
            "internal error: a result struct is not a struct".to_owned(),
        ));
    };
    let mut aggregate = struct_ty.get_undef();
    aggregate = insert_field(builder, aggregate, ok, 0, "ok")?;
    if let Some(value) = value {
        aggregate = insert_field(builder, aggregate, value, 1, "value")?;
    }
    if let Some(error) = error {
        // The exception is the last field whether or not the result carries a
        // value: `{ ok, value?, error }`.
        let index = struct_ty.count_fields() - 1;
        aggregate = insert_field(builder, aggregate, error, index, "error")?;
    }
    Ok(aggregate.as_basic_value_enum())
}

/// Narrows a payload value to the type it stands for.
///
/// - `builder`: where the conversion is emitted
/// - `value`: the payload read from the frame
/// - `target`: the type the value belongs to
fn narrow_payload(
    builder: &Builder<'static>,
    value: BasicValueEnum<'static>,
    target: inkwell::types::BasicTypeEnum<'static>,
) -> Result<BasicValueEnum<'static>, CodegenError> {
    if value.get_type() == target {
        return Ok(value);
    }
    match (value, target) {
        (BasicValueEnum::IntValue(int), inkwell::types::BasicTypeEnum::IntType(target)) => {
            if target.get_bit_width() <= int.get_type().get_bit_width() {
                builder
                    .build_int_truncate(int, target, "payload.value")
                    .map(BasicValueEnum::IntValue)
                    .map_err(|error| CodegenError::Emit(error.to_string()))
            } else {
                builder
                    .build_int_z_extend(int, target, "payload.value")
                    .map(BasicValueEnum::IntValue)
                    .map_err(|error| CodegenError::Emit(error.to_string()))
            }
        }
        // A payload slot holds the larger of `Yield` and `Return`; when the two
        // are of different kinds -- an `i64` yielded and an `f64` returned --
        // the value read back is converted rather than reinterpreted.
        // One slot holds both halves of the coroutine's pair, so a value of the
        // other kind is stored as its *bits* and read back as its bits: the
        // slot is a place, not an arithmetic type, and converting between an
        // integer and a float here would change which bits are stored rather
        // than which type they are read as.
        (BasicValueEnum::IntValue(int), inkwell::types::BasicTypeEnum::FloatType(target))
            if int.get_type().get_bit_width() == target.get_bit_width() =>
        {
            builder
                .build_bit_cast(int, target, "payload.bits")
                .map_err(|error| CodegenError::Emit(error.to_string()))
        }
        (BasicValueEnum::FloatValue(float), inkwell::types::BasicTypeEnum::IntType(target))
            if float.get_type().get_bit_width() == target.get_bit_width() =>
        {
            builder
                .build_bit_cast(float, target, "payload.bits")
                .map_err(|error| CodegenError::Emit(error.to_string()))
        }
        (BasicValueEnum::FloatValue(float), inkwell::types::BasicTypeEnum::FloatType(target))
            if float.get_type() != target =>
        {
            // A `f64` slot holding a `f32` widens, and a `f32` slot holding a
            // `f64` narrows; the direction is the two types' own order.
            if target.get_context() == float.get_type().get_context() && target != float.get_type()
            {
                builder
                    .build_float_ext(float, target, "payload.value")
                    .map(BasicValueEnum::FloatValue)
                    .map_err(|error| CodegenError::Emit(error.to_string()))
            } else {
                builder
                    .build_float_trunc(float, target, "payload.value")
                    .map(BasicValueEnum::FloatValue)
                    .map_err(|error| CodegenError::Emit(error.to_string()))
            }
        }
        (value, _) => Ok(value),
    }
}

impl<'ctx> Lowerer<'ctx> {
    /// Builds a `yield`ed value with the constructor the checker chose.
    ///
    /// The rule is the one `return` uses: a value written under a `yield` that
    /// is not already the `Yield` type may be built by that type's own
    /// `__init__`, and the checker records which constructor answers. The call
    /// fills a temporary, and the temporary is what the frame stores.
    ///
    /// - `ast`: the tree the value is in
    /// - `types`: the checked types
    /// - `builder`: where the call is emitted
    /// - `value`: the expression under the `yield`
    /// - `symbol`: the constructor the checker selected
    /// - `yield_ty`: what the coroutine hands over
    /// - `span`: where the `yield` is written
    fn lower_constructed_yield(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        value: fallc_ast::ExprId,
        symbol: &str,
        yield_ty: &fallc_sema::Type,
        span: Span,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        let mapper = TypeMapper::new(self.backend.context());
        let Some(llvm_ty) = mapper.map(yield_ty) else {
            return Err(CodegenError::waiting_for_spec(
                "a `yield` of this type",
                span,
            ));
        };
        let function = self
            .current_function
            .expect("a coroutine body is being lowered");
        let entry = function
            .get_first_basic_block()
            .expect("a coroutine body has an entry block");
        let slot = self.create_entry_alloca(builder, entry, "yield.value", llvm_ty);
        let callee = match self.backend.module().get_function(symbol) {
            Some(callee) => callee,
            None => {
                let qualified = self
                    .external_symbols
                    .iter()
                    .find(|(_, signature)| signature.symbol == symbol)
                    .map(|(qualified, _)| qualified.clone());
                match qualified {
                    Some(qualified) => {
                        self.declare_external(symbol, &qualified, ast.expr(value).span)?
                    }
                    None => {
                        return Err(CodegenError::Emit(format!(
                            "internal error: `{symbol}` was resolved as a constructor but not \
                             declared"
                        )));
                    }
                }
            }
        };
        // The propagation marker, if any, belongs to the construction: the
        // argument is the value under it.
        let argument = match &ast.expr(value).kind {
            fallc_ast::ExprKind::Propagate(inner) => inner.value,
            _ => value,
        };
        let argument = self.lower_argument(ast, types, builder, argument, ast.expr(value).span)?;
        let call = builder
            .build_call(callee, &[slot.into(), argument], "")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        let _ = self.unwrap_throwing(ast, types, builder, ast.expr(value).span, call)?;
        let built = builder
            .build_load(llvm_ty, slot, "yield.value")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        Ok(Some(built))
    }
}
