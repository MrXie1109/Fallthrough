//! Items: globals, external variables, declarations and function bodies.
//!
//! This is where a declaration becomes an LLVM function or global, which is
//! the point where the language's signatures acquire the ABI the module uses
//! for aggregates and hidden return pointers.

use super::*;

impl<'ctx> Lowerer<'ctx> {
    /// Lowers every top-level item.
    ///
    /// A function becomes a definition, a module-level variable an object
    /// initialised once as part of the program image, and an `extern` variable
    /// a declaration of the library's symbol. A type, constant, enum, trait,
    /// `import`, or `link` produces no code: what it means was settled by the
    /// checker, the loader, or the driver, and nothing is left to compute.
    pub(super) fn lower_items(&mut self, ast: &Ast, types: &TypeTable, bindings: &BindingTypes) {
        // Two passes, because a function may call one declared later. The first
        // pass gives every function its LLVM declaration, so a call site can
        // resolve the symbol whatever order the source is in; the second
        // fills in the bodies.
        //
        // One pass made `.{ .. }` fill a type's fields directly rather than
        // call its `__init__`, because the constructor had not been declared
        // yet when an earlier function was lowered.
        // A module-level variable is emitted in this pass too, so that a
        // function body lowered later can read or write it however the source
        // orders them: `fn read() { return value; }` before `value int = 7;` is
        // legal, as every declaration is order-independent.
        for &item in &ast.items {
            match &ast.item(item).kind {
                ItemKind::Variable(variable) => self.lower_global(ast, bindings, variable),
                // A name whose storage is elsewhere: the module gets a
                // declaration, and the linker finds the object in the library.
                ItemKind::ExternVariable(variable) => self.declare_extern_variable(variable),
                _ => {}
            }
        }
        for &item in &ast.items {
            let ItemKind::Function(function) = &ast.item(item).kind else {
                continue;
            };
            // A template is not a function, so there is nothing to declare: its
            // parameters name types that do not exist. Each *instantiation* is
            // declared and defined instead, under the symbol the checker gave
            // it; see `lower_instantiations`.
            if !function.template_params.is_empty() || function.has_pack() {
                continue;
            }
            // `@enable_if` removed this declaration from the program, and its
            // body was never checked.
            if self.disabled_functions.contains(&function.name_span) {
                continue;
            }
            // A method of a templated type is written once against `T`, and
            // each member of the family is emitted from its own instantiation.
            // Emitting it here would declare a function over a type that does
            // not exist.
            if self.templated_methods.contains(&function.name_span) {
                continue;
            }
            self.declare_function(function, &ast.item(item).attrs);
        }

        for &item in &ast.items {
            let item_ref = ast.item(item);
            match &item_ref.kind {
                // A top-level `if @constexpr` is flattened before checking;
                // its selected declarations appear in the item list directly.
                ItemKind::Conditional(_) => {}
                ItemKind::Function(function) => {
                    // A template's body is emitted once per instantiation, not
                    // here; its own declaration names types that do not exist.
                    // A method of a templated type is the same case: the `T` in
                    // its receiver is a family's parameter, and the member is
                    // the instantiation.
                    if function.template_params.is_empty()
                        && !function.has_pack()
                        && !self.templated_methods.contains(&function.name_span)
                        && !self.disabled_functions.contains(&function.name_span)
                    {
                        self.lower_function(ast, types, bindings, function);
                    }
                }
                // An assertion is checked by the checker; nothing is emitted
                // for it.
                ItemKind::StaticAssert(_) => {}
                // A `__setup__` block is emitted by `lower_setup_blocks`, once
                // every item exists: it may call anything in the program.
                ItemKind::Setup(_) => {}
                // An interface declares signatures and nothing to run.
                ItemKind::Interface(_) => {}
                // An import names another module; its contents are lowered as
                // part of that module, not here.
                ItemKind::Import(_) => {}
                // A link instruction produces no code at all: it is consumed
                // by the driver, which is the part that links.
                ItemKind::Link(_) => {}
                // A type declaration has no code of its own: its layout is
                // produced on demand by the type mapper.
                ItemKind::TypeDeclaration(_) => {}
                // A module-level variable is storage for the whole run, so it
                // becomes a global. Its initialiser is a constant -- the image
                // is laid out before the program runs -- and the checker already
                // folded it into `constant_values`.
                // Emitted in the declaring pass above, so that a body lowered
                // earlier in this pass can still reach it.
                ItemKind::Variable(_) | ItemKind::ExternVariable(_) => {}
                // A constant produces no code either: every use of it was
                // folded into its value while the types were being checked, so
                // there is nothing left to compute at run time. A constant that
                // could not be folded was already reported.
                ItemKind::Const(_) => {}
                // An enum produces no code for the same reason a constant
                // does: each member was folded to its value while the types
                // were being checked, so `Color.RED` is already a number.
                ItemKind::EnumDeclaration(_) => {}
                // A trait declares no code: its requirements are checked where
                // a type is asked about it, and nothing is emitted.
                ItemKind::Trait(_) => {}
                // `Unresolved` is the parser's placeholder for an item it
                // could not parse. The diagnostic was reported there, so there
                // is nothing here to emit.
                ItemKind::Unresolved(_) => {}
            }
        }
    }

    /// Declares a function in the module, without a body.
    ///
    /// Does nothing if the function is already declared, so a symbol defined
    /// once is not declared twice.
    /// Returns the symbol a function is emitted under.
    ///
    /// A `main` that may raise is emitted under another name, because the C
    /// `main` the linker wants returns an `int` and not a result that may carry
    /// an exception. The wrapper is generated once the function exists; see
    /// [`Lowerer::emit_main_wrapper`].
    /// Prefixes a private free function's symbol with this module's name.
    ///
    /// The prefix is what makes two modules' private `helper` functions
    /// distinct symbols. It is applied at emission only: the checker records
    /// the unqualified symbol, and call lowering tries the prefixed form when
    /// the bare one is not defined here, so the two stages agree without the
    /// checker needing the module name.
    fn private_free_symbol(&self, function: &FunctionItem, name: String) -> String {
        let is_method = function
            .params
            .first()
            .is_some_and(|parameter| parameter.name == "this");
        if function.exported
            || function.abi.is_some()
            || is_method
            || function.name == "main"
            || self.current_module.is_empty()
            || !self.private_function_prefix
        {
            return name;
        }
        format!("{}.{}", self.current_module, name)
    }

    pub(super) fn function_symbol(&mut self, function: &FunctionItem) -> String {
        // A C function's name is the symbol, in a declaration and in a
        // definition alike: the other side of the call is the C library, which
        // reads the name as written.
        if function.abi.is_some() {
            return function.name.clone();
        }
        if function.name == "main" {
            // A `main` that may raise is run through the wrapper that reports
            // an escaping exception.
            if function.throws.is_some() {
                return "__ft_main".to_owned();
            }
            // The C entry is `int main(void)`, and it is the only shape that
            // can stand for itself. A `main` that returns nothing, or that
            // takes the two arguments the C runtime hands over, is an ordinary
            // function the wrapper adapts -- as is one whose `__setup__` blocks
            // have to run first.
            //
            // Without this a `void main` was emitted *as* the C entry, and the
            // runtime read an `int` the function never wrote.
            let returns_int = matches!(
                self.resolve_type(&function.return_type),
                fallc_sema::Type::Int(fallc_sema::IntType::I32)
            );
            if self.has_setup || !function.params.is_empty() || !returns_int {
                return "__ft_user_main".to_owned();
            }
        }
        symbol_of(function, |ty| self.resolve_type(ty))
    }

    /// Returns the bytes a string-like literal denotes, as a constant array.
    ///
    /// The three spellings differ exactly in what they carry: a string and a
    /// raw string end in a NUL so they are valid C strings, a byte string does
    /// not, and a raw string decodes no escapes. The NUL is not decoration: a
    /// consumer of a byte pointer reads until it, so a literal without one
    /// would read past the end. The element width follows the literal's prefix
    /// (`u16"ab"` is an array of `i16`), which is why the characters are
    /// encoded to bytes here rather than left to `const_string`.
    ///
    /// Keeping the three in one place means expression lowering and a
    /// module-level initialiser cannot disagree about the bytes.
    pub(super) fn literal_bytes(
        &self,
        kind: &ExprKind,
    ) -> Option<inkwell::values::ArrayValue<'static>> {
        let context = self.backend.context();
        match kind {
            ExprKind::ByteStringLiteral(text) => {
                let bytes = self.byte_string_bytes(text);
                Some(context.const_string(&bytes, false))
            }
            ExprKind::StringLiteral(text) => {
                let elements = self.string_literal_elements(text);
                let width = self.string_literal_width(text);
                Some(match width {
                    16 => {
                        let mut raw = Vec::new();
                        for unit in elements.iter().copied().chain(std::iter::once(0)) {
                            raw.extend_from_slice(&(unit as u16).to_le_bytes());
                        }
                        context.const_string(&raw, false)
                    }
                    _ => {
                        let raw: Vec<u8> = elements
                            .iter()
                            .copied()
                            .chain(std::iter::once(0))
                            .map(|unit| unit as u8)
                            .collect();
                        context.const_string(&raw, false)
                    }
                })
            }
            ExprKind::RawStringLiteral(text) => {
                let mut raw = self.raw_string_bytes(text);
                raw.push(0);
                Some(context.const_string(&raw, false))
            }
            _ => None,
        }
    }

    /// Emits a private global holding a literal's bytes and returns its address.
    ///
    /// A module-level pointer can be initialised with that address because the
    /// address of a global is a constant; a function's use of the same literal
    /// gets the same private object.
    pub(super) fn intern_literal(
        &mut self,
        kind: &ExprKind,
        prefix: &str,
    ) -> Option<inkwell::values::PointerValue<'static>> {
        let bytes = self.literal_bytes(kind)?;
        let global = self.backend.module().add_global(
            bytes.get_type(),
            None,
            &format!("{prefix}.{}", self.string_literal_count),
        );
        self.string_literal_count += 1;
        global.set_initializer(&bytes);
        global.set_constant(true);
        global.set_linkage(inkwell::module::Linkage::Private);
        Some(global.as_pointer_value())
    }

    /// Builds the constant a module-level initialiser denotes.
    ///
    /// Returns `None` for a form this stage cannot turn into an LLVM constant;
    /// the caller reports that rather than leaving the variable silently zeroed.
    pub(super) fn constant_initializer(
        &mut self,
        ast: &Ast,
        expected: &Type,
        llvm: inkwell::types::BasicTypeEnum<'static>,
        init: &fallc_ast::Initializer,
    ) -> Option<BasicValueEnum<'static>> {
        match init {
            fallc_ast::Initializer::Expr(expr) => {
                self.constant_expression(ast, expected, llvm, *expr)
            }
            fallc_ast::Initializer::Designated(designated) => {
                self.constant_elements(ast, expected, llvm, designated)
            }
        }
    }

    /// Arranges an initialiser's values by field, in declaration order.
    ///
    /// A positional value fills the field in its position; a named one fills
    /// the field it names. The result has one entry per field, with `None`
    /// where nothing was given -- which the checker has already refused for a
    /// record, so it only happens for an array or for a type it let through.
    pub(super) fn init_field_values(
        init: &fallc_ast::DesignatedInit,
        fields: &[(String, fallc_sema::Type)],
    ) -> Vec<Option<fallc_ast::ExprId>> {
        let mut slots: Vec<Option<fallc_ast::ExprId>> = vec![None; fields.len()];
        for (index, &element) in init.elements.iter().enumerate() {
            if let Some(slot) = slots.get_mut(index) {
                *slot = Some(element);
            }
        }
        for argument in &init.named {
            if let Some(index) = fields
                .iter()
                .position(|(name, _)| name == &argument.name)
            {
                slots[index] = Some(argument.value);
            }
        }
        slots
    }

    /// Builds a constant record or array from positional elements.
    pub(super) fn constant_elements(
        &mut self,
        ast: &Ast,
        expected: &Type,
        llvm: inkwell::types::BasicTypeEnum<'static>,
        init: &fallc_ast::DesignatedInit,
    ) -> Option<BasicValueEnum<'static>> {
        let context = self.backend.context();
        match expected.unqualified() {
            Type::Array { element, len } => {
                if !matches!(llvm, inkwell::types::BasicTypeEnum::ArrayType(_)) {
                    return None;
                }
                let element_llvm = TypeMapper::new(context).map(element)?;
                let mut values = Vec::with_capacity(usize::try_from(*len).ok()?);
                for expr in &init.elements {
                    if values.len() as u64 >= *len {
                        return None;
                    }
                    values.push(self.constant_expression(ast, element, element_llvm, *expr)?);
                }
                while (values.len() as u64) < *len {
                    values.push(element_llvm.const_zero());
                }
                constant_array(element_llvm, values)
            }
            Type::Nominal(nominal) => {
                let inkwell::types::BasicTypeEnum::StructType(struct_ty) = llvm else {
                    return None;
                };
                let mut values: Vec<BasicValueEnum<'static>> = struct_ty
                    .get_field_types()
                    .into_iter()
                    .map(inkwell::types::BasicTypeEnum::const_zero)
                    .collect();
                // A union's representation is its one storage member, which may
                // be a field other than the first. Writing a positional
                // initialiser into it would need a constant reinterpretation of
                // the value, which LLVM's constant API does not expose here, so
                // only the case where the first field *is* the storage member is
                // built.
                if nominal.is_union && nominal.union_storage != Some(0) {
                    return None;
                }
                for (index, element) in Self::init_field_values(init, &nominal.fields)
                    .into_iter()
                    .enumerate()
                {
                    let element = element?;
                    let (_, field_ty) = nominal.fields.get(index)?;
                    let field_llvm = TypeMapper::new(context).map(field_ty)?;
                    let value = self.constant_expression(ast, field_ty, field_llvm, element)?;
                    let member = usize::try_from(llvm_member_index(nominal, index)).ok()?;
                    if member >= values.len() || values[member].get_type() != value.get_type() {
                        return None;
                    }
                    values[member] = value;
                }
                Some(context.const_struct(&values, false).as_basic_value_enum())
            }
            _ => None,
        }
    }

    /// Builds the constant one expression denotes, when it is a constant.
    pub(super) fn constant_expression(
        &mut self,
        ast: &Ast,
        expected: &Type,
        llvm: inkwell::types::BasicTypeEnum<'static>,
        expr: ExprId,
    ) -> Option<BasicValueEnum<'static>> {
        let node = ast.expr(expr);
        // The checker folds every integer constant expression it can, including
        // a negative literal and a name of a constant. Its record is the first
        // answer, and it covers forms this walk does not repeat.
        if let Some(value) = self.constant_values.get(&node.span).copied() {
            match llvm {
                inkwell::types::BasicTypeEnum::IntType(int) => {
                    return Some(int.const_int(value as u64, false).as_basic_value_enum());
                }
                inkwell::types::BasicTypeEnum::FloatType(float) => {
                    #[allow(clippy::cast_precision_loss)]
                    let value = value as f64;
                    return Some(float.const_float(value).as_basic_value_enum());
                }
                _ => {}
            }
        }
        match &node.kind {
            ExprKind::IntLiteral(literal) => {
                let value = parse_integer_literal(&literal.digits)?;
                match llvm {
                    inkwell::types::BasicTypeEnum::IntType(int) => {
                        Some(int.const_int(value, false).as_basic_value_enum())
                    }
                    inkwell::types::BasicTypeEnum::FloatType(float) => {
                        #[allow(clippy::cast_precision_loss)]
                        let value = value as f64;
                        Some(float.const_float(value).as_basic_value_enum())
                    }
                    _ => None,
                }
            }
            ExprKind::BoolLiteral(value) => match llvm {
                inkwell::types::BasicTypeEnum::IntType(int) => {
                    Some(int.const_int(u64::from(*value), false).as_basic_value_enum())
                }
                _ => None,
            },
            ExprKind::CharLiteral(literal) => {
                let value = decode_char_literal(&literal.text, node.span).ok()?;
                match llvm {
                    inkwell::types::BasicTypeEnum::IntType(int) => {
                        Some(int.const_int(u64::from(value), false).as_basic_value_enum())
                    }
                    _ => None,
                }
            }
            ExprKind::FloatLiteral(literal) => {
                let value: f64 = literal.text.parse().ok()?;
                match llvm {
                    inkwell::types::BasicTypeEnum::FloatType(float) => {
                        Some(float.const_float(value).as_basic_value_enum())
                    }
                    _ => None,
                }
            }
            ExprKind::NullPointer => match llvm {
                inkwell::types::BasicTypeEnum::PointerType(pointer) => {
                    Some(pointer.const_null().as_basic_value_enum())
                }
                _ => None,
            },
            // A string literal is an array of characters in the language; bound
            // to a pointer, its private global's address is the value. Bound to
            // an array, the bytes themselves are the initialiser.
            ExprKind::ByteStringLiteral(_)
            | ExprKind::StringLiteral(_)
            | ExprKind::RawStringLiteral(_) => match llvm {
                inkwell::types::BasicTypeEnum::ArrayType(_) => {
                    self.literal_bytes(&node.kind).map(|bytes| bytes.as_basic_value_enum())
                }
                inkwell::types::BasicTypeEnum::PointerType(_) => self
                    .intern_literal(&node.kind, ".str")
                    .map(|pointer| pointer.as_basic_value_enum()),
                _ => None,
            },
            // A sign in front of a literal is folded here because the checker's
            // record only covers integers. A float constant is rebuilt with the
            // sign applied: LLVM 18 has no constant-fneg helper in this binding.
            ExprKind::Unary(unary) if matches!(unary.op, UnaryOp::Neg | UnaryOp::Plus) => {
                if let ExprKind::FloatLiteral(literal) = &ast.expr(unary.operand).kind {
                    let mut value: f64 = literal.text.parse().ok()?;
                    if unary.op == UnaryOp::Neg {
                        value = -value;
                    }
                    return match llvm {
                        inkwell::types::BasicTypeEnum::FloatType(float) => {
                            Some(float.const_float(value).as_basic_value_enum())
                        }
                        _ => None,
                    };
                }
                let value = self.constant_expression(ast, expected, llvm, unary.operand)?;
                match (unary.op, value) {
                    (UnaryOp::Plus, value) => Some(value),
                    (UnaryOp::Neg, BasicValueEnum::IntValue(value)) => {
                        Some(value.const_neg().as_basic_value_enum())
                    }
                    _ => None,
                }
            }
            ExprKind::NestedInit(init) => self.constant_elements(ast, expected, llvm, init),
            ExprKind::TypedInit(init) => self.constant_elements(ast, expected, llvm, &init.init),
            // A name of an integer constant was answered by `constant_values`
            // above. A name of a float or of a constant aggregate is not
            // recorded in a form this stage can read, so it is reported by the
            // caller instead of being zeroed.
            _ => None,
        }
    }

    /// Emits a module-level variable as a global.
    ///
    /// The initialiser is a constant: a global's bytes are part of the program
    /// image, so nothing may depend on run-time state. The checker folded what
    /// it could, and this builds the LLVM constant of the right type from the
    /// literal or positional initialiser that is left.
    /// A variable with no initialiser is zero-initialised, which is what C's
    /// globals are too.
    pub(super) fn lower_global(
        &mut self,
        ast: &Ast,
        bindings: &fallc_sema::BindingTypes,
        variable: &fallc_ast::BindStmt,
    ) {
        let declared = match &variable.ty {
            fallc_ast::BindingType::Written(ty) => self.resolve_type(ty),
            // The checker deduced it, and filed the answer under the binding's
            // name span -- the same record a local's `auto` uses. Without it a
            // global whose type is `auto` was left unemitted, and every importer
            // of it failed to link.
            fallc_ast::BindingType::Auto(span) => bindings
                .get(variable.name_span)
                .cloned()
                .or_else(|| self.type_of_types.get(span).cloned())
                .unwrap_or_else(|| fallc_sema::Type::unspecified("auto", *span)),
        };
        let llvm = match TypeMapper::new(self.backend.context()).map(&declared) {
            Some(llvm) => llvm,
            None => return,
        };
        let constant: BasicValueEnum<'static> = match &variable.init {
            None => llvm.const_zero(),
            Some(init) => match self.constant_initializer(ast, &declared, llvm, init) {
                Some(value) => value,
                // Reaching here means the checker accepted an initialiser this
                // stage cannot make constant. Zeroing it silently would give the
                // program the wrong bytes, so it is reported instead. A call is
                // the usual example, and the checker rejects it before lowering.
                None => {
                    let span = match init {
                        fallc_ast::Initializer::Expr(expr) => ast.expr(*expr).span,
                        fallc_ast::Initializer::Designated(designated) => designated.span,
                    };
                    self.report_waiting_for_spec(
                        span,
                        "a module-level initialiser of this form",
                    );
                    llvm.const_zero()
                }
            },
        };
        // An imported module may have referred to this variable before the
        // module that defines it was lowered, which declared it under the same
        // name. That declaration *is* this object; adding a second one would
        // give LLVM two names for one variable.
        // The symbol is qualified by the defining module: a module-level
        // variable is one object for the whole program, and a program that
        // declares `out` must not share a symbol with `std.io`'s `out`. The
        // module's own tables keep using the written name; importers use this
        // symbol, which travels in the export table.
        let symbol = format!("{}.{}", self.current_module, variable.name);
        let global = match self.backend.module().get_global(&symbol) {
            Some(existing) => existing,
            None => self.backend.module().add_global(llvm, None, &symbol),
        };
        global.set_initializer(&constant);
        global.set_constant(false);
        // `thread_local` puts the object in thread-local storage: each thread
        // sees its own copy, and the linker resolves it through the platform's
        // TLS model.
        global.set_thread_local(variable.is_thread_local);
        // The symbol is module-qualified, so *every* module-level variable is
        // visible: an instantiation in the importing program names it (the
        // module library is a separate object, and its internal symbols are not
        // there to be found), which is exactly what `std.log._target` needs.
        // `export` controls whether importers may write the variable's *name*,
        // not whether the linker can find its storage.
        global.set_linkage(inkwell::module::Linkage::External);
        let _ = variable.exported;
        self.apply_global_alignment(&variable.name_span, global);
        self.globals.insert(variable.name.clone(), (global, llvm));
    }

    /// Declares a variable that lives in another library.
    ///
    /// The name is the symbol: `extern "C" stdin *c8;` names the `stdin` the C
    /// library exports. Nothing is initialised here, and the object is external,
    /// which is what tells the linker this module only refers to it.
    pub(super) fn declare_extern_variable(&mut self, variable: &fallc_ast::ExternVariableItem) {
        let declared = self.resolve_type(&variable.ty);
        let Some(llvm) = TypeMapper::new(self.backend.context()).map(&declared) else {
            return;
        };
        let global = match self.globals.get(&variable.name).copied() {
            Some((global, _)) => global,
            None => match self.backend.module().get_global(&variable.name) {
                Some(global) => global,
                None => {
                    let global = self
                        .backend
                        .module()
                        .add_global(llvm, None, &variable.name);
                    global.set_linkage(inkwell::module::Linkage::External);
                    global
                }
            },
        };
        // The declaration is one symbol whatever number of modules mention it.
        // Adding it again made LLVM rename the second one to `stdout.1`, and the
        // linker then looked for an object that does not exist.
        self.globals.insert(variable.name.clone(), (global, llvm));
    }

    /// Returns `true` when a value of `ty` is passed as its address rather than
    /// as an LLVM aggregate value.
    ///
    /// This is the convention for an aggregate return -- the hidden slot -- and
    /// for an aggregate parameter declared by value, which is passed as an
    /// address and copied into the callee's own storage. Every array takes this
    /// path, however small, so that the convention depends on the *kind* of the
    /// type rather than on its size. A record does too once it is larger than
    /// [`MAX_RETURN_BY_VALUE`], because the same instruction-selection limit
    /// applies to a large struct. Smaller records keep the value convention:
    /// they travel in registers on every target this backend emits for, and
    /// every existing `??` result struct and standard-library signature that
    /// carries one stays as it was.
    pub(super) fn aggregate_is_by_address(&self, ty: &Type) -> bool {
        let ty = ty.unqualified();
        match ty {
            Type::Array { .. } => true,
            Type::Nominal(_) => {
                let Some(llvm) = TypeMapper::new(self.backend.context()).map(ty) else {
                    return false;
                };
                llvm.size_of()
                    .and_then(|size| size.get_zero_extended_constant())
                    .is_some_and(|bytes| bytes > MAX_RETURN_BY_VALUE)
            }
            _ => false,
        }
    }

    /// Applies the code-generation attributes written on a function.
    ///
    /// `@inline` is LLVM's `alwaysinline` and `@noreturn` is `noreturn`; the
    /// other attributes are questions for the checker and leave no IR behind.
    /// They are attached to the function value, so they appear on the `define`
    /// whichever pass emits it.
    pub(super) fn apply_function_attributes(
        &mut self,
        function: FunctionValue<'static>,
        attrs: &[fallc_ast::Attr],
    ) {
        let context = self.backend.context();
        for attr in attrs {
            let attribute = match attr.name.as_str() {
                "inline" => "alwaysinline",
                "noinline" => "noinline",
                "noreturn" => "noreturn",
                "cold" => "cold",
                // Nothing in the backend warns, but the attribute records the
                // contract: a result the program is not allowed to drop.
                "must_use" => "warn_unused_result",
                _ => continue,
            };
            let kind = inkwell::attributes::Attribute::get_named_enum_kind_id(attribute);
            if kind == 0 {
                continue;
            }
            function.add_attribute(
                inkwell::attributes::AttributeLoc::Function,
                context.create_enum_attribute(kind, 0),
            );
        }
    }

    /// Records what a call site needs to know about a declared function:
    /// which parameters have defaults, and their names and types.
    ///
    /// Kept apart from the LLVM declaration because a function may be declared
    /// twice -- once as an external when a caller in another module reaches it,
    /// and once as its own module's definition -- and the second declaration
    /// must not lose what the first recorded.
    fn record_call_site_metadata(&mut self, function: &FunctionItem, name: &str) {
        self.function_defaults.insert(
            name.to_owned(),
            function.params.iter().map(|parameter| parameter.default).collect(),
        );
        let pre_resolved = self.pre_resolved_params.clone();
        let parameters: Vec<(String, Type)> = function
            .params
            .iter()
            .enumerate()
            .map(|(index, parameter)| {
                let ty = match pre_resolved.get(index) {
                    Some(ty) => ty.clone(),
                    None => self.resolve_type(&parameter.ty),
                };
                (parameter.name.clone(), ty)
            })
            .collect();
        self.function_parameters.insert(name.to_owned(), parameters);
    }

    pub(super) fn declare_function(&mut self, function: &FunctionItem, attrs: &[fallc_ast::Attr]) {
        let name = self
            .current_symbol
            .clone()
            .unwrap_or_else(|| self.function_symbol(function));
        // A private free function is the module's own business, and two
        // modules may each keep a `helper` of the same signature. Its symbol
        // carries the module so the two do not collide at link time; exported
        // functions keep the symbol importers compute from the signature, and
        // a receiver already appears in the mangled name of a method.
        // A body in another module calls this function by the symbol its own
        // check recorded -- the mangled name, before the module prefix. That
        // spelling has to find the one emitted here, so the pair is recorded
        // before the early return for the same reason the emitted symbol is: a
        // second declaration must not hide it.
        let unprefixed = name.clone();
        let name = self.private_free_symbol(function, name);
        if name != unprefixed {
            self.module_function_symbols
                .entry(unprefixed)
                .or_insert_with(|| name.clone());
        }
        // Recorded before the early return, so a second declaration of the same
        // symbol does not hide it from a call.
        if !self.emitted_symbols.contains(&name) {
            self.emitted_symbols.push(name.clone());
        }
        if let Some(existing) = self.backend.module().get_function(&name) {
            self.apply_function_attributes(existing, attrs);
            // The call-site metadata is recorded even when the declaration
            // already exists: a unit's call reaches a library function as an
            // external declaration *before* the library's own bodies are
            // lowered, and returning here skipped the defaults and parameter
            // names that a call in the library's body may omit. The default
            // then had no value to fill its slot with, and the emitted call
            // passed fewer arguments than the callee takes.
            self.record_call_site_metadata(function, &name);
            return;
        }

        let mapper = TypeMapper::new(self.backend.context());
        // An instantiation's return type is the one the checker resolved under
        // its arguments; the written type is one expression in one tree, so
        // reading it again gives whichever member was resolved last.
        let return_type = self
            .pre_resolved_return
            .take()
            .unwrap_or_else(|| self.resolve_type(&function.return_type));
        let returns_nothing = return_type.is_void()
            || matches!(
                &function.return_type,
                TypeSyntax::Named(named) if named.path == "void"
            );
        let value_llvm = if returns_nothing {
            None
        } else {
            match mapper.map(&return_type) {
                Some(llvm) => Some(llvm),
                None => return,
            }
        };
        // An aggregate too big for LLVM's SSA selection is written through a
        // hidden first parameter. A `??` function does not use it: its result
        // struct carries the value inline, and the tag has to come back too, so
        // the hidden slot would leave the result nowhere. Returning the struct
        // by value is the shape that works.
        let out_of_line = !returns_nothing
            && function.throws.is_none()
            && self.aggregate_is_by_address(&return_type);
        // A `??` function returns its value *or* an exception, so the shape of
        // its result is decided by the annotation and by nothing else.
        let return_llvm = if function.throws.is_some() {
            match self.throwing_result_type(value_llvm) {
                Some(llvm) => Some(llvm),
                None => return,
            }
        } else if out_of_line {
            // The value goes through the slot, so there is no return value.
            None
        } else {
            value_llvm
        };

        // A parameter's LLVM type comes from the type the checker resolved when
        // the function is an instantiation: a written parameter may be a pack
        // expansion, `Box<Args...>`, which only the checker can splice.
        let pre_resolved = self.pre_resolved_params.clone();
        let mut sema_parameters = Vec::with_capacity(function.params.len());
        let mut param_types = Vec::with_capacity(function.params.len() + usize::from(out_of_line));
        if out_of_line {
            param_types.push(
                self.backend
                    .context()
                    .ptr_type(inkwell::AddressSpace::default())
                    .into(),
            );
        }
        for (index, param) in function.params.iter().enumerate() {
            let ty = match pre_resolved.get(index) {
                Some(ty) => ty.clone(),
                None => self.resolve_type(&param.ty),
            };
            sema_parameters.push(ty.clone());
            let Some(llvm) = mapper.map(&ty) else { return };
            // A by-value aggregate parameter is passed as an address, like a
            // returned one: an aggregate value of that size cannot be selected.
            // The callee copies into its own storage, so the parameter is still
            // a value to its body.
            let llvm = if self.aggregate_is_by_address(&ty) {
                self.backend
                    .context()
                    .ptr_type(inkwell::AddressSpace::default())
                    .into()
            } else {
                llvm
            };
            param_types.push(llvm.into());
        }

        let fn_type = match return_llvm {
            Some(return_llvm) => {
                inkwell::types::BasicType::fn_type(&return_llvm, &param_types, function.variadic)
            }
            None => self
                .backend
                .context()
                .void_type()
                .fn_type(&param_types, function.variadic),
        };
        let defined = self.backend.module().add_function(&name, fn_type, None);
        self.apply_function_attributes(defined, attrs);
        // The call site needs the convention, because the LLVM signature cannot
        // express it: an out-of-line function and a `void` function both return
        // nothing.
        self.return_kinds.insert(
            name.clone(),
            if out_of_line {
                ReturnKind::OutOfLine(return_type.clone())
            } else {
                ReturnKind::Value
            },
        );
        // Recorded so a call site that omits an argument can supply it. The
        // expression is lowered where it is used, which is what makes a
        // default equivalent to writing the argument at every call.
        self.function_defaults.insert(
            name.clone(),
            function.params.iter().map(|param| param.default).collect(),
        );
        self.function_parameters.insert(
            name.clone(),
            function
                .params
                .iter()
                .map(|param| param.name.clone())
                .zip(sema_parameters.iter().cloned())
                .collect(),
        );

        // Recorded under the name the program writes, because that is what a
        // value-position use spells: `&f` names a function as `f`, and the
        // symbol it was emitted under may be mangled beyond recognition.
        self.function_values
            .entry(function.name.clone())
            .or_default()
            .push((
                name,
                fallc_sema::Type::Function {
                    parameters: sema_parameters,
                    returns: Box::new(return_type),
                    abi: function.abi.clone(),
                    variadic: function.variadic,
                    throws: function.throws.is_some(),
                },
            ));
    }

    /// Lowers one function declaration into an LLVM function.
    ///
    /// The signature is the declared parameters and return type, with one
    /// exception: an array or record larger than 128 bytes is returned through
    /// a hidden first pointer instead of by value, because an aggregate that
    /// large does not fit LLVM's SSA value convention.
    pub(super) fn lower_function(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        bindings: &BindingTypes,
        function: &FunctionItem,
    ) {
        let mapper = TypeMapper::new(self.backend.context());

        // The symbol recorded for this function, if it is an instantiation; the
        // ordinary rule below is what every other function uses.
        let recorded_symbol = self.current_symbol.clone();
        // A lambda's body has no written return type: the checker deduced it,
        // and it is handed over here rather than read from the syntax.
        let return_type = self
            .pre_resolved_return
            .take()
            .unwrap_or_else(|| self.resolve_type(&function.return_type));
        // A function that returns nothing has no LLVM return type at all, which
        // is how both `fn f()` and `fn f() void` are declared.
        let returns_nothing = return_type.is_void()
            || matches!(
                &function.return_type,
                TypeSyntax::Named(named) if named.path == "void"
            );
        let value_llvm = if returns_nothing {
            None
        } else {
            match mapper.map(&return_type) {
                Some(llvm) => Some(llvm),
                None => {
                    self.report_waiting_for_spec(
                        function.return_type.span(),
                        &format!("functions returning `{}`", return_type.spelling()),
                    );
                    return;
                }
            }
        };
        // An aggregate too large for LLVM's SSA selection is written through a
        // hidden first parameter instead of being returned. A `??` function
        // keeps its inline result struct instead, tag and value together.
        let out_of_line = !returns_nothing
            && function.throws.is_none()
            && self.aggregate_is_by_address(&return_type);
        // A `??` function returns `{ ok, value, error }` instead of its value.
        let return_llvm = if function.throws.is_some() {
            match self.throwing_result_type(value_llvm) {
                Some(llvm) => Some(llvm),
                None => {
                    self.report_waiting_for_spec(
                        function.return_type.span(),
                        "exceptions without `import std`",
                    );
                    return;
                }
            }
        } else if out_of_line {
            None
        } else {
            value_llvm
        };

        // Each parameter's LLVM type comes from its declared type -- or, for an
        // instantiation, from the type the checker *resolved* it to. A written
        // type can contain a pack expansion, `Box<Args...>`, which cannot be
        // expanded here: expanding it needs the substitution that only the
        // checker holds, and re-reading the syntax found `Box` alone.
        let pre_resolved = self.pre_resolved_params.clone();
        let mut param_types = Vec::with_capacity(function.params.len() + usize::from(out_of_line));
        if out_of_line {
            param_types.push(
                self.backend
                    .context()
                    .ptr_type(inkwell::AddressSpace::default())
                    .into(),
            );
        }
        for (index, param) in function.params.iter().enumerate() {
            let ty = match pre_resolved.get(index) {
                Some(ty) => ty.clone(),
                None => self.resolve_type(&param.ty),
            };
            let Some(llvm) = mapper.map(&ty) else {
                self.report_waiting_for_spec(
                    param.ty.span(),
                    &format!("a parameter of type `{}`", ty.spelling()),
                );
                return;
            };
            // A by-value aggregate parameter is passed as an address; the
            // callee copies it into the storage below, so the body still sees a
            // value with an address of its own.
            let llvm = if self.aggregate_is_by_address(&ty) {
                self.backend
                    .context()
                    .ptr_type(inkwell::AddressSpace::default())
                    .into()
            } else {
                llvm
            };
            param_types.push(llvm.into());
        }

        // `fn_type` comes from `BasicType`; the enum needs the trait in scope.
        // A variadic declaration takes any number of extra arguments; LLVM
        // records that in the function type so that calls type check.
        let fn_type = match return_llvm {
            Some(return_llvm) => {
                inkwell::types::BasicType::fn_type(&return_llvm, &param_types, function.variadic)
            }
            None => self
                .backend
                .context()
                .void_type()
                .fn_type(&param_types, function.variadic),
        };
        // The declaration was emitted in the first pass, so the symbol exists.
        //
        // An instantiation's symbol is the one the checker recorded instead:
        // two instantiations of one template differ only in their arguments,
        // which the *written* parameters do not yet show, so rebuilding it here
        // would give both the same name.
        let symbol = recorded_symbol
            .clone()
            .unwrap_or_else(|| self.function_symbol(function));
        // The same prefix the declaration got: a private free function is
        // emitted under its module's name, and the body must be added to that
        // function rather than to a second, bare one.
        let symbol = self.private_free_symbol(function, symbol);
        // Recording the mangled name lets a call site find it without
        // repeating the mangling.
        if function.overloaded {
            // Every overload is recorded with its parameter types, so a call
            // site can pick the one the checker chose. A single entry per name
            // would let the last overload win for every call.
            let mapper = TypeMapper::new(self.backend.context());
            let parameter_types: Vec<Type> = function
                .params
                .iter()
                .map(|param| self.resolve_type(&param.ty))
                .collect();
            let _ = mapper;
            self.overload_symbols
                .entry(function.name.clone())
                .or_default()
                .push((parameter_types, symbol.clone()));
        }
        let llvm_function = self
            .backend
            .module()
            .get_function(&symbol)
            .unwrap_or_else(|| self.backend.module().add_function(&symbol, fn_type, None));
        self.return_kinds.insert(
            symbol.clone(),
            if out_of_line {
                ReturnKind::OutOfLine(return_type.clone())
            } else {
                ReturnKind::Value
            },
        );

        // An external declaration -- a C function, for instance -- has no body
        // to lower. Nothing is emitted for it beyond the symbol that
        // `declare_function` already added, and in particular it must not get a
        // basic block: a block with no terminator is invalid IR, and creating
        // one here made every function in an imported module that declares
        // rather than defines fail verification.
        let Some(body) = function.body else {
            return;
        };

        self.current_function = Some(llvm_function);
        self.current_return_type = Some(return_type.clone());

        let entry = self
            .backend
            .context()
            .append_basic_block(llvm_function, "entry");
        let builder = self.backend.context().create_builder();
        builder.position_at_end(entry);

        // Loop targets are per-function, and the binding stack is per-function
        // too, so both are reset before lowering the body.
        self.loop_stack.clear();
        self.binding_scopes.clear();
        // The escape paths run `finally` blocks, which are statements, so they
        // need the bindings the body was lowered with.
        self.current_bindings = bindings.clone();
        // A `??` function builds its result in one place, because there are
        // three ways to fill it -- `return`, `raise`, and an exception passing
        // through -- and they must agree on the layout.
        self.exc_result = None;
        self.exc_result_type = None;
        self.exc_frames.clear();
        // The aggregate-return slot is the hidden first parameter. It is kept
        // for the whole body because every `return` writes into it.
        self.sret_slot = out_of_line.then(|| {
            llvm_function
                .get_nth_param(0)
                .expect("the hidden return slot is the first parameter")
                .into_pointer_value()
        });
        if function.throws.is_some()
            && let Some(result_type) = return_llvm
        {
            let slot =
                self.create_entry_alloca(&builder, entry, "result", result_type);
            self.exc_result = Some(slot);
            self.exc_result_type = Some(result_type);
        }

        // Parameters arrive as values, so each gets storage in the entry block
        // and an initial store. That matches how locals are handled and means a
        // parameter can be read through the same load path as any binding.
        self.binding_scopes.push(HashMap::new());
        // The parameters are the function's outermost scope: a by-value
        // aggregate is copied into the callee's own storage here and destroyed
        // on every way out, exactly like a local of that type.
        self.drop_scopes.push(Vec::new());
        self.defer_scopes.push(Vec::new());
        let parameter_offset = usize::from(out_of_line);
        for (index, param) in function.params.iter().enumerate() {
            // A parameter's type is the one the checker resolved when there is
            // one -- a lambda's parameters, and an instantiation's, may have
            // no written type at all.
            let ty = pre_resolved
                .get(index)
                .cloned()
                .unwrap_or_else(|| self.resolve_type(&param.ty));
            let Some(llvm_ty) = mapper.map(&ty) else {
                continue;
            };
            let value = llvm_function
                .get_nth_param(
                    u32::try_from(index + parameter_offset)
                        .expect("parameter count fits in u32"),
                )
                .expect("LLVM created one parameter per declared type");
            // A capture of a lambda arrives as the address of the variable it
            // names, and that address *is* the variable's storage: binding it
            // directly makes a read or a write in the body reach the caller's
            // variable rather than a copy.
            if index < self.reference_params {
                let BasicValueEnum::PointerValue(address) = value else {
                    self.report_error(
                        "internal error: a captured variable was not passed by address",
                    );
                    continue;
                };
                let Some(captured) = ty.pointee().cloned() else {
                    self.report_error(
                        "internal error: a capture parameter is not a pointer",
                    );
                    continue;
                };
                let Some(captured_llvm) = mapper.map(&captured) else {
                    continue;
                };
                self.current_scope_mut().insert(
                    param.name.clone(),
                    LocalBinding {
                        storage: address,
                        ty: captured_llvm,
                    },
                );
                continue;
            }
            let alloca = self.create_entry_alloca(&builder, entry, &param.name, llvm_ty);
            let mut copied_by_constructor = false;
            if self.aggregate_is_by_address(&ty) {
                // The argument was passed as an address, and the parameter is a
                // value: copy it so the body owns it and a write to the
                // parameter cannot reach the argument.
                let BasicValueEnum::PointerValue(source) = value else {
                    self.report_error(
                        "internal error: an aggregate parameter is not an address",
                    );
                    continue;
                };
                // The copy is the type's own, when it declares one: §4.3.2 of
                // the functions volume says the copy constructor *must* be
                // called, and byte-wise copying a type that owns a buffer makes
                // two owners of one buffer.
                if let Some(constructor) = self.parameter_copy_constructor(&ty) {
                    if let Err(error) = builder.build_call(
                        constructor,
                        &[alloca.into(), source.into()],
                        "parameter.copy",
                    ) {
                        self.report_error(&error.to_string());
                    }
                    copied_by_constructor = true;
                } else {
                    let Some(size) = llvm_ty.size_of() else {
                        self.report_error("internal error: an aggregate parameter has no size");
                        continue;
                    };
                    if let Err(error) = builder.build_memcpy(alloca, 1, source, 1, size) {
                        self.report_error(&error.to_string());
                    }
                }
            } else {
                builder
                    .build_store(alloca, value)
                    .map_err(|error| {
                        self.report_error(&error.to_string());
                    })
                    .ok();
                // A small record travels in a register, but that is an ABI
                // detail: the parameter is still a copy made by the type's own
                // copy constructor. The register value is written to a
                // temporary to give the constructor the address it takes; the
                // caller's object is untouched.
                if !self.aggregate_is_by_address(&ty)
                    && !ty.is_void()
                    && matches!(ty, fallc_sema::Type::Nominal(_))
                {
                    if let Some(constructor) = self.parameter_copy_constructor(&ty) {
                        let source =
                            self.create_entry_alloca(&builder, entry, "parameter.source", llvm_ty);
                        builder
                            .build_store(source, value)
                            .map_err(|error| {
                                self.report_error(&error.to_string());
                            })
                            .ok();
                        if let Err(error) = builder.build_call(
                            constructor,
                            &[alloca.into(), source.into()],
                            "parameter.copy",
                        ) {
                            self.report_error(&error.to_string());
                        }
                        copied_by_constructor = true;
                    }
                }
            }
            let parameter_ty = self.resolve_type(&param.ty);
            self.register_binding(&param.name, &parameter_ty, alloca, llvm_ty);
            // A type with a destructor but without a usable copy constructor is
            // still copied byte-wise; its two values would then share whatever
            // the fields own, and destroying both is a double free. Until the
            // move-out paths can hand ownership over, such a parameter is not
            // destroyed -- the alternative is a crash.
            if !copied_by_constructor && self.type_needs_drop(&parameter_ty)
                && let Some(scope) = self.drop_scopes.last_mut()
                && scope.last().is_some_and(|entry| entry.storage == alloca)
            {
                scope.pop();
            }
        }

        if let Err(error) = self.lower_block(ast, types, bindings, &builder, body) {
            // A failed statement aborts the function: continuing would emit IR
            // for a construct the compiler does not understand, which is worse
            // than stopping. The diagnostic is recorded and the function keeps
            // whatever it built, so LLVM's verifier still runs on it.
            self.diagnostics.push(error.to_diagnostic());
        }
        // A body that reaches its end falls out of every remaining scope: the
        // parameters are destroyed here (their scope is the one left open).
        if !Self::is_terminated(&builder)
            && let Err(error) = self.emit_all_drops(ast, types, bindings, &builder, None)
        {
            self.diagnostics.push(error.to_diagnostic());
        }
        // Pop the parameter scope.
        self.binding_scopes.pop();
        self.drop_scopes.pop();
        self.defer_scopes.pop();

        // Every LLVM block needs a terminator. A body that reaches its end
        // without returning is unreachable code when the function returns a
        // value, so the block is closed with `unreachable` rather than
        // silently returning a zero value; no diagnostic rejects it earlier.
        if builder
            .get_insert_block()
            .is_some_and(|b| b.get_terminator().is_none())
        {
            // A function that returns nothing falls off its end into `ret
            // void`; one that returns a value and reaches its end without
            // returning is unreachable code.
            //
            // Emitting `unreachable` for both made a `void` function fall
            // through into whatever the linker placed next, which printed its
            // output forever. `ret void` is what actually ends the function.
            if self.exc_result.is_some() {
                // A `??` function's fallthrough is still a *result*: `ok` with
                // no value. A plain `ret void` has the wrong shape, and LLVM
                // refuses the module rather than emitting it.
                let _ = self.build_function_return(&builder, None);
            } else if returns_nothing {
                let _ = builder.build_return(None);
            } else if function.name == "main" {
                // A program's entry that reaches its end has finished
                // successfully. `fn main() int { …; }` with no `return` is the
                // shape every reader writes first, and the status it means is
                // 0 -- the status a C program gets from falling off the end of
                // its `main`. Emitting `unreachable` there stopped the program
                // with SIGILL instead.
                let zero = self.backend.context().i32_type().const_zero();
                let value: BasicValueEnum<'static> = match value_llvm {
                    Some(inkwell::types::BasicTypeEnum::IntType(int)) => {
                        int.const_zero().as_basic_value_enum()
                    }
                    _ => zero.as_basic_value_enum(),
                };
                let _ = self.build_function_return(&builder, Some(value));
            } else {
                let _ = builder.build_unreachable();
            }
        }

        self.current_function = None;
        self.current_return_type = None;
    }

    /// The copy constructor of a by-value aggregate parameter, when one exists
    /// and cannot raise.
    ///
    /// The constructor is the two-parameter `__init__(this &T, other &const T)`
    /// of the parameter's type, with the same mangling every method call uses.
    /// A constructor that can raise is skipped: calling it at a function's
    /// entry would have to propagate, and a parameter copy has no call site to
    /// carry the `??` marker.
    fn parameter_copy_constructor(
        &mut self,
        ty: &fallc_sema::Type,
    ) -> Option<inkwell::values::FunctionValue<'static>> {
        let reference = |referent_is_const: bool| fallc_sema::Type::Reference {
            referent: Box::new(ty.clone()),
            referent_is_const,
            is_rvalue: false,
        };
        let params = [
            fallc_sema::mangle_type(&reference(false)),
            fallc_sema::mangle_type(&reference(true)),
        ];
        let symbol = fallc_ast::special::symbol_for("__init__", &params);
        let module = self.backend.module();
        let local = module.get_function(&symbol).or_else(|| {
            self.module_function_symbols
                .get(&symbol)
                .and_then(|symbol| module.get_function(symbol))
        });
        if let Some(function) = local {
            // A may-raise function returns a result struct; a plain `void`
            // return is a constructor that never raises.
            return function
                .get_type()
                .get_return_type()
                .is_none()
                .then_some(function);
        }
        let (qualified, signature) = self
            .external_symbols
            .iter()
            .find(|(_, signature)| signature.symbol == symbol)
            .map(|(qualified, signature)| (qualified.clone(), signature.clone()))?;
        if signature.throws || !signature.returns.is_void() {
            return None;
        }
        self.declare_external(&symbol, &qualified, fallc_span::Span::SYNTHETIC)
            .ok()
    }

    /// Resolves a written type annotation to a type.
    ///
    /// The type checker has already resolved every annotation, and its results
    /// are what codegen reads; this is only for the few places codegen resolves
    /// a type on its own, such as a cast target. It therefore mirrors the
    /// checker's rules rather than inventing its own.
    pub(super) fn resolve_type(&self, ty: &TypeSyntax) -> Type {
        // A template parameter, while an instantiation is being emitted. The
        // checker resolved the body against the arguments, and this is the same
        // substitution applied where code generation resolves a written type --
        // so the two agree without the body having been rewritten.
        if let Some(substitution) = &self.substitution
            && let TypeSyntax::Named(named) = ty
            && let Some(ty) = substitution.type_of(&named.path)
        {
            return ty.clone();
        }
        // The checker resolved every written type it saw and filed the answer
        // under the written form's span. Reading it here keeps the two stages
        // in agreement, and it is the only way to know the length of an array
        // type: `[N][N]double` was folded by the checker, in a constant table
        // this stage would have to rebuild.
        if let Some(resolved) = self.type_of_types.get(&ty.span()) {
            return resolved.clone();
        }
        match ty {
            // `fn(int) int` written in code generation's own reach -- a cast
            // target, say. The checker filed it too; this is the same answer.
            TypeSyntax::Function(function) => Type::Function {
                parameters: function
                    .parameters
                    .iter()
                    .map(|parameter| self.resolve_type(parameter))
                    .collect(),
                returns: Box::new(self.resolve_type(&function.returns)),
                abi: function.abi.clone(),
                variadic: function.variadic,
                throws: function.throws,
            },
            TypeSyntax::Named(named) => {
                // A declared type's fields live in the type table, so it is
                // looked up there before falling back to an unknown name.
                //
                // A member of a type family is keyed by its *full* name, the
                // one the checker built from the arguments: `Box<f64>` is a
                // different entry from `Box<int>`, and looking up the bare
                // `Box` found whichever member happened to be built first --
                // which is how a `Box<f64>` came to hold an `int`'s bits.
                //
                // A type declared in an imported module is in the table too:
                // every module's declarations are registered before lowering,
                // so a name the unit learned from an exported signature
                // resolves here.
                if !named.arguments.is_empty() {
                    let spelled = self.spell_type_arguments(&named.path, &named.arguments);
                    if let Some(nominal) = self.nominal_types.get(&spelled) {
                        return Type::Nominal(nominal.clone());
                    }
                }
                // An alias is the type it names, so the lookup ends here
                // rather than producing a type of its own. Code generation
                // needs this because it resolves a *written* parameter type
                // itself, and `Name` in `fn f(n Name)` would otherwise be an
                // unknown name with no LLVM type at all.
                if let Some(target) = self.aliases.get(&named.path) {
                    return target.clone();
                }
                if let Some(nominal) = self.nominal_types.get(&named.path) {
                    return Type::Nominal(nominal.clone());
                }
                Type::from_name(&named.path)
                    .unwrap_or_else(|| Type::unspecified(named.path.clone(), named.span))
            }
            // `auto` as a return type: the checker deduced it from the body's
            // `return` and recorded it under the written form's span, which is
            // the one key that means the same thing in both stages.
            TypeSyntax::Auto(span) | TypeSyntax::DecltypeAuto(span) => self
                .type_of_types
                .get(span)
                .cloned()
                .unwrap_or_else(|| Type::unspecified("auto", *span)),
            // A nested type's name includes the type that declares it, which
            // this stage does not resolve: the checker already did.
            TypeSyntax::Member(member) => Type::unspecified(member.name.clone(), member.span),
            TypeSyntax::Unresolved(u) => Type::unspecified(u.text.clone(), u.span),
            TypeSyntax::Const(inner) => self.resolve_type(inner),
            TypeSyntax::Pointer(inner) => Type::Pointer {
                pointee: Box::new(self.resolve_type(inner)),
                pointee_is_const: false,
                pointer_is_const: false,
            },
            TypeSyntax::PointerToConst(inner) => Type::Pointer {
                pointee: Box::new(self.resolve_type(inner)),
                pointee_is_const: true,
                pointer_is_const: false,
            },
            TypeSyntax::ConstPointer(inner) => Type::Pointer {
                pointee: Box::new(self.resolve_type(inner)),
                pointee_is_const: false,
                pointer_is_const: true,
            },
            TypeSyntax::Reference(inner) => Type::Reference {
                referent: Box::new(self.resolve_type(inner)),
                referent_is_const: false,
                is_rvalue: false,
            },
            TypeSyntax::ConstReference(inner) => Type::Reference {
                referent: Box::new(self.resolve_type(inner)),
                referent_is_const: true,
                is_rvalue: false,
            },
            TypeSyntax::RvalueReference(inner) => Type::Reference {
                referent: Box::new(self.resolve_type(inner)),
                referent_is_const: false,
                is_rvalue: true,
            },
            TypeSyntax::ConstRvalueReference(inner) => Type::Reference {
                referent: Box::new(self.resolve_type(inner)),
                referent_is_const: true,
                is_rvalue: true,
            },
            // `volatile T` has `T`'s representation. The qualifier changes which
            // LLVM instructions are used to touch the memory, not what the
            // memory looks like, so the type maps to the same LLVM type.
            TypeSyntax::Volatile(inner) => self.resolve_type(inner),
            // `[]T` in a parameter list is `*T`, and `[]const T` is
            // `*const T`. This has to agree with the checker's resolution to
            // the letter: a symbol is built from these types, and two spellings
            // of one parameter type are two symbols -- a call to a function
            // that is not there.
            TypeSyntax::UnsizedArray(inner) => match &**inner {
                TypeSyntax::Const(element) => Type::Pointer {
                    pointee: Box::new(self.resolve_type(element)),
                    pointee_is_const: true,
                    pointer_is_const: false,
                },
                element => Type::Pointer {
                    pointee: Box::new(self.resolve_type(element)),
                    pointee_is_const: false,
                    pointer_is_const: false,
                },
            },
            TypeSyntax::Array(array) => {
                // An array's length is a constant, and the checker folded it.
                // The fold is keyed by the length expression's span, which is
                // the one thing this stage can look up -- a written type is
                // resolved here rather than handed over, so a return type like
                // `[N][N]double` has no resolved form to read.
                let element = self.resolve_type(&array.element);
                let length = self
                    .constant_values
                    .get(&array.len_span)
                    .copied()
                    .and_then(|value| u64::try_from(value).ok());
                match length {
                    Some(len) => Type::Array {
                        element: Box::new(element),
                        len,
                    },
                    None => Type::unspecified("<unresolved array type>", ty.span()),
                }
            }
            // `type(expr)` was answered by the checker, which had the
            // expression in hand. The answer travels by span, because a written
            // type is resolved here too and there is no tree to ask.
            TypeSyntax::TypeOf(node) => self
                .type_of_types
                .get(&node.span)
                .cloned()
                .unwrap_or_else(|| Type::unspecified("type(..)", node.span)),
        }
    }

}
