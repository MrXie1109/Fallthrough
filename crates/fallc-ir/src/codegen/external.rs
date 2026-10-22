//! Tables a module's bodies resolve against, and declarations of things from
//! other modules.
//!
//! Every record the checker made is installed here under its span, and an
//! imported function is declared here from the signature its own module
//! exported.

use super::*;

impl<'ctx> Lowerer<'ctx> {
    /// Records a declared type, so a written type name resolves to it.
    pub fn add_nominal_type(&mut self, nominal: fallc_sema::NominalType) {
        self.nominal_types.insert(nominal.name.clone(), nominal);
    }

    /// Records the `extern` variables other modules exported, and which uses
    /// name them.
    pub fn add_external_variables(
        &mut self,
        globals: HashMap<String, (fallc_sema::Type, String, bool)>,
        refs: HashMap<Span, String>,
    ) {
        self.external_globals.extend(globals);
        self.external_global_refs.extend(refs);
    }

    /// Returns the global a qualified name stands for, declaring it if needed.
    ///
    /// The declaration is made once; a program that reads `C.stdin` twice gets
    /// one symbol.
    /// Returns the address a function value names.
    ///
    /// `wanted` is the type the context asks for: when several overloads share
    /// a name, it is what chooses between them, exactly as it does at a call.
    pub(super) fn function_pointer(
        &self,
        name: &str,
        wanted: Option<&fallc_sema::Type>,
    ) -> Option<inkwell::values::PointerValue<'static>> {
        let entries = self.function_values.get(name)?;
        let symbol = match entries.as_slice() {
            [(symbol, _)] => symbol,
            several => {
                let wanted = wanted?.unqualified().clone();
                let target = match &wanted {
                    fallc_sema::Type::Pointer { pointee, .. } => pointee.unqualified().clone(),
                    other => other.clone(),
                };
                several
                    .iter()
                    .find(|(_, ty)| ty.unqualified() == &target)
                    .map(|(symbol, _)| symbol)?
            }
        };
        let function = self.backend.module().get_function(symbol)?;
        Some(function.as_global_value().as_pointer_value())
    }

    /// Lowers a call through a value, if the callee's type is callable.
    ///
    /// `None` means the callee is not a function pointer and the caller should
    /// keep looking. `Some(None)` is a call that produced no value -- a call to
    /// a function returning `void` -- which is a successful lowering.
    #[allow(clippy::option_option)]
    pub(super) fn lower_indirect_call(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        call: &CallExpr,
        span: fallc_span::Span,
    ) -> Result<Option<Option<BasicValueEnum<'static>>>, CodegenError> {
        let Some(callable) = types
            .get(call.callee)
            .and_then(|ty| indirect_call_type(ty).cloned())
        else {
            return Ok(None);
        };
        let Some(BasicValueEnum::PointerValue(pointer)) =
            self.lower_expr(ast, types, builder, call.callee)?
        else {
            return Ok(None);
        };
        let Some(function_type) = self.function_llvm_type(&callable) else {
            return Err(CodegenError::waiting_for_spec(
                "a call through this function pointer",
                span,
            ));
        };
        let args = self.lower_arguments(ast, types, builder, call, "", span)?;
        let site = builder
            .build_indirect_call(function_type, pointer, &args, "call")
            .map_err(|error| CodegenError::Emit(error.to_string()))?;
        // A pointer to a function that may raise returns the exception
        // structure, and the value it carries is the call's result only when
        // the tag says so. `unwrap_throwing` reads the shape, so a pointer that
        // cannot raise passes through unchanged.
        Ok(Some(self.unwrap_throwing(ast, types, builder, span, site)?))
    }

    /// The LLVM function type a written function type describes.
    ///
    /// Needed for an indirect call: an LLVM pointer does not carry the type of
    /// what it points at, so the call has to say which convention it uses.
    pub(super) fn function_llvm_type(
        &mut self,
        ty: &fallc_sema::Type,
    ) -> Option<inkwell::types::FunctionType<'static>> {
        let fallc_sema::Type::Function {
            parameters,
            returns,
            variadic,
            throws,
            ..
        } = ty.unqualified()
        else {
            return None;
        };
        let parameters = parameters.clone();
        let returns = returns.clone();
        let variadic = *variadic;
        let throws = *throws;
        let context = self.backend.context();
        let mapper = TypeMapper::new(context);
        let mut listed = Vec::with_capacity(parameters.len());
        for parameter in &parameters {
            listed.push(mapper.map(parameter)?.into());
        }
        // No value for a function that returns nothing, which is also what a
        // throwing one's result structure has to be built without.
        let returned = if returns.is_void() {
            None
        } else {
            Some(mapper.map(&returns)?)
        };
        // A function that may raise returns `{ ok, value?, error }`, so a call
        // through a pointer to one is the same call the declaration's own call
        // sites make.
        if throws {
            return Some(self.throwing_result_type(returned)?.fn_type(&listed, variadic));
        }
        // A function that returns nothing still has a function type, and its
        // return type is `void`.
        Some(match returned {
            Some(returned) => returned.fn_type(&listed, variadic),
            None => context.void_type().fn_type(&listed, variadic),
        })
    }

    pub(super) fn external_global(
        &mut self,
        qualified: &str,
    ) -> Option<(inkwell::values::GlobalValue<'static>, inkwell::types::BasicTypeEnum<'static>)>
    {
        let (ty, symbol, thread_local) = self.external_globals.get(qualified)?.clone();
        if let Some(existing) = self.globals.get(&symbol).copied() {
            return Some(existing);
        }
        let llvm = TypeMapper::new(self.backend.context()).map(&ty)?;
        // A global that is already in the module -- declared here earlier, or
        // defined by the module that owns it -- is one object. Declaring a
        // second one gave LLVM two names for one variable, and the definition
        // went to whichever of them nothing referenced.
        let global = match self.backend.module().get_global(&symbol) {
            Some(existing) => existing,
            None => {
                let global = self.backend.module().add_global(llvm, None, &symbol);
                global.set_linkage(inkwell::module::Linkage::External);
                // The declaration has to match the definition's storage class:
                // a thread-local object reached through a plain declaration
                // reads the initial-exec copy instead of the thread's.
                global.set_thread_local(thread_local);
                global
            }
        };
        let llvm = match global.get_value_type() {
            inkwell::types::AnyTypeEnum::ArrayType(ty) => ty.into(),
            inkwell::types::AnyTypeEnum::FloatType(ty) => ty.into(),
            inkwell::types::AnyTypeEnum::IntType(ty) => ty.into(),
            inkwell::types::AnyTypeEnum::PointerType(ty) => ty.into(),
            inkwell::types::AnyTypeEnum::StructType(ty) => ty.into(),
            _ => llvm,
        };
        self.globals.insert(symbol, (global, llvm));
        Some((global, llvm))
    }

    /// Records the linker symbol a name in this unit must call.
    ///
    /// A name imported from another module is a local name here but the
    /// defining module's own symbol at link time, and the two can differ in
    /// every way: `from C import printf` calls `printf`, and `import C as Libc`
    /// makes `Libc.printf` call it too. Deriving the symbol from the spelling
    /// of the name gets both of those wrong, so the mapping is recorded
    /// explicitly by whoever resolved the name.
    pub fn add_imported_symbol(
        &mut self,
        local: impl Into<String>,
        symbol: impl Into<String>,
        qualified: impl Into<String>,
    ) {
        self.imported_symbols
            .insert(local.into(), (symbol.into(), qualified.into()));
    }

    /// Records the symbols the checker resolved for index expressions.
    pub fn set_index_methods(&mut self, methods: HashMap<Span, String>) {
        self.index_methods = methods;
    }

    /// Records the symbols the checker resolved for `__call__` expressions.
    /// Records what each `str(x)`/`repr(x)` resolved to, as decided by the
    /// checker: a symbol to call, or nothing to render the value here.
    pub fn set_text_calls(&mut self, calls: HashMap<Span, Option<String>>) {
        self.text_calls = calls;
    }

    /// Records which definition each plain function call reached, as the
    /// checker decided it.
    /// Records which method each operator resolved to, as the checker decided
    /// it.
    pub fn set_operator_methods(&mut self, methods: HashMap<Span, fallc_sema::OperatorCall>) {
        self.operator_methods = methods;
    }

    /// Records the types argument literals adopted, so they can be converted.
    pub fn set_adopted_types(&mut self, types: HashMap<Span, fallc_sema::Type>) {
        self.adopted_types = types;
    }

    /// Records which constructor each string literal used as a declared type
    /// calls.
    pub fn set_literal_constructions(&mut self, constructions: HashMap<Span, String>) {
        self.literal_constructions = constructions;
    }

    /// Records the type each constructed literal has.
    pub fn set_literal_construction_types(&mut self, types: HashMap<Span, fallc_sema::Type>) {
        self.literal_construction_types = types;
    }

    /// Installs the call of a closure value each call site makes.
    pub fn set_closure_calls(&mut self, calls: HashMap<Span, fallc_sema::ClosureCall>) {
        self.closure_calls = calls;
    }

    /// Installs the value each interface conversion boxes.
    pub fn set_interface_conversions(
        &mut self,
        conversions: HashMap<Span, fallc_sema::InterfaceConversion>,
    ) {
        self.interface_conversions = conversions;
    }

    /// Installs the interface method each virtual call reaches.
    pub fn set_interface_calls(&mut self, calls: HashMap<Span, fallc_sema::InterfaceCall>) {
        self.interface_calls = calls;
    }

    /// Installs the declarations `@enable_if` removed.
    pub fn set_disabled_functions(&mut self, functions: std::collections::HashSet<Span>) {
        self.disabled_functions = functions;
    }

    /// Installs the methods of a type family, by their name spans.
    pub fn set_templated_methods(
        &mut self,
        methods: std::collections::HashSet<Span>,
    ) {
        self.templated_methods = methods;
    }

    /// Installs the lambda each call calls.
    pub fn set_lambda_calls(&mut self, calls: HashMap<Span, fallc_sema::LambdaInstance>) {
        self.lambda_calls = calls;
    }

    /// Installs the lambda each value-position use denotes.
    pub fn set_lambda_values(&mut self, values: HashMap<Span, fallc_sema::LambdaInstance>) {
        self.lambda_values = values;
    }

    pub fn set_call_symbols(&mut self, symbols: HashMap<Span, String>) {
        self.call_symbols = symbols;
    }

    /// Records which constructor each construction call reaches.
    pub fn set_constructor_calls(&mut self, calls: HashMap<Span, String>) {
        self.constructor_calls = calls;
    }

    /// Records the type each construction call builds.
    pub fn set_constructor_types(&mut self, types: HashMap<Span, fallc_sema::Type>) {
        self.constructor_types = types;
    }

    /// Records every function a call instantiated from a template.
    pub fn set_instantiations(
        &mut self,
        instantiations: HashMap<String, fallc_sema::Instantiation>,
    ) {
        self.instantiations = instantiations;
    }

    /// Records the folded value of each constant expression.
    pub fn set_constant_values(&mut self, values: HashMap<Span, i64>) {
        self.constant_values = values;
    }

    /// Records every `if` the checker decided at compile time.
    pub fn set_compile_time_ifs(&mut self, ifs: HashMap<Span, bool>) {
        self.compile_time_ifs = ifs;
    }

    /// Records what each `type Name = Other;` alias stands for.
    pub fn set_aliases(&mut self, aliases: HashMap<String, fallc_sema::Type>) {
        self.aliases = aliases;
    }

    /// Records which arguments are passed as their address.
    pub fn set_by_address_arguments(&mut self, arguments: std::collections::HashSet<Span>) {
        self.by_address_arguments = arguments;
    }

    pub fn set_call_methods(&mut self, methods: HashMap<Span, String>) {
        self.call_methods = methods;
    }

    /// Records the symbols the checker resolved for `__bool__` conditions.
    pub fn set_bool_methods(&mut self, methods: HashMap<Span, String>) {
        self.bool_methods = methods;
    }

    /// Records the symbols the checker resolved for unary operators.
    pub fn set_unary_methods(&mut self, methods: HashMap<Span, String>) {
        self.unary_methods = methods;
    }

    /// Records the symbols the checker resolved for compound assignments.
    /// Records whether each compound assignment's operand is passed by address.
    pub fn set_in_place_by_address(&mut self, by_address: HashMap<Span, bool>) {
        self.in_place_by_address = by_address;
    }

    pub fn set_in_place_methods(&mut self, methods: HashMap<Span, String>) {
        self.in_place_methods = methods;
    }

    /// Records the constructors the checker resolved for bindings.
    pub fn set_init_calls(&mut self, calls: HashMap<Span, String>) {
        self.init_calls = calls;
    }

    /// Records the symbols the checker resolved for method calls.
    pub fn set_type_of_types(&mut self, types: HashMap<Span, Type>) {
        self.type_of_types = types;
    }

    /// Records the alignment each binding asked for with `alignas`.
    ///
    /// Keyed by the binding's name span, which is what the checker recorded.
    /// A binding with no entry keeps the alignment its type gives it.
    pub fn add_binding_alignments(&mut self, alignments: HashMap<Span, u64>) {
        self.binding_alignments.extend(alignments);
    }

    pub fn set_method_calls(&mut self, calls: HashMap<Span, String>) {
        self.method_calls = calls;
    }

    /// Records the symbols the checker resolved for plain assignments.
    pub fn set_assign_methods(&mut self, methods: HashMap<Span, String>) {
        self.assign_methods = methods;
    }

    /// Records where a dereference result must be read as a value.
    pub fn set_deref_reads(&mut self, reads: HashMap<Span, Type>) {
        self.deref_reads = reads;
    }

    /// Records the calls whose callee may raise.
    /// Installs the constant arguments a call site filled omitted defaults with.
    pub fn set_default_arguments(
        &mut self,
        values: std::collections::HashMap<
            fallc_span::Span,
            Vec<(usize, i64, fallc_sema::Type)>,
        >,
    ) {
        self.default_argument_values = values;
    }

    /// Installs the defaults a call site left to an expression.
    pub fn set_default_expressions(
        &mut self,
        values: fallc_sema::ExpressionDefaults,
    ) {
        self.default_expressions = values;
    }

    pub fn set_throwing_calls(
        &mut self,
        calls: std::collections::HashSet<Span>,
        propagating: std::collections::HashSet<Span>,
    ) {
        self.throwing_calls = calls;
        self.propagating_calls = propagating;
    }

    /// Records what each `->` chain ends at.
    pub fn set_pointer_targets(&mut self, targets: HashMap<Span, Type>) {
        self.pointer_targets = targets;
    }

    /// Records the explicit construction and destruction calls the checker
    /// answered without a declared method.
    pub fn set_in_place_operations(
        &mut self,
        operations: HashMap<Span, fallc_sema::InPlaceOperation>,
    ) {
        self.in_place_operations = operations;
    }

    /// Records the conversion methods the checker resolved for this unit.
    pub fn set_conversions(&mut self, conversions: HashMap<Span, fallc_sema::ConversionMethod>) {
        self.conversions = conversions;
    }

    /// Records a module's name, so a qualified call can be told from a method
    /// call.
    pub fn add_module(&mut self, name: impl Into<String>) {
        self.module_names.insert(name.into());
    }

    /// Sets the module whose items are about to be lowered.
    ///
    /// Its name qualifies the symbol of each module-level variable it defines.
    /// Enables the module-object cache at `directory`.
    pub fn set_module_cache(&mut self, directory: std::path::PathBuf) {
        self.module_cache = Some(directory);
    }

    /// Keeps the module library as native object code at `optimization`.
    ///
    /// Set by the driver when the program is going to be linked. The library's
    /// machine code is emitted once, into the cache, and the linker combines it
    /// with the unit's object; linking its LLVM IR into this module instead
    /// would generate machine code for the whole standard library on every
    /// compilation.
    pub fn set_object_library(&mut self, optimization: OptLevel) {
        self.object_library = true;
        self.optimization = optimization;
    }

    /// Returns the object and manifest paths of a cached module library.
    pub fn object_library_entries(&self, cache_hash: u64) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
        let entry = self.cache_entry(cache_hash)?;
        let object = entry.with_extension(format!("o{}", self.optimization.spelling()));
        let manifest = entry.with_extension(format!("inst{}", self.optimization.spelling()));
        Some((object, manifest))
    }

    /// Returns the bitcode path for a module with this dependency hash, or
    /// `None` when the cache is disabled.
    pub fn cache_entry(&self, cache_hash: u64) -> Option<std::path::PathBuf> {
        let directory = self.module_cache.clone()?;
        let target = format!("{:?}", self.backend.host_triple());
        let opt = format!("{:?}", self.mode);
        let key = crate::codegen::cache::CacheKey {
            cache_hash,
            target: &target,
            opt: &opt,
        };
        Some(directory.join(key.entry_name()))
    }

    /// Sets whether private free functions get the module prefix.
    pub fn set_private_function_prefix(&mut self, prefix: bool) {
        self.private_function_prefix = prefix;
    }

    pub fn set_current_module(&mut self, name: impl Into<String>) {
        self.current_module = name.into();
    }

    /// Records an external function so that calls to it can be declared.
    ///
    /// Called for each `extern "C"` declaration the unit can see.
    pub fn add_external(&mut self, qualified: impl Into<String>, signature: ExternalSignature) {
        self.external_symbols.insert(qualified.into(), signature);
    }

    /// Declares an external function in the LLVM module.
    ///
    /// The declaration uses the ABI the source named: `extern "C"` means the C
    /// calling convention, which is LLVM's default for a declaration, so there
    /// is nothing to set beyond the signature.
    /// Returns the function a resolved symbol names, declaring it from the
    /// exported signature if this module does not define it.
    ///
    /// A resolved symbol may belong to another module: the checker resolves
    /// against every definition in the program, while this module only emitted
    /// the ones it declares. The external table is where the defining module's
    /// signature was filed, so it is the one place to look.
    ///
    /// One helper rather than one per call site: method calls, operators,
    /// `+=`, `str`/`repr`, indexing and literal constructions all need exactly
    /// this, and they had six near-copies of it that disagreed about what to do
    /// when the symbol was missing.
    /// Whether this module defines `symbol` under its private-function prefix.
    pub(super) fn private_symbol_defined(&self, symbol: &str) -> bool {
        !self.current_module.is_empty()
            && self
                .backend
                .module()
                .get_function(&format!("{}.{}", self.current_module, symbol))
                .is_some()
    }

    pub(super) fn resolved_function(
        &mut self,
        symbol: &str,
        what: &str,
        span: fallc_span::Span,
    ) -> Result<inkwell::values::FunctionValue<'static>, CodegenError> {
        if let Some(function) = self.backend.module().get_function(symbol) {
            return Ok(function);
        }
        // A private free function is emitted under this module's prefix; a
        // call site recorded the bare symbol. Try the prefixed spelling before
        // treating the symbol as external.
        let private = format!("{}.{}", self.current_module, symbol);
        if let Some(function) = self.backend.module().get_function(&private) {
            return Ok(function);
        }
        let qualified = self
            .external_symbols
            .iter()
            .find(|(_, signature)| signature.symbol == symbol)
            .map(|(qualified, _)| qualified.clone())
            .ok_or_else(|| {
                CodegenError::Emit(format!(
                    "internal error: `{symbol}` was resolved as {what} but is neither defined \
                     nor imported"
                ))
            })?;
        self.declare_external(symbol, &qualified, span)
    }

    pub(super) fn declare_external(
        &mut self,
        symbol: &str,
        qualified: &str,
        span: fallc_span::Span,
    ) -> Result<inkwell::values::FunctionValue<'static>, CodegenError> {
        // The signature of *this* symbol. A module may publish two overloads
        // under one name, and each has its own keyed entry; falling back to the
        // bare name would declare the first overload's shape for the second
        // one's symbol, and the call would then pass the wrong arguments.
        let signature = self
            .external_symbols
            .get(&format!("{qualified}#{symbol}"))
            .or_else(|| self.external_symbols.get(qualified))
            .cloned()
            .ok_or_else(|| {
                CodegenError::waiting_for_spec(
                    format!("a call to the undeclared `{qualified}`"),
                    span,
                )
            })?;

        let mapper = TypeMapper::new(self.backend.context());
        let mut params = Vec::with_capacity(signature.params.len());
        for ty in &signature.params {
            let Some(llvm) = mapper.map(ty) else {
                return Err(CodegenError::waiting_for_spec(
                    "an external function with this parameter type",
                    span,
                ));
            };
            // The defining module passes a by-value aggregate as an address, so
            // the declaration has to say the same.
            let llvm = if self.aggregate_is_by_address(ty) {
                self.backend
                    .context()
                    .ptr_type(inkwell::AddressSpace::default())
                    .into()
            } else {
                llvm
            };
            params.push(llvm.into());
        }

        let context = self.backend.context();
        let value_llvm = match mapper.map(&signature.returns) {
            // No return type means the function returns nothing.
            _ if signature.returns.is_void() => None,
            Some(llvm) => Some(llvm),
            // A function declared in another module has only its *written*
            // signature here, so a return type of `auto` was never deduced for
            // this compilation: the deduction happened where the body is, and
            // the answer did not travel with the declaration.
            None if matches!(
                signature.returns.unqualified(),
                fallc_sema::Type::Unspecified(unspecified) if unspecified.text == "auto"
            ) =>
            {
                return Err(CodegenError::Emit(
                    "a function whose return type is deduced with `auto` cannot be called \
                     from another module; write the type out"
                        .to_owned(),
                ));
            }
            None => {
                return Err(CodegenError::waiting_for_spec(
                    "an external function with this return type",
                    span,
                ));
            }
        };
        // The defining module used the same convention for an aggregate it must
        // pass out of line, so the declaration has to say so too: it gets the
        // hidden first pointer and returns nothing.
        let out_of_line = self.aggregate_is_by_address(&signature.returns);
        if out_of_line && signature.throws {
            return Err(CodegenError::waiting_for_spec(
                "an imported `?!` function returning an aggregate that is passed out of line",
                span,
            ));
        }
        if out_of_line {
            params.insert(
                0,
                context
                    .ptr_type(inkwell::AddressSpace::default())
                    .into(),
            );
        }
        // A `?!` function in another module returns `{ ok, value, error }` like
        // this module's own, so the declaration built here has to say the same.
        let return_llvm = if signature.throws {
            match self.throwing_result_type(value_llvm) {
                Some(llvm) => Some(llvm),
                None => {
                    return Err(CodegenError::waiting_for_spec(
                        "an imported function that raises, without `import std`",
                        span,
                    ));
                }
            }
        } else if out_of_line {
            None
        } else {
            value_llvm
        };
        let fn_type = match return_llvm {
            Some(llvm) => inkwell::types::BasicType::fn_type(&llvm, &params, signature.variadic),
            None => context.void_type().fn_type(&params, signature.variadic),
        };

        // The declaration uses the symbol the defining module emitted, which
        // the signature carries.
        let declared = if signature.symbol.is_empty() {
            symbol.to_owned()
        } else {
            signature.symbol.clone()
        };
        self.return_kinds.insert(
            declared.clone(),
            if out_of_line {
                ReturnKind::OutOfLine(signature.returns.clone())
            } else {
                ReturnKind::Value
            },
        );
        let defined = self.backend.module().add_function(&declared, fn_type, None);
        // A named argument is placed by the parameter's name, which the
        // declaration brought across the module boundary. Without recording
        // them here the name had nothing to match, and the call could only say
        // that the parameter was unknown.
        if !signature.param_names.is_empty() {
            self.function_parameters.insert(
                declared.clone(),
                signature
                    .param_names
                    .iter()
                    .cloned()
                    .zip(signature.params.iter().cloned())
                    .collect(),
            );
        }
        Ok(defined)
    }

    /// Lowers a call to a function in an imported module.
    ///
    /// The symbol is the bare name the module declared, since that is what its
    /// own compilation emitted.
    pub(super) fn lower_external_call(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        builder: &Builder<'static>,
        qualified: &str,
        call: &CallExpr,
        span: fallc_span::Span,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        // The symbol is the one the defining module emitted, mangled from the
        // same parameter types. It is recorded with the signature rather than
        // derived here, because a module's symbols and a call site's must agree
        // exactly and only the module knows how its own types resolved.
        let symbol = self
            .external_symbols
            .get(qualified)
            .map(|signature| signature.symbol.clone())
            .unwrap_or_else(|| {
                qualified
                    .split_once('.')
                    .map_or(qualified, |(_, member)| member)
                    .to_owned()
            });
        self.lower_symbol_call(ast, types, builder, qualified, &symbol, call, span)
    }

}
