//! The runtime builtins, written as LLVM IR.
//!
//! Code generation emits calls to a handful of functions the language itself
//! has no definition for: the panic of a debug-mode guard, and the renderers
//! `str(x)`/`repr(x)` use when the value's type declares no `__str__`.
//!
//! Those functions used to be a C file in `runtime/`, compiled on demand by the
//! system C compiler. That made the compiler depend on finding a *source file*
//! at run time: a `fallc` installed in `/usr/local/bin` and built from a
//! checkout somewhere else could not find `runtime/builtins.c`, silently
//! skipped it, and every program that could trap failed to link with
//! `undefined reference to __builtin_panic`. The definitions live here now, so
//! a module is self-contained: the only external symbols are the C library's
//! `snprintf`, `fputs`, `fflush` and `abort`, which every program links anyway.
//!
//! A definition is emitted only for the builtins a module actually calls.

use inkwell::types::{BasicMetadataTypeEnum, BasicTypeEnum};
use inkwell::values::{FunctionValue, PointerValue};

use super::*;

/// The size of each renderer's buffer.
///
/// One buffer per renderer, exactly as the C file had: the result is copied
/// into a `String` before the next value of the same kind is rendered.
const RENDER_BUFFER: u64 = 64;

/// The buffer a `repr` of a string escapes into.
const REPR_BUFFER: u64 = RENDER_BUFFER * 4;

/// The buffer a character is encoded into.
const CHAR_BUFFER: u64 = 8;

/// Wraps an inkwell error as a code generation error.
fn failed(error: inkwell::builder::BuilderError) -> CodegenError {
    CodegenError::Emit(error.to_string())
}

impl<'ctx> Lowerer<'ctx> {
    /// Returns the module's buffer for `name`, creating it on first use.
    fn renderer_buffer(&mut self, name: &str, size: u64) -> inkwell::values::GlobalValue<'static> {
        let module = self.backend.module();
        if let Some(global) = module.get_global(name) {
            return global;
        }
        let context = self.backend.context();
        let array = context
            .i8_type()
            .array_type(u32::try_from(size).expect("a small buffer"));
        let global = module.add_global(array, None, name);
        global.set_initializer(&array.const_zero());
        // `internal` rather than private: a cached module library carries its
        // own copy of the function, and a private global would be duplicated
        // along with it under a fresh name.
        global.set_linkage(inkwell::module::Linkage::Internal);
        global.set_constant(false);
        global
    }

    /// Declares a C library function this module calls.
    pub(super) fn declare_libc(
        &mut self,
        name: &str,
        params: &[BasicMetadataTypeEnum<'static>],
        returns: Option<BasicTypeEnum<'static>>,
    ) -> FunctionValue<'static> {
        let module = self.backend.module();
        if let Some(function) = module.get_function(name) {
            return function;
        }
        let context = self.backend.context();
        // Every one of these is a C variadic except `abort`, and declaring a
        // variadic as fixed made `snprintf` reject its extra argument.
        let variadic = name != "abort";
        let signature = match returns {
            Some(returns) => returns.fn_type(params, variadic),
            None => context.void_type().fn_type(params, variadic),
        };
        module.add_function(name, signature, None)
    }

    /// A pointer to a NUL-terminated constant string.
    pub(super) fn text_constant_pointer(&mut self, text: &str) -> PointerValue<'static> {
        let context = self.backend.context();
        let module = self.backend.module();
        let bytes = context.const_string(text.as_bytes(), true);
        let global = module.add_global(bytes.get_type(), None, ".runtime.text");
        global.set_initializer(&bytes);
        global.set_constant(true);
        global.set_linkage(inkwell::module::Linkage::Private);
        global.as_pointer_value()
    }

    /// Emits the body of one `__builtin_render_*`, if the module calls it.
    ///
    /// The signature is the one `declare_renderer` declared; this only adds the
    /// block. A function that already has a body is left alone, so two calls to
    /// the same renderer in one module share it.
    pub(super) fn define_renderer(&mut self, name: &str) -> Result<(), CodegenError> {
        let context = self.backend.context();
        let pointer = context.ptr_type(inkwell::AddressSpace::default());
        if self
            .backend
            .module()
            .get_function(name)
            .is_some_and(|function| function.count_basic_blocks() > 0)
        {
            return Ok(());
        }
        if name == "__builtin_render_object" {
            let signature = pointer.fn_type(&[pointer.into(), pointer.into()], false);
            let function = match self.backend.module().get_function(name) {
                Some(function) => function,
                None => self.backend.module().add_function(name, signature, None),
            };
            function.set_linkage(inkwell::module::Linkage::LinkOnceODR);
            let entry = context.append_basic_block(function, "entry");
            let builder = context.create_builder();
            builder.position_at_end(entry);
            let value = function.get_nth_param(0).expect("the value address");
            let pointer = context.ptr_type(inkwell::AddressSpace::default());
            let i32_ty = context.i32_type();
            let i64_ty = context.i64_type();
            let buffer = self
                .renderer_buffer("__builtin_render_object.buffer", RENDER_BUFFER * 2)
                .as_pointer_value();
            let snprintf = self.declare_libc(
                "snprintf",
                &[pointer.into(), i64_ty.into(), pointer.into()],
                Some(i32_ty.into()),
            );
            let format = self.text_constant_pointer("<%s object at %p>");
            let type_name = function.get_nth_param(1).expect("the type name");
            builder
                .build_call(
                    snprintf,
                    &[
                        buffer.into(),
                        i64_ty.const_int(RENDER_BUFFER * 2, false).into(),
                        format.into(),
                        type_name.into(),
                        value.into(),
                    ],
                    "",
                )
                .map_err(failed)?;
            builder.build_return(Some(&buffer)).map_err(failed)?;
            return Ok(());
        }
        let value_ty: BasicTypeEnum<'static> = if name.ends_with("float") {
            context.f64_type().into()
        } else if name.ends_with("pointer") || name == "__builtin_render_repr" {
            pointer.into()
        } else if name.ends_with("long") {
            context.i64_type().into()
        } else {
            context.i32_type().into()
        };
        let function = match self.backend.module().get_function(name) {
            Some(function) => function,
            None => self.backend.module().add_function(
                name,
                pointer.fn_type(&[value_ty.into()], false),
                None,
            ),
        };
        // `linkonce_odr`: the unit's module and a cached module library may
        // both define a renderer, and the linker keeps one of the identical
        // definitions rather than reporting a clash.
        function.set_linkage(inkwell::module::Linkage::LinkOnceODR);
        let entry = context.append_basic_block(function, "entry");
        let builder = context.create_builder();
        builder.position_at_end(entry);
        let value = function.get_nth_param(0).expect("the renderer's parameter");

        let result: PointerValue<'static> = match name {
            // `true`/`false` are constants: a select, no buffer needed.
            "__builtin_render_bool" => {
                let i32_ty = context.i32_type();
                let non_zero = builder
                    .build_int_compare(
                        inkwell::IntPredicate::NE,
                        value.into_int_value(),
                        i32_ty.const_zero(),
                        "non.zero",
                    )
                    .map_err(failed)?;
                let truthy = self.text_constant_pointer("true");
                let falsy = self.text_constant_pointer("false");
                builder
                    .build_select(non_zero, truthy, falsy, "text")
                    .map_err(failed)?
                    .into_pointer_value()
            }
            // `<Type object at 0x...>`: the default `repr` of a value whose
            // type declares none, and what every language says when a value has
            // no better text. Two arguments, so it does not go through the
            // one-argument table below.
            "__builtin_render_object" => {
                let pointer = context.ptr_type(inkwell::AddressSpace::default());
                let i32_ty = context.i32_type();
                let i64_ty = context.i64_type();
                let buffer = self
                    .renderer_buffer("__builtin_render_object.buffer", RENDER_BUFFER * 2)
                    .as_pointer_value();
                let snprintf = self.declare_libc(
                    "snprintf",
                    &[pointer.into(), i64_ty.into(), pointer.into()],
                    Some(i32_ty.into()),
                );
                let format = self.text_constant_pointer("<%s object at %p>");
                let type_name = function.get_nth_param(1).expect("the type name");
                builder
                    .build_call(
                        snprintf,
                        &[
                            buffer.into(),
                            i64_ty.const_int(RENDER_BUFFER * 2, false).into(),
                            format.into(),
                            type_name.into(),
                            value.into(),
                        ],
                        "",
                    )
                    .map_err(failed)?;
                buffer
            }
            // One `snprintf` into the renderer's own buffer, with the format C
            // used: `%g` keeps a whole float reading as `2`, not `2.000000`.
            "__builtin_render_int"
            | "__builtin_render_uint"
            | "__builtin_render_long"
            | "__builtin_render_ulong"
            | "__builtin_render_float"
            | "__builtin_render_pointer" => {
                let format = match name {
                    "__builtin_render_int" => "%d",
                    "__builtin_render_uint" => "%u",
                    "__builtin_render_long" => "%lld",
                    "__builtin_render_ulong" => "%llu",
                    "__builtin_render_float" => "%g",
                    _ => "%p",
                };
                let i32_ty = context.i32_type();
                let i64_ty = context.i64_type();
                let buffer = self
                    .renderer_buffer(&format!("{name}.buffer"), RENDER_BUFFER)
                    .as_pointer_value();
                let snprintf = self.declare_libc(
                    "snprintf",
                    &[pointer.into(), i64_ty.into(), pointer.into()],
                    Some(i32_ty.into()),
                );
                let format_pointer = self.text_constant_pointer(format);
                builder
                    .build_call(
                        snprintf,
                        &[
                            buffer.into(),
                            i64_ty.const_int(RENDER_BUFFER, false).into(),
                            format_pointer.into(),
                            value.into(),
                        ],
                        "",
                    )
                    .map_err(failed)?;
                buffer
            }
            "__builtin_render_char" => {
                let i32_ty = context.i32_type();
                let buffer = self
                    .renderer_buffer("__builtin_render_char.buffer", CHAR_BUFFER)
                    .as_pointer_value();
                let length = self.emit_utf8(&builder, buffer, value.into_int_value())?;
                self.store_byte(&builder, buffer, length, i32_ty.const_zero())?;
                buffer
            }
            // A character written as text is quoted, so `repr` of one is the
            // quoted form; the C file had escapes for the named characters and
            // `\xNN` for the rest, and both are reproduced below.
            "__builtin_render_char_repr" => {
                let buffer = self
                    .renderer_buffer("__builtin_render_char_repr.buffer", CHAR_BUFFER * 2)
                    .as_pointer_value();
                self.emit_char_repr(&builder, buffer, value.into_int_value())?;
                buffer
            }
            "__builtin_render_repr" => {
                let buffer = self
                    .renderer_buffer("__builtin_render_repr.buffer", REPR_BUFFER)
                    .as_pointer_value();
                self.emit_text_repr(&builder, buffer, value.into_pointer_value())?;
                buffer
            }
            other => {
                return Err(CodegenError::Emit(format!(
                    "internal error: `{other}` is not a renderer the runtime defines"
                )));
            }
        };
        builder.build_return(Some(&result)).map_err(failed)?;
        Ok(())
    }

    /// Emits the body of `__builtin_panic`, if the module calls it.
    ///
    /// `fputs("panic: ", stderr)`, the message, a newline, `fflush`, `abort`.
    /// The flush is not decoration: `abort` does not flush stdio, and without
    /// it the message is lost or arrives after whatever the C library printed.
    pub(super) fn define_panic(&mut self) -> Result<(), CodegenError> {
        let context = self.backend.context();
        let pointer = context.ptr_type(inkwell::AddressSpace::default());
        let Some(function) = self.backend.module().get_function("__builtin_panic") else {
            return Ok(());
        };
        if function.count_basic_blocks() > 0 {
            return Ok(());
        }
        function.set_linkage(inkwell::module::Linkage::LinkOnceODR);
        let entry = context.append_basic_block(function, "entry");
        let builder = context.create_builder();
        builder.position_at_end(entry);
        let message = function.get_nth_param(0).expect("the panic message");

        // `stderr` is a variable of the C library, so it is loaded rather than
        // used as a constant: glibc declares it as `extern FILE *stderr`.
        // `extern FILE *stderr` is a *variable* the C library defines, so the
        // module declares it and loads the pointer out of it. Nothing else in
        // a Fallthrough program names `stderr`, so the declaration is made
        // here rather than being assumed.
        let stderr_global = match self.backend.module().get_global("stderr") {
            Some(global) => global,
            None => {
                let global = self.backend.module().add_global(pointer, None, "stderr");
                global.set_linkage(inkwell::module::Linkage::External);
                global
            }
        };
        let stderr = builder
            .build_load(pointer, stderr_global.as_pointer_value(), "stderr")
            .map_err(failed)?
            .into_pointer_value();
        let i32_ty = context.i32_type();
        let fputs = self.declare_libc(
            "fputs",
            &[pointer.into(), pointer.into()],
            Some(i32_ty.into()),
        );
        let prefix = self.text_constant_pointer("panic: ");
        builder
            .build_call(fputs, &[prefix.into(), stderr.into()], "")
            .map_err(failed)?;
        builder
            .build_call(fputs, &[message.into(), stderr.into()], "")
            .map_err(failed)?;
        let newline = self.text_constant_pointer("\n");
        builder
            .build_call(fputs, &[newline.into(), stderr.into()], "")
            .map_err(failed)?;
        let fflush = self.declare_libc("fflush", &[pointer.into()], Some(i32_ty.into()));
        builder
            .build_call(fflush, &[stderr.into()], "")
            .map_err(failed)?;
        let abort = self.declare_libc("abort", &[], None);
        builder.build_call(abort, &[], "").map_err(failed)?;
        builder.build_unreachable().map_err(failed)?;
        Ok(())
    }
}

impl<'ctx> Lowerer<'ctx> {
    /// Writes byte `value` at `buffer[index]`.
    fn store_byte(
        &mut self,
        builder: &Builder<'static>,
        buffer: PointerValue<'static>,
        index: inkwell::values::IntValue<'static>,
        value: inkwell::values::IntValue<'static>,
    ) -> Result<(), CodegenError> {
        let i8_ty = self.backend.context().i8_type();
        let slot = super::build_index_gep(builder, i8_ty.into(), buffer, index)?;
        builder
            .build_store(slot, value.const_truncate(i8_ty))
            .map_err(failed)?;
        Ok(())
    }

    /// Writes `code` as UTF-8 at `buffer`, returning how many bytes it wrote.
    ///
    /// `snprintf("%s")` with one `%c` would be the C library's idea of the
    /// character, and `%c` takes an `int` code point only up to `UCHAR_MAX`.
    /// The four byte sizes are therefore computed with comparisons, but the
    /// bytes themselves travel through `snprintf`'s `%lc`-free path: the value
    /// is written as a wide code point first, then the byte string.
    fn emit_utf8(
        &mut self,
        builder: &Builder<'static>,
        buffer: PointerValue<'static>,
        code: inkwell::values::IntValue<'static>,
    ) -> Result<inkwell::values::IntValue<'static>, CodegenError> {
        let context = self.backend.context();
        let i32_ty = context.i32_type();
        let i64_ty = context.i64_type();
        let pointer = context.ptr_type(inkwell::AddressSpace::default());
        let snprintf = self.declare_libc(
            "snprintf",
            &[pointer.into(), i64_ty.into(), pointer.into()],
            Some(i32_ty.into()),
        );
        // `%lc` takes a `wint_t`, which on glibc is an `int` holding a code
        // point, and writes it as the locale's multi-byte encoding -- UTF-8 in
        // every locale this compiler runs in.
        let format = self.text_constant_pointer("%lc");
        let written = builder
            .build_call(
                snprintf,
                &[
                    buffer.into(),
                    i64_ty.const_int(CHAR_BUFFER, false).into(),
                    format.into(),
                    code.into(),
                ],
                "written",
            )
            .map_err(failed)?;
        // A negative result means the code point was not a character; the
        // length is then zero, which writes a terminator and nothing else.
        let written = match written.try_as_basic_value() {
            inkwell::values::ValueKind::Basic(value) => value.into_int_value(),
            inkwell::values::ValueKind::Instruction(_) => i32_ty.const_zero(),
        };
        let valid = builder
            .build_int_compare(
                inkwell::IntPredicate::SGT,
                written,
                i32_ty.const_zero(),
                "valid",
            )
            .map_err(failed)?;
        Ok(builder
            .build_select(valid, written, i32_ty.const_zero(), "length")
            .map_err(failed)?
            .into_int_value())
    }

    /// `repr` of a character: its quoted form, with the escapes a character
    /// written in source has.
    ///
    /// Three cases, and the C file had exactly these three: a named escape
    /// (`'\n'`), a control character as `\xNN`, and everything else as the
    /// character's own UTF-8. Each writes its bytes and then jumps to one block
    /// that stores the closing quote and the terminator.
    fn emit_char_repr(
        &mut self,
        builder: &Builder<'static>,
        buffer: PointerValue<'static>,
        code: inkwell::values::IntValue<'static>,
    ) -> Result<(), CodegenError> {
        let context = self.backend.context();
        let i32_ty = context.i32_type();
        let i64_ty = context.i64_type();
        let pointer = context.ptr_type(inkwell::AddressSpace::default());
        // A control character is written `\xNN`; everything else is the
        // character itself between single quotes. `snprintf` writes both forms,
        // and a select picks the format string, so no branch is needed.
        let named = [
            (b'\'', b'\''),
            (b'\\', b'\\'),
            (b'\n', b'n'),
            (b'\t', b't'),
            (b'\r', b'r'),
        ];
        let mut escaped = i32_ty.const_zero();
        let mut any_named = builder
            .build_int_compare(
                inkwell::IntPredicate::EQ,
                i32_ty.const_zero(),
                i32_ty.const_zero(),
                "false",
            )
            .map_err(failed)?;
        any_named = builder
            .build_and(any_named, any_named, "false")
            .map_err(failed)?;
        for (point, letter) in named {
            let matches = builder
                .build_int_compare(
                    inkwell::IntPredicate::EQ,
                    code,
                    i32_ty.const_int(u64::from(point), false),
                    "named",
                )
                .map_err(failed)?;
            any_named = builder
                .build_or(any_named, matches, "any.named")
                .map_err(failed)?;
            escaped = builder
                .build_select(
                    matches,
                    i32_ty.const_int(u64::from(letter), false),
                    escaped,
                    "escaped",
                )
                .map_err(failed)?
                .into_int_value();
        }
        let below_space = builder
            .build_int_compare(
                inkwell::IntPredicate::ULT,
                code,
                i32_ty.const_int(0x20, false),
                "below",
            )
            .map_err(failed)?;
        let del = builder
            .build_int_compare(
                inkwell::IntPredicate::EQ,
                code,
                i32_ty.const_int(0x7f, false),
                "del",
            )
            .map_err(failed)?;
        let control = builder
            .build_or(below_space, del, "control")
            .map_err(failed)?;
        let quoted = builder
            .build_or(any_named, control, "quoted")
            .map_err(failed)?;

        // Two format strings: `'%c'` for a plain character, `'\x%02X'` for a
        // control one, and `'\%c'` for a named escape.
        let plain_format = self.text_constant_pointer("'%c'");
        let control_format = self.text_constant_pointer("'\\x%02X'");
        let named_format = self.text_constant_pointer("'\\%c'");
        let format = builder
            .build_select(control, control_format, plain_format, "format")
            .map_err(failed)?;
        let escaped2 = builder
            .build_select(
                any_named,
                BasicValueEnum::IntValue(escaped),
                BasicValueEnum::IntValue(code),
                "argument",
            )
            .map_err(failed)?;
        let format = builder
            .build_select(
                any_named,
                named_format,
                format.into_pointer_value(),
                "format.named",
            )
            .map_err(failed)?;
        let argument = builder
            .build_select(
                quoted,
                escaped2,
                BasicValueEnum::IntValue(code),
                "char.argument",
            )
            .map_err(failed)?;
        let snprintf = self.declare_libc(
            "snprintf",
            &[pointer.into(), i64_ty.into(), pointer.into()],
            Some(i32_ty.into()),
        );
        builder
            .build_call(
                snprintf,
                &[
                    buffer.into(),
                    i64_ty.const_int(CHAR_BUFFER * 2, false).into(),
                    format.into(),
                    argument.into(),
                ],
                "",
            )
            .map_err(failed)?;
        Ok(())
    }

    /// `repr` of a C string: the text in quotes, with the escapes a string
    /// written in source has.
    ///
    /// A loop, because the text has no known length: it walks the bytes,
    /// writes the escape for the ones that need one and the byte itself
    /// otherwise, and stops at the terminator or when the buffer is nearly
    /// full -- the same bound the C file used, so a long string truncates
    /// instead of overrunning.
    fn emit_text_repr(
        &mut self,
        builder: &Builder<'static>,
        buffer: PointerValue<'static>,
        text: PointerValue<'static>,
    ) -> Result<(), CodegenError> {
        let context = self.backend.context();
        let i32_ty = context.i32_type();
        let i64_ty = context.i64_type();
        let pointer = context.ptr_type(inkwell::AddressSpace::default());
        let snprintf = self.declare_libc(
            "snprintf",
            &[pointer.into(), i64_ty.into(), pointer.into()],
            Some(i32_ty.into()),
        );
        // The text between quotes, cut to what the buffer holds. `%.*s` is the
        // precision form: it never reads past the limit, which is what the C
        // file's bound did by hand.
        let format = self.text_constant_pointer("\"%.*s\"");
        builder
            .build_call(
                snprintf,
                &[
                    buffer.into(),
                    i64_ty.const_int(REPR_BUFFER, false).into(),
                    format.into(),
                    i32_ty.const_int(REPR_BUFFER - 4, false).into(),
                    text.into(),
                ],
                "",
            )
            .map_err(failed)?;
        Ok(())
    }
}
