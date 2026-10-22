//! Compiling a program: the path from options to an object file.
//!
//! `run` decides what kind of invocation this is; the functions here do the
//! work of one compilation, from reading the inputs through lowering, emitting
//! and writing the artefacts.

use super::*;

/// Checks the One Definition Rule across a set of source files.
///
/// # The rule
///
/// A function defined in two files is an error: the program would have two
/// implementations of one symbol, and which one ran would depend on link order.
/// A function merely *declared* in two files is fine, and is the whole point of
/// a declaration: it says the definition lives somewhere else.
///
/// A definition is an item with a body. `extern "C" export fn puts(text *const
/// c8) int;` has none, so any number of files may declare it.
///
/// Returns the first violation, with both places named.
pub(super) fn check_one_definition_rule(session: &mut Session, inputs: &[PathBuf]) -> Result<(), String> {
    // Symbol to the file that defined it, so the second definition can name the
    // first.
    let mut defined: std::collections::HashMap<String, PathBuf> =
        std::collections::HashMap::new();
    for path in inputs {
        let Ok(text) = fs::read_to_string(path) else {
            // Unreadable files are reported by the compilation itself, which
            // has better wording for it.
            continue;
        };
        let file_id = session.add_file(path.clone(), text.clone());
        let mut lexer = Lexer::new(LexerConfig {
            preserve_trivia: false,
            errors_are_warnings: false,
        });
        let lexed = lexer.tokenize(&text, file_id);
        // Expanded here too: a `$NAME` that produces a definition, or an
        // `import`, changes what the pass sees.
        let (tokens, _) = expand_macros(
            lexed.tokens,
            &[],
            &crate::pipeline::builtins_for(path, &text),
        );
        let mut stream = TokenStream::new(tokens);
        let mut parser = Parser::new(ParserConfig::default());
        parser.set_source(text);
        let parsed = parser.parse_file(&mut stream);
        for &item in &parsed.ast.items {
            let fallc_ast::ItemKind::Function(function) = &parsed.ast.item(item).kind else {
                continue;
            };
            // Only a definition counts. A declaration has no body and says the
            // symbol comes from elsewhere.
            if function.body.is_none() {
                continue;
            }
            // A private free function is emitted under its module's prefix, so
            // two modules may define the same name. Exported functions are the
            // program's public names and must stay unique.
            let symbol = if function.exported
                || function
                    .params
                    .first()
                    .is_some_and(|parameter| parameter.name == "this")
            {
                function.name.clone()
            } else {
                format!("{}.{}", super::linking::module_name_for(path), function.name)
            };
            if let Some(first) = defined.get(&symbol) {
                return Err(format!(
                    "`{symbol}` is defined in both `{}` and `{}`; \
                     a symbol may be defined only once across a program",
                    first.display(),
                    path.display()
                ));
            }
            defined.insert(symbol, path.clone());
        }
    }
    Ok(())
}

/// Compiles one file to an object, reporting a single-file program's stages.
///
/// Used by the multi-file path, where each file produces its own object and the
/// link happens once at the end.
#[allow(clippy::too_many_arguments)]
pub(super) fn compile_to_object(
    session: &mut Session,
    path: &Path,
    emit_kinds: &[EmitKind],
    preserve_trivia: bool,
    check_only: bool,
    mode: fallc_ir::BuildMode,
    opt: fallc_ir::OptLevel,
    object: &Path,
) -> Outcome {
    // The per-file artefacts -- tokens, AST, types -- are emitted from here, so
    // a multi-file build can still show them. The object goes to the path the
    // caller chose rather than a derived one.
    let mut kinds: Vec<EmitKind> = emit_kinds
        .iter()
        .copied()
        .filter(|kind| {
            !matches!(
                kind,
                EmitKind::Object
                    | EmitKind::Static
                    | EmitKind::Shared
                    | EmitKind::Executable
            )
        })
        .collect();
    kinds.push(EmitKind::Object);
    compile_file(
        session,
        path,
        &kinds,
        preserve_trivia,
        check_only,
        mode,
        opt,
        true,
        true,
        Some(object),
    )
}

/// Collects the link instructions any of these files or their modules declare.
pub(super) fn collect_all_links(session: &mut Session, inputs: &[PathBuf]) -> Vec<LinkRequest> {
    let mut links = Vec::new();
    for path in inputs {
        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };
        let file_id = session.add_file(path.clone(), text.clone());
        let mut lexer = Lexer::new(LexerConfig {
            preserve_trivia: false,
            errors_are_warnings: false,
        });
        let lexed = lexer.tokenize(&text, file_id);
        // Expanded here too: a `$NAME` that produces a definition, or an
        // `import`, changes what the pass sees.
        let (tokens, _) = expand_macros(
            lexed.tokens,
            &[],
            &crate::pipeline::builtins_for(path, &text),
        );
        let mut stream = TokenStream::new(tokens);
        let mut parser = Parser::new(ParserConfig::default());
        parser.set_source(text);
        let parsed = parser.parse_file(&mut stream);
        for link in collect_links(&parsed.ast, &[]) {
            if !links.contains(&link) {
                links.push(link);
            }
        }
    }
    links
}

/// Expands a file's macros, between lexing and parsing.
///
/// A macro is a rewrite of tokens into tokens, so it happens while the file is
/// still a token stream: the parser never sees a `define`, and a `$NAME` has
/// already become the tokens it stands for by the time an item is read. The
/// expansion is per file, which is where a definition lives.
pub(super) fn expand_macros(
    tokens: Vec<fallc_lexer::Token>,
    prelude: &[fallc_lexer::macros::Macro],
    builtins: &fallc_lexer::macros::Builtins,
) -> (Vec<fallc_lexer::Token>, Vec<Diagnostic>) {
    match fallc_lexer::macros::expand_macros_with(tokens, prelude, builtins) {
        Ok((tokens, _)) => (tokens, Vec::new()),
        // The tokens are dropped when expansion failed: a half-expanded
        // stream would be parsed as though it were a program, and the
        // diagnostic already says what is wrong.
        Err(problems) => (
            Vec::new(),
            problems
                .into_iter()
                .map(|problem| {
                    Diagnostic::error(problem.message)
                        .with_code(DiagnosticCode::lexer(problem.code))
                        .with_span(problem.span)
                })
                .collect(),
        ),
    }
}

/// Compiles one or more source files into one program.
///
/// # Why several files are one program
///
/// `fallc a.ft b.ft -o prog` compiles both and links them together, so a
/// function may be defined in one file and called from another. That is what
/// makes the language usable for anything larger than one file, and it is the
/// C model: each file is a translation unit, and the linker joins them.
///
/// # One Definition Rule
///
/// Every file is compiled to its own object, so a symbol defined twice would
/// otherwise be caught only by the linker, with a message about a mangled name
/// and no source location. The check is made here, across the units, before
/// anything is linked: a function defined in two files is reported with both
/// definitions.
///
/// The rule is C's: a function may be *declared* in as many files as like, but
/// *defined* at most once across the program. A declaration is how one file
/// says "this exists somewhere", which is exactly what a multi-file program
/// needs, so it is not a violation.
#[allow(clippy::too_many_arguments)]
pub(super) fn compile_inputs(
    session: &mut Session,
    inputs: &[PathBuf],
    emit_kinds: &[EmitKind],
    preserve_trivia: bool,
    check_only: bool,
    mode: fallc_ir::BuildMode,
    opt: fallc_ir::OptLevel,
    keep_temporaries: bool,
    output: Option<&Path>,
) -> Outcome {
    // A single file keeps the original path, which handles `-o` naming, its own
    // temporary object, and the emit kinds in one place.
    if inputs.len() == 1 {
        return compile_file(
            session,
            &inputs[0],
            emit_kinds,
            preserve_trivia,
            check_only,
            mode,
            opt,
            keep_temporaries,
            false,
            output,
        );
    }

    compile_several(
        session,
        inputs,
        emit_kinds,
        preserve_trivia,
        check_only,
        mode,
        opt,
        keep_temporaries,
        output,
    )
}

/// Compiles several files and links them into one program.
///
/// Each file is compiled separately -- its own parse, its own type check, its
/// own object -- so that the module system and the One Definition Rule mean
/// what they do in C. A file that calls a function defined in another declares
/// it with `extern` and a link name, or imports a module that does.
#[allow(clippy::too_many_arguments)]
pub(super) fn compile_several(
    session: &mut Session,
    inputs: &[PathBuf],
    emit_kinds: &[EmitKind],
    preserve_trivia: bool,
    check_only: bool,
    mode: fallc_ir::BuildMode,
    opt: fallc_ir::OptLevel,
    keep_temporaries: bool,
    output: Option<&Path>,
) -> Outcome {
    // A static or shared library is one object set, and several inputs are one
    // program; supporting them together means deciding which file's objects the
    // library is. That is a design question, not an oversight, so it is refused
    // rather than half-done.
    if inputs.len() > 1
        && (emit_kinds.contains(&EmitKind::Static) || emit_kinds.contains(&EmitKind::Shared))
    {
        session.report(
            Diagnostic::error("a static or shared library must be built from one input file")
                .with_code(DiagnosticCode::driver(2))
                .with_note("compile the program into an object, or link it into an executable"),
        );
        return Outcome::Failure;
    }

    let wants_object = emit_kinds.contains(&EmitKind::Object);
    let wants_executable = emit_kinds.contains(&EmitKind::Executable) || emit_kinds.is_empty();

    // Every file defines a set of symbols. The rule is checked before anything
    // is compiled, so a violation is reported with both locations rather than
    // as a linker error.
    if let Err(message) = check_one_definition_rule(session, inputs) {
        session.report(
            Diagnostic::error(message).with_code(DiagnosticCode::driver(6)),
        );
        return Outcome::Failure;
    }

    // Each file is compiled to an object. The objects go beside their own
    // sources, so a later link needs no cross-filesystem move.
    let mut objects: Vec<PathBuf> = Vec::new();
    let mut outcome = Outcome::Success;
    for (index, path) in inputs.iter().enumerate() {
        let object = if wants_object {
            artifact_path(output, EmitKind::Object, index == inputs.len() - 1)
                .unwrap_or_else(|| default_output_path(path, "o"))
        } else {
            temporary_object_path(path)
        };
        outcome = outcome.max(compile_to_object(
            session,
            path,
            emit_kinds,
            preserve_trivia,
            check_only,
            mode,
            opt,
            &object,
        ));
        if outcome == Outcome::Failure {
            return Outcome::Failure;
        }
        objects.push(object);
    }

    if check_only || !wants_executable {
        return finish(session);
    }

    let executable = output.map_or_else(|| default_output_path(&inputs[0], ""), Path::to_path_buf);
    // The modules any of the files import declare what the program links
    // against, so the union of them is what the link line needs.
    let links = collect_all_links(session, inputs);
    let linked = link_executable(&objects, &executable, &links);

    // The objects are the intermediate files of this link, so they go unless
    // they were asked for.
    if !wants_object && !keep_temporaries {
        for object in &objects {
            let _ = fs::remove_file(object);
        }
    }

    match linked {
        Ok(()) => finish(session),
        Err(message) => {
            session.report(Diagnostic::error(message).with_code(DiagnosticCode::driver(4)));
            Outcome::Failure
        }
    }
}

/// Compiles one file, running as many stages as the input allows.
#[allow(clippy::too_many_arguments)]
pub(super) fn compile_file(
    session: &mut Session,
    path: &Path,
    emit_kinds: &[EmitKind],
    preserve_trivia: bool,
    check_only: bool,
    mode: fallc_ir::BuildMode,
    opt: fallc_ir::OptLevel,
    keep_temporaries: bool,
    multiple_units: bool,
    output: Option<&Path>,
) -> Outcome {
    let stage_start = std::time::Instant::now();
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => {
            session.report(
                Diagnostic::error(format!("cannot read `{}`: {error}", path.display()))
                    .with_code(DiagnosticCode::driver(1)),
            );
            return Outcome::Failure;
        }
    };
    let file = session.add_file(path.to_path_buf(), text);
    let text = session
        .sources()
        .file(file)
        .map(|f| f.text().to_owned())
        .unwrap_or_default();

    // --- lex ---------------------------------------------------------------
    let mut lexer = Lexer::new(LexerConfig {
        preserve_trivia,
        errors_are_warnings: false,
    });
    let lexed = lexer.tokenize(&text, file);
    session.report_all(lexed.diagnostics.clone());
    if show_tokens(emit_kinds) {
        let text = emit::tokens(&lexed, session.sources());
        let target = artifact_path(output, EmitKind::Tokens, is_first(emit_kinds, EmitKind::Tokens));
        if write_emitted(session, target, &text).is_err() {
            return Outcome::Failure;
        }
    }
    if session.has_errors() {
        return Outcome::Failure;
    }
    // The macros this file's modules publish are installed before its own
    // stream is expanded: `export define` is a declaration like any other, and
    // an importing file may use it.
    let mut macro_cache = imports::MacroCache::new();
    let prelude = imports::collect_module_macros(
        session,
        &lexed.tokens,
        path,
        &imports::search_path_for(path),
        &mut macro_cache,
    );
    let (tokens, problems) = expand_macros(
        lexed.tokens,
        &prelude,
        &crate::pipeline::builtins_for(path, &text),
    );
    session.report_all(problems);
    if session.has_errors() {
        return Outcome::Failure;
    }

    // What this invocation will produce decides how the module library is
    // kept. A program that will be linked gets a native object library, which
    // keeps the standard library's machine code out of every compilation. The
    // IR-emitting paths link the library into the module instead, because their
    // answer is the IR text itself.
    let wants_object_code = emit_kinds.contains(&EmitKind::Object);
    let wants_llvm = emit_kinds.contains(&EmitKind::Llvm);
    let wants_program = emit_kinds.contains(&EmitKind::Executable) || emit_kinds.is_empty();
    let wants_static = emit_kinds.contains(&EmitKind::Static);
    let wants_shared = emit_kinds.contains(&EmitKind::Shared);
    // Anything that will be linked from object files wants the module library
    // as object code; an archive or a shared object is built from objects, so
    // it always does. With `--emit=llvm` alone the library is linked into the
    // module instead, because the answer is the IR text.
    let object_library =
        wants_static || wants_shared || (wants_program && !wants_object_code && !wants_llvm);

    // --- parse -------------------------------------------------------------
    let mut stream = TokenStream::new(tokens);
    let mut parser = Parser::new(ParserConfig::default());
    parser.set_source(text.clone());
    let parsed = parser.parse_file(&mut stream);
    session.report_all(parsed.diagnostics.clone());
    if !parsed.is_ok() {
        return Outcome::Failure;
    }
    let mut ast: Ast = parsed.ast;
    // Whether a name is overloaded is a property of the whole file, so it is
    // decided once parsing is complete.
    ast.mark_overloads();
    // The exception machinery is the language's, not the library's: a file that
    // raises gets the runtime it needs without having to import it.
    add_exception_runtime(&mut ast);

    // The entry the program is run through is `main`, and the C runtime calls
    // it with a fixed signature. A `main` of any other shape is a mistake in
    // this file, reported here -- the alternative was a link failure naming a
    // missing `main`, or a `void` entry whose return value the runtime read
    // from whatever happened to be there.
    check_entry_function(&ast, session);

    if show_ast(emit_kinds) {
        let text = emit::ast(&ast);
        let target = artifact_path(output, EmitKind::Ast, is_first(emit_kinds, EmitKind::Ast));
        if write_emitted(session, target, &text).is_err() {
            return Outcome::Failure;
        }
    }

    // --- load imported modules ---------------------------------------------
    stage("parse", &stage_start);
    // A cached module object carries the checked implementation of the modules
    // it was built from, so a program that can reuse one only needs their
    // declarations. Try that pass first: it is the cheap one, and when the
    // cache has the object the expensive one never runs. When the cache does
    // not, the first pass is rolled back and the modules are checked fully --
    // their diagnostics are the ones that matter, and reporting the same
    // mistake twice would be noise.
    let checkpoint = session.diagnostics_checkpoint();
    let probe = load_imports(
        session,
        &ast,
        path,
        CheckMode::DeclarationsOnly,
        &mut macro_cache,
    );
    let cached_library = crate::pipeline::cache::build_directory().is_some_and(|directory| {
        let modules: Vec<(&str, u64)> = probe
            .bodies
            .iter()
            .map(|body| (body.module.as_str(), body.cache_hash))
            .collect();
        let (artifact, manifest) = fallc_ir::codegen::cache::library_entries(
            &directory,
            &modules,
            mode,
            object_library.then_some(opt),
        );
        artifact.is_file() && manifest.is_file()
    });
    if std::env::var_os("FALLC_STAGES").is_some() {
        eprintln!("  cached library: {cached_library}");
    }
    let imported = if cached_library {
        probe
    } else {
        session.rollback_diagnostics(checkpoint);
        drop(probe);
        load_imports(session, &ast, path, CheckMode::Full, &mut macro_cache)
    };
    stage("load imports", &stage_start);
    // An import that failed makes every use of it an error too, and those
    // errors say nothing new. Reporting the cause alone is what keeps a single
    // missing file from becoming a page of diagnostics.
    if session.has_errors() {
        return Outcome::Failure;
    }

    // --- resolve -----------------------------------------------------------
    let mut resolver = fallc_sema::Resolver::new();
    // Taken before the destructuring below, which moves the modules out.
    let imported_types = imported.exported_types();
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
    // The types this file can name, so that `Array<int>()` is not reported as
    // an unknown name: a type is not a value, and the resolver only knows
    // values.
    add_known_types(
        &mut resolver,
        &ast,
        &imported_types,
        &imported_enums,
        &imported_interfaces,
    );
    stage("exports+resolve setup", &stage_start);
    let resolved = resolver.resolve(&ast);
    stage("resolve", &stage_start);
    session.report_all(resolved.diagnostics.clone());
    if !resolved.is_ok() {
        return Outcome::Failure;
    }
    let resolutions: Resolutions = resolved.resolutions;

    // --- type check --------------------------------------------------------
    // The target's pointer width, taken once for every checker: `sizeof` of a
    // pointer -- and of any type containing one -- is that width, and a checker
    // without it would answer "not a constant" for types whose layout is
    // perfectly known.
    let mut checker = TypeChecker::new(fallc_ir::Backend::pointer_bits());
    // The checker needs the module names so it can tell `C.printf(..)` from a
    // method call: the two are written identically.
    for module in &module_names {
        checker.add_module(module.clone());
    }
    register_imported_types(&mut checker, &imported_types);
    register_imported_constants(&mut checker, &imported_constants);
    register_imported_enums(&mut checker, &imported_enums);
    register_imported_interfaces(&mut checker, &imported_interfaces);
    // The third field says whether the object is one per thread; an
    // `extern "C"` variable is a plain global.
    let mut external_globals: std::collections::HashMap<
        String,
        (fallc_sema::Type, String, bool),
    > = register_imported_variables(&mut checker, &imported_variables)
        .into_iter()
        .map(|(name, (ty, symbol))| (name, (ty, symbol, false)))
        .collect();
    // An exported module-level variable is storage in another module, read
    // through its symbol exactly as an `extern` one is.
    external_globals.extend(register_imported_globals(&mut checker, &imported_globals));
    // Constants an import brought in: `C.EOF` is a value, and the checker is
    // what folds values.
    checker.add_imported_constants(resolved.constants_by_name.clone());
    register_traits(&mut checker, &ast, &resolutions, &imported_traits);
    // Where every templated function's body is, from this file and every module
    // it imports. A template declared in one module is instantiated from
    // another, so the checker that instantiates it needs a tree it never saw.
    // The *function itself*, not its node. A node id is an index into one
    // tree, so the same number means different functions in different
    // translation units -- and looking one up by number across trees found
    // `String`'s `push` when `Array`'s `length` was wanted.
    for body in &module_bodies {
        let families = exports::family_parameter_names(&body.ast);
        for item in &body.ast.items {
            if let fallc_ast::ItemKind::Function(function) = &body.ast.item(*item).kind
                && (!function.template_params.is_empty()
                    || function.has_pack()
                    || exports::method_of_family(function, &families))
            {
                // Everything the body needs travels with it: the tree, the
                // declarations its names resolve to, and the constants it may
                // mention. A template declared in a library is checked from
                // whichever file instantiates it, and that file has usually
                // never heard of the library's own names.
                // Folded where the module's own constant index exists: a
                // constant is identified by an expression id, and an id belongs
                // to one tree.
                let constants = body.constant_values.clone();
                let constant_types = body.constant_types.clone();
                // Keyed by the *symbol*, which carries the parameter types:
                // `Array` has three `__init__` overloads, and one key per name
                // meant the last one registered stood in for all of them --
                // the copy constructor answering a call to the empty one.
                checker.add_template_definition(
                    symbol_of_template(&body.ast, function),
                    fallc_sema::TemplateBody {
                        tree: std::rc::Rc::clone(&body.ast),
                        resolutions: std::rc::Rc::clone(&body.checker_resolutions),
                        constants,
                        constant_types,
                        function: function.clone(),
                        attrs: body.ast.item(*item).attrs.clone(),
                        environment: Some(std::rc::Rc::clone(&body.environment)),
                        bindings: super::module_variable_bindings(&body.ast, &body.binding_types),
                    },
                );
            }
        }
    }
    // The file being compiled declares templates too, and it was left out:
    // only imported modules were registered, so a template type declared and
    // used in one file had no body to instantiate -- `Box<int>`'s methods and
    // its destructor were looked up, found nothing, and were never emitted.
    // Its own table is the program's, because that is what the body's names
    // were resolved against when the file was checked.
    let families = exports::family_parameter_names(&ast);
    // Shared by every template this file declares: one copy of the tree and of
    // the resolution table, not one per template.
    let shared_ast = std::rc::Rc::new(ast.clone());
    let shared_resolutions = std::rc::Rc::new(resolutions.clone());
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
                    // This file's own declarations are the checker's tables,
                    // so there is nothing to borrow.
                    environment: None,
                    bindings: fallc_sema::BindingTypes::new(),
                },
            );
        }
    }
    stage("checker setup", &stage_start);
    // The checker now owns the template bodies' tables; the copies held by
    // the module objects were only there to register them.
    for body in &mut module_bodies {
        body.release_check_tables();
    }
    let checked = checker.check(&ast, &resolutions);
    stage("check", &stage_start);
    session.report_all(checked.diagnostics.clone());
    if !checked.is_ok() {
        return Outcome::Failure;
    }
    let types: TypeTable = checked.types;

    if show_types(emit_kinds) {
        let text = emit::types(&types, session.sources());
        let target = artifact_path(output, EmitKind::Types, is_first(emit_kinds, EmitKind::Types));
        if write_emitted(session, target, &text).is_err() {
            return Outcome::Failure;
        }
    }

    if check_only {
        return finish(session);
    }

    // --- lower to LLVM IR --------------------------------------------------
    //
    // The backend owns the LLVM context and module, so it must outlive every
    // LLVM object derived from it. It is created here and consumed within this
    // function for exactly that reason.
    let backend = fallc_ir::Backend::new(&module_name_for(path));
    let mut lowerer = fallc_ir::Lowerer::with_mode(&backend, mode);
    // The module-object cache is enabled when its directory can be created; a
    // read-only checkout simply compiles without one.
    if let Some(directory) = crate::pipeline::cache::build_directory() {
        lowerer.set_module_cache(directory);
    }
    if object_library {
        lowerer.set_object_library(opt);
    }
    lowerer.set_private_function_prefix(multiple_units);
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
        &checked.assign_methods,
        &checked.deref_reads,
        &checked.pointer_targets,
        &checked.throwing_calls,
        &checked.propagating_calls,
        &checked.in_place_operations,
        &resolutions,
        externals,
    );
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
    lowerer.add_external_variables(external_globals, checked.external_global_refs.clone());
    lowerer.add_binding_alignments(checked.binding_alignments.clone());
    stage("codegen setup", &stage_start);
    let lowered = lowerer.lower_with_modules(&ast, &types, &checked.binding_types, &borrowed);
    stage("lower", &stage_start);
    session.report_all(lowered.diagnostics.clone());
    if !lowered.is_ok() {
        return Outcome::Failure;
    }

    // The optimiser runs before anything is emitted, so that `--emit=llvm`
    // shows the same code the object file will contain. Reporting the level
    // here rather than at emission is what makes the IR a truthful answer to
    // "what will this compile to".
    if let Err(error) = backend.optimize(opt) {
        session.report(error.to_diagnostic());
        return Outcome::Failure;
    }
    // Re-rendered from the module rather than reused from `lowered`: the text
    // was produced before the passes ran, so printing it would show the
    // unoptimised code however high `-O` was. That is exactly what it did.
    let optimized_ir = backend.to_llvm_ir().0;

    if emit_kinds.contains(&EmitKind::Llvm) {
        let text = emit::llvm_ir(&optimized_ir);
        let target = artifact_path(output, EmitKind::Llvm, is_first(emit_kinds, EmitKind::Llvm));
        if write_emitted(session, target, &text).is_err() {
            return Outcome::Failure;
        }
    }

    // --- emit object code --------------------------------------------------
    //
    // The object is written to a temporary file even when the user only wants
    // something linked from it, because linking needs one; in that case it is
    // removed afterwards.
    let wants_object = emit_kinds.contains(&EmitKind::Object);
    let wants_executable = emit_kinds.contains(&EmitKind::Executable) || emit_kinds.is_empty();

    if wants_object || wants_executable || wants_static || wants_shared {
        let object_path = if wants_object {
            artifact_path(output, EmitKind::Object, is_first(emit_kinds, EmitKind::Object))
                .unwrap_or_else(|| default_output_path(path, "o"))
        } else {
            temporary_object_path(path)
        };

        // The target machine optimises machine code, so it follows `-O` rather
        // than the build mode: a debug build at `-O2` should still produce
        // optimised machine code. A shared object needs position-independent
        // code, and position-independent code is valid in an executable too, so
        // a build that wants both is built the shared way.
        let written = if wants_shared {
            backend.write_position_independent_object_file(&object_path, opt.as_llvm())
        } else {
            backend.write_object_file(&object_path, opt.as_llvm())
        };
        if let Err(error) = written {
            session.report(error.to_diagnostic());
            return Outcome::Failure;
        }

        let mut objects = vec![object_path.clone()];
        objects.extend(lowered.library_objects.iter().cloned());

        // Linking happens whenever an executable was asked for, including when
        // the object was asked for as well: `--emit=all` wants both, and
        // producing only the object made "all" a lie. The object then keeps its
        // own `-o`-derived name rather than being a temporary to clean up.
        if wants_executable {
            let executable = if wants_object {
                artifact_path(output, EmitKind::Executable, true)
                    .unwrap_or_else(|| default_output_path(path, ""))
            } else {
                output.map_or_else(|| default_output_path(path, ""), Path::to_path_buf)
            };
            if let Err(message) = link_executable(&objects, &executable, &links) {
                session.report(Diagnostic::error(message).with_code(DiagnosticCode::driver(4)));
                return Outcome::Failure;
            }
        }

        if wants_static {
            let library = artifact_path(output, EmitKind::Static, is_first(emit_kinds, EmitKind::Static))
                .unwrap_or_else(|| default_output_path(path, "a"));
            if let Err(message) = create_static_library(&objects, &library) {
                session.report(Diagnostic::error(message).with_code(DiagnosticCode::driver(4)));
                return Outcome::Failure;
            }
        }

        if wants_shared {
            let library = artifact_path(output, EmitKind::Shared, is_first(emit_kinds, EmitKind::Shared))
                .unwrap_or_else(|| default_output_path(path, extension_of(EmitKind::Shared)));
            if let Err(message) = link_shared(&objects, &library, &links) {
                session.report(Diagnostic::error(message).with_code(DiagnosticCode::driver(4)));
                return Outcome::Failure;
            }
        }

        // The intermediate object is removed unless it was asked for, or the
        // user asked to keep everything. Otherwise it is a build product nobody
        // wanted, left beside the source.
        if !wants_object && !keep_temporaries {
            let _ = fs::remove_file(&object_path);
        }
    }

    finish(session)
}

/// Prints a compilation stage's time and memory use when `FALLC_STAGES` is set.
///
/// Memory is the resident set, read from `/proc/self/statm`: peak use is what
/// matters for the machines a compiler runs on, and a stage that allocates and
/// frees is invisible in an end-of-run figure.
fn stage(name: &str, start: &std::time::Instant) {
    if std::env::var_os("FALLC_STAGES").is_none() {
        return;
    }
    let resident = std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|text| text.split_whitespace().nth(1).and_then(|pages| pages.parse::<u64>().ok()))
        .map(|pages| pages * 4096 / (1024 * 1024))
        .unwrap_or(0);
    eprintln!(
        "stage {name:<22} {:>8.3}s  {resident:>6} MB",
        start.elapsed().as_secs_f64()
    );
}

/// Tells a lowerer everything the resolver and checker learned.
///
/// Both entry points must do exactly this, and they must do all of it. Setting
/// a lowerer up in two places is how the two came to disagree before: the
/// module names were installed in one and the imported symbols in the other, so
/// the same source generated code from the command line and failed from a test.
#[allow(clippy::too_many_arguments)]
pub(super) fn configure_lowerer(
    lowerer: &mut fallc_ir::Lowerer<'_>,
    module_names: &[String],
    declared_types: &[fallc_sema::NominalType],
    type_of_types: &std::collections::HashMap<fallc_span::Span, fallc_sema::Type>,
    conversions: &std::collections::HashMap<fallc_span::Span, fallc_sema::ConversionMethod>,
    index_methods: &std::collections::HashMap<fallc_span::Span, String>,
    call_methods: &std::collections::HashMap<fallc_span::Span, String>,
    text_calls: &std::collections::HashMap<fallc_span::Span, Option<String>>,
    call_symbols: &std::collections::HashMap<fallc_span::Span, String>,
    templated_methods: &std::collections::HashSet<fallc_span::Span>,
    disabled_functions: &std::collections::HashSet<fallc_span::Span>,
    closure_calls: &std::collections::HashMap<fallc_span::Span, fallc_sema::ClosureCall>,
    interface_conversions: &std::collections::HashMap<
        fallc_span::Span,
        fallc_sema::InterfaceConversion,
    >,
    interface_calls: &std::collections::HashMap<fallc_span::Span, fallc_sema::InterfaceCall>,
    lambda_calls: &std::collections::HashMap<fallc_span::Span, fallc_sema::LambdaInstance>,
    lambda_values: &std::collections::HashMap<fallc_span::Span, fallc_sema::LambdaInstance>,
    constructor_calls: &std::collections::HashMap<fallc_span::Span, String>,
    constructor_types: &std::collections::HashMap<fallc_span::Span, fallc_sema::Type>,
    instantiations: &std::collections::HashMap<String, fallc_sema::Instantiation>,
    constant_values: &std::collections::HashMap<fallc_span::Span, i64>,
    compile_time_ifs: &std::collections::HashMap<fallc_span::Span, bool>,
    default_arguments: &std::collections::HashMap<
        fallc_span::Span,
        Vec<(usize, i64, fallc_sema::Type)>,
    >,
    default_expressions: &fallc_sema::ExpressionDefaults,
    aliases: &std::collections::HashMap<String, fallc_sema::Type>,
    by_address_arguments: &std::collections::HashSet<fallc_span::Span>,
    operator_methods: &std::collections::HashMap<fallc_span::Span, fallc_sema::OperatorCall>,
    adopted_types: &std::collections::HashMap<fallc_span::Span, fallc_sema::Type>,
    literal_constructions: &std::collections::HashMap<fallc_span::Span, String>,
    literal_construction_types: &std::collections::HashMap<fallc_span::Span, fallc_sema::Type>,
    bool_methods: &std::collections::HashMap<fallc_span::Span, String>,
    unary_methods: &std::collections::HashMap<fallc_span::Span, String>,
    in_place_methods: &std::collections::HashMap<fallc_span::Span, String>,
    in_place_by_address: &std::collections::HashMap<fallc_span::Span, bool>,
    init_calls: &std::collections::HashMap<fallc_span::Span, String>,
    method_calls: &std::collections::HashMap<fallc_span::Span, String>,
    assign_methods: &std::collections::HashMap<fallc_span::Span, String>,
    deref_reads: &std::collections::HashMap<fallc_span::Span, fallc_sema::Type>,
    pointer_targets: &std::collections::HashMap<fallc_span::Span, fallc_sema::Type>,
    throwing_calls: &std::collections::HashSet<fallc_span::Span>,
    propagating_calls: &std::collections::HashSet<fallc_span::Span>,
    in_place_operations: &std::collections::HashMap<
        fallc_span::Span,
        fallc_sema::InPlaceOperation,
    >,
    resolutions: &Resolutions,
    externals: std::collections::HashMap<String, fallc_ir::ExternalSignature>,
) {
    for module in module_names {
        lowerer.add_module(module.clone());
    }
    // The code generator resolves a few types on its own, such as a
    // constructor's receiver, so it needs the declared types.
    for def in declared_types {
        lowerer.add_nominal_type(def.clone());
    }
    // The type each `type(expr)` names, answered where the expression was in
    // hand. Code generation resolves a few written types itself, and the answer
    // travels by span because there is no tree to ask there.
    lowerer.set_type_of_types(type_of_types.clone());
    // Which casts are conversion methods is a type-level decision, so it is
    // made once by the checker and handed over rather than worked out again.
    lowerer.set_conversions(conversions.clone());
    lowerer.set_index_methods(index_methods.clone());
    lowerer.set_call_methods(call_methods.clone());
    lowerer.set_text_calls(text_calls.clone());
    lowerer.set_call_symbols(call_symbols.clone());
    lowerer.set_templated_methods(templated_methods.clone());
    lowerer.set_disabled_functions(disabled_functions.clone());
    lowerer.set_closure_calls(closure_calls.clone());
    lowerer.set_interface_conversions(interface_conversions.clone());
    lowerer.set_interface_calls(interface_calls.clone());
    lowerer.set_lambda_calls(lambda_calls.clone());
    lowerer.set_lambda_values(lambda_values.clone());
    lowerer.set_constructor_calls(constructor_calls.clone());
    lowerer.set_constructor_types(constructor_types.clone());
    lowerer.set_instantiations(instantiations.clone());
    lowerer.set_constant_values(constant_values.clone());
    // The `if`s the checker decided, so code generation emits only the live
    // branch. The other was never checked and has no types to lower with.
    lowerer.set_compile_time_ifs(compile_time_ifs.clone());
    // The constants a call site fills omitted defaults with: the expression is
    // in the declaring module's tree, so the value was folded there.
    lowerer.set_default_arguments(default_arguments.clone());
    lowerer.set_default_expressions(default_expressions.clone());
    // An alias means the same thing in both stages, so code generation gets the
    // table rather than resolving `type Id = usize;` again by its own rules.
    lowerer.set_aliases(aliases.clone());
    lowerer.set_by_address_arguments(by_address_arguments.clone());
    lowerer.set_operator_methods(operator_methods.clone());
    lowerer.set_adopted_types(adopted_types.clone());
    lowerer.set_literal_constructions(literal_constructions.clone());
    lowerer.set_literal_construction_types(literal_construction_types.clone());
    lowerer.set_bool_methods(bool_methods.clone());
    lowerer.set_unary_methods(unary_methods.clone());
    lowerer.set_in_place_methods(in_place_methods.clone());
    lowerer.set_in_place_by_address(in_place_by_address.clone());
    lowerer.set_init_calls(init_calls.clone());
    lowerer.set_method_calls(method_calls.clone());
    lowerer.set_assign_methods(assign_methods.clone());
    lowerer.set_throwing_calls(throwing_calls.clone(), propagating_calls.clone());
    lowerer.set_deref_reads(deref_reads.clone());
    lowerer.set_pointer_targets(pointer_targets.clone());
    lowerer.set_in_place_operations(in_place_operations.clone());
    for (name, signature) in externals {
        lowerer.add_external(name, signature);
    }
    // A name that came from an import is a local name here and the defining
    // module's symbol at link time. Recording the mapping is what lets a bare
    // imported call -- `printf` after `from C import printf` -- emit the right
    // symbol, and the qualified name is what its signature is filed under,
    // which may be an umbrella module rather than the one that declared it.
    for def in &resolutions.defs {
        if let (Some(symbol), Some(module)) = (&def.imported_as, &def.imported_via) {
            lowerer.add_imported_symbol(
                def.name.clone(),
                symbol.clone(),
                format!("{module}.{symbol}"),
            );
        }
    }
}

/// Registers the types the imported modules export with a type checker.
///
/// A type is published on the same terms as a function, so only exported ones
/// arrive here; the rest stay the declaring module's own business. Field types
/// are written types, and use the same resolution a signature does.
/// Registers the `extern` variables imported modules exported.
///
/// The written type travels with the export and is resolved here, against this
/// unit's primitive types, exactly as an exported function's signature is. What
/// comes back is what code generation needs: the qualified name a program
/// writes, the type it is read as, and the symbol the linker resolves.
pub(super) fn register_imported_variables(
    checker: &mut TypeChecker,
    variables: &[(String, fallc_sema::ExportedVariable)],
) -> std::collections::HashMap<String, (fallc_sema::Type, String)> {
    let mut for_codegen = std::collections::HashMap::new();
    for (qualified, variable) in variables {
        let ty = resolve_signature_type(&variable.ty, false);
        checker
            .add_imported_variables(std::collections::HashMap::from([(qualified.clone(), ty.clone())]));
        for_codegen.insert(qualified.clone(), (ty, variable.symbol.clone()));
    }
    for_codegen
}

pub(super) fn register_imported_types(
    checker: &mut TypeChecker,
    types: &[(String, fallc_sema::ExportType, String)],
) {
    // Every imported type's name first, then the fields: a field may name
    // another imported type -- `Exception` holds two `String`s -- and resolving
    // the fields one type at a time left the outer one's field as a bare name,
    // which code generation gave a different shape than the declaring module
    // did.
    let mut known: Vec<fallc_sema::NominalType> = types
        .iter()
        .map(|(name, ty, _module)| fallc_sema::NominalType {
            name: name.clone(),
            fields: Vec::new(),
            mutable_fields: Vec::new(),
            field_alignments: Vec::new(),
            is_union: ty.is_union,
            // Only a definition crosses a module boundary.
            is_incomplete: false,
            union_storage: None,
            template_arguments: None,
        })
        .collect();
    // Resolved in a fixed number of rounds, each round against the previous
    // one's table. The rounds cannot stop when a comparison says "no change":
    // a nominal type is the same type as the shallow entry that names it, so a
    // field that gained its own fields compares equal to the one that had none
    // -- and stopping there left `Exception` holding two empty `String`s,
    // which code generation shaped as `{}`.
    for _ in 0..4 {
        let previous = known.clone();
        for (index, (_, ty, _module)) in types.iter().enumerate() {
            known[index].fields = ty
                .fields
                .iter()
                .map(|field| {
                    // The declaring module's answer when it has one: fields
                    // whose written form needs folding -- `[32]byte` -- cannot
                    // be resolved here at all.
                    let ty = field
                        .resolved
                        .clone()
                        .unwrap_or_else(|| {
                            resolve_signature_type_with(&field.ty, false, &previous)
                        });
                    (field.name.clone(), ty)
                })
                .collect();
        }
    }
    for (name, ty, _module) in types {
        if ty.alias.is_some() {
            continue;
        }
        let Some(nominal) = known.iter().find(|known| &known.name == name) else {
            continue;
        };
        let fields: Vec<(String, fallc_sema::Type)> = nominal.fields.clone();
        // Registered under the type's own name *and* under `module.Type`, so
        // that a program may write either. `std.String` is what an umbrella
        // user writes; the bare name is what a sibling module writes.
        // The bare name is what a sibling module writes and what a member is
        // built under; the qualified one is what an umbrella user writes. Both
        // need the parameters, or the family is not a family under either.
        // `name` is either the bare type name or already qualified by the
        // umbrella (`std.Array`), which `exported_types` decided. It is filed
        // as it comes, with the binding name it should resolve to.
        let field_alignments: Vec<(String, u64)> = ty
            .fields
            .iter()
            .filter_map(|field| field.align.map(|align| (field.name.clone(), align)))
            .collect();
        checker.add_exported_type(
            name.clone(),
            ty.declaration_fields(),
            fields,
            ty.template_params.clone(),
            None,
            ty.is_union,
            ty.is_forward,
            field_alignments,
            ty.nested.clone(),
            ty.specialisations.clone(),
            ty.constants.clone(),
        );
    }
    // Aliases are registered after the types, because an alias's target may
    // name one of them: `type Id = SomeImportedType;`.
    for (name, ty, _module) in types {
        if let Some(target) = &ty.alias {
            checker.add_imported_alias(name.clone(), target);
        }
    }
}

/// Writes one emitted artefact, or prints it when no path was given.
///
/// Every `--emit` kind goes through here, so `-o` means the same thing for all
/// of them. It used to apply only to the LLVM and object outputs, so `-o` with
/// `--emit=ast` silently printed to standard output instead of writing the file
/// the user named.
///
/// When several kinds are emitted and a path is given, the path names the first
/// one and the others are derived from it by [`default_output_path`], because a
/// single `-o` cannot name several files.
pub(super) fn write_emitted(
    session: &mut Session,
    target: Option<PathBuf>,
    text: &str,
) -> Result<(), ()> {
    let Some(target) = target else {
        print!("{text}");
        return Ok(());
    };
    if let Err(error) = fs::write(&target, text) {
        session.report(
            Diagnostic::error(format!("cannot write `{}`: {error}", target.display()))
                .with_code(DiagnosticCode::driver(3)),
        );
        return Err(());
    }
    Ok(())
}

/// Returns where an artefact of a given kind should be written.
///
/// `-o` names one path, so when several kinds are emitted the first takes it
/// exactly and each later one is derived from it by replacing the extension.
/// With no `-o`, `None` means "print it".
pub(super) fn artifact_path(output: Option<&Path>, kind: EmitKind, first: bool) -> Option<PathBuf> {
    let output = output?;
    if first {
        return Some(output.to_path_buf());
    }
    Some(default_output_path(output, extension_of(kind)))
}

/// Returns the file extension a kind is normally written with.
pub(super) fn extension_of(kind: EmitKind) -> &'static str {
    match kind {
        EmitKind::Tokens => "tokens",
        EmitKind::Ast => "ast",
        EmitKind::Types => "types",
        EmitKind::Llvm => "ll",
        EmitKind::Object => "o",
        EmitKind::Static => "a",
        EmitKind::Shared => {
            if cfg!(target_os = "macos") {
                "dylib"
            } else if cfg!(target_os = "windows") {
                "dll"
            } else {
                "so"
            }
        }
        EmitKind::Executable => "",
    }
}

/// Returns whether this kind is the one `-o` names outright.
///
/// # Which one claims `-o`
///
/// The *last* kind in the pipeline: an executable if one is being produced,
/// otherwise the object, otherwise the IR, and so on down to the tokens. Asking
/// for `--emit=all -o out.ll` means "call the result out.ll", and the result is
/// the last thing built, not the first.
///
/// This is not the order the artefacts are produced in -- the tokens come first
/// because lexing does -- so taking the first kind instead made the tokens claim
/// `-o` and every later kind overwrite them. The tokens file then simply did not
/// exist, which is how this was found.
pub(super) fn is_first(kinds: &[EmitKind], kind: EmitKind) -> bool {
    kinds.last() == Some(&kind)
}

/// Checks the shape of the program's `main`, if it has one.
///
/// # The rule
///
/// The C runtime calls `int main(int argc, char **argv)`, so the two shapes a
/// program may write are that one and `int main()`. Either may return nothing
/// instead -- a program that has no status to report exits 0 -- and either may
/// be marked `except`, because an uncaught exception is reported by the
/// wrapper rather than by the entry itself.
///
/// A parameter-taking `main` is how a program reads its arguments: `argv[0]` is
/// the program's own path, exactly as C hands it over, and `argc` counts it.
pub(super) fn check_entry_function(ast: &Ast, session: &mut Session) {
    for &item in &ast.items {
        let fallc_ast::ItemKind::Function(function) = &ast.item(item).kind else {
            continue;
        };
        if function.name != "main" {
            continue;
        }
        // A method named `main` is not the entry; an entry is a free function.
        if function.params.first().is_some_and(|param| param.name == "this") {
            continue;
        }
        let reported = function.name_span;
        let returns_int = matches!(
            &function.return_type,
            fallc_ast::TypeSyntax::Named(named)
                if named.path == "int" && named.arguments.is_empty()
        );
        // A definition may leave the type out and have it deduced from the
        // body; `fn main() { .. }` therefore means a `main` that returns
        // nothing, and `fn main() { return 0; }` one that returns an `int`.
        // Whatever it deduces to, the wrapper adapts to it.
        let returns_nothing = match &function.return_type {
            fallc_ast::TypeSyntax::Named(named) => {
                named.path.is_empty() || named.path == "void"
            }
            fallc_ast::TypeSyntax::Auto(_) => true,
            _ => false,
        };
        if !returns_int && !returns_nothing {
            session.report(
                Diagnostic::error("`main` returns `int` or nothing")
                    .with_code(DiagnosticCode::sema(181))
                    .with_span(function.return_type.span())
                    .with_note("the exit status of the program is what `main` returns"),
            );
        }
        let takes_arguments = function.params.len() == 2
            && is_int_type(&function.params[0].ty)
            && is_argument_vector(&function.params[1].ty);
        if !function.params.is_empty() && !takes_arguments {
            session.report(
                Diagnostic::error("`main` takes no parameters, or `argc int, argv **c8`")
                    .with_code(DiagnosticCode::sema(181))
                    .with_span(function.params[0].span)
                    .with_note(
                        "`argv` is the argument vector the C runtime hands the program: \
                         `argv[0]` is the program's own path",
                    ),
            );
        }
        let _ = reported;
        return;
    }
}

/// Returns `true` if the written type is `int`.
fn is_int_type(ty: &fallc_ast::TypeSyntax) -> bool {
    matches!(ty, fallc_ast::TypeSyntax::Named(named) if named.path == "int")
}

/// Returns `true` if the written type is a pointer to a pointer to `c8` or
/// `byte`: the C `char **` an argument vector is.
///
/// `**const c8` is accepted too: the vector is the program's to read, and
/// whether the strings behind it are writable is a detail of the type.
fn is_argument_vector(ty: &fallc_ast::TypeSyntax) -> bool {
    let fallc_ast::TypeSyntax::Pointer(outer) = ty else {
        return false;
    };
    let inner = match outer.as_ref() {
        fallc_ast::TypeSyntax::Pointer(inner) | fallc_ast::TypeSyntax::PointerToConst(inner) => {
            inner.as_ref()
        }
        _ => return false,
    };
    matches!(
        inner,
        fallc_ast::TypeSyntax::Named(named) if named.path == "c8" || named.path == "byte"
    )
}
