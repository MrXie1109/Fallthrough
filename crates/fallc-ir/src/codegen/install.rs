//! Installing a module: the entry points, the module tables, and the
//! instantiations a module needs.
//!
//! `lower_with_modules` is what the driver calls. It installs the tables a
//! module's bodies resolve against, declares every instantiation it will need,
//! emits the `main` wrapper, and then lowers the items themselves.

use super::*;

impl<'ctx> Lowerer<'ctx> {
    /// Creates a lowerer that emits into `backend`.
    #[must_use]
    pub fn new(backend: &'ctx Backend) -> Self {
        Lowerer::with_mode(backend, BuildMode::default())
    }

    /// Creates a lowerer targeting a specific build mode.
    #[must_use]
    pub fn with_mode(backend: &'ctx Backend, mode: BuildMode) -> Self {
        Lowerer {
            mode,
            module_cache: None,
            object_library: false,
            optimization: OptLevel::None,
            library_objects: Vec::new(),
            backend,
            diagnostics: DiagnosticBag::new(),
            current_function: None,
            current_return_type: None,
            sret_slot: None,
            binding_alignments: HashMap::new(),
            static_function_names: std::collections::HashSet::new(),
            static_count: 0,
            drop_scopes: Vec::new(),
            defer_scopes: Vec::new(),
            exc_frames: Vec::new(),
            exc_result_type: None,
            exc_result: None,
            exc_type: None,
            handled_global: None,
            handling: Vec::new(),
            binding_scopes: Vec::new(),
            globals: std::collections::HashMap::new(),
            external_globals: std::collections::HashMap::new(),
            module_declarations: Vec::new(),
            current_module: String::new(),
            private_function_prefix: false,
            lowering_literal: None,
            external_global_refs: std::collections::HashMap::new(),
            loop_stack: Vec::new(),
            branched_to: Vec::new(),
            used_binding_names: Vec::new(),
            string_literal_count: 0,
            function_defaults: HashMap::new(),
            default_argument_values: HashMap::new(),
            default_expressions: HashMap::new(),
            default_trees: HashMap::new(),
            function_parameters: HashMap::new(),
            function_values: HashMap::new(),
            overload_symbols: HashMap::new(),
            emitted_symbols: Vec::new(),
            library_instantiated: std::collections::HashSet::new(),
            module_function_symbols: HashMap::new(),
            return_kinds: HashMap::new(),
            conversions: HashMap::new(),
            index_methods: HashMap::new(),
            call_methods: HashMap::new(),
            text_calls: HashMap::new(),
            call_symbols: HashMap::new(),
            templated_methods: std::collections::HashSet::new(),
            has_setup: false,
            disabled_functions: std::collections::HashSet::new(),
            module_setup_symbols: Vec::new(),
            unit_setup_symbols: Vec::new(),
            closure_calls: HashMap::new(),
            interface_conversions: HashMap::new(),
            interface_calls: HashMap::new(),
            interface_vtables: HashMap::new(),
            lambda_calls: HashMap::new(),
            lambda_values: HashMap::new(),
            lambda_functions: HashMap::new(),
            pending_lambdas: Vec::new(),
            lambda_emitted: std::collections::HashSet::new(),
            pre_resolved_return: None,
            reference_params: 0,
            constructor_calls: HashMap::new(),
            instantiations: HashMap::new(),
            constructor_types: HashMap::new(),
            substitution: None,
            template_value_types: std::collections::HashMap::new(),
            current_symbol: None,
            instantiated: std::collections::HashSet::new(),
            constant_values: HashMap::new(),
            compile_time_ifs: HashMap::new(),
            aliases: HashMap::new(),
            pre_resolved_params: Vec::new(),
            by_address_arguments: std::collections::HashSet::new(),
            operator_methods: HashMap::new(),
            adopted_types: HashMap::new(),
            literal_constructions: HashMap::new(),
            literal_construction_types: HashMap::new(),
            temporaries: HashMap::new(),
            bool_methods: HashMap::new(),
            unary_methods: HashMap::new(),
            in_place_methods: HashMap::new(),
            in_place_by_address: HashMap::new(),
            init_calls: HashMap::new(),
            method_calls: HashMap::new(),
            indirect_receivers: HashMap::new(),
            type_of_types: HashMap::new(),
            throwing_calls: std::collections::HashSet::new(),
            propagating_calls: std::collections::HashSet::new(),
            current_bindings: BindingTypes::default(),
            abort_lowering: 0,
            assign_methods: HashMap::new(),
            deref_reads: HashMap::new(),
            pointer_targets: HashMap::new(),
            in_place_operations: HashMap::new(),
            imported_symbols: HashMap::new(),
            nominal_types: HashMap::new(),
            module_names: std::collections::HashSet::new(),
            external_symbols: HashMap::new(),
        }
    }

    /// Returns the backend being lowered into.
    #[must_use]
    pub fn backend(&self) -> &'ctx Backend {
        self.backend
    }

    /// Lowers every item in `ast`.
    ///
    /// Runs LLVM's verifier afterwards: a module that does not verify would
    /// otherwise fail much later, inside LLVM, with a far worse message.
    pub fn lower(&mut self, ast: &Ast, types: &TypeTable, bindings: &BindingTypes) -> LowerResult {
        self.collect_static_functions(ast);
        self.has_setup = ast.items.iter().any(|item| {
            matches!(&ast.item(*item).kind, fallc_ast::ItemKind::Setup(_))
        });
        self.lower_items(ast, types, bindings);
        self.lower_pending_lambdas();
        self.lower_setup_blocks(ast, types, bindings, true);
        self.emit_main_wrapper();
        self.verify_and_finish()
    }

    /// Records the functions that own `static` storage, by written name.
    ///
    /// Done before any body is lowered, so a call to a function defined later in
    /// the file is known to be non-foldable too.
    pub(super) fn collect_static_functions(&mut self, ast: &Ast) {
        for &item in &ast.items {
            let ItemKind::Function(function) = &ast.item(item).kind else {
                continue;
            };
            let mut finder = StaticStorageFinder::default();
            fallc_ast::visit::walk_item(ast, item, &mut finder);
            if finder.found {
                self.static_function_names.insert(function.name.clone());
            }
        }
    }

    /// Verifies the module and collects the result.
    ///
    /// LLVM's own verifier is the last line of defence on the IR this crate
    /// produces: a malformed module is a compiler bug, and it is far better to
    /// report it than to emit it.
    pub(super) fn verify_and_finish(&mut self) -> LowerResult {
        let llvm_ir = match self.backend.verify() {
            Ok(()) => self.backend.to_llvm_ir().0,
            Err(error) => {
                self.diagnostics.push(error.to_diagnostic());
                String::new()
            }
        };

        LowerResult {
            llvm_ir,
            library_objects: std::mem::take(&mut self.library_objects),
            diagnostics: std::mem::take(&mut self.diagnostics).into_vec(),
        }
    }

    /// Lowers this unit and then every imported module's bodies.
    ///
    /// # Why the modules come after
    ///
    /// An imported module contributes its signatures *and* its definitions: a
    /// module that writes `export fn f() int { .. }` defines `f` here, and if
    /// its body is not lowered nothing else will emit it, so the link fails
    /// with an undefined symbol. `standard/C.ftm` hid this, because every
    /// function in it is `extern "C"` and really is implemented elsewhere.
    ///
    /// The unit goes first so that the symbols it defines take precedence: a
    /// name declared in the file being compiled is that file's, and only names
    /// it does not define come from an import. That is also the order that
    /// makes a duplicate definition a detectable clash rather than a silent
    /// overwrite, since `declare_function` leaves an existing symbol alone.
    pub fn lower_with_modules(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        bindings: &BindingTypes,
        modules: &[ModuleBodyRef<'_>],
    ) -> LowerResult {
        // The instantiations are *declared* before the items are lowered, so
        // that a call inside a function can find its callee. Their bodies are
        // emitted afterwards, when every declaration in the program exists --
        // an instantiation's body may call anything, including another
        // instantiation.
        // The trees, so that a template declared in one can be emitted while
        // another is compiled: a family lives in a library and is instantiated
        // by its users, so the body to emit is usually not in this tree.
        self.module_declarations = modules
            .iter()
            .flat_map(|module| module.declarations.iter().cloned())
            .collect();
        // The modules' trees and tables, by name: a default written in one is
        // lowered at a call site in another, and it has to be lowered with the
        // tree it was written in.
        for module in modules {
            self.default_trees.insert(
                module.name.to_owned(),
                DefaultTree {
                    ast: std::rc::Rc::clone(module.ast),
                    types: std::rc::Rc::clone(module.types),
                    resolutions: std::rc::Rc::clone(module.resolutions),
                },
            );
        }
        // A function with `static` storage is stateful even though it may look
        // foldable, so every tree is scanned before any call is lowered.
        self.collect_static_functions(ast);
        for module in modules {
            self.collect_static_functions(module.ast);
        }
        // A setup block anywhere means `main` needs a wrapper: the setups run
        // before it, and the user's `main` becomes a function of its own.
        self.has_setup = std::iter::once(ast)
            .chain(modules.iter().map(|module| &**module.ast))
            .any(|tree| {
                tree.items
                    .iter()
                    .any(|item| matches!(&tree.item(*item).kind, fallc_ast::ItemKind::Setup(_)))
            });
        // The imported modules' declared types, so that a type the unit names --
        // `std.Exception` in a `raise` -- has the same layout here as it does in
        // the module that declared it. The declaring module's copy is preferred
        // over the unit's: the unit learned the type from an exported signature,
        // which carries the shape but not always the fields' own types.
        let unit_types = self.nominal_types.clone();
        for module in modules {
            for declaration in module.declarations {
                self.add_nominal_type(declaration.clone());
            }
        }
        // The unit's own declarations win over anything an import happened to
        // share a name with.
        for (name, nominal) in unit_types {
            self.add_nominal_type(nominal);
            let _ = name;
        }
        // `std.exception()` calls this accessor, and it is defined by code
        // generation rather than by the module. It is emitted as soon as
        // `std.Exception` is in scope -- which is when that module is loaded --
        // so that a program which only *imports* std still links.
        if self.exception_type().is_some() {
            self.handled_global();
        }
        // A module's own templates are instantiated *in that module*: its
        // `__setup__` may construct a member of a family it declared, and the
        // request was recorded by its check, not by this unit's. The bodies are
        // emitted here like the module's other bodies, so its instantiations
        // are declared and emitted here too -- an instantiation carries its own
        // tree, so it does not matter which check asked for it.
        // The instantiations the *modules* asked for: their bodies are part of
        // the library object, so they are compiled into it once rather than in
        // every program that imports the modules.
        let mut module_instantiations: HashMap<String, fallc_sema::Instantiation> = HashMap::new();
        for module in modules {
            for (key, instantiation) in &module.resolutions.instantiations {
                module_instantiations
                    .entry(key.clone())
                    .or_insert_with(|| instantiation.clone());
            }
        }
        // The program-wide set, for declarations: a unit call needs a
        // declaration whether the body comes from the library or from here.
        for (key, instantiation) in &module_instantiations {
            self.instantiations
                .entry(key.clone())
                .or_insert_with(|| instantiation.clone());
        }
        self.declare_instantiations(ast, types, bindings);
        if std::env::var_os("FALLC_STAGES").is_some() {
            eprintln!(
                "  instantiations before unit lowering: {} ({} from modules)",
                self.instantiations.len(),
                modules
                    .iter()
                    .map(|module| module.resolutions.instantiations.len())
                    .sum::<usize>()
            );
        }
        // The unit's own variables take the unit's name; the loop below sets
        // each module's own name before lowering its items.
        let unit_module = self.current_module.clone();
        self.lower_items(ast, types, bindings);
        // A lambda body is emitted after the function that wrote it: its call
        // site only needed the declaration, and lowering a body needs the
        // lowering state to itself.
        self.lower_pending_lambdas();
        // The imported modules are compiled into one library object, and a
        // program that imports the same modules in the same order links that
        // object instead of compiling them again.
        //
        // One object for *all* of them, not one per module: the modules are not
        // independent translation units. Two modules may each export a `pid`,
        // and two private types may each have an `at` method; lowering
        // everything into one module resolves those duplicates the way the
        // in-place path always has, while separate objects would collide at
        // link time.
        let modules_cached = self.link_cached_module_library(modules, &module_instantiations);
        if !modules_cached {
            // The modules' bodies are lowered *before* the instantiations, so
            // that a body of an instantiation can call a module's private
            // function: the call needs a declaration, and a module's functions
            // are declared while its items are lowered.
            for module in modules {
                let saved_types = self.nominal_types.clone();
                self.set_current_module(module.name);
                self.private_function_prefix = true;
                for declaration in module.declarations {
                    self.add_nominal_type(declaration.clone());
                }
                let saved = self.install(module.resolutions);
                self.external_global_refs.extend(
                    module
                        .external_global_refs
                        .iter()
                        .map(|(span, name)| (*span, name.clone())),
                );
                self.lower_items(module.ast, module.types, module.bindings);
                self.lower_pending_lambdas();
                self.install(&saved);
                self.nominal_types = saved_types;
                self.set_current_module(unit_module.clone());
                self.private_function_prefix = false;
            }
        }
        // A cached library supplies the modules' bodies, not their module-level
        // variables: those live in globals the unit never declared. An
        // instantiation emitted here may name one -- `std.print`'s body writes
        // `out` -- so the declarations are made before any instantiation body
        // is lowered. The definitions come from the library object or module.
        if modules_cached {
            for module in modules {
                self.declare_module_globals(module);
                self.declare_module_functions(module);
            }
            if std::env::var_os("FALLC_STAGES").is_some() {
                eprintln!(
                    "  cached module functions declared: {}",
                    self.module_function_symbols.len()
                );
            }
        }
        self.lower_instantiations(ast, types, bindings);
        self.lower_pending_lambdas();
        self.lower_setup_blocks(ast, types, bindings, true);
        if !modules_cached {
            for module in modules {
                // Each module's bodies are lowered with *that module's*
                // resolution decisions installed. A body's calls were resolved
                // during its own check, and the spans in it mean nothing under
                // another module's table: `this->reserve(..)` inside
                // `std.string` is a call the string module resolved, and
                // installing the importer's table instead made it
                // unresolvable.
                //
                // The tables are swapped rather than merged, because spans are
                // not unique across modules -- two modules both have a call at
                // the same byte offset in their own file.
                //
                // The module's own declared types, so that a value it returns
                // by value has the layout its definition gives it. Without them
                // a nominal type resolved to an empty struct here and to a real
                // one at the signature, and the two halves of one function
                // disagreed.
                let saved_types = self.nominal_types.clone();
                for declaration in module.declarations {
                    self.add_nominal_type(declaration.clone());
                }
                let saved = self.install(module.resolutions);
                self.set_current_module(module.name);
                self.private_function_prefix = true;
                // The module's reads of another module's variables, which are
                // recorded by span and therefore belong to its own file.
                self.external_global_refs.extend(
                    module
                        .external_global_refs
                        .iter()
                        .map(|(span, name)| (*span, name.clone())),
                );
                // The bodies were lowered before the instantiations; what is
                // left is each module's `__setup__`, which may call anything by
                // now.
                self.lower_setup_blocks(module.ast, module.types, module.bindings, false);
                self.install(&saved);
                self.nominal_types = saved_types;
                self.set_current_module(unit_module.clone());
                self.private_function_prefix = false;
            }
        }
        // After every setup exists: the wrapper calls them, then `main`. A
        // `main` that may raise was emitted under another name, and the wrapper
        // reports an exception that escaped the program as an exit code.
        self.emit_main_wrapper();
        self.verify_and_finish()
    }

    /// Swaps in a set of resolution decisions, returning the previous ones.
    ///
    /// Every table is replaced wholesale; the caller is responsible for putting
    /// them back, which `lower_with_modules` does.
    pub(super) fn install(&mut self, next: &ModuleResolutions) -> ModuleResolutions {
        ModuleResolutions {
            conversions: std::mem::replace(&mut self.conversions, next.conversions.clone()),
            index_methods: std::mem::replace(&mut self.index_methods, next.index_methods.clone()),
            call_methods: std::mem::replace(&mut self.call_methods, next.call_methods.clone()),
            text_calls: std::mem::replace(&mut self.text_calls, next.text_calls.clone()),
            call_symbols: std::mem::replace(&mut self.call_symbols, next.call_symbols.clone()),
            templated_methods: std::mem::replace(
                &mut self.templated_methods,
                next.templated_methods.clone(),
            ),
            disabled_functions: std::mem::replace(
                &mut self.disabled_functions,
                next.disabled_functions.clone(),
            ),
            closure_calls: std::mem::replace(&mut self.closure_calls, next.closure_calls.clone()),
            interface_conversions: std::mem::replace(
                &mut self.interface_conversions,
                next.interface_conversions.clone(),
            ),
            interface_calls: std::mem::replace(
                &mut self.interface_calls,
                next.interface_calls.clone(),
            ),
            lambda_calls: std::mem::replace(&mut self.lambda_calls, next.lambda_calls.clone()),
            lambda_values: std::mem::replace(&mut self.lambda_values, next.lambda_values.clone()),
            constructor_calls: std::mem::replace(
                &mut self.constructor_calls,
                next.constructor_calls.clone(),
            ),
            constructor_types: std::mem::replace(
                &mut self.constructor_types,
                next.constructor_types.clone(),
            ),
            // Left alone: an instantiation belongs to the *program* rather
            // than to one module. It carries a node, which says which tree its
            // body is in, so installing a module's own (empty) set hid every
            // instantiation a user's file had asked for.
            instantiations: self.instantiations.clone(),
            constant_values: std::mem::replace(
                &mut self.constant_values,
                next.constant_values.clone(),
            ),
            compile_time_ifs: std::mem::replace(
                &mut self.compile_time_ifs,
                next.compile_time_ifs.clone(),
            ),
            by_address_arguments: std::mem::replace(
                &mut self.by_address_arguments,
                next.by_address_arguments.clone(),
            ),
            operator_methods: std::mem::replace(
                &mut self.operator_methods,
                next.operator_methods.clone(),
            ),
            adopted_types: std::mem::replace(&mut self.adopted_types, next.adopted_types.clone()),
            literal_constructions: std::mem::replace(
                &mut self.literal_constructions,
                next.literal_constructions.clone(),
            ),
            literal_construction_types: std::mem::replace(
                &mut self.literal_construction_types,
                next.literal_construction_types.clone(),
            ),
            bool_methods: std::mem::replace(&mut self.bool_methods, next.bool_methods.clone()),
            unary_methods: std::mem::replace(&mut self.unary_methods, next.unary_methods.clone()),
            in_place_methods: std::mem::replace(
                &mut self.in_place_methods,
                next.in_place_methods.clone(),
            ),
            in_place_by_address: std::mem::replace(
                &mut self.in_place_by_address,
                next.in_place_by_address.clone(),
            ),
            init_calls: std::mem::replace(&mut self.init_calls, next.init_calls.clone()),
            method_calls: std::mem::replace(&mut self.method_calls, next.method_calls.clone()),
            indirect_receivers: std::mem::replace(
                &mut self.indirect_receivers,
                next.indirect_receivers.clone(),
            ),
            assign_methods: std::mem::replace(&mut self.assign_methods, next.assign_methods.clone()),
            deref_reads: std::mem::replace(&mut self.deref_reads, next.deref_reads.clone()),
            pointer_targets: std::mem::replace(
                &mut self.pointer_targets,
                next.pointer_targets.clone(),
            ),
            type_of_types: std::mem::replace(&mut self.type_of_types, next.type_of_types.clone()),
            in_place_operations: std::mem::replace(
                &mut self.in_place_operations,
                next.in_place_operations.clone(),
            ),
            default_arguments: std::mem::replace(
                &mut self.default_argument_values,
                next.default_arguments.clone(),
            ),
            default_expressions: std::mem::replace(
                &mut self.default_expressions,
                next.default_expressions.clone(),
            ),
            throwing_calls: std::mem::replace(
                &mut self.throwing_calls,
                next.throwing_calls.clone(),
            ),
            propagating_calls: std::mem::replace(
                &mut self.propagating_calls,
                next.propagating_calls.clone(),
            ),
        }
    }

    /// Emits every function a call instantiated from a template.
    ///
    /// # Why after the items
    ///
    /// An instantiation's body may call anything the program declares,
    /// including another instantiation. Every declaration is made by
    /// `lower_items`, so the instantiations come after it and can reach them
    /// all. They are emitted from the template's *own* body: an instantiation
    /// is not a copy of the source, it is the source seen with the parameters
    /// replaced, so there is nothing else to emit it from.
    ///
    /// A template declared in a module is emitted while that module's
    /// resolutions are installed, and `instantiated` remembers what has been
    /// done, so a template used from two places is one function.
    pub(super) fn declare_instantiations(&mut self, ast: &Ast, types: &TypeTable, bindings: &BindingTypes) {
        for instantiation in self.instantiation_list(ast) {
            self.declare_instantiation(&instantiation);
        }
        let _ = (types, bindings);
    }

    /// Lowers the value a template parameter stands for.
    ///
    /// An integer becomes a constant. A character sequence becomes a constant
    /// array, which is the same thing a byte-string literal is -- so indexing
    /// it, decaying it to a pointer, and comparing it all work exactly as they
    /// do for a literal written in the source.
    pub(super) fn lower_template_value(
        &mut self,
        ast: &Ast,
        declared: Option<fallc_sema::Type>,
        builder: &Builder<'static>,
        argument: &fallc_sema::TemplateArgument,
        span: fallc_span::Span,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        let _ = (ast, builder);
        match argument {
            fallc_sema::TemplateArgument::Int(value) => {
                // The type the parameter *declares* -- `index usize` is a
                // `usize`, not whatever another parameter happened to be.
                let ty = declared.unwrap_or(fallc_sema::Type::INT);
                let llvm = TypeMapper::new(self.backend.context())
                    .map(&ty)
                    .ok_or_else(|| {
                        CodegenError::waiting_for_spec(
                            format!("a value of type `{}`", ty.spelling()),
                            span,
                        )
                    })?;
                let inkwell::types::BasicTypeEnum::IntType(int) = llvm else {
                    return Err(CodegenError::waiting_for_spec(
                        "an integer template argument",
                        span,
                    ));
                };
                Ok(Some(
                    int.const_int(*value as u64, false).as_basic_value_enum(),
                ))
            }
            fallc_sema::TemplateArgument::Text(bytes) => {
                // A private global holding the sequence *with its terminator*.
                //
                // The terminator is the point: `template <name []char>` is
                // given a string literal, and the way such a parameter is used
                // is to walk it to the end. Emitting the bytes bare left
                // `name[n] != '\0'` reading whatever happened to follow in
                // memory, so a length loop ran past the text and
                // `get_pos<4, "abcdef">` returned a byte of an unrelated
                // constant.
                let context = self.backend.context();
                let mut terminated = bytes.clone();
                terminated.push(0);
                let constant = context.const_string(&terminated, false);
                let name = format!(".tmpl.{}", self.string_literal_count);
                self.string_literal_count += 1;
                let global = self
                    .backend
                    .module()
                    .add_global(constant.get_type(), None, &name);
                global.set_initializer(&constant);
                global.set_constant(true);
                global.set_linkage(inkwell::module::Linkage::Private);
                Ok(Some(global.as_pointer_value().as_basic_value_enum()))
            }
            fallc_sema::TemplateArgument::Type(_) => Err(CodegenError::waiting_for_spec(
                "a type used as a value",
                span,
            )),
            // A pack stands for several arguments, so it has no single value.
            // It means something only where it is *expanded*, and the expansion
            // is what produces one value per element -- reaching here means a
            // pack was used as though it named one thing.
            fallc_sema::TemplateArgument::Pack(_) => Err(CodegenError::Emit(
                "internal error: a template parameter pack used as a single value".to_owned(),
            )),
        }
    }

    /// Spells `Box<int>` the way the checker named the member it built.
    ///
    /// The name has to agree exactly, because it is the key the member was
    /// registered under. It is the same rule the checker uses: the base name,
    /// then each argument's spelling, comma-separated.
    pub(super) fn spell_type_arguments(
        &self,
        base: &str,
        arguments: &[fallc_ast::TemplateArgumentSyntax],
    ) -> String {
        let spelled: Vec<String> = arguments
            .iter()
            .map(|argument| match argument {
                // A type argument is resolved here, because the member's name
                // carries the *type* rather than the text.
                fallc_ast::TemplateArgumentSyntax::Type(ty) => self.resolve_type(ty).spelling(),
                fallc_ast::TemplateArgumentSyntax::Value(expr) => {
                    let _ = expr;
                    String::new()
                }
                // A dotted name is a type's or a constant's; only the checker
                // knows which, and this spelling is only for a diagnostic.
                fallc_ast::TemplateArgumentSyntax::Named { path, .. } => path.clone(),
            })
            .collect();
        format!("{base}<{}>", spelled.join(", "))
    }

    /// Declares one instantiation, without emitting its body.
    ///
    /// The signature is the template's own, resolved under the instantiation's
    /// arguments -- so the declaration and the definition below agree by
    /// construction rather than by the rule being written twice.
    pub(super) fn declare_instantiation(&mut self, instantiation: &fallc_sema::Instantiation) {
        // The body travels with the instantiation, so nothing is looked up by
        // node id -- which is an index into one tree, and names different
        // functions in different translation units.
        let function = instantiation.function.clone();
        let Some(substitution) =
            fallc_sema::Substitution::new(&instantiation.params, &instantiation.arguments)
        else {
            return;
        };
        let previous_symbol = self
            .current_symbol
            .replace(instantiation.symbol.clone());
        let previous = self.substitution.replace(substitution);
        let previous_values = std::mem::take(&mut self.template_value_types);
        for param in &instantiation.params {
            if let fallc_ast::TemplateParamKind::Value(ty) = &param.kind {
                let resolved = self.resolve_type(ty);
                self.template_value_types.insert(param.name.clone(), resolved);
            }
        }
        // The parameter types the checker resolved, for the same reason
        // `lower_instantiation` passes them: a written parameter may be a pack
        // expansion, which cannot be expanded here.
        self.pre_resolved_params = instantiation.resolved_params.clone();
        self.pre_resolved_return = Some(instantiation.resolved_returns.clone());
        self.declare_function(&function, &[]);
        self.pre_resolved_params.clear();
        self.pre_resolved_return = None;
        // An instantiation is one function however many translation units ask
        // for it: `Array<int>.push` is written once in its module and used
        // from everywhere, and every unit that needed it would otherwise emit
        // a strong definition of the same symbol. `LinkOnceODR` says the
        // copies are the same function and the linker may keep one, which is
        // what lets a module be compiled to an object of its own.
        self.template_value_types = previous_values;
        self.substitution = previous;
        self.current_symbol = previous_symbol;
    }

    /// Emits the C `main` that calls a throwing `main`.
    ///
    /// An exception may leave a program, and when it does the runtime has to say
    /// so: its name and message go to standard error, and the exit code is a
    /// number of its own (`70`, the conventional "software error"), so a caller
    /// can tell an uncaught exception from a program that returned 70.
    pub(super) fn emit_main_wrapper(&mut self) {
        let context = self.backend.context();
        let module = self.backend.module();
        let i32_type = context.i32_type();
        let pointer = context.ptr_type(inkwell::AddressSpace::default());
        // The setup functions run in dependency order: every imported module's
        // first, in the order the driver loaded them, and this file's last.
        // A cached library keeps its setups in the library object, where this
        // module cannot see them: they are declared here so the wrapper has
        // something to call, and the linker resolves it against that object.
        let setup_type = context.void_type().fn_type(&[], false);
        let setups: Vec<inkwell::values::FunctionValue<'static>> = self
            .module_setup_symbols
            .iter()
            .chain(self.unit_setup_symbols.iter())
            .map(|symbol| {
                module.get_function(symbol).unwrap_or_else(|| {
                    module.add_function(symbol, setup_type, None)
                })
            })
            .collect();
        // The user's `main` is wrapped whenever it cannot stand for the C entry
        // itself: one that raises returns a result, one that returns nothing
        // returns nothing, and one that takes arguments takes two the C runtime
        // supplies. Every shape is adapted here, in one place, so the program's
        // entry means the same thing however it was written.
        let Some(user_main) = module
            .get_function("__ft_main")
            .or_else(|| module.get_function("__ft_user_main"))
        else {
            return;
        };
        if module.get_function("main").is_some() {
            // A program with both a throwing `main` and an ordinary one cannot
            // exist: the parser allows one `main`, and two would be two
            // definitions of the program's entry.
            return;
        }
        // `int main(int argc, char **argv)` is what the C runtime calls, so a
        // wrapped `main` that takes arguments is given them, unchanged.
        let takes_arguments = user_main.get_type().count_param_types() == 2;
        let parameters: Vec<inkwell::types::BasicMetadataTypeEnum<'static>> = if takes_arguments {
            vec![i32_type.into(), pointer.into()]
        } else {
            Vec::new()
        };
        let wrapper = module.add_function("main", i32_type.fn_type(&parameters, false), None);
        let entry = context.append_basic_block(wrapper, "entry");
        let builder = context.create_builder();
        builder.position_at_end(entry);
        // The setups run before the program's own `main`, whatever kind it is.
        for setup in &setups {
            let _ = builder.build_call(*setup, &[], "");
        }
        let arguments: Vec<inkwell::values::BasicMetadataValueEnum<'static>> = wrapper
            .get_params()
            .iter()
            .map(|parameter| inkwell::values::BasicMetadataValueEnum::from(*parameter))
            .collect();
        let Ok(call) = builder.build_call(user_main, &arguments, "result") else {
            return;
        };
        // What the wrapper returns on the success path. A `main` that returns
        // nothing ends the program with status 0, which is what the C runtime
        // would have read from a `void` entry only by accident.
        let zero = i32_type.const_int(0, false).as_basic_value_enum();
        // The caught exception is what the report below prints, so it is kept
        // where the two paths meet.
        let mut exception: Option<BasicValueEnum<'static>> = None;
        let returned = match call.try_as_basic_value() {
            inkwell::values::ValueKind::Basic(BasicValueEnum::StructValue(result)) => {
                let ok = context.append_basic_block(wrapper, "ok");
                let failed = context.append_basic_block(wrapper, "uncaught");
                let fields = result.get_type().count_fields();
                let (Ok(flag), Ok(error)) = (
                    builder.build_extract_value(result, 0, "ok"),
                    builder.build_extract_value(result, fields - 1, "error"),
                ) else {
                    return;
                };
                let BasicValueEnum::IntValue(flag) = flag else {
                    return;
                };
                // The result is `{ ok, value?, error }`: a `main` that returns
                // nothing has no field to read a value from. The value is read
                // before the branch, because a branch ends the block and an
                // instruction added after it leaves the block without a
                // terminator.
                let value = if fields >= 3 {
                    match builder.build_extract_value(result, 1, "value") {
                        Ok(value) => Self::status_value(&builder, value, i32_type, zero),
                        Err(_) => zero,
                    }
                } else {
                    zero
                };
                if builder
                    .build_conditional_branch(flag, ok, failed)
                    .is_err()
                {
                    return;
                }
                exception = Some(error);
                builder.position_at_end(failed);
                // The success path is the end of this function, and it is
                // emitted once the failure path has been written.
                Some((value, ok))
            }
            // An `int` is the status as it stands. Any other width -- a
            // deduced `bool`, say -- is widened or truncated to it, because a
            // program's status is an `int` whatever its `main` wrote.
            inkwell::values::ValueKind::Basic(value @ BasicValueEnum::IntValue(_)) => {
                let status = Self::status_value(&builder, value, i32_type, zero);
                let _ = builder.build_return(Some(&status));
                None
            }
            inkwell::values::ValueKind::Basic(_) => {
                let _ = builder.build_return(Some(&zero));
                None
            }
            inkwell::values::ValueKind::Instruction(_) => {
                let _ = builder.build_return(Some(&zero));
                None
            }
        };
        let (Some((value, ok)), Some(error)) = (returned, exception) else {
            return;
        };
        // `fprintf(stderr, "uncaught exception: %s: %s\n", name, message)`.
        // `stderr` is declared rather than defined: the C runtime owns it, and
        // the linker finds it there.
        let stderr = match module.get_global("stderr") {
            Some(global) => global,
            None => module.add_global(pointer, None, "stderr"),
        };
        let format = context.const_string(b"uncaught exception: %s", true);
        let format_global = module.add_global(format.get_type(), None, ".uncaught");
        format_global.set_initializer(&format);
        format_global.set_constant(true);
        format_global.set_linkage(inkwell::module::Linkage::Private);
        let fprintf = match module.get_function("fprintf") {
            Some(function) => function,
            None => module.add_function(
                "fprintf",
                i32_type.fn_type(&[pointer.into(), pointer.into()], true),
                None,
            ),
        };
        // The exception's two fields are `std.String`s, and their text is what
        // gets printed. Each is given storage and `data()` is called on it; a
        // text that cannot be read becomes the empty string rather than stopping
        // the report.
        let empty = context.const_string(b"\0", false);
        let empty_global = module.add_global(empty.get_type(), None, ".empty.text");
        empty_global.set_initializer(&empty);
        empty_global.set_constant(true);
        empty_global.set_linkage(inkwell::module::Linkage::Private);
        let mut texts: Vec<BasicValueEnum<'static>> = Vec::new();
        if let BasicValueEnum::StructValue(aggregate) = error {
            for index in 0..2 {
                let text = (|| -> Option<BasicValueEnum<'static>> {
                    let field = builder.build_extract_value(aggregate, index, "text").ok()?;
                    let slot = builder.build_alloca(field.get_type(), "held").ok()?;
                    builder.build_store(slot, field).ok()?;
                    // The receiver is mangled from the nominal's *declared*
                    // name, so the qualified spelling an import registers must
                    // be reduced first: `std.string.String` and `String` are one
                    // type, and only one of them is the symbol `data` was
                    // emitted under.
                    let mut string = self
                        .nominal_types
                        .iter()
                        .find(|(name, _)| name.rsplit('.').next() == Some("String"))
                        .map(|(_, nominal)| nominal.clone())?;
                    if let Some(bare) = string.name.rsplit('.').next() {
                        string.name = bare.to_owned();
                    }
                    let receiver = fallc_sema::Type::Reference {
                        referent: Box::new(fallc_sema::Type::Nominal(string)),
                        referent_is_const: true,
                        is_rvalue: false,
                    };
                    let data = self
                        .std_function("data", std::slice::from_ref(&receiver), fallc_span::Span::SYNTHETIC)
                        .ok()?;
                    let call = builder
                        .build_call(data, &[slot.as_basic_value_enum().into()], "text")
                        .ok()?;
                    match call.try_as_basic_value() {
                        inkwell::values::ValueKind::Basic(value) => Some(value),
                        _ => None,
                    }
                })()
                .unwrap_or_else(|| empty_global.as_pointer_value().as_basic_value_enum());
                texts.push(text);
            }
        }
        // `stderr` is a *variable* holding the stream, so its value is loaded;
        // passing its address handed glibc a pointer to the pointer.
        let stream = builder
            .build_load(pointer, stderr.as_pointer_value(), "stderr")
            .unwrap_or_else(|_| stderr.as_pointer_value().as_basic_value_enum());
        // The name always goes out; the message follows it only when there is
        // one, separated by `: `. A `raise "X";` has no message, and printing
        // the separator anyway left `uncaught exception: X: ` with nothing
        // after the colon.
        let mut arguments: Vec<inkwell::values::BasicMetadataValueEnum<'static>> = vec![
            stream.into(),
            format_global.as_pointer_value().into(),
        ];
        let message = texts.get(1).map(|text| text.to_owned());
        if let Some(name) = texts.first() {
            arguments.push((*name).into());
        }
        let _ = builder.build_call(fprintf, &arguments, "report");
        if let Some(message) = message
            && let BasicValueEnum::PointerValue(text) = message
        {
            // Is there anything to say? The message is a C string, so its
            // first byte says: an empty one holds the terminator.
            let first = builder.build_load(context.i8_type(), text, "first");
            if let Ok(first) = first {
                let has_text = builder.build_int_compare(
                    inkwell::IntPredicate::NE,
                    first.into_int_value(),
                    context.i8_type().const_zero(),
                    "has.message",
                );
                let with_message = context.append_basic_block(
                    builder
                        .get_insert_block()
                        .and_then(|block| block.get_parent())
                        .unwrap_or(wrapper),
                    "uncaught.message",
                );
                let after = context.append_basic_block(wrapper, "uncaught.after");
                if let Ok(has_text) = has_text {
                    let _ = builder.build_conditional_branch(has_text, with_message, after);
                    builder.position_at_end(with_message);
                    let message_format = context.const_string(b": %s", true);
                    let message_global =
                        module.add_global(message_format.get_type(), None, ".uncaught.message");
                    message_global.set_initializer(&message_format);
                    message_global.set_constant(true);
                    message_global.set_linkage(inkwell::module::Linkage::Private);
                    let _ = builder.build_call(
                        fprintf,
                        &[
                            stream.into(),
                            message_global.as_pointer_value().into(),
                            text.into(),
                        ],
                        "report.message",
                    );
                    let _ = builder.build_unconditional_branch(after);
                    builder.position_at_end(after);
                }
            }
        }
        // A newline of its own, so the report ends the line whatever was in it.
        let newline = context.const_string(b"\n", true);
        let newline_global = module.add_global(newline.get_type(), None, ".uncaught.end");
        newline_global.set_initializer(&newline);
        newline_global.set_constant(true);
        newline_global.set_linkage(inkwell::module::Linkage::Private);
        let newline_arguments: Vec<inkwell::values::BasicMetadataValueEnum<'static>> = vec![
            stream.into(),
            newline_global.as_pointer_value().into(),
        ];
        let _ = builder.build_call(fprintf, &newline_arguments, "report.end");
        let seventy = i32_type.const_int(70, false).as_basic_value_enum();
        let _ = builder.build_return(Some(&seventy));
        // The success path returns what the program's `main` returned, or 0
        // when it returned nothing.
        builder.position_at_end(ok);
        let _ = builder.build_return(Some(&value));
    }

/// Returns a program status as an `int`, whatever width `main` returned.
///
/// The C runtime reads an `int` from the entry, so a status of another width --
/// a deduced `bool`, say -- is widened or truncated to one. A value that is not
/// an integer at all becomes 0: only `int` and nothing are entry statuses, and
/// the checker says so.
fn status_value(
    builder: &inkwell::builder::Builder<'static>,
    value: BasicValueEnum<'static>,
    i32_type: inkwell::types::IntType<'static>,
    zero: BasicValueEnum<'static>,
) -> BasicValueEnum<'static> {
    let BasicValueEnum::IntValue(status) = value else {
        return zero;
    };
    if status.get_type() == i32_type {
        return status.as_basic_value_enum();
    }
    let coerced = if status.get_type().get_bit_width() < 32 {
        builder.build_int_z_extend(status, i32_type, "status")
    } else {
        builder.build_int_truncate(status, i32_type, "status")
    };
    coerced.map_or(zero, |value| value.as_basic_value_enum())
}

    /// Declares the module-level variables of an imported module.
    ///
    /// Their storage is defined by the module's own object (or by lowering its
    /// bodies in place), but an instantiation lowered here may name them. The
    /// symbol is the one the module emits: `std.io.out`.
    fn declare_module_globals(&mut self, module: &ModuleBodyRef<'_>) {
        for &item in &module.ast.items {
            let fallc_ast::ItemKind::Variable(variable) = &module.ast.item(item).kind else {
                continue;
            };
            let Some(declared) = module.bindings.get(variable.name_span).cloned() else {
                continue;
            };
            let Some(llvm) = TypeMapper::new(self.backend.context()).map(&declared) else {
                continue;
            };
            let symbol = format!("{}.{}", module.name, variable.name);
            let global = match self.backend.module().get_global(&symbol) {
                Some(global) => global,
                None => self.backend.module().add_global(llvm, None, &symbol),
            };
            global.set_linkage(inkwell::module::Linkage::External);
            self.globals
                .insert(variable.name.clone(), (global, llvm));
        }
    }

    /// Declares the functions an imported module defines.
    ///
    /// Their bodies are in the cached library, but an instantiation lowered
    /// here may call one -- including a private function, which importers never
    /// see in an export table. The declaration is built from the module's own
    /// tree under the module's own resolutions, so its symbol and type are the
    /// ones the library emitted.
    fn declare_module_functions(&mut self, module: &ModuleBodyRef<'_>) {
        let saved_types = self.nominal_types.clone();
        for declaration in module.declarations {
            self.add_nominal_type(declaration.clone());
        }
        let saved = self.install_declaration_tables(module.resolutions);
        self.set_current_module(module.name);
        self.private_function_prefix = true;
        for &item in &module.ast.items {
            let item_ref = module.ast.item(item);
            if let fallc_ast::ItemKind::Function(function) = &item_ref.kind
                && function.body.is_some()
            {
                self.declare_function(function, &item_ref.attrs);
            }
        }
        self.type_of_types = saved;
        self.nominal_types = saved_types;
        self.set_current_module("");
        self.private_function_prefix = false;
    }

    /// Swaps in the only per-module table that declaring a function reads.
    ///
    /// `declare_function` resolves written types, and the types a module's
    /// signatures were resolved to are filed by span in `type_of_types`. The
    /// other thirty-odd tables belong to lowering bodies, and cloning them for
    /// a declaration-only pass over seventy modules was most of the pass's cost.
    fn install_declaration_tables(
        &mut self,
        next: &ModuleResolutions,
    ) -> std::collections::HashMap<Span, Type> {
        std::mem::replace(&mut self.type_of_types, next.type_of_types.clone())
    }

    /// Emits the bodies of every instantiation belonging to this module.
    pub(super) fn lower_instantiations(&mut self, ast: &Ast, types: &TypeTable, bindings: &BindingTypes) {
        for instantiation in self.instantiation_list(ast) {
            self.lower_instantiation(bindings, &instantiation);
            // An instantiation is one function however many translation units
            // ask for it: `Array<int>.push` is written once in its module and
            // used from everywhere. `LinkOnceODR` says the copies are the same
            // function, which is what lets a module be compiled to an object of
            // its own without two objects both defining the symbol.
            if let Some(lowered) = self.backend.module().get_function(&instantiation.symbol) {
                lowered.set_linkage(inkwell::module::Linkage::LinkOnceODR);
            }
        }
        let _ = types;
    }

    /// The instantiations this module declares a template for, not yet emitted.
    pub(super) fn instantiation_list(&self, ast: &Ast) -> Vec<fallc_sema::Instantiation> {
        let _ = ast;
        self.instantiations
            .values()
            .filter(|instantiation| {
                !self.instantiated.contains(&instantiation.symbol)
                    && !self.library_instantiated.contains(&instantiation.symbol)
            })
            .cloned()
            .collect()
    }

    /// Emits one instantiation under the symbol the checker recorded.
    pub(super) fn lower_instantiation(
        &mut self,
        bindings: &BindingTypes,
        instantiation: &fallc_sema::Instantiation,
    ) {
        // The *instantiation's* type table, not the program's: the template's
        // body is one set of expressions shared by every instantiation, and the
        // table is keyed by expression. Checking a second instantiation
        // overwrites the first's entries, so lowering the first would read the
        // second's types -- which is how a `double` function came to contain
        // `ret i32`.
        //
        // The body's records are its own for the same reason, and are installed
        // for the whole of its lowering.
        let ast = instantiation.tree.clone();
        let function = instantiation.function.clone();
        let types = &instantiation.types;
        // The body's own binding types, for the same reason as its type table:
        // a local like `fresh *T` has the substituted type `*i32`, and the
        // importer's table has never heard of it.
        let _ = bindings;
        let bindings = &instantiation.bindings;
        let saved_records = self.install_records(&instantiation.records);
        self.instantiated.insert(instantiation.symbol.clone());
        let Some(substitution) =
            fallc_sema::Substitution::new(&instantiation.params, &instantiation.arguments)
        else {
            return;
        };
        let previous = self.substitution.replace(substitution);
        let previous_symbol = self.current_symbol.take();
        let previous_values = std::mem::take(&mut self.template_value_types);
        for param in &instantiation.params {
            if let fallc_ast::TemplateParamKind::Value(ty) = &param.kind {
                let resolved = self.resolve_type(ty);
                self.template_value_types.insert(param.name.clone(), resolved);
            }
        }
        self.current_symbol = Some(instantiation.symbol.clone());
        // The parameter and return types the checker resolved, which is what
        // lets a parameter written `Box<Args...>` become `Box<i32>` here, and
        // what keeps one member's `T` from standing for another's.
        self.pre_resolved_params = instantiation.resolved_params.clone();
        self.pre_resolved_return = Some(instantiation.resolved_returns.clone());
        self.lower_function(&ast, types, bindings, &function);
        self.pre_resolved_params.clear();
        self.pre_resolved_return = None;
        self.template_value_types = previous_values;
        self.current_symbol = previous_symbol;
        self.substitution = previous;
        self.restore_records(saved_records);
    }

    /// Installs one instantiation's records, returning the previous set.
    ///
    /// A template's body is one set of expressions, and everything a check
    /// resolves is filed by span. Two instantiations therefore write to the
    /// same keys, and the records of whichever was checked last are the ones
    /// every instantiation would otherwise be lowered with.
    pub(super) fn install_records(
        &mut self,
        records: &fallc_sema::InstantiationRecords,
    ) -> fallc_sema::InstantiationRecords {
        fallc_sema::InstantiationRecords {
            deref_reads: std::mem::replace(&mut self.deref_reads, records.deref_reads.clone()),
            by_address_arguments: std::mem::replace(
                &mut self.by_address_arguments,
                records.by_address_arguments.clone(),
            ),
            call_symbols: std::mem::replace(&mut self.call_symbols, records.call_symbols.clone()),
            closure_calls: std::mem::replace(
                &mut self.closure_calls,
                records.closure_calls.clone(),
            ),
            interface_conversions: std::mem::replace(
                &mut self.interface_conversions,
                records.interface_conversions.clone(),
            ),
            interface_calls: std::mem::replace(
                &mut self.interface_calls,
                records.interface_calls.clone(),
            ),
            lambda_calls: std::mem::replace(
                &mut self.lambda_calls,
                records.lambda_calls.clone(),
            ),
            lambda_values: std::mem::replace(
                &mut self.lambda_values,
                records.lambda_values.clone(),
            ),
            method_calls: std::mem::replace(&mut self.method_calls, records.method_calls.clone()),
            indirect_receivers: std::mem::replace(
                &mut self.indirect_receivers,
                records.indirect_receivers.clone(),
            ),
            call_methods: std::mem::replace(&mut self.call_methods, records.call_methods.clone()),
            throwing_calls: std::mem::replace(
                &mut self.throwing_calls,
                records.throwing_calls.clone(),
            ),
            propagating_calls: std::mem::replace(
                &mut self.propagating_calls,
                records.propagating_calls.clone(),
            ),
            operator_methods: std::mem::replace(
                &mut self.operator_methods,
                records.operator_methods.clone(),
            ),
            constructor_calls: std::mem::replace(
                &mut self.constructor_calls,
                records.constructor_calls.clone(),
            ),
            constructor_types: std::mem::replace(
                &mut self.constructor_types,
                records.constructor_types.clone(),
            ),
            init_calls: std::mem::replace(&mut self.init_calls, records.init_calls.clone()),
            index_methods: std::mem::replace(&mut self.index_methods, records.index_methods.clone()),
            bool_methods: std::mem::replace(&mut self.bool_methods, records.bool_methods.clone()),
            unary_methods: std::mem::replace(&mut self.unary_methods, records.unary_methods.clone()),
            in_place_methods: std::mem::replace(
                &mut self.in_place_methods,
                records.in_place_methods.clone(),
            ),
            in_place_by_address: std::mem::replace(
                &mut self.in_place_by_address,
                records.in_place_by_address.clone(),
            ),
            assign_methods: std::mem::replace(&mut self.assign_methods, records.assign_methods.clone()),
            literal_constructions: std::mem::replace(
                &mut self.literal_constructions,
                records.literal_constructions.clone(),
            ),
            literal_construction_types: std::mem::replace(
                &mut self.literal_construction_types,
                records.literal_construction_types.clone(),
            ),
            adopted_types: std::mem::replace(&mut self.adopted_types, records.adopted_types.clone()),
            constant_values: std::mem::replace(
                &mut self.constant_values,
                records.constant_values.clone(),
            ),
            compile_time_ifs: std::mem::replace(
                &mut self.compile_time_ifs,
                records.compile_time_ifs.clone(),
            ),
            text_calls: std::mem::replace(&mut self.text_calls, records.text_calls.clone()),
            in_place_operations: std::mem::replace(
                &mut self.in_place_operations,
                records.in_place_operations.clone(),
            ),
        }
    }

    /// Puts back a set of records, discarding whatever was installed.
    pub(super) fn restore_records(&mut self, previous: fallc_sema::InstantiationRecords) {
        let _ = self.install_records(&previous);
    }

}

impl<'ctx> Lowerer<'ctx> {
    /// Emits a tree's `__setup__` blocks as functions that run before `main`.
    ///
    /// The body is lowered through the ordinary function lowering, as a
    /// function of its own, because a setup block is a body: it declares
    /// bindings, destroys them, and may call anything the module can.
    pub(super) fn lower_setup_blocks(
        &mut self,
        ast: &Ast,
        types: &TypeTable,
        bindings: &BindingTypes,
        is_unit: bool,
    ) {
        for &item in &ast.items {
            let fallc_ast::ItemKind::Setup(block) = &ast.item(item).kind else {
                continue;
            };
            let span = ast.block(*block).span;
            // The module's name is part of the symbol so that the setup blocks
            // of every imported module can share one cached library object
            // without an index collision, and so that a cached library's blocks
            // have the names the wrapper looks for.
            let prefix = format!("__ft_setup.{}.", self.current_module);
            let index = self
                .unit_setup_symbols
                .iter()
                .chain(self.module_setup_symbols.iter())
                .filter(|symbol| symbol.starts_with(&prefix))
                .count();
            let symbol = format!("{prefix}{index}");
            let function = fallc_ast::FunctionItem {
                abi: None,
                is_constexpr: false,
                // Exported so that the name is the symbol the `main` wrapper
                // looks for: a private free function would be emitted under the
                // module prefix a second time, and the wrapper's call would
                // refer to nothing. A setup runs for the whole program, which
                // is exactly external linkage.
                exported: true,
                throws: None,
                is_external: false,
                variadic: false,
                template_params: Vec::new(),
                name: symbol.clone(),
                overloaded: false,
                name_span: span,
                params: Vec::new(),
                return_type: fallc_ast::TypeSyntax::Auto(fallc_span::Span::SYNTHETIC),
                body: Some(*block),
            };
            // A setup returns nothing, and the body says so: `return;` is the
            // only form it may write.
            self.pre_resolved_return = Some(fallc_sema::Type::unspecified("", span));
            let previous = self.current_symbol.replace(symbol.clone());
            self.lower_function(ast, types, bindings, &function);
            self.current_symbol = previous;
            self.pre_resolved_return = None;
            if is_unit {
                self.unit_setup_symbols.push(symbol);
            } else {
                self.module_setup_symbols.push(symbol);
            }
        }
    }
}

/// The program-wide tables a module body is lowered against.
///
/// A module body is compiled on its own so that its bitcode can be cached, and
/// that needs a lowerer of its own. Everything in here belongs to the *program*
/// rather than to one module: the checker decided it once, for all the files.
/// It is moved into the child lowerer for the duration of one module and moved
/// back afterwards, so nothing is cloned and the parent's tables gain whatever
/// the module's declarations recorded.
///
/// The per-module decision tables are deliberately absent: those are installed
/// per module by [`Lowerer::install`], and the child installs its own before
/// lowering anything.
struct ModuleScope {
    nominal_types: HashMap<String, fallc_sema::NominalType>,
    module_names: std::collections::HashSet<String>,
    external_symbols: HashMap<String, ExternalSignature>,
    imported_symbols: HashMap<String, (String, String)>,
    external_globals: HashMap<String, (fallc_sema::Type, String, bool)>,
    external_global_refs: HashMap<Span, String>,
    default_trees: HashMap<String, DefaultTree>,
    aliases: HashMap<String, fallc_sema::Type>,
    instantiations: HashMap<String, fallc_sema::Instantiation>,
    binding_alignments: HashMap<Span, u64>,
    static_function_names: std::collections::HashSet<String>,
    module_declarations: Vec<fallc_sema::NominalType>,
    function_defaults: HashMap<String, Vec<Option<fallc_ast::ExprId>>>,
    function_parameters: HashMap<String, Vec<(String, fallc_sema::Type)>>,
    function_values: HashMap<String, Vec<(String, fallc_sema::Type)>>,
    overload_symbols: HashMap<String, Vec<(Vec<fallc_sema::Type>, String)>>,
}

impl Lowerer<'_> {
    /// Moves the program-wide tables out, leaving empty ones behind.
    fn take_module_scope(&mut self) -> ModuleScope {
        ModuleScope {
            nominal_types: std::mem::take(&mut self.nominal_types),
            module_names: std::mem::take(&mut self.module_names),
            external_symbols: std::mem::take(&mut self.external_symbols),
            imported_symbols: std::mem::take(&mut self.imported_symbols),
            external_globals: std::mem::take(&mut self.external_globals),
            external_global_refs: std::mem::take(&mut self.external_global_refs),
            default_trees: std::mem::take(&mut self.default_trees),
            aliases: std::mem::take(&mut self.aliases),
            instantiations: std::mem::take(&mut self.instantiations),
            binding_alignments: std::mem::take(&mut self.binding_alignments),
            static_function_names: std::mem::take(&mut self.static_function_names),
            module_declarations: std::mem::take(&mut self.module_declarations),
            function_defaults: std::mem::take(&mut self.function_defaults),
            function_parameters: std::mem::take(&mut self.function_parameters),
            function_values: std::mem::take(&mut self.function_values),
            overload_symbols: std::mem::take(&mut self.overload_symbols),
        }
    }

    /// Puts the program-wide tables in place.
    fn restore_module_scope(&mut self, scope: ModuleScope) {
        self.nominal_types = scope.nominal_types;
        self.module_names = scope.module_names;
        self.external_symbols = scope.external_symbols;
        self.imported_symbols = scope.imported_symbols;
        self.external_globals = scope.external_globals;
        self.external_global_refs = scope.external_global_refs;
        self.default_trees = scope.default_trees;
        self.aliases = scope.aliases;
        self.instantiations = scope.instantiations;
        self.binding_alignments = scope.binding_alignments;
        self.static_function_names = scope.static_function_names;
        self.module_declarations = scope.module_declarations;
        self.function_defaults = scope.function_defaults;
        self.function_parameters = scope.function_parameters;
        self.function_values = scope.function_values;
        self.overload_symbols = scope.overload_symbols;
    }
}

/// Returns the symbol a tree's `__setup__` block at `index` is emitted under.
///
/// The module's name is part of it so that the blocks of every imported module
/// live in one cached library object without colliding: an index alone would
/// name the first block of every module `__ft_setup.0`.
fn setup_symbol(module: &str, index: usize) -> String {
    format!("__ft_setup.{module}.{index}")
}

/// Returns the setup symbols the given modules define, in the order they run.
fn module_setup_symbols(modules: &[ModuleBodyRef<'_>]) -> Vec<String> {
    let mut symbols = Vec::new();
    for module in modules {
        let setups = module
            .ast
            .items
            .iter()
            .filter(|item| matches!(&module.ast.item(**item).kind, fallc_ast::ItemKind::Setup(_)))
            .count();
        for index in 0..setups {
            symbols.push(setup_symbol(module.name, index));
        }
    }
    symbols
}

/// Returns the key the imported modules' library object is cached under.
///
/// The object is every module body compiled into one LLVM module -- which is
/// what makes duplicate symbols *within* it harmless, exactly as they were when
/// everything was lowered into one module. The key is the modules as loaded:
/// their order decides the order setups run in and which definition of a
/// duplicated symbol wins, so another order is another object.
fn module_library_hash(modules: &[ModuleBodyRef<'_>]) -> u64 {
    crate::codegen::cache::library_hash(
        modules
            .iter()
            .map(|module| (module.name, module.cache_hash)),
    )
}

/// What a compiled module library is written as.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LibraryOutput {
    /// LLVM bitcode, linked into the unit's module.
    Bitcode,
    /// Native object code, handed to the linker.
    Object,
}

impl<'ctx> Lowerer<'ctx> {
    /// Links the imported modules' bodies, using the module-object cache.
    ///
    /// Returns `true` when the library is in this backend's module, whether it
    /// was linked from the cache or compiled just now; `false` when this path
    /// could not be used and the caller must lower the modules in place. The
    /// setup symbols are recorded here because the `main` wrapper calls them and
    /// a cached library's blocks are not walked by the caller.
    fn link_cached_module_library(
        &mut self,
        modules: &[ModuleBodyRef<'_>],
        module_instantiations: &HashMap<String, fallc_sema::Instantiation>,
    ) -> bool {
        if modules.is_empty() {
            return true;
        }
        let hash = module_library_hash(modules);
        if self.object_library {
            return self.link_cached_module_object(modules, module_instantiations, hash);
        }
        let Some(entry) = self.cache_entry(hash) else {
            return false;
        };
        let manifest = entry.with_extension("tl");
        // A library already in the cache is linked in as it stands. The
        // manifest says which instantiations it defines; without it the entry
        // cannot be used, because the unit would emit a second definition of
        // each of those symbols.
        if entry.is_file() && manifest.is_file() {
            if let Some(defined) = read_manifest(&manifest)
                && let Ok(compiled) = self.backend.read_module(&entry)
                && self.link_module(compiled)
            {
                self.library_instantiated = defined;
                self.module_setup_symbols = module_setup_symbols(modules);
                return true;
            }
            // A truncated entry -- a compiler killed while writing one -- is not
            // fatal: drop it and build the library again below.
            let _ = std::fs::remove_file(&entry);
            let _ = std::fs::remove_file(&manifest);
        }
        let Some((built, setups, defined)) =
            self.compile_module_library(
                modules,
                module_instantiations,
                &entry,
                LibraryOutput::Bitcode,
            )
        else {
            return false;
        };
        let compiled = match self.backend.read_module(&built) {
            Ok(compiled) => compiled,
            Err(_) => {
                let _ = std::fs::remove_file(&built);
                return false;
            }
        };
        if self.link_module(compiled) {
            // Both files appear only once they are complete: a temporary file
            // and a rename, so a second compiler reading the cache at the same
            // time never sees a half-written entry. If the manifest cannot be
            // written, the bitcode goes too: an entry without one is unusable.
            let temporary_manifest = entry.with_extension(format!("tl.{}", std::process::id()));
            let wrote_manifest = std::fs::write(&temporary_manifest, manifest_text(&defined))
                .and_then(|()| std::fs::rename(&temporary_manifest, &manifest))
                .is_ok();
            let _ = std::fs::rename(&built, &entry);
            if !wrote_manifest {
                let _ = std::fs::remove_file(&entry);
                return false;
            }
            self.library_instantiated = defined.into_iter().collect();
            self.module_setup_symbols = setups;
        } else {
            let _ = std::fs::remove_file(&built);
        }
        true
    }

    /// Reuses the imported modules' bodies as a native object file.
    ///
    /// The object is built once, optimised for the level the program is being
    /// built at, and cached beside the bitcode entry. A later compilation only
    /// records the path: the linker combines it with the unit's own object, so
    /// machine code for the standard library is not generated again.
    fn link_cached_module_object(
        &mut self,
        modules: &[ModuleBodyRef<'_>],
        module_instantiations: &HashMap<String, fallc_sema::Instantiation>,
        hash: u64,
    ) -> bool {
        let Some((object, manifest)) = self.object_library_entries(hash) else {
            return false;
        };
        if object.is_file() && manifest.is_file() {
            if let Some(defined) = read_manifest(&manifest) {
                self.library_instantiated = defined;
                self.module_setup_symbols = module_setup_symbols(modules);
                self.library_objects.push(object);
                return true;
            }
            let _ = std::fs::remove_file(&object);
            let _ = std::fs::remove_file(&manifest);
        }
        let Some((built, setups, defined)) =
            self.compile_module_library(modules, module_instantiations, &object, LibraryOutput::Object)
        else {
            return false;
        };
        // Both files appear complete or not at all; an entry without its
        // manifest is unusable, because the unit would emit a second definition
        // of every instantiation the object already defines.
        let temporary_manifest = object.with_extension(format!("tl.{}", std::process::id()));
        let wrote_manifest = std::fs::write(&temporary_manifest, manifest_text(&defined))
            .and_then(|()| std::fs::rename(&temporary_manifest, &manifest))
            .is_ok();
        if !wrote_manifest {
            let _ = std::fs::remove_file(&built);
            return false;
        }
        if std::fs::rename(&built, &object).is_err() {
            let _ = std::fs::remove_file(&manifest);
            return false;
        }
        self.library_instantiated = defined.into_iter().collect();
        self.module_setup_symbols = setups;
        self.library_objects.push(object);
        true
    }

    /// Links a compiled module into this backend's module.
    fn link_module(&mut self, compiled: inkwell::module::Module<'static>) -> bool {
        match self.backend.link_in_module(compiled) {
            Ok(()) => true,
            Err(error) => {
                // A library built from these same sources in this same context
                // has to link; failing means the cache or the symbol rules are
                // wrong, and reporting it is better than silently emitting a
                // program with pieces missing.
                self.diagnostics.push(error.to_diagnostic());
                true
            }
        }
    }

    /// Compiles the imported modules' bodies and setups into one LLVM module of
    /// their own, and writes it as bitcode under a temporary name.
    ///
    /// The parent's program-wide tables are moved across for the duration, so
    /// the module's bodies are lowered against the same declarations they were
    /// before the cache existed.
    fn compile_module_library(
        &mut self,
        modules: &[ModuleBodyRef<'_>],
        module_instantiations: &HashMap<String, fallc_sema::Instantiation>,
        entry: &std::path::Path,
        output: LibraryOutput,
    ) -> Option<(std::path::PathBuf, Vec<String>, Vec<String>)> {
        let backend = self.backend.new_in("module.library");
        let mut child = Lowerer::with_mode(&backend, self.mode);
        let scope = self.take_module_scope();
        child.restore_module_scope(scope);
        // The library emits the instantiations the modules themselves asked
        // for. The parent's table (the unit's requests) is set aside for the
        // duration and put back before the scope returns to the parent.
        let inherited_instantiations =
            std::mem::replace(&mut child.instantiations, module_instantiations.clone());
        child.private_function_prefix = true;
        child.has_setup = false;
        // `std.exception()` reaches a global this compiler emits rather than a
        // module defines. It has weak linkage, so the parent and the library may
        // both define it and the linker keeps one.
        if child.exception_type().is_some() {
            child.handled_global();
        }
        // Every instantiation the modules asked for is declared before any body
        // is lowered: a module body may call one, and the body is emitted below.
        // The tree argument is unused: an instantiation carries its own tree,
        // so declaring them needs nothing from the caller. Passing an empty
        // tree keeps the library compilation from copying a module's tree for
        // a parameter nothing reads.
        child.declare_instantiations(
            &fallc_ast::Ast::new(),
            modules[0].types,
            modules[0].bindings,
        );
        // The bodies first, then the setups: a setup of a module may call
        // anything the module's body declared, and the two passes are how the
        // in-place path orders them too.
        for module in modules {
            child.set_current_module(module.name);
            child.private_function_prefix = true;
            for declaration in module.declarations {
                child.add_nominal_type(declaration.clone());
            }
            let saved = child.install(module.resolutions);
            child.external_global_refs.extend(
                module
                    .external_global_refs
                    .iter()
                    .map(|(span, name)| (*span, name.clone())),
            );
            child.lower_items(module.ast, module.types, module.bindings);
            child.lower_pending_lambdas();
            child.install(&saved);
            child.private_function_prefix = false;
        }
        // The instantiations come after the bodies, so a member's body may call
        // any module function, and before the setups, so a setup may call one.
        child.lower_instantiations(
            &fallc_ast::Ast::new(),
            modules[0].types,
            modules[0].bindings,
        );
        child.lower_pending_lambdas();
        for module in modules {
            for declaration in module.declarations {
                child.add_nominal_type(declaration.clone());
            }
            let saved = child.install(module.resolutions);
            child.set_current_module(module.name);
            child.private_function_prefix = true;
            child.external_global_refs.extend(
                module
                    .external_global_refs
                    .iter()
                    .map(|(span, name)| (*span, name.clone())),
            );
            child.lower_setup_blocks(module.ast, module.types, module.bindings, false);
            child.install(&saved);
            child.private_function_prefix = false;
        }
        let setups = std::mem::take(&mut child.module_setup_symbols);
        let mut defined: Vec<String> = child.instantiated.iter().cloned().collect();
        defined.sort();
        // The symbols these bodies define are part of the program's picture: the
        // instantiation and setup lowering that follows asks whether a symbol
        // exists in this module.
        self.emitted_symbols
            .extend(std::mem::take(&mut child.emitted_symbols));
        child.instantiations = inherited_instantiations;
        let scope = child.take_module_scope();
        self.restore_module_scope(scope);
        // A warning does not stop the library being used, but it is reported;
        // the in-place path runs only when there is an error, and its
        // diagnostics are deduplicated against these.
        let mut diagnostics = std::mem::take(&mut child.diagnostics);
        let failed = diagnostics.has_errors();
        self.diagnostics.absorb(&mut diagnostics);
        if failed {
            return None;
        }
        if let Err(error) = backend.verify() {
            self.diagnostics.push(error.to_diagnostic());
            return None;
        }
        let temporary = entry.with_extension(format!("tmp.{}", std::process::id()));
        let written = match output {
            LibraryOutput::Bitcode => backend.write_bitcode_to_file(&temporary),
            LibraryOutput::Object => {
                // The object is machine code, so it is optimised here: the
                // unit's own module is optimised by the driver, but this module
                // is never part of it again.
                if let Err(error) = backend.optimize(self.optimization) {
                    self.diagnostics.push(error.to_diagnostic());
                    return None;
                }
                backend.write_position_independent_object_file(&temporary, self.optimization.as_llvm())
            }
        };
        if written.is_err() {
            return None;
        }
        Some((temporary, setups, defined))
    }
}

/// Reads the list of instantiations a cached library defines.
///
/// `None` means the manifest is unreadable, which makes the entry unusable: the
/// unit would otherwise emit a second definition of every symbol the library
/// already defines.
fn read_manifest(path: &std::path::Path) -> Option<std::collections::HashSet<String>> {
    let text = std::fs::read_to_string(path).ok()?;
    Some(text.lines().map(str::to_owned).collect())
}

/// Renders the instantiation manifest of a cached library.
fn manifest_text(defined: &[String]) -> String {
    let mut text = defined.join("\n");
    text.push('\n');
    text
}
