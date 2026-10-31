//! What the driver says about a run, and the library entry points.
//!
//! `finish` turns a session into an outcome and `report` prints what it has to
//! say; the `compile_text*` functions are the same path without files, which is
//! how tests compile a string.

use super::*;

pub(super) fn show_tokens(kinds: &[EmitKind]) -> bool {
    kinds.contains(&EmitKind::Tokens)
}

pub(super) fn show_ast(kinds: &[EmitKind]) -> bool {
    kinds.contains(&EmitKind::Ast)
}

pub(super) fn show_types(kinds: &[EmitKind]) -> bool {
    kinds.contains(&EmitKind::Types)
}

/// Produces the final outcome for a file once every requested stage has run.
pub(super) fn finish(session: &Session) -> Outcome {
    if session.has_errors() {
        Outcome::Failure
    } else if session.has_unspecified() {
        Outcome::Unspecified
    } else {
        Outcome::Success
    }
}

/// Prints diagnostics and the trailing summary.
pub(super) fn report(session: &Session) {
    let rendered = session.render_diagnostics();
    if !rendered.is_empty() {
        eprint!("{rendered}");
    }

    let errors = session.count(fallc_diag::Severity::Error);
    let warnings = session.count(fallc_diag::Severity::Warning);
    if let Some((span, what)) = session.unspecified().first() {
        eprintln!(
            "note: this compiler cannot yet process {what} (at {}).",
            session.sources().describe(*span)
        );
        eprintln!(
            "note: the program is understood; it is this part of the code generator that is \
             missing."
        );
    }
    if errors > 0 || warnings > 0 {
        let color = session.color();
        eprintln!(
            "{} {}, {} {}",
            fallc_diag::paint(
                &errors.to_string(),
                if errors > 0 { "1;31" } else { "1" },
                color
            ),
            if errors == 1 { "error" } else { "errors" },
            fallc_diag::paint(
                &warnings.to_string(),
                if warnings > 0 { "1;33" } else { "1" },
                color
            ),
            if warnings == 1 { "warning" } else { "warnings" }
        );
    }
}

/// Compiles in-memory source through every stage, without touching the file
/// system.
///
/// Runs the same pipeline as `compile_file`, so a test exercising this
/// exercises the real compiler. Returns the outcome plus everything that would
/// have been printed, diagnostics and emitted output together.
/// Compiles source text with LLVM's optimiser run over the result.
///
/// Used by tests that check a constant-folded outcome: at `-O0` a debug build
/// guards arithmetic, so a fold does not happen and the guard structure is the
/// only evidence of how an expression grouped. Folding is the stronger check
/// where it is available.
pub fn compile_text_optimized(
    name: &str,
    text: &str,
    emit_kinds: &[EmitKind],
    opt: fallc_ir::OptLevel,
) -> (Outcome, String) {
    compile_text_in_mode_opt(name, text, emit_kinds, fallc_ir::BuildMode::Debug, opt)
}

pub fn compile_text(name: &str, text: &str, emit_kinds: &[EmitKind]) -> (Outcome, String) {
    compile_text_in_mode(name, text, emit_kinds, fallc_ir::BuildMode::Debug)
}

/// The same, with an explicit build mode.
pub fn compile_text_in_mode(
    name: &str,
    text: &str,
    emit_kinds: &[EmitKind],
    mode: fallc_ir::BuildMode,
) -> (Outcome, String) {
    compile_text_in_mode_opt(name, text, emit_kinds, mode, fallc_ir::OptLevel::None)
}

/// Compiles source text with an explicit warning policy.
///
/// The driver builds one from `-W` flags; a test that wants to see how a
/// policy changes what is reported builds one directly.
pub fn compile_text_with_warnings(
    name: &str,
    text: &str,
    emit_kinds: &[EmitKind],
    warnings: &fallc_diag::WarningPolicy,
) -> (Outcome, String) {
    compile_text_in_mode_opt_with_warnings(
        name,
        text,
        emit_kinds,
        fallc_ir::BuildMode::Debug,
        fallc_ir::OptLevel::None,
        warnings,
    )
}

/// Compiles source text, choosing both the build mode and the optimisation
/// level.
pub fn compile_text_in_mode_opt(
    name: &str,
    text: &str,
    emit_kinds: &[EmitKind],
    mode: fallc_ir::BuildMode,
    opt: fallc_ir::OptLevel,
) -> (Outcome, String) {
    compile_text_in_mode_opt_with_warnings(
        name,
        text,
        emit_kinds,
        mode,
        opt,
        &fallc_diag::WarningPolicy::new(),
    )
}

/// The one implementation both entry points share.
pub(super) fn compile_text_in_mode_opt_with_warnings(
    name: &str,
    text: &str,
    emit_kinds: &[EmitKind],
    mode: fallc_ir::BuildMode,
    opt: fallc_ir::OptLevel,
    warnings: &fallc_diag::WarningPolicy,
) -> (Outcome, String) {
    let mut session = Session::new();
    session.set_warnings(warnings.clone());
    session.set_input_files([std::path::PathBuf::from(name)]);
    let file: FileId = session.add_file(name, text);
    let owned = session
        .sources()
        .file(file)
        .map(|f| f.text().to_owned())
        .unwrap_or_default();

    let mut output = String::new();

    // --- lex ---------------------------------------------------------------
    let mut lexer = Lexer::new(LexerConfig {
        preserve_trivia: true,
        errors_are_warnings: false,
    });
    let lexed = lexer.tokenize(&owned, file);
    session.report_all(lexed.diagnostics.clone());
    if emit_kinds.contains(&EmitKind::Tokens) {
        output.push_str(&emit::tokens(&lexed, session.sources()));
    }
    if session.has_errors() {
        return (Outcome::Failure, join(session.render_diagnostics(), output));
    }
    // The same prelude the file entry point installs: a module's `export
    // define`s, collected from the modules this name imports.
    let mut macro_cache = imports::MacroCache::new();
    let prelude = imports::collect_module_macros(
        &mut session,
        &lexed.tokens,
        Path::new(name),
        &imports::search_path_for(Path::new(name)),
        &mut macro_cache,
    );
    let (tokens, problems) = expand_macros(
        lexed.tokens,
        &prelude,
        &crate::pipeline::builtins_for(Path::new(name), &owned),
    );
    session.report_all(problems);
    if session.has_errors() {
        return (Outcome::Failure, join(session.render_diagnostics(), output));
    }

    // --- parse -------------------------------------------------------------
    let mut stream = TokenStream::new(tokens);
    let mut parser = Parser::new(ParserConfig::default());
    parser.set_source(owned);
    let mut parsed = parser.parse_file(&mut stream);
    // Whether a name is overloaded is a property of the whole file, so it is
    // decided once parsing is complete. Both entry points must do this, or a
    // program compiled through one of them behaves differently.
    parsed.ast.mark_overloads();
    add_exception_runtime(&mut parsed.ast);
    session.report_all(parsed.diagnostics.clone());
    if emit_kinds.contains(&EmitKind::Ast) {
        output.push_str(&emit::ast(&parsed.ast));
    }
    if !parsed.is_ok() {
        return (Outcome::Failure, join(session.render_diagnostics(), output));
    }
    let ast = parsed.ast;
    // The same entry check the file path makes: a program compiled from text is
    // still a program, and a `main` of the wrong shape must be reported here
    // too rather than reaching code generation.
    super::compile::check_entry_function(&ast, &mut session);

    // --- load imported modules ---------------------------------------------
    // The dry-run path has no object of its own, so it checks what it is asked
    // about; a program compiled from text is normally a small probe, and the
    // cache decision is the file path's job.
    let imported = load_imports(
        &mut session,
        &ast,
        Path::new(name),
        CheckMode::Full,
        &mut macro_cache,
    );
    if session.has_errors() {
        return (Outcome::Failure, join(session.render_diagnostics(), output));
    }

    // --- resolve -----------------------------------------------------------
    let mut resolver = fallc_sema::Resolver::new();
    // Taken before the destructuring below, which moves the modules out.
    let imported_types = imported.exported_types();
    // A module's exported variables travel exactly as its functions do: an
    // imported name that stands for the library's storage is still a name this
    // file type-checks and generates code for.
    let imported_variables = imported.exported_variables();
    // Taken before the destructuring below, which moves the modules out. A
    // trait is a name this file may ask about, exactly as a type is a name it
    // may mention.
    let imported_traits = imported.exported_traits();
    // An enum is a type plus its members' values; an interface is a type plus
    // its method list. Both are members a program names, so both are taken
    // before the destructuring below moves the modules out.
    let imported_globals = imported.exported_globals();
    let imported_constants = imported.exported_constants();
    let imported_enums = imported.exported_enums();
    let imported_interfaces = imported.exported_interfaces();
    let LoadedImports {
        modules,
        direct,
        umbrellas: _,
        externals,
        bindings,
        aliases,
        links: module_links,
        bodies: mut module_bodies,
    } = imported;
    // This file's own `link` instructions, followed by those of the modules it
    // imports: a module's link instruction applies to whatever imports it, so
    // `standard/C.ftm` can say which libraries the C bindings need.
    let mut links = collect_links(&ast, &[]);
    for link in module_links {
        if !links.contains(&link) {
            links.push(link);
        }
    }
    let module_names = install_imports(&mut resolver, modules, bindings, &aliases, &direct);
    // A trait used as a question rather than as a value: `Input(x)` is not a
    // call to an undeclared function.
    add_known_traits(&mut resolver, &ast, &imported_traits);
    // The types this file can name, for the same reason the other entry point
    // does it: a type name is not a value, and a query names one.
    add_known_types(
        &mut resolver,
        &ast,
        &imported_types,
        &imported_enums,
        &imported_interfaces,
    );
    let resolved = resolver.resolve(&ast);
    session.report_all(resolved.diagnostics.clone());
    if !resolved.is_ok() {
        return (Outcome::Failure, join(session.render_diagnostics(), output));
    }

    // --- type check --------------------------------------------------------
    let mut checker = TypeChecker::new(fallc_ir::Backend::pointer_bits());
    for module in &module_names {
        checker.add_module(module.clone());
    }
    register_imported_types(&mut checker, &imported_types);
    register_imported_constants(&mut checker, &imported_constants);
    register_imported_enums(&mut checker, &imported_enums);
    register_imported_interfaces(&mut checker, &imported_interfaces);
    // The third field says whether the object is one per thread; an
    // `@extern_C` variable is a plain global.
    let mut external_globals: std::collections::HashMap<String, (fallc_sema::Type, String, bool)> =
        register_imported_variables(&mut checker, &imported_variables)
            .into_iter()
            .map(|(name, (ty, symbol))| (name, (ty, symbol, false)))
            .collect();
    // An exported module-level variable is storage in another module, read
    // through its symbol exactly as an `extern` one is.
    external_globals.extend(register_imported_globals(&mut checker, &imported_globals));
    // Constants an import brought in: `C.EOF` is a value, and the checker is
    // what folds values.
    checker.add_imported_constants(resolved.constants_by_name.clone());
    register_traits(&mut checker, &ast, &resolved.resolutions, &imported_traits);
    // Where every templated function's body is, from this file and every module
    // it imports. The other entry point does this too, and without it a
    // template -- or a method of a type family -- cannot be instantiated at
    // all: its body is looked up and found missing.
    for body in &module_bodies {
        let families = exports::family_parameter_names(&body.ast);
        for item in &body.ast.items {
            if let fallc_ast::ItemKind::Function(function) = &body.ast.item(*item).kind
                && (!function.template_params.is_empty()
                    || function.has_pack()
                    || exports::method_of_family(function, &families))
            {
                checker.add_template_definition(
                    symbol_of_template(&body.ast, function),
                    fallc_sema::TemplateBody {
                        tree: std::rc::Rc::clone(&body.ast),
                        resolutions: std::rc::Rc::clone(&body.checker_resolutions),
                        constants: body.constant_values.clone(),
                        constant_types: body.constant_types.clone(),
                        function: function.clone(),
                        attrs: body.ast.item(*item).attrs.clone(),
                        environment: Some(std::rc::Rc::clone(&body.environment)),
                        bindings: super::module_variable_bindings(&body.ast, &body.binding_types),
                    },
                );
            }
        }
    }
    let families = exports::family_parameter_names(&ast);
    // Shared by every template this file declares: one copy of the tree and of
    // the resolution table, not one per template.
    let shared_ast = std::rc::Rc::new(ast.clone());
    let shared_resolutions = std::rc::Rc::new(resolved.resolutions.clone());
    for item in &ast.items {
        if let fallc_ast::ItemKind::Function(function) = &ast.item(*item).kind
            && (!function.template_params.is_empty()
                || function.has_pack()
                || exports::method_of_family(function, &families))
        {
            checker.add_template_definition(
                symbol_of_template(&ast, function),
                fallc_sema::TemplateBody {
                    tree: std::rc::Rc::clone(&shared_ast),
                    resolutions: std::rc::Rc::clone(&shared_resolutions),
                    constants: std::collections::HashMap::new(),
                    constant_types: std::collections::HashMap::new(),
                    function: function.clone(),
                    attrs: ast.item(*item).attrs.clone(),
                    environment: None,
                    bindings: fallc_sema::BindingTypes::new(),
                },
            );
        }
        // The checker now owns the template bodies' tables; the module objects'
        // copies were only needed to register them.
        for body in &mut module_bodies {
            body.release_check_tables();
        }
    }
    let checked = checker.check(&ast, &resolved.resolutions);
    session.report_all(checked.diagnostics.clone());
    if emit_kinds.contains(&EmitKind::Types) {
        output.push_str(&emit::types(&checked.types, session.sources()));
    }
    if !checked.is_ok() {
        return (Outcome::Failure, join(session.render_diagnostics(), output));
    }

    // --- lower to LLVM IR --------------------------------------------------
    let backend = fallc_ir::Backend::new(&module_name_for(Path::new(name)));
    let mut lowerer = fallc_ir::Lowerer::with_mode(&backend, mode);
    // The module-object cache is enabled when its directory can be created; a
    // read-only checkout simply compiles without one.
    if let Some(directory) = crate::pipeline::cache::build_directory() {
        lowerer.set_module_cache(directory);
    }
    lowerer.set_current_module(module_name_for(Path::new(name)));
    configure_lowerer(
        &mut lowerer,
        &module_names,
        &checked.types_declarations,
        &checked.type_of_types,
        &checked.conversions,
        &checked.index_methods,
        &checked.call_methods,
        &checked.text_calls,
        &checked.call_symbols,
        &checked.templated_methods,
        &checked.disabled_functions,
        &checked.closure_calls,
        &checked.interface_conversions,
        &checked.interface_calls,
        &checked.lambda_calls,
        &checked.lambda_values,
        &checked.constructor_calls,
        &checked.constructor_types,
        &checked.instantiations,
        &checked.constant_values,
        &checked.compile_time_ifs,
        &checked.default_arguments,
        &checked.default_expressions,
        &checked.aliases,
        &checked.by_address_arguments,
        &checked.operator_methods,
        &checked.adopted_types,
        &checked.literal_constructions,
        &checked.literal_construction_types,
        &checked.bool_methods,
        &checked.unary_methods,
        &checked.in_place_methods,
        &checked.in_place_by_address,
        &checked.init_calls,
        &checked.method_calls,
        &checked.coroutine_calls,
        &checked.indirect_receivers,
        &checked.assign_methods,
        &checked.deref_reads,
        &checked.pointer_targets,
        &checked.throwing_calls,
        &checked.propagating_calls,
        &checked.in_place_operations,
        &resolved.resolutions,
        externals,
    );
    lowerer.add_external_variables(external_globals, checked.external_global_refs.clone());
    lowerer.add_binding_alignments(checked.binding_alignments.clone());
    let borrowed: Vec<fallc_ir::ModuleBodyRef<'_>> = module_bodies
        .iter()
        .map(|body| fallc_ir::ModuleBodyRef {
            name: &body.module,
            cache_hash: body.cache_hash,
            ast: &body.ast,
            types: &body.types,
            bindings: &body.binding_types,
            declarations: &body.declarations,
            resolutions: &body.resolved,
            external_global_refs: &body.external_global_refs,
        })
        .collect();
    let lowered =
        lowerer.lower_with_modules(&ast, &checked.types, &checked.binding_types, &borrowed);
    session.report_all(lowered.diagnostics.clone());
    if !lowered.is_ok() {
        return (Outcome::Failure, join(session.render_diagnostics(), output));
    }

    // The optimiser runs before the IR is rendered, so that the text reflects
    // the level that was asked for.
    if let Err(error) = backend.optimize(opt) {
        session.report(error.to_diagnostic());
        return (Outcome::Failure, join(session.render_diagnostics(), output));
    }
    let optimized_ir = backend.to_llvm_ir().0;
    if emit_kinds.contains(&EmitKind::Llvm) {
        output.push_str(&emit::llvm_ir(&optimized_ir));
    }

    let outcome = finish(&session);
    (outcome, join(session.render_diagnostics(), output))
}

/// Concatenates diagnostics and emitted output, keeping output first when
/// there are no diagnostics.
pub(super) fn join(diagnostics: String, output: String) -> String {
    if diagnostics.is_empty() {
        output
    } else {
        format!("{diagnostics}{output}")
    }
}
