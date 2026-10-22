//! Method calls, constructor calls, and the text conversion behind `str`.
//!
//! A method call passes its receiver as the first argument; a constructor call
//! allocates the receiver and calls `__init__`. Both are ordinary calls once
//! their arguments are in place.

use super::*;

impl<'ctx> Lowerer<'ctx> {
    /// Lowers `T(args)`: allocate the value, then construct into it.
    ///
    /// This is the same call `T x = ...;` makes, with one difference: there is
    /// no binding, so the storage is a temporary of the enclosing function.
    /// The value is loaded out, so the expression has the value the constructor
    /// produced.
    pub(super) fn lower_constructor_call(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        call: &CallExpr,
        symbol: &str,
        span: fallc_span::Span,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        // What is being constructed is the call's own type.
        // The callee names a *type*, which has no type of its own, so what is
        // being built was recorded by the checker.
        let constructed = self
            .constructor_types
            .get(&span)
            .cloned()
            .ok_or_else(|| {
                CodegenError::waiting_for_spec("the type this call constructs", span)
            })?;
        let llvm_ty = TypeMapper::new(self.backend.context())
            .map(&constructed)
            .ok_or_else(|| {
                CodegenError::waiting_for_spec(
                    format!("a value of type `{}`", constructed.spelling()),
                    span,
                )
            })?;
        let entry = self
            .current_function
            .and_then(|function| function.get_first_basic_block())
            .ok_or_else(|| {
                CodegenError::Emit("internal error: no function is being lowered".to_owned())
            })?;
        let storage = self.create_entry_alloca(builder, entry, ".construct", llvm_ty);

        let callee = self.resolved_function(symbol, "a constructor", span)?;
        // The receiver is the first parameter, so the written arguments fill the
        // parameters after it; a named one goes to the parameter it names.
        let mut slots = self.argument_slots(ast, types, builder, call, symbol, span, 1, true)?;
        slots[0] = Some(storage.into());
        let args: Vec<inkwell::values::BasicMetadataValueEnum<'static>> =
            slots.into_iter().flatten().collect();
        let constructed = builder
            .build_call(callee, &args, "construct")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        // A constructor that may raise leaves nothing constructed, so the raise
        // escapes before the temporary is read.
        let _ = self.unwrap_throwing(ast, types, builder, span, constructed)?;
        builder
            .build_load(llvm_ty, storage, "constructed")
            .map(Some)
            .map_err(|error| CodegenError::Emit(error.to_string()))
    }

    /// Lowers a call through a type's `__call__`.
    ///
    /// The callee is the receiver and the written arguments follow it, which is
    /// the same shape as a method call.
    pub(super) fn lower_call_method(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        call: &CallExpr,
        symbol: &str,
        span: fallc_span::Span,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        let function = self.resolved_function(symbol, "`__call__`", span)?;
        let mut args: Vec<inkwell::values::BasicMetadataValueEnum<'static>> =
            Vec::with_capacity(call.args.len() + 1);
        let callee_ty = types.get(call.callee).cloned();
        let callee = self
            .lower_expr(ast, types, builder, call.callee)?
            .ok_or_else(|| CodegenError::waiting_for_spec("this callable", span))?;
        // The receiver is passed by address, so what a call on a *variable* must
        // pass is the variable's own storage, not a copy of it. A `Random` is
        // over two kilobytes of state: a copy taken here made every draw mutate
        // the copy, and `std.random()` returned the same number forever. A
        // reference already is the address, and a temporary gets the copy.
        let already_address = callee_ty
            .as_ref()
            .is_some_and(|ty| ty.is_pointer() || ty.is_reference());
        let names_storage = matches!(
            &ast.expr(call.callee).kind,
            ExprKind::Name(_) | ExprKind::Unary(_) | ExprKind::Field(_) | ExprKind::Index(_)
        );
        let storage = if already_address || !names_storage {
            None
        } else {
            self.lower_lvalue(ast, types, builder, call.callee)
        };
        let receiver = match storage.map(|binding| binding.storage.as_basic_value_enum()) {
            Some(BasicValueEnum::PointerValue(pointer)) => pointer,
            _ => match callee {
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
            },
        };
        args.push(receiver.into());
        for &arg in &call.args {
            args.push(self.lower_argument(ast, types, builder, arg, span)?);
        }
        let value = self.build_value_call(builder, function, symbol, args, span)?;
        self.unwrap_throwing_value(ast, types, builder, span, value)
    }

    /// Lowers `str(x)`/`repr(x)` when the argument's type declares the method.
    ///
    /// The shape is a method call -- the value's address, then nothing else --
    /// because `__str__` is declared with a `this` parameter like any member
    /// function. The checker chose the symbol, so this only takes the address
    /// and emits the call.
    pub(super) fn lower_text_method(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        call: &CallExpr,
        symbol: &str,
        span: fallc_span::Span,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        let callee = match self.backend.module().get_function(symbol) {
            Some(function) => function,
            None => {
                // The method belongs to an imported type, so its definition was
                // compiled in the module that declares it.
                let qualified = self
                    .external_symbols
                    .iter()
                    .find(|(_, signature)| signature.symbol == symbol)
                    .map(|(qualified, _)| qualified.clone())
                    .ok_or_else(|| {
                        CodegenError::Emit(format!(
                            "internal error: `{symbol}` was resolved as `__str__` but is \
                             neither defined nor imported"
                        ))
                    })?;
                self.declare_external(symbol, &qualified, span)?
            }
        };

        let Some(&argument) = call.args.first() else {
            return Err(CodegenError::waiting_for_spec("a text conversion", span));
        };
        let argument_ty = types
            .get(argument)
            .cloned()
            .ok_or_else(|| CodegenError::waiting_for_spec("this value", span))?;

        // The receiver is an address, as in any method call: a pointer is one
        // already, a value has its address taken, and a reference *is* the
        // address of its referent -- `str(r)` for `r: &T` renders the referent.
        // Asking for the reference's own lvalue gave the address of the slot
        // holding the pointer, so `__str__` read a pointer as a string.
        let address: BasicValueEnum<'static> = if argument_ty.is_pointer()
            || argument_ty.is_reference()
        {
            self.lower_expr(ast, types, builder, argument)?
                .ok_or_else(|| CodegenError::waiting_for_spec("this value", span))?
        } else {
            let binding = self
                .lower_lvalue(ast, types, builder, argument)
                .ok_or_else(|| {
                    CodegenError::waiting_for_spec("a value whose text was asked for", span)
                })?;
            binding.storage.as_basic_value_enum()
        };

        let args: Vec<inkwell::values::BasicMetadataValueEnum<'static>> = vec![address.into()];
        let call_site = builder
            .build_call(callee, &args, "text")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        let Some(value) = self.unwrap_throwing(ast, types, builder, span, call_site)? else {
            return Ok(None);
        };
        // The result is stored, so that a member of it can be reached:
        // `repr(x).data()` asks the temporary for its address. A method call
        // returns a value with no storage otherwise.
        let storage = self.store_temporary(builder, value, "text")?;
        self.temporaries.insert(
            span,
            LocalBinding {
                storage,
                ty: value.get_type(),
            },
        );
        Ok(Some(value))
    }

    /// Lowers `str(x)`/`repr(x)` for a value the compiler renders itself.
    ///
    /// # The rule
    ///
    /// A value whose type has no `__str__` is rendered by the compiler, which
    /// is the only thing that knows how a number or a pointer is written. The
    /// result is a `String`, and building one means allocating a buffer -- the
    /// string type's own business, not the backend's.
    ///
    /// So the work is split. A small runtime function turns the value into a C
    /// string, and the `String` is then built from it by calling the very
    /// `__init__(*String, *const c8)` a program would call. That keeps the
    /// layout of `String` in one place: the backend needs only the symbol,
    /// which is emitted here by the same rule as every other function's, and
    /// the runtime needs to know nothing about the type at all.
    pub(super) fn lower_render(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        call: &CallExpr,
        span: fallc_span::Span,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        let Some(&argument) = call.args.first() else {
            return Err(CodegenError::waiting_for_spec("a text conversion", span));
        };
        let mut value = self
            .lower_expr(ast, types, builder, argument)?
            .ok_or_else(|| CodegenError::waiting_for_spec("this value", span))?;
        let mut argument_ty = types
            .get(argument)
            .cloned()
            .ok_or_else(|| CodegenError::waiting_for_spec("this value", span))?;
        // A reference is rendered as what it refers to. Lowering produced the
        // address it holds, so character text is the address itself -- the C
        // string -- and anything else is loaded from it. Without this, `str(r)`
        // on a `&int` rendered the pointer.
        if let Some(referent) = argument_ty.referent().cloned() {
            let is_text = match referent.unqualified() {
                Type::Pointer { pointee, .. } => matches!(
                    pointee.as_ref().unqualified(),
                    Type::Character(fallc_sema::CharType::C8)
                ),
                Type::Array { element, .. } => matches!(
                    element.as_ref().unqualified(),
                    Type::Character(fallc_sema::CharType::C8)
                ),
                _ => false,
            };
            if !is_text
                && let BasicValueEnum::PointerValue(address) = value
                && let Some(llvm) = TypeMapper::new(self.backend.context()).map(&referent)
            {
                value = builder
                    .build_load(llvm, address, "text.value")
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
            }
            argument_ty = referent;
        }

        // The storage the resulting `String` lives in. Its type is the declared
        // string type, which the caller then owns and drops like any other.
        let result_ty = self.standard_string_type(span)?;
        let llvm_result = TypeMapper::new(self.backend.context())
            .map(&result_ty)
            .ok_or_else(|| {
                CodegenError::waiting_for_spec(
                    format!("a value of type `{}`", result_ty.spelling()),
                    span,
                )
            })?;
        let storage = builder
            .build_alloca(llvm_result, "text")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;

        // Step one: the value as a C string, in a buffer the runtime owns.
        //
        // The renderer is chosen by what the value *means* rather than by its
        // machine type: a `bool` is written `true`, a character is written as
        // the character, and an integer keeps its signedness and its width. One
        // entry point per kind, because C has no overloading for the backend to
        // rely on -- and because `%d` on a `u32` with its top bit set prints a
        // negative number.
        //
        // A C string is its own text: it is already the bytes to copy, so
        // `str("hi")` is `hi` rather than the address the literal decays to.
        // `repr` quotes it, which is the only difference the two have there.
        let context = self.backend.context();
        let is_repr = matches!(&ast.expr(call.callee).kind, ExprKind::Name(name) if name == "repr");
        let c_string = match argument_ty.unqualified() {
            Type::Pointer { pointee, .. } => matches!(
                pointee.as_ref().unqualified(),
                Type::Character(fallc_sema::CharType::C8)
            ),
            Type::Array { element, .. } => matches!(
                element.as_ref().unqualified(),
                Type::Character(fallc_sema::CharType::C8)
            ),
            _ => false,
        };
        // An array of characters is text too, once it has an address: a literal
        // is already a pointer by the time it is lowered, and a value -- an
        // array parameter -- is stored here so that its bytes can be read.
        let array_text = if c_string
            && matches!(value, BasicValueEnum::ArrayValue(_))
        {
            Some(self.store_temporary(builder, value, "text.array")?)
        } else {
            None
        };
        let text: BasicValueEnum<'static> = if c_string
            && let Some(text) = array_text.or(match value {
                BasicValueEnum::PointerValue(pointer) => Some(pointer),
                _ => None,
            })
        {
            if is_repr {
                let renderer = self.declare_renderer("__builtin_render_repr", span)?;
                builder
                    .build_call(renderer, &[text.into()], "render")
                    .map_err(|error| CodegenError::Emit(error.to_string()))?
                    .try_as_basic_value()
                    .unwrap_basic()
            } else {
                text.as_basic_value_enum()
            }
        } else {
            let (name, rendered) = match (&value, argument_ty.unqualified().clone()) {
                (BasicValueEnum::IntValue(integer), shape) if shape.is_bool() => {
                    let widened = builder
                        .build_int_z_extend(*integer, context.i32_type(), "flag")
                        .map_err(|error| CodegenError::Emit(error.to_string()))?;
                    ("__builtin_render_bool", widened.into())
                }
                (BasicValueEnum::IntValue(integer), shape) => {
                    let (bits, unsigned, character) = match &shape {
                        Type::Int(int) => (int.bits(), int.is_unsigned(), false),
                        Type::Byte => (8, true, false),
                        Type::Character(character) => (character.bits(), true, true),
                        _ => (32, false, false),
                    };
                    if character {
                        // A character is a code point, and the runtime encodes
                        // it; every width goes through the same entry point,
                        // and `repr` asks for the quoted spelling.
                        let widened = builder
                            .build_int_z_extend(*integer, context.i32_type(), "wide")
                            .map_err(|error| CodegenError::Emit(error.to_string()))?;
                        let renderer = if is_repr {
                            "__builtin_render_char_repr"
                        } else {
                            "__builtin_render_char"
                        };
                        (renderer, widened.into())
                    } else {
                        let (renderer, wider) = if bits > 32 {
                            (
                                if unsigned {
                                    "__builtin_render_ulong"
                                } else {
                                    "__builtin_render_long"
                                },
                                true,
                            )
                        } else if unsigned {
                            ("__builtin_render_uint", false)
                        } else {
                            ("__builtin_render_int", false)
                        };
                        let target = if wider {
                            context.i64_type()
                        } else {
                            context.i32_type()
                        };
                        let converted = if bits < target.get_bit_width() {
                            if unsigned {
                                builder.build_int_z_extend(*integer, target, "wide")
                            } else {
                                builder.build_int_s_extend(*integer, target, "wide")
                            }
                        } else if bits > target.get_bit_width() {
                            builder.build_int_truncate(*integer, target, "narrow")
                        } else {
                            Ok(*integer)
                        };
                        let converted =
                            converted.map_err(|error| CodegenError::Emit(error.to_string()))?;
                        (renderer, converted.into())
                    }
                }
                (BasicValueEnum::FloatValue(float), _) => {
                    // The runtime formats a `double`; a `float` is widened
                    // rather than passed at its own width.
                    let value = if float.get_type() == context.f64_type() {
                        *float
                    } else {
                        builder
                            .build_float_ext(*float, context.f64_type(), "wide")
                            .map_err(|error| CodegenError::Emit(error.to_string()))?
                    };
                    ("__builtin_render_float", value.into())
                }
                (BasicValueEnum::PointerValue(pointer), _) => {
                    ("__builtin_render_pointer", (*pointer).into())
                }
                (
                    BasicValueEnum::ArrayValue(_)
                    | BasicValueEnum::StructValue(_)
                    | BasicValueEnum::VectorValue(_)
                    | BasicValueEnum::ScalableVectorValue(_),
                    _,
                ) => {
                    return Err(CodegenError::waiting_for_spec(
                        format!("rendering a value of type `{}`", argument_ty.spelling()),
                        span,
                    ));
                }
            };
            let renderer = self.declare_renderer(name, span)?;
            builder
                .build_call(renderer, &[rendered], "render")
                .map_err(|error| CodegenError::Emit(error.to_string()))?
                .try_as_basic_value()
                .unwrap_basic()
        };

        // Step two: the `String` that owns a copy of it. The constructor is
        // declared in whatever module defines the string type, so when this
        // module only imports it the declaration comes from the external table
        // -- the same route a method call to an imported type takes.
        let constructor = self.string_from_c_symbol(span)?;
        let callee = match self.backend.module().get_function(&constructor) {
            Some(function) => function,
            None => {
                let qualified = self
                    .external_symbols
                    .iter()
                    .find(|(_, signature)| signature.symbol == constructor)
                    .map(|(qualified, _)| qualified.clone())
                    .ok_or_else(|| {
                        CodegenError::Emit(format!(
                            "internal error: `{constructor}` builds a string from a C string, \
                             but is neither defined nor imported"
                        ))
                    })?;
                self.declare_external(&constructor, &qualified, span)?
            }
        };
        let args: Vec<inkwell::values::BasicMetadataValueEnum<'static>> =
            vec![storage.into(), text.into()];
        builder
            .build_call(callee, &args, "string")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        // The result is remembered as storage, so that a member of it can be
        // reached: `str(x).data()` asks the temporary for its address.
        self.temporaries.insert(
            span,
            LocalBinding {
                storage,
                ty: llvm_result,
            },
        );
        // The *value* is the expression's result, not its address. Returning
        // the address made `String s = str(x);` pass a pointer where the copy
        // constructor declared a `String`.
        builder
            .build_load(llvm_result, storage, "text.value")
            .map(Some)
            .map_err(|error| CodegenError::Emit(error.to_string()))
    }

    /// The declared standard string type.
    ///
    /// Found by its bare name, because the same type is `std.String` in one
    /// file and `String` in another -- and inside `std.string` itself, which
    /// declares it.
    pub(super) fn standard_string_type(&self, span: fallc_span::Span) -> Result<Type, CodegenError> {
        // The unqualified one comes first: a file that imports `std` may have
        // the qualified name in hand, while the module that *declares* the type
        // -- and therefore the one whose symbols the definitions were emitted
        // under -- knows it as the bare `String`. The two name one type and the
        // mangling must agree with the definition, so the bare name wins.
        let mut candidates: Vec<&fallc_sema::NominalType> = self
            .nominal_types
            .values()
            .filter(|def| def.name.rsplit('.').next().unwrap_or(&def.name) == "String")
            .collect();
        candidates.sort_by_key(|def| def.name.contains('.'));
        candidates
            .first()
            .map(|def| Type::Nominal((*def).clone()))
            .ok_or_else(|| {
                CodegenError::waiting_for_spec("the standard string type, which `str` returns", span)
            })
    }

    /// The symbol of the `String` constructor that takes a C string.
    ///
    /// The symbol is built by the same rule every function's is -- the written
    /// name and the mangled parameter types -- because that is what the
    /// definition emitted. Nothing here is a special case: it is `__init__`
    /// with two parameters, and the checker would have produced the same string
    /// for a call to it written in source.
    pub(super) fn string_from_c_symbol(&self, span: fallc_span::Span) -> Result<String, CodegenError> {
        let string_ty = self.standard_string_type(span)?;
        let parameter = Type::Pointer {
            pointee: Box::new(Type::Character(fallc_sema::CharType::C8)),
            pointee_is_const: true,
            pointer_is_const: false,
        };
        // The receiver is `&String`, which is how the definition declares it:
        // a member's `this` is written as a reference in the library, and a
        // reference and a pointer to the same type mangle differently. Building
        // a pointer here named a constructor that does not exist, so rendering
        // a number into a `String` failed with "neither defined nor imported".
        let receiver = Type::Reference {
            referent: Box::new(string_ty),
            referent_is_const: false,
            is_rvalue: false,
        };
        let params = [
            fallc_sema::mangle_type(&receiver),
            fallc_sema::mangle_type(&parameter),
        ];
        // `__init__` is the explicit constructor and `__im_init__` also
        // declares one; the library uses the implicit spelling so that
        // `String s = "text"` works. Prefer whichever this program has.
        let explicit = fallc_ast::special::symbol_for("__init__", &params);
        if self.backend.module().get_function(&explicit).is_some()
            || self
                .external_symbols
                .values()
                .any(|signature| signature.symbol == explicit)
        {
            return Ok(explicit);
        }
        Ok(fallc_ast::special::symbol_for("__im_init__", &params))
    }

    /// Declares a runtime rendering function.
    ///
    /// `const char *__builtin_render_int(int value)`: the buffer is the
    /// runtime's, and the caller copies out of it immediately. That is enough
    /// for a renderer the compiler calls and then forgets, and it keeps the
    /// runtime ignorant of the string type.
    pub(super) fn declare_renderer(
        &mut self,
        name: &str,
        _span: fallc_span::Span,
    ) -> Result<inkwell::values::FunctionValue<'static>, CodegenError> {
        if let Some(function) = self.backend.module().get_function(name) {
            return Ok(function);
        }
        let context = self.backend.context();
        let bytes = context.ptr_type(inkwell::AddressSpace::default());
        // Which value type the renderer takes: one entry point per machine
        // kind, since C has no overloading for the backend to rely on. A
        // pointer is rendered as an address, so it takes one too.
        let value_ty = if name.ends_with("float") {
            context.f64_type().into()
        } else if name.ends_with("pointer")
            || name == "__builtin_render_repr"
        {
            bytes.into()
        } else if name.ends_with("long") {
            // `long long` and `unsigned long long`, which is what the 64-bit
            // renderers take.
            context.i64_type().into()
        } else {
            // `int`, `unsigned`, `bool` and a character's code point are all
            // 32 bits wide.
            context.i32_type().into()
        };
        let signature = bytes.fn_type(&[value_ty], false);
        Ok(self.backend.module().add_function(name, signature, None))
    }

    /// Lowers a method call, passing the receiver as the first argument.
    ///
    /// The receiver is an address in both cases: a pointer receiver already is
    /// one, and a value receiver has its address taken. That is what lets a
    /// method be called on a value and on a pointer with the same syntax.
    pub(super) fn lower_method_call(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        field: &FieldExpr,
        call: &CallExpr,
        span: fallc_span::Span,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        // A call on an interface value dispatches through its table; the
        // checker chose the entry.
        if let Some(info) = self.interface_calls.get(&span).cloned() {
            return self.lower_interface_call(ast, types, builder, field, call, &info, span);
        }
        let name = field.field.as_str();
        // A method's symbol follows the same rule as every other function's --
        // the name, then the mangled parameter types -- so it is built from the
        // receiver's type followed by the arguments'. Looking the plain name up
        // found nothing, which is how this was noticed.
        //
        // A member function's receiver is always a pointer, so the receiver's
        // parameter type is a pointer to whatever the object is.
        // The receiver's declared type is what the definition was mangled
        // with, and it is not recoverable from the object's type: `this *const
        // Point` and `this *Point` are different signatures over the same
        // object. The candidates are therefore tried, and the one that exists
        // is the one that was declared.
        //
        // The checker recorded which definition the call selected, so nothing
        // about the receiver has to be worked out again here.
        let symbol = self.method_symbol(span).ok_or_else(|| {
            CodegenError::waiting_for_spec(
                format!("a call to `{name}`, which is not defined in this module"),
                span,
            )
        })?;
        let callee = match self.backend.module().get_function(&symbol) {
            Some(function) => function,
            None => {
                // The method belongs to a type defined in another module, so
                // its definition was compiled there. This is the declaration
                // that makes the call resolvable at link time, with the
                // signature the defining module exported.
                //
                // A method the defining module calls on its *own* type is in the
                // same position: the body was lowered by whichever module
                // declares the type, and the caller may be that module or
                // another. Either way the external table is where the signature
                // is filed, so it is the one place to look.
                let qualified = self
                    .external_symbols
                    .iter()
                    .find(|(_, signature)| signature.symbol == symbol)
                    .map(|(qualified, _)| qualified.clone())
                    .ok_or_else(|| {
                        CodegenError::waiting_for_spec(
                            format!("a call to `{name}`, which is not defined in this module"),
                            span,
                        )
                    })?;
                self.declare_external(&symbol, &qualified, span)?
            }
        };

        // The receiver as an address.
        let receiver_ty = types
            .get(field.object)
            .cloned()
            // A value read through a reference carries its type in the
            // checker's record of the read: `(*it).data()` on an iterator's
            // `__deref__` result has no entry of its own in the expression
            // table, and the record says what was read.
            .or_else(|| self.deref_reads.get(&ast.expr(field.object).span).cloned())
            // A receiver with no recorded type -- the result of a conversion
            // or another temporary -- is still lowered: the value it produces
            // is stored in a temporary and its address passed, which is what a
            // value receiver wants. The old `?` here failed the whole call for
            // `(bytes as String).data()`.
            .unwrap_or_else(|| fallc_sema::Type::unspecified("", span));
        // A value receiver's address is taken. The value is lowered *first*,
        // because a value the compiler produced -- the result of `str(x)`, say
        // -- only becomes storage while it is being lowered, and its address is
        // what is wanted here. Asking for the address before producing the
        // value found nothing to point at.
        let lowered = self.lower_expr(ast, types, builder, field.object)?;
        let receiver: BasicValueEnum<'static> = if receiver_ty.is_pointer() {
            // Already an address.
            lowered.ok_or_else(|| CodegenError::waiting_for_spec("this receiver", span))?
        } else if receiver_ty.is_reference() {
            // A reference *is* an address, so its value is what to pass -- but
            // only when the expression produced one. `*p` and `a[i]` both name
            // storage and yield the address of it, so that address is the
            // receiver. Using the lowered *value* instead passed a loaded
            // struct where the method takes a pointer: `a[0].show()` handed
            // `show` an `{i32,i32}` and LLVM refused to verify the module.
            //
            // A reference-typed expression that is not one of those -- a
            // reference-returning call, say -- already yields its address as
            // its value, so it needs no special case.
            let names_pointer_storage = match &ast.expr(field.object).kind {
                ExprKind::Unary(unary) if unary.op == UnaryOp::Deref => {
                    // `*p` on a pointer names the pointee's storage; `*it` on a
                    // class calls its `__deref__`, whose value already is the
                    // address the method wants.
                    types
                        .get(unary.operand)
                        .is_some_and(|operand| operand.is_pointer() || operand.is_reference())
                }
                ExprKind::Index(_) => true,
                _ => false,
            };
            let named_storage = names_pointer_storage;
            if named_storage {
                self.lower_lvalue(ast, types, builder, field.object)
                    .ok_or_else(|| {
                        CodegenError::waiting_for_spec("a method call on this dereference", span)
                    })?
                    .storage
                    .as_basic_value_enum()
            } else {
                lowered.ok_or_else(|| CodegenError::waiting_for_spec("this receiver", span))?
            }
        } else {
            // A name whose binding *holds* an address -- a reference or a
            // pointer parameter, such as `this` inside a reference-receiver
            // method -- is passed as that address. Taking the address of its
            // slot instead hands the callee a pointer to the pointer.
            let held_address = match &ast.expr(field.object).kind {
                ExprKind::Name(name) => self.current_scope_mut().get(name).copied(),
                _ => None,
            };
            if let Some(binding) = held_address
                && matches!(
                    binding.ty,
                    inkwell::types::BasicTypeEnum::PointerType(_)
                )
            {
                let loaded = builder
                    .build_load(binding.ty, binding.storage, "receiver")
                    .map_err(|error| CodegenError::Emit(error.to_string()))?;
                return self.finish_method_call(
                    ast, types, builder, field, call, span, callee, symbol, loaded,
                );
            }
            // Only these forms name storage of their own. Asking for the
            // address of anything else means lowering it a second time, and
            // `a.pop().data()` popped twice for exactly that reason.
            let object_span = ast.expr(field.object).span;
            let names_storage = matches!(
                &ast.expr(field.object).kind,
                ExprKind::Name(_) | ExprKind::Unary(_) | ExprKind::Field(_) | ExprKind::Index(_)
            );
            let from_lvalue = if names_storage {
                self.lower_lvalue(ast, types, builder, field.object)
            } else {
                None
            };
            match from_lvalue.or_else(|| self.temporaries.get(&object_span).copied()) {
                Some(binding) => binding.storage.as_basic_value_enum(),
                // A value the lowering produced but did not keep -- a call's
                // result. A method takes its receiver by address, so the value
                // is given one here; it was already lowered above, so this
                // stores what it produced rather than running it again.
                None => {
                    let value = lowered
                        .ok_or_else(|| CodegenError::waiting_for_spec("this receiver", span))?;
                    let storage = self.store_temporary(builder, value, "receiver")?;
                    storage.as_basic_value_enum()
                }
            }
        };

        // Through the shared path, not `lower_expr`: an argument may need its
        // address taken, may adopt a literal's type, or may be an array that
        // decays. Lowering it directly here silently skipped all three, which
        // is how a reference argument was passed as a loaded value.
        //
        // A method's receiver is parameter zero, so the written arguments fill
        // the parameters after it, and a named one goes to the parameter its
        // name says.
        self.finish_method_call(
            ast, types, builder, field, call, span, callee, symbol, receiver,
        )
    }

    /// Places the receiver and the written arguments, then emits the call.
    #[allow(clippy::too_many_arguments)]
    fn finish_method_call(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        field: &FieldExpr,
        call: &CallExpr,
        span: fallc_span::Span,
        callee: inkwell::values::FunctionValue<'static>,
        symbol: String,
        receiver: BasicValueEnum<'static>,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        let _ = field;
        let mut slots =
            self.argument_slots(ast, types, builder, call, &symbol, span, 1, true)?;
        slots[0] = Some(receiver.into());
        self.fill_argument_defaults(ast, types, builder, &symbol, &mut slots, span)?;
        let args: Vec<inkwell::values::BasicMetadataValueEnum<'static>> =
            slots.into_iter().flatten().collect();

        let value = self.build_value_call(builder, callee, &symbol, args, span)?;
        self.unwrap_throwing_value(ast, types, builder, span, value)
    }

}
