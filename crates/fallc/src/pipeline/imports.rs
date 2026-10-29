//! Imports: finding a module, reading it, and binding what it exports.
//!
//! A module is found by searching next to the importer and then the standard
//! locations, loaded once, and its exported names are bound into the importing
//! file under the names it asked for.

use super::*;

/// Resolves a type written in a signature.
///
/// A minimal resolver, because the code generator needs a `Type` and the
/// checker's own resolution is not reachable from here.
pub(super) fn resolve_signature_type(ty: &fallc_ast::TypeSyntax, in_parameter: bool) -> fallc_sema::Type {
    resolve_signature_type_with(ty, in_parameter, &[])
}

/// Resolves a written type, given the types the declaring module knows.
///
/// A declared type is not a primitive, so it can only be resolved against the
/// declarations in scope where it was written. Passing none is right for a
/// type that has to be primitive, such as a signature's.
pub(super) fn resolve_signature_type_with(
    ty: &fallc_ast::TypeSyntax,
    in_parameter: bool,
    declared: &[fallc_sema::NominalType],
) -> fallc_sema::Type {
    use fallc_ast::TypeSyntax;
    use fallc_sema::Type;
    match ty {
        TypeSyntax::Named(named) => {
            if named.path.is_empty() {
                // No written type: "no value".
                return Type::unspecified("", named.span);
            }
            // `void` names no type; it is the same "no value" the omitted form
            // produces, and the checker's resolver reads it that way. Spelling
            // it as an unknown type called `void` made a written `void` in an
            // exported pattern not match the query argument the checker had
            // reduced to the empty type.
            if named.path == "void" {
                return Type::unspecified("", named.span);
            }
            // A declared type before a primitive, matching how the checker
            // resolves names: a program may declare `type String`. Only a
            // spelling without arguments can name the declaration itself: a
            // written `Crate<int>` names one *member* of the family, and
            // taking the family first would build the symbol from `Crate` while
            // the declaring module emitted the member's.
            if named.arguments.is_empty()
                && let Some(found) = declared.iter().find(|ty| ty.name == named.path)
            {
                return Type::Nominal(found.clone());
            }
            // A member of a type family: its name carries the arguments, and
            // the member itself is one the declaring module instantiated. The
            // spelling has to be the checker's -- `Crate<i32>`, not
            // `Crate<int>` -- because that name is what the symbol is built
            // from, and the two modules have to agree on it.
            if !named.arguments.is_empty() {
                let arguments: Vec<Type> = named
                    .arguments
                    .iter()
                    .filter_map(|argument| match argument {
                        fallc_ast::TemplateArgumentSyntax::Type(ty) => {
                            Some(resolve_signature_type_with(ty, false, declared))
                        }
                        fallc_ast::TemplateArgumentSyntax::Value(_) => None,
                        // A dotted name here is a type: a value argument is not
                        // part of a member's name.
                        fallc_ast::TemplateArgumentSyntax::Named { .. } => None,
                    })
                    .collect();
                let base = named
                    .path
                    .rsplit_once('.')
                    .map_or(named.path.as_str(), |(_, last)| last);
                let written: Vec<String> =
                    arguments.iter().map(fallc_sema::Type::spelling).collect();
                let member = format!("{}<{}>", base, written.join(", "));
                if let Some(found) = declared.iter().find(|ty| ty.name == member) {
                    return Type::Nominal(found.clone());
                }
                let qualified = format!("{}<{}>", named.path, written.join(", "));
                if let Some(found) = declared.iter().find(|ty| ty.name == qualified) {
                    return Type::Nominal(found.clone());
                }
                // A written use of the family itself -- `Crate<T>` in an
                // exported method -- names the family, not a member.
                if let Some(found) = declared.iter().find(|ty| ty.name == named.path) {
                    return Type::Nominal(found.clone());
                }
            }
            Type::from_name(&named.path)
                .unwrap_or_else(|| Type::unspecified(named.path.clone(), named.span))
        }
        TypeSyntax::Auto(span) => Type::unspecified("auto", *span),
        // `type(auto)` is deduced from an initialiser or a `return`, neither of
        // which a written signature carries; the checker that saw it knows.
        TypeSyntax::DecltypeAuto(span) => Type::unspecified("type(auto)", *span),
        // A function type in a signature: its parts are resolved the same way,
        // because a parameter written `fn(int) int` names the same type a
        // declaration would.
        TypeSyntax::Function(function) => Type::Function {
            parameters: function
                .parameters
                .iter()
                .map(|parameter| resolve_signature_type_with(parameter, true, declared))
                .collect(),
            returns: Box::new(resolve_signature_type_with(
                &function.returns,
                false,
                declared,
            )),
            abi: function.abi.clone(),
            variadic: function.variadic,
            throws: function.throws,
        },
        TypeSyntax::Member(member) => Type::unspecified(member.name.clone(), member.span),
        TypeSyntax::Unresolved(u) => Type::unspecified(u.text.clone(), u.span),
        // `type(expr)` is resolved by the checker, which has the expression in
        // hand; a signature that only sees the written form cannot ask.
        TypeSyntax::TypeOf(node) => Type::unspecified("type(..)".to_owned(), node.span),
        TypeSyntax::Const(inner) => {
            Type::Const(Box::new(resolve_signature_type_with(inner, in_parameter, declared)))
        }
        TypeSyntax::Pointer(inner) => Type::Pointer {
            pointee: Box::new(resolve_signature_type_with(inner, false, declared)),
            pointee_is_const: false,
            pointer_is_const: false,
        },
        TypeSyntax::PointerToConst(inner) => Type::Pointer {
            pointee: Box::new(resolve_signature_type_with(inner, false, declared)),
            pointee_is_const: true,
            pointer_is_const: false,
        },
        TypeSyntax::ConstPointer(inner) => Type::Pointer {
            pointee: Box::new(resolve_signature_type_with(inner, false, declared)),
            pointee_is_const: false,
            pointer_is_const: true,
        },
        TypeSyntax::RvalueReference(inner) => Type::Reference {
            referent: Box::new(resolve_signature_type_with(inner, false, declared)),
            referent_is_const: false,
            is_rvalue: true,
        },
        TypeSyntax::ConstRvalueReference(inner) => Type::Reference {
            referent: Box::new(resolve_signature_type_with(inner, false, declared)),
            referent_is_const: true,
            is_rvalue: true,
        },
        TypeSyntax::Volatile(inner) => {
            Type::Volatile(Box::new(resolve_signature_type_with(inner, false, declared)))
        }
        TypeSyntax::Reference(inner) => Type::Reference {
            referent: Box::new(resolve_signature_type_with(inner, false, declared)),
            referent_is_const: false,
            is_rvalue: false,
        },
        TypeSyntax::ConstReference(inner) => Type::Reference {
            referent: Box::new(resolve_signature_type_with(inner, false, declared)),
            referent_is_const: true,
            is_rvalue: false,
        },
        TypeSyntax::UnsizedArray(inner) => {
            let element = resolve_signature_type_with(inner, false, declared);
            if in_parameter {
                Type::Pointer {
                    pointee: Box::new(element),
                    pointee_is_const: false,
                    pointer_is_const: false,
                }
            } else {
                Type::UnsizedArray(Box::new(element))
            }
        }
        TypeSyntax::Array(array) => Type::Array {
            element: Box::new(resolve_signature_type_with(&array.element, false, declared)),
            // The length needs constant folding, which this resolver does not
            // do. Arrays do not appear in the C bindings, so this is not
            // reachable today.
            len: 0,
        },
    }
}

/// Returns the directories to search for imported modules.
///
/// The importing file's own directory comes first, so a module beside its user
/// needs no configuration, followed by the directories that hold the shipped
/// modules: `standard/` in this repository, or whatever `FALLTHROUGH_STANDARD`
/// names.
pub(super) fn search_path_for(importer: &Path) -> Vec<PathBuf> {
    search_path_for_impl(importer)
}

/// The implementation, kept separate so the public wrapper reads clearly.
pub(super) fn search_path_for_impl(importer: &Path) -> Vec<PathBuf> {
    let mut path = Vec::new();
    if let Some(directory) = importer.parent()
        && !directory.as_os_str().is_empty()
    {
        path.push(directory.to_path_buf());
    }
    // The modules shipped with the compiler live in `standard`; where that is
    // relative to the caller is what `standard_module_directories` works out.
    for directory in standard_module_directories(importer) {
        if !path.contains(&directory) {
            path.push(directory);
        }
    }
    path
}

/// Finds the directories that may hold the shipped modules, best first.
///
/// Which directory a program is compiled *from* must not decide whether it can
/// import the standard library: `fallc /tmp/prog.ft` run in `/tmp` has to find
/// the same `standard/` as the same command run in the repository. Four places
/// are searched, in order:
///
/// 1. `FALLTHROUGH_STANDARD`, when set, so an installation can say where its
///    library is without the compiler guessing.
/// 2. Beside the compiler and above it: a build tree keeps the binary in
///    `target/debug` and the library at the workspace root.
/// 3. Above the file being compiled, so a program inside a tree finds that
///    tree's library however the compiler was invoked.
/// 4. Above the current directory, which is how a test run from a crate finds
///    the workspace's copy.
pub(super) fn standard_module_directories(importer: &Path) -> Vec<PathBuf> {
    fn walk_up(directories: &mut Vec<PathBuf>, start: Option<&Path>) {
        let mut current = start;
        // A handful of levels is enough for any layout this repository has, and
        // the bound stops the walk at the filesystem root.
        for _ in 0..8 {
            let Some(directory) = current else { break };
            let candidate = directory.join(modules::STANDARD_MODULE_DIRECTORY);
            if !directories.contains(&candidate) {
                directories.push(candidate);
            }
            current = directory.parent();
        }
    }

    let mut directories = Vec::new();
    if let Some(explicit) = std::env::var_os("FALLTHROUGH_STANDARD") {
        directories.push(PathBuf::from(explicit));
    }
    // The executable's own directory first: that is where an installed compiler
    // looks, and walking up from it finds the workspace copy in a build tree.
    if let Ok(executable) = std::env::current_exe() {
        walk_up(&mut directories, executable.parent());
    }
    walk_up(&mut directories, importer.parent());
    let current = std::env::current_dir().ok();
    walk_up(&mut directories, current.as_deref());
    directories
}

/// The modules a token stream imports, as written.
///
/// Macros are expanded before the stream is parsed, so the import declarations
/// have to be read at the token level. `from M import x` and
/// `export from M import *` name the module *before* the `import` keyword, and
/// a plain `import M;` names it after.
fn written_imports(tokens: &[fallc_lexer::Token]) -> Vec<String> {
    use fallc_lexer::{Keyword, TokenKind};
    let mut modules = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if !matches!(token.kind, TokenKind::Keyword(Keyword::Import)) {
            continue;
        }
        // `from std.string import x`: the module's last segment is right
        // before the keyword, and the path runs back through the dots.
        if let Some(path) = path_before(tokens, index) {
            modules.push(path);
            continue;
        }
        if let Some((path, _)) = path_after(tokens, index + 1) {
            modules.push(path);
        }
    }
    modules
}

/// A module name written as a string, `import "my-module";`.
///
/// The parser strips the quotes and keeps the text, so the token scan does the
/// same.
fn quoted_name(token: &fallc_lexer::Token) -> Option<String> {
    let text = token.spelling()?;
    let name = text
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(text);
    Some(name.to_owned())
}

/// The dotted module path ending just before `index`, if one is there.
fn path_before(tokens: &[fallc_lexer::Token], index: usize) -> Option<String> {
    let start = index.checked_sub(1)?;
    let token = tokens.get(start)?;
    if matches!(token.kind, fallc_lexer::TokenKind::String) {
        return quoted_name(token);
    }
    let mut start = start;
    if !matches!(token.kind, fallc_lexer::TokenKind::Ident) {
        return None;
    }
    while start >= 2 {
        let dot = tokens.get(start - 1)?;
        let previous = tokens.get(start - 2)?;
        if !matches!(dot.kind, fallc_lexer::TokenKind::Punct(fallc_lexer::Punctuation::Dot))
            || !matches!(previous.kind, fallc_lexer::TokenKind::Ident)
        {
            break;
        }
        start -= 2;
    }
    let mut parts = Vec::new();
    let mut index = start;
    while index <= tokens.len() {
        let Some(token) = tokens.get(index) else {
            break;
        };
        let Some(text) = token.spelling() else {
            break;
        };
        parts.push(text.to_owned());
        index += 2;
    }
    Some(parts.join("."))
}

/// The dotted module path beginning at `start`, and the index after it.
fn path_after(tokens: &[fallc_lexer::Token], start: usize) -> Option<(String, usize)> {
    if let Some(token) = tokens.get(start)
        && matches!(token.kind, fallc_lexer::TokenKind::String)
    {
        return Some((quoted_name(token)?, start + 1));
    }
    let mut parts = Vec::new();
    let mut index = start;
    loop {
        let token = tokens.get(index)?;
        let text = token.spelling()?;
        if !matches!(token.kind, fallc_lexer::TokenKind::Ident) {
            return None;
        }
        parts.push(text.to_owned());
        let Some(dot) = tokens.get(index + 1) else {
            break;
        };
        if !matches!(dot.kind, fallc_lexer::TokenKind::Punct(fallc_lexer::Punctuation::Dot)) {
            break;
        }
        index += 2;
    }
    index += 1;
    Some((parts.join("."), index))
}

/// The modules a token stream re-exports macros from, with the names selected.
///
/// `export from inner import *` publishes everything `inner` published, and
/// `export from inner import NAME` publishes that one name. `None` means all.
fn written_reexports(tokens: &[fallc_lexer::Token]) -> Vec<(String, Option<Vec<String>>)> {
    use fallc_lexer::{Keyword, Punctuation, TokenKind};
    let mut reexports = Vec::new();
    let mut index = 0;
    while index + 2 < tokens.len() {
        // `export from M import ..` and the attribute spelling
        // `@export from M import ..`; both publish the same names.
        let attribute = matches!(tokens[index].kind, TokenKind::Punct(Punctuation::At));
        let export_at = if attribute { index + 1 } else { index };
        let export = tokens
            .get(export_at)
            .is_some_and(|token| token.kind == TokenKind::Ident && token.spelling() == Some("export"));
        let from = tokens
            .get(export_at + 1)
            .is_some_and(|token| matches!(token.kind, TokenKind::Keyword(Keyword::From)));
        if !(export && from) {
            index += 1;
            continue;
        }
        let Some((module, after)) = path_after(tokens, export_at + 2) else {
            index += 1;
            continue;
        };
        if !tokens
            .get(after)
            .is_some_and(|token| matches!(token.kind, TokenKind::Keyword(Keyword::Import)))
        {
            index += 1;
            continue;
        }
        let mut members = Vec::new();
        let mut all = false;
        let mut at = after + 1;
        while let Some(token) = tokens.get(at) {
            match token.kind {
                TokenKind::Punct(Punctuation::Semi) => break,
                TokenKind::Punct(Punctuation::Star) => {
                    all = true;
                    at += 1;
                }
                TokenKind::Punct(Punctuation::Comma) => at += 1,
                TokenKind::Ident => {
                    if let Some(text) = token.spelling() {
                        members.push(text.to_owned());
                    }
                    at += 1;
                }
                _ => break,
            }
        }
        reexports.push((module, if all { None } else { Some(members) }));
        index = at;
    }
    reexports
}

/// Collects the macros the modules a token stream imports publish.
///
/// A macro is expanded before parsing, so an imported module has to be read at
/// the token level: its `export define`s are installed before this stream is
/// expanded, and its own imports are collected first, so a macro may be written
/// in terms of one from a module it imports.
pub(super) fn collect_module_macros(
    session: &mut Session,
    tokens: &[fallc_lexer::Token],
    importer: &Path,
    search_path: &[PathBuf],
    cache: &mut MacroCache,
) -> Vec<fallc_lexer::macros::Macro> {
    imported_macros(
        session,
        tokens,
        importer,
        search_path,
        &mut Vec::new(),
        cache,
    )
}

/// The macros each module publishes, by the file they were read from.
///
/// Collecting a module's macros means lexing it and its imports, which is the
/// same work for every module that imports it. A compilation therefore does it
/// once per file: `import std` reached the same 73 files from nearly every
/// module before this cache existed.
pub(super) type MacroCache =
    std::collections::HashMap<std::path::PathBuf, Vec<fallc_lexer::macros::Macro>>;

/// The macros a stream's imports publish, with the modules already being
/// loaded recorded so that a cycle terminates.
fn imported_macros(
    session: &mut Session,
    tokens: &[fallc_lexer::Token],
    importer: &Path,
    search_path: &[PathBuf],
    visited: &mut Vec<String>,
    cache: &mut MacroCache,
) -> Vec<fallc_lexer::macros::Macro> {
    let mut macros: Vec<fallc_lexer::macros::Macro> = Vec::new();
    for module in written_imports(tokens) {
        for definition in module_macros(session, &module, importer, search_path, visited, cache) {
            // A name imported along two paths is one name; the first module
            // that publishes it wins, as it does for every other member.
            if !macros.iter().any(|existing| existing.name == definition.name) {
                macros.push(definition.clone());
            }
            // The same definition is also reachable through the module that
            // publishes it: `$meta.FILE` names `meta`'s `FILE`, so the dotted
            // spelling is a name of its own. Only the dotted one, the plain one
            // being the unqualified form an importer already has.
            let qualified = format!("{module}.{}", definition.name);
            if !macros.iter().any(|existing| existing.name == qualified) {
                let mut dotted = definition;
                dotted.name = qualified;
                macros.push(dotted);
            }
        }
    }
    macros
}

/// The macros one module publishes, its own dependencies collected first.
fn module_macros(
    session: &mut Session,
    module: &str,
    importer: &Path,
    search_path: &[PathBuf],
    visited: &mut Vec<String>,
    cache: &mut MacroCache,
) -> Vec<fallc_lexer::macros::Macro> {
    if visited.iter().any(|name| name == module) {
        return Vec::new();
    }
    let Some(found) = modules::find_module_file(module, importer, search_path) else {
        // The loader reports a missing module with the import's span; this pass
        // is only looking for macros, and has no span to point at.
        return Vec::new();
    };
    // A file's macros are a property of the file and its own imports, so an
    // answer computed for one importer is the answer for every importer.
    if let Some(cached) = cache.get(&found) {
        return cached.clone();
    }
    let Ok(text) = fs::read_to_string(&found) else {
        return Vec::new();
    };
    visited.push(module.to_owned());
    let file = session.add_file(found.clone(), text.clone());
    let mut lexer = Lexer::new(LexerConfig {
        preserve_trivia: false,
        errors_are_warnings: false,
    });
    let lexed = lexer.tokenize(&text, file);
    let prelude = imported_macros(session, &lexed.tokens, &found, search_path, visited, cache);
    // Whatever this module publishes is its own `export define`s, plus what it
    // re-exports with `export from M import ..`.
    let reexports = written_reexports(&lexed.tokens);
    let expanded = fallc_lexer::macros::expand_macros_with(
        lexed.tokens,
        &prelude,
        &crate::pipeline::builtins_for(&found, &text),
    );
    // A module that does not expand is reported when it is compiled. Here the
    // only useful answer is "it publishes nothing".
    let Ok((_, table)) = expanded else {
        visited.pop();
        return Vec::new();
    };
    // What it publishes, plus everything those macros are written in terms of:
    // a body is expanded where it is *used*, so every macro it names has to be
    // installed there too. A private macro reached this way travels only
    // because a published one needs it.
    let mut details: std::collections::HashMap<&str, &fallc_lexer::macros::Macro> =
        std::collections::HashMap::new();
    for definition in &table.available {
        details.insert(definition.name.as_str(), definition);
    }
    let mut exported: Vec<fallc_lexer::macros::Macro> = Vec::new();
    let mut pending: Vec<String> = table.exported.clone();
    while let Some(name) = pending.pop() {
        let Some(definition) = details.get(name.as_str()).copied() else {
            continue;
        };
        if exported.iter().any(|existing| existing.name == definition.name) {
            continue;
        }
        exported.push(definition.clone());
        pending.extend(macro_references(&definition.body));
    }
    // A macro it re-exports travels with the module it came from.
    for (dependency, selection) in reexports {
        for definition in module_macros(session, &dependency, &found, search_path, visited, cache) {
            let wanted = selection
                .as_ref()
                .is_none_or(|names| names.contains(&definition.name));
            if wanted && !exported.iter().any(|existing| existing.name == definition.name) {
                exported.push(definition);
            }
        }
    }
    visited.pop();
    cache.insert(found, exported.clone());
    exported
}

/// The macros a replacement list invokes, `$NAME` by `$NAME`.
fn macro_references(body: &[fallc_lexer::Token]) -> Vec<String> {
    use fallc_lexer::{Punctuation, TokenKind};
    let mut names = Vec::new();
    for window in body.windows(2) {
        if matches!(
            window[0].kind,
            TokenKind::Punct(Punctuation::Dollar)
        ) && matches!(window[1].kind, TokenKind::Ident)
            && let Some(text) = window[1].spelling()
        {
            names.push(text.to_owned());
        }
    }
    names
}


/// Prints the resident size under `FALLC_STAGES`, for finding what a module's
/// check actually costs.
fn report_rss(label: &str, module: &str) {
    if std::env::var_os("FALLC_STAGES").is_none() {
        return;
    }
    let resident = std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|text| text.split_whitespace().nth(1).and_then(|p| p.parse::<u64>().ok()))
        .map(|p| p * 4096 / (1024 * 1024))
        .unwrap_or(0);
    eprintln!("    {module:<32} {label:<24} {resident:>5} MB");
}

/// How much of an imported module is checked.
///
/// A module's declarations are what an importer needs; its implementation is
/// compiled once into the module's object. When that object is already in the
/// cache, checking its bodies again is pure cost, so the loader can ask for the
/// declarations only.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum CheckMode {
    /// Check declarations *and* every function body. Used when the module's
    /// object has to be built, and by a check that has no object to reuse.
    Full,
    /// Check types, signatures, constants, defaults, and `@enable_if`; leave
    /// bodies to the module's object.
    DeclarationsOnly,
}

/// Loads every module the file imports, and returns what they export.
///
/// A module is parsed but not type checked here: its exported signatures are
/// what an importer needs, and checking its bodies is the job of compiling that
/// module. A module imported twice is loaded once, so that two importers agree
/// on its types.
pub(super) fn load_imports(
    session: &mut Session,
    ast: &Ast,
    importer: &Path,
    mode: CheckMode,
    macro_cache: &mut MacroCache,
) -> LoadedImports {
    let search_path = search_path_for(importer);
    // The importing file's own type declarations, which its exported signatures
    // need in order to name a type rather than a string.
    let declared_types = declared_types_of(ast);
    let mut imported = std::collections::HashMap::new();
    // Which module each is re-exported through, so `std.Array` can name what
    // lives in `std.array`.
    let mut umbrellas: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    let mut externals = std::collections::HashMap::new();
    let mut bindings: Vec<fallc_sema::ImportBinding> = Vec::new();
    let mut aliases: Vec<(String, String)> = Vec::new();
    // Link instructions declared by imported modules. They apply to anything
    // that imports them, so `standard/C.ftm` can say what the C bindings need.
    let mut module_links: Vec<LinkRequest> = Vec::new();
    let mut bodies: Vec<ModuleBody> = Vec::new();
    // Which modules each loaded module imports, so that a module's checker gets
    // the template bodies of everything it can reach and nothing else: a name
    // is the same name in two modules, and std.memory's `Box` is not boxmod's.
    let mut module_imports: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    // The file each module was read from, so that a name two files claim is
    // reported with both paths rather than silently settled by load order.
    let mut files: std::collections::HashMap<String, PathBuf> = std::collections::HashMap::new();
    // The names this file wrote, so a short name exists only for those.
    let mut direct: Vec<(String, String)> = Vec::new();
    // Modules already reached, so a module imported twice is loaded once and a
    // cycle terminates.
    let mut visited: Vec<String> = Vec::new();
    for &item in &ast.items {
        let fallc_ast::ItemKind::Import(import) = &ast.item(item).kind else {
            continue;
        };
        let Some(module) = load_module(
            session,
            &import.module,
            import.name_span,
            importer,
            &search_path,
            &mut visited,
            &mut imported,
            &mut files,
            &mut umbrellas,
            &mut externals,
            &mut module_links,
            &mut bodies,
            &mut module_imports,
            mode,
            macro_cache,
        ) else {
            continue;
        };
        if std::env::var_os("FALLC_STAGES").is_some() {
            let resident = std::fs::read_to_string("/proc/self/statm")
                .ok()
                .and_then(|text| text.split_whitespace().nth(1).and_then(|p| p.parse::<u64>().ok()))
                .map(|p| p * 4096 / (1024 * 1024))
                .unwrap_or(0);
            eprintln!(
                "  module {:<32} {:>3} loaded  {:>5} MB",
                import.module,
                bodies.len() + 1,
                resident
            );
        }
        if let Some(path) = files.get(&import.module) {
            direct.push((import.module.clone(), path.display().to_string()));
        }
        bind_import(
            session,
            import,
            &module,
            &mut bindings,
            &mut aliases,
            &mut externals,
            &declared_types,
        );
    }
    // A short name is a second name for a module, so two modules cannot share
    // one: `import a.math; import b.math;` makes `math` mean two things, and
    // which one won used to depend on load order. Reported here, where both
    // files are known.
    let mut short_names: std::collections::HashMap<String, (String, String)> =
        std::collections::HashMap::new();
    for (module, path) in &direct {
        let Some((_, last)) = module.rsplit_once('.') else {
            continue;
        };
        match short_names.get(last) {
            Some((existing_module, existing_path)) if existing_module != module => {
                session.report(
                    Diagnostic::error(format!("two modules are reachable as `{last}`"))
                        .with_code(DiagnosticCode::driver(7))
                        .with_note(format!(
                            "`{existing_module}` is `{existing_path}`, and `{module}` is `{path}`"
                        ))
                        .with_note(
                            "a dotted module is also reachable by its last segment, so the \
                             two cannot both answer to it",
                        ),
                );
            }
            Some(_) => {}
            None => {
                short_names.insert(last.to_owned(), (module.clone(), path.clone()));
            }
        }
    }
    if std::env::var_os("FALLC_STAGES").is_some() {
        let mut ast_items = 0;
        let mut type_entries = 0;
        let mut binding_entries = 0;
        let mut defs = 0;
        let mut name_bindings = 0;
        let mut nominal = 0;
        let mut families = 0;
        let mut interfaces = 0;
        let mut declarations = 0;
        for body in &bodies {
            ast_items += body.ast.items.len();
            type_entries += body.types.len();
            binding_entries += body.binding_types.len();
            defs += body.checker_resolutions.defs.len();
            name_bindings += body.checker_resolutions.binding_count();
            nominal += body.environment.nominal_types.len();
            families += body.environment.type_templates.len();
            interfaces += body.environment.interfaces.len();
            declarations += body.declarations.len();
        }
        eprintln!(
            "  retained: ast_items={ast_items} types={type_entries} bindings={binding_entries} defs={defs} names={name_bindings} env_nominal={nominal} env_families={families} env_interfaces={interfaces} declarations={declarations}"
        );
    }
    stamp_cache_hashes(&mut bodies, &module_imports);
    LoadedImports {
        modules: imported,
        direct,
        umbrellas,
        externals,
        bindings,
        aliases,
        links: module_links,
        bodies,
    }
}

/// Replaces each module's source hash with one covering its dependency closure.
///
/// A module's body is compiled against the exported interfaces of everything it
/// imports, so its generated code can change without its own text changing --
/// an added field to an imported type changes a struct layout. Keying the cache
/// entry on the module's own source alone would then reuse an object built
/// against the old interface, so the key covers the whole closure of sources the
/// module's compilation reads.
fn stamp_cache_hashes(
    bodies: &mut [ModuleBody],
    module_imports: &std::collections::HashMap<String, Vec<String>>,
) {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let own: std::collections::HashMap<String, u64> = bodies
        .iter()
        .map(|body| (body.module.clone(), body.cache_hash))
        .collect();
    let mut memo: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    let mut visiting: std::collections::HashSet<String> = std::collections::HashSet::new();
    fn closure(
        name: &str,
        own: &std::collections::HashMap<String, u64>,
        module_imports: &std::collections::HashMap<String, Vec<String>>,
        memo: &mut std::collections::HashMap<String, u64>,
        visiting: &mut std::collections::HashSet<String>,
    ) -> u64 {
        if let Some(hash) = memo.get(name) {
            return *hash;
        }
        // A cycle terminates on the name alone: the modules still form one
        // unit, so what matters is that the key is the same every time.
        if !visiting.insert(name.to_owned()) {
            let mut hasher = DefaultHasher::new();
            name.hash(&mut hasher);
            return hasher.finish();
        }
        let mut parts: Vec<(String, u64)> = vec![(
            name.to_owned(),
            own.get(name).copied().unwrap_or_default(),
        )];
        if let Some(dependencies) = module_imports.get(name) {
            for dependency in dependencies {
                parts.push((
                    dependency.clone(),
                    closure(dependency, own, module_imports, memo, visiting),
                ));
            }
        }
        visiting.remove(name);
        // Sorted, because an import list is a set: rewriting it in another
        // order must not invalidate the cache.
        parts.sort();
        let mut hasher = DefaultHasher::new();
        for (module, hash) in &parts {
            module.hash(&mut hasher);
            hash.hash(&mut hasher);
        }
        let hash = hasher.finish();
        memo.insert(name.to_owned(), hash);
        hash
    }
    for body in bodies.iter_mut() {
        body.cache_hash = closure(&body.module.clone(), &own, module_imports, &mut memo, &mut visiting);
    }
}

/// Registers every trait a file can ask about with its checker.
///
/// Its own, from the tree it is being checked in, and the imported ones. Without
/// this the checker has no requirements to check a query against.
pub(super) fn register_traits(
    checker: &mut TypeChecker,
    ast: &Ast,
    resolutions: &fallc_sema::Resolutions,
    imported_traits: &[(String, fallc_sema::TraitBody)],
) {
    for (name, body) in imported_traits {
        checker.add_trait_definition(name.clone(), body.clone());
    }
    let mut tree: Option<std::rc::Rc<Ast>> = None;
    let mut own_resolutions: Option<std::rc::Rc<fallc_sema::Resolutions>> = None;
    for &item in &ast.items {
        // An interface states its restrictions the way a trait does, so the
        // same body is registered for both: the question "does this type
        // satisfy it" is asked of either name, and boxing an interface asks it
        // of the type being boxed.
        let declaration = match &ast.item(item).kind {
            fallc_ast::ItemKind::Trait(declaration) => declaration.clone(),
            fallc_ast::ItemKind::Interface(interface) if !interface.requirements.is_empty() => {
                interface.as_trait()
            }
            _ => continue,
        };
        let tree = tree.get_or_insert_with(|| std::rc::Rc::new(ast.clone()));
        let own = own_resolutions.get_or_insert_with(|| std::rc::Rc::new(resolutions.clone()));
        checker.add_trait_definition(
            declaration.name.clone(),
            fallc_sema::TraitBody {
                tree: std::rc::Rc::clone(tree),
                resolutions: std::rc::Rc::clone(own),
                declaration,
            },
        );
    }
}

/// Registers the module-level variables the imported modules exported.
///
/// Each is one object for the whole program: the checker reads its type from
/// the defining module's resolution, and code generation refers to the symbol
/// it was given, which is the declaring module's name for the variable.
pub(super) fn register_imported_globals(
    checker: &mut fallc_sema::TypeChecker,
    globals: &[(String, fallc_sema::ExportedGlobal)],
) -> std::collections::HashMap<String, (fallc_sema::Type, String, bool)> {
    let mut for_codegen = std::collections::HashMap::new();
    for (qualified, global) in globals {
        checker.add_imported_variables(std::collections::HashMap::from([(
            qualified.clone(),
            global.ty.clone(),
        )]));
        if let Some(message) = &global.deprecated {
            let message = (!message.is_empty()).then(|| message.clone());
            checker.add_deprecated_name(qualified.clone(), message.clone());
            // The bare name is the one a `from` import writes, and the last
            // segment is the one a qualified use names.
            if let Some((_, last)) = qualified.rsplit_once('.') {
                checker.add_deprecated_name(last.to_owned(), message.clone());
            }
            checker.add_deprecated_name(global.symbol.clone(), message);
        }
        for_codegen.insert(
            qualified.clone(),
            (
                global.ty.clone(),
                global.symbol.clone(),
                global.thread_local,
            ),
        );
    }
    for_codegen
}

/// Registers the constants the imported modules exported.
///
/// The resolver folds them for a qualified read -- `C.EOF` -- but a *bare* name
/// from `from m import LIMIT` is typed by the checker, and a type is what says
/// how many bytes to store.
pub(super) fn register_imported_constants(
    checker: &mut fallc_sema::TypeChecker,
    constants: &[(String, i64, fallc_ast::TypeSyntax)],
) {
    for (name, value, ty) in constants {
        checker.add_imported_constant(name.clone(), *value, ty);
    }
}

/// Registers the enums the imported modules exported.
///
/// Each name a program may write is registered in full: the base type, the
/// enum itself, and every member as a constant of it. A member is named through
/// the enum -- `Color.RED` -- so both halves have to arrive together.
pub(super) fn register_imported_enums(
    checker: &mut fallc_sema::TypeChecker,
    enums: &[(String, fallc_sema::ExportEnum)],
) {
    for (name, exported) in enums {
        checker.add_imported_enum(
            name.clone(),
            exported.base.as_ref(),
            &exported.members,
        );
    }
}

/// Registers the interfaces the imported modules exported.
///
/// The value's layout is the same everywhere, so what arrives is the method
/// list, under each name a program may write for the interface.
pub(super) fn register_imported_interfaces(
    checker: &mut fallc_sema::TypeChecker,
    interfaces: &[(String, fallc_sema::ExportInterface)],
) {
    for (name, exported) in interfaces {
        checker.add_imported_interface(name.clone(), exported);
    }
}

/// Teaches the resolver that these names are types.
///
/// A type is not a value, so a use of one as a callee -- `Array<int>()` -- must
/// not be reported as an unknown name, and a *query* is written the same way:
/// `InputIterator(IntIter)` names a type. Both entry points call this,
/// because a name that resolves in one and not the other makes the same program
/// behave differently depending on how it was compiled.
pub(super) fn add_known_types(
    resolver: &mut fallc_sema::Resolver,
    ast: &Ast,
    imported_types: &[(String, fallc_sema::ExportType, String)],
    imported_enums: &[(String, fallc_sema::ExportEnum)],
    imported_interfaces: &[(String, fallc_sema::ExportInterface)],
) {
    for item in &ast.items {
        match &ast.item(*item).kind {
            fallc_ast::ItemKind::TypeDeclaration(declaration) => {
                resolver.add_known_type(declaration.name.clone());
            }
            // An enum and an interface are types too. `Color.RED` names its
            // enum's type through a member, and a query or a cast may name
            // either, so the resolver has to know the name is not a value.
            fallc_ast::ItemKind::EnumDeclaration(declaration) => {
                resolver.add_known_type(declaration.name.clone());
            }
            fallc_ast::ItemKind::Interface(declaration) => {
                resolver.add_known_type(declaration.name.clone());
            }
            _ => {}
        }
    }
    for (name, _, _) in imported_types {
        resolver.add_known_type(name.clone());
    }
    for (name, _) in imported_enums {
        resolver.add_known_type(name.clone());
    }
    for (name, _) in imported_interfaces {
        resolver.add_known_type(name.clone());
    }
}

/// Teaches the resolver that these names are traits, so that a query is not
/// reported as a call to something that does not exist.
pub(super) fn add_known_traits(
    resolver: &mut fallc_sema::Resolver,
    ast: &Ast,
    imported_traits: &[(String, fallc_sema::TraitBody)],
) {
    for (name, _) in imported_traits {
        resolver.add_known_type(name.clone());
        resolver.add_known_trait(name.clone());
    }
    // The compiler's own type traits are not declared in a module, so they are
    // always names to ask about: `IsFloating(T)` is a question, not a call to a
    // value nothing defines. `IsInteger` and the other category predicates are
    // standard-library families now, so they are known as imported *types* and
    // the resolver leaves their calls alone for the same reason.
    for (name, arity) in fallc_sema::resolve::BUILTIN_TYPE_TRAITS {
        let _ = arity;
        resolver.add_known_trait(*name);
    }
    for &item in &ast.items {
        match &ast.item(item).kind {
            fallc_ast::ItemKind::Trait(declaration) => {
                resolver.add_known_type(declaration.name.clone());
                resolver.add_known_trait(declaration.name.clone());
            }
            // An interface with restrictions answers the same question, so its
            // name is one to ask about as well as one to name a type.
            fallc_ast::ItemKind::Interface(interface) if !interface.requirements.is_empty() => {
                resolver.add_known_type(interface.name.clone());
                resolver.add_known_trait(interface.name.clone());
            }
            _ => {}
        }
    }
}

/// Whether this file uses the exception machinery.
///
/// Any of it counts: an `except` function (which may raise), or a `try`
/// statement (whose handler reads the exception through `std`). Both need the
/// standard library's string and exception types, whether or not the author
/// wrote `import std`.
pub(super) fn uses_exceptions(ast: &Ast) -> bool {
    struct Scan {
        found: bool,
    }
    impl fallc_ast::visit::Visitor for Scan {
        fn visit_item(&mut self, ast: &Ast, _id: fallc_ast::ItemId, item: &fallc_ast::Item) {
            if let fallc_ast::ItemKind::Function(function) = &item.kind
                && function.throws.is_some()
            {
                self.found = true;
            }
            let _ = ast;
        }

        fn visit_stmt(&mut self, ast: &Ast, id: fallc_ast::StmtId) {
            if matches!(
                ast.stmt(id).kind,
                fallc_ast::StmtKind::Try(_) | fallc_ast::StmtKind::Raise(_)
            ) {
                self.found = true;
            }
        }
    }
    let mut scan = Scan { found: false };
    fallc_ast::visit::walk_ast(ast, &mut scan);
    scan.found
}

/// Whether the file already imports the standard library.
pub(super) fn imports_std(ast: &Ast) -> bool {
    ast.items.iter().any(|item| {
        let fallc_ast::ItemKind::Import(import) = &ast.item(*item).kind else {
            return false;
        };
        import.module == "std" || import.module.starts_with("std.")
    })
}

/// Makes the exception runtime available to a file that uses exceptions.
///
/// `raise "text"` constructs a `std.String`, and the report of an uncaught
/// exception prints one, so exceptions need the standard library's string and
/// exception types. A file that writes `except` without `import std` would
/// otherwise fail in code generation with an internal error, which is a rule
/// about the compiler leaking into the language: the machinery is the
/// language's, so the compiler brings what it needs.
///
/// The module it brings is the one that declares those two types, and not the
/// umbrella. `import std` would have compiled every module of the standard
/// library -- 67 of them, and the 947 functions their bodies instantiate -- for
/// a program that wrote `except` and nothing else. A program that writes
/// `except` around `C.printf` took ten seconds to build and a program without
/// it took a twentieth of that, and the difference was this import.
///
/// The import is added as a plain one, so `std.member` becomes reachable and
/// nothing is put into scope bare -- exactly as if the author had written it.
pub(super) fn add_exception_runtime(ast: &mut Ast) {
    if imports_std(ast) || !uses_exceptions(ast) {
        return;
    }
    let item = ast.builder().alloc_item(
        fallc_ast::ItemKind::Import(fallc_ast::ImportItem {
            module: "std.string".to_owned(),
            name_span: fallc_span::Span::SYNTHETIC,
            alias: None,
            exported: false,
            members: None,
        }),
        fallc_span::Span::SYNTHETIC,
    );
    ast.items.push(item);
}

/// Loads one module, and everything it imports, and returns what it exports.
///
/// # Why this recurses
///
/// A module may be an umbrella: `standard/C.ftm` declares nothing itself and
/// re-exports six other modules. Its exports are therefore only known once its
/// own imports have been loaded, which is what the recursion is for. The
/// `visited` list makes a cycle terminate instead of recursing forever; a
/// module already being loaded contributes whatever it has, and a cyclic import
/// is reported by the resolver rather than hanging the compiler.
#[allow(clippy::too_many_arguments)]
pub(super) fn load_module(
    session: &mut Session,
    name: &str,
    span: fallc_span::Span,
    importer: &Path,
    search_path: &[PathBuf],
    visited: &mut Vec<String>,
    imported: &mut std::collections::HashMap<String, fallc_sema::ExternalModule>,
    files: &mut std::collections::HashMap<String, PathBuf>,
    umbrellas: &mut std::collections::HashMap<String, Vec<String>>,
    externals: &mut std::collections::HashMap<String, fallc_ir::ExternalSignature>,
    module_links: &mut Vec<LinkRequest>,
    bodies: &mut Vec<ModuleBody>,
    module_imports: &mut std::collections::HashMap<String, Vec<String>>,
    mode: CheckMode,
    macro_cache: &mut MacroCache,
) -> Option<fallc_sema::ExternalModule> {
    let Some(found) = modules::find_module_file(name, importer, search_path) else {
        // Reported here rather than by the caller: only this frame knows the
        // name that was looked for, and a silent miss leaves the checker to
        // report an unknown name instead, which points at the use rather than
        // at the import that failed.
        session.report(
            Diagnostic::error(format!("cannot find module `{name}`"))
                .with_code(DiagnosticCode::driver(5))
                .with_span(span)
                .with_note(format!("looked for `{name}.ftm`")),
        );
        return None;
    };
    // One name means one file. A module reached along two paths -- the
    // importing file's own directory and the one a standard module lives in --
    // used to resolve to whichever was found first, so which `math` a program
    // got depended on load order. That is a conflict in the program, not a
    // choice for the compiler, so both files are named.
    let identity = fs::canonicalize(&found).unwrap_or_else(|_| found.clone());
    if let Some(existing) = files.get(name) {
        if existing != &identity {
            session.report(
                Diagnostic::error(format!("two files provide the module `{name}`"))
                    .with_code(DiagnosticCode::driver(6))
                    .with_span(span)
                    .with_note(format!(
                        "`{}` is here, and `{}` is where it was first found",
                        identity.display(),
                        existing.display()
                    ))
                    .with_note(format!(
                        "a module's name is its identity, so the two cannot both be `{name}`"
                    )),
            );
            return None;
        }
        // The same file: loaded, or being loaded right now. A module that is
        // being loaded contributes nothing to itself, which is what stops a
        // cycle; a loaded one is what its importers get.
        return imported.get(name).cloned();
    }
    visited.push(name.to_owned());
    files.insert(name.to_owned(), identity);

    let Ok(text) = fs::read_to_string(&found) else {
        session.report(
            Diagnostic::error(format!("cannot read `{}`", found.display()))
                .with_code(DiagnosticCode::driver(1)),
        );
        return None;
    };
    let source_hash = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut hasher);
        hasher.finish()
    };
    let file_id = session.add_file(found.clone(), text.clone());
    let mut lexer = Lexer::new(LexerConfig {
        preserve_trivia: false,
        errors_are_warnings: false,
    });
    let lexed = lexer.tokenize(&text, file_id);
    session.report_all(lexed.diagnostics.clone());
    // This module may itself use a macro another module exported, so its own
    // imports are collected first.
    let prelude = collect_module_macros(session, &lexed.tokens, &found, search_path, macro_cache);
    let (tokens, problems) = expand_macros(
        lexed.tokens,
        &prelude,
        &crate::pipeline::builtins_for(&found, &text),
    );
    session.report_all(problems);
    let mut stream = TokenStream::new(tokens);
    let mut parser = Parser::new(ParserConfig::default());
    parser.set_source(text);
    let parsed = parser.parse_file(&mut stream);
    session.report_all(parsed.diagnostics.clone());
    report_rss("parsed", name);
    // The module's tree is shared: every templated function it declares
    // carries it, and a copy per template is the difference between one tree
    // and hundreds. `parsed` is left with its diagnostics only.
    let parsed_ast = std::rc::Rc::new(parsed.ast);

    // This module's own imports, so that what it re-exports is known. The
    // module's directory becomes the base for its own relative imports.
    let direct_imports: Vec<String> = parsed_ast
        .items
        .iter()
        .filter_map(|item| match &parsed_ast.item(*item).kind {
            fallc_ast::ItemKind::Import(import) => Some(import.module.clone()),
            _ => None,
        })
        .collect();
    module_imports.insert(name.to_owned(), direct_imports.clone());
    let mut reexported = Vec::new();
    for &item in &parsed_ast.items {
        let fallc_ast::ItemKind::Import(import) = &parsed_ast.item(item).kind else {
            continue;
        };
        let Some(dependency) = load_module(
            session,
            &import.module,
            import.name_span,
            &found,
            search_path,
            visited,
            imported,
            files,
            umbrellas,
            externals,
            module_links,
            bodies,
            module_imports,
            mode,
            macro_cache,        ) else {
            continue;
        };
        // Only `export from` re-exports. A plain `from M import *` brings the
        // names into this file and stops there, so what a module publishes is
        // always something a reader can find by looking for `export`.
        if import.exported {
            reexported.push(bind_reexport(import, &dependency));
            // This module republishes `import.module`'s names as its own, so a
            // qualified reference may start with *this* module's name: `std`
            // for `std.array`'s `Array`. Recorded here because this is the only
            // place that sees both halves of the relationship.
            umbrellas
                .entry(import.module.clone())
                .or_default()
                .push(name.to_owned());
        }
    }

    // The module's own names are resolved and typed here, for the same reason
    // the main file's are: lowering a body needs to know what every expression
    // in it means. A module's declarations are its own, so this uses a fresh
    // resolver rather than the importer's.
    // The module's own declarations *and* the types it imported: a signature
    // may name either -- `export fn exception() &const Exception` names its own
    // type, and `name()` returns `String`, which comes from `std.string`. A
    // signature is filed by its types, so both have to resolve to the same
    // nominal type the definition does.
    // The module's own declarations *and* the types it imported: a signature may
    // name either -- `exception() &const Exception` names its own type, and
    // `name()` returns `String`, which comes from `std.string`. A signature is
    // filed by its types, so both have to resolve to the same nominal type the
    // definition does, fields included.
    // A declared type's name, its fields' names and written types, and whether
    // it is a union.
    type WrittenType = (String, Vec<(String, fallc_ast::TypeSyntax)>, bool);
    let mut written: Vec<WrittenType> = Vec::new();
    for &item in &parsed_ast.items {
        if let fallc_ast::ItemKind::TypeDeclaration(declaration) = &parsed_ast.item(item).kind {
            written.push((
                declaration.name.clone(),
                declaration
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.ty.clone()))
                    .collect(),
                matches!(declaration.kind, fallc_ast::TypeDeclKind::Union),
            ));
        }
    }
    for dependency in direct_imports.iter().filter_map(|name| imported.get(name)) {
        for (type_name, ty) in &dependency.types {
            if written.iter().any(|(name, _, _)| name == type_name) {
                continue;
            }
            written.push((
                type_name.clone(),
                ty.fields
                    .iter()
                    .map(|field| (field.name.clone(), field.ty.clone()))
                    .collect(),
                ty.is_union,
            ));
        }
    }
    // Two passes: every name first, then every field, so that a type whose field
    // is another declared type -- `Exception` holding two `String`s -- has a
    // layout. Resolving the fields one declaration at a time left the second one
    // as a bare name, and a signature carrying the type by value then disagreed
    // with the definition about its shape.
    let names: Vec<fallc_sema::NominalType> = written
        .iter()
        .map(|(name, _, is_union)| fallc_sema::NominalType {
            name: name.clone(),
            fields: Vec::new(),
            mutable_fields: Vec::new(),
            field_alignments: Vec::new(),
            is_union: *is_union,
            is_incomplete: false,
            union_storage: None,
            template_arguments: None,
        })
        .collect();
    // Resolved in rounds until nothing changes: a type's field may name another
    // declared type, which has fields of its own -- `Exception` holds two
    // `String`s, and a `String` has three fields. One round leaves the outer
    // type's field naming an empty struct, and code generation then gives the
    // type a shape the checker's does not have.
    let mut known = names.clone();
    // Fixed rounds, each against the previous table: two nominal types with one
    // name compare equal however many fields each carries, so "nothing changed"
    // is not a signal that the fields are complete. See the note in
    // `register_imported_types`.
    for _ in 0..4 {
        let previous = known.clone();
        for (index, (_, fields, _)) in written.iter().enumerate() {
            known[index].fields = fields
                .iter()
                .map(|(field, ty)| {
                    (
                        field.clone(),
                        resolve_signature_type_with(ty, false, &previous),
                    )
                })
                .collect();
        }
    }
    let module_declared_types = known.clone();
    let module_exports = {
        let mut resolver = fallc_sema::Resolver::new();
        resolver.add_module(name.to_owned(), fallc_sema::ExternalModule::default());
        for dependency_name in &direct_imports {
            let Some(dependency) = imported.get(dependency_name) else {
                continue;
            };
            for key in module_keys(dependency_name) {
                resolver.add_module(key, dependency.clone());
            }
        }
        // Its own traits, and none of its imports': a module's dependencies
        // travel as export tables rather than as trees, so their requirements
        // are not in hand here. A module that asks about a trait it imported is
        // the next round's problem; the file being compiled sees every trait it
        // imported.
        add_known_traits(&mut resolver, &parsed_ast, &[]);
        // Imported type names are not values. A body that writes
        // `Array<int>()` -- the constructor of a type its module imported --
        // was reported as an unknown name, because only the file being
        // compiled was told which imported names are types. The names are
        // collected here under the spellings the module may write.
        {
            let mut known_types: Vec<(String, fallc_sema::ExportType, String)> = Vec::new();
            let mut known_enums: Vec<(String, fallc_sema::ExportEnum)> = Vec::new();
            let mut known_interfaces: Vec<(String, fallc_sema::ExportInterface)> = Vec::new();
            for dependency_name in &direct_imports {
                let Some(dependency) = imported.get(dependency_name) else {
                    continue;
                };
                for (type_name, ty) in &dependency.types {
                    known_types.push((type_name.clone(), ty.clone(), dependency_name.clone()));
                    for key in module_keys(dependency_name) {
                        known_types.push((
                            format!("{key}.{type_name}"),
                            ty.clone(),
                            dependency_name.clone(),
                        ));
                    }
                }
                for (enum_name, exported) in &dependency.enums {
                    known_enums.push((enum_name.clone(), exported.clone()));
                }
                for (interface_name, exported) in &dependency.interfaces {
                    known_interfaces.push((interface_name.clone(), exported.clone()));
                }
            }
            add_known_types(
                &mut resolver,
                &parsed_ast,
                &known_types,
                &known_enums,
                &known_interfaces,
            );
        }
        // The module's own imports, applied before its names are resolved:
        // `from libx import twice;` binds `twice` in this module exactly as it
        // does in a program, and without this a module could reach another
        // module's functions only through the qualified spelling -- so a
        // family body that writes `this.mem.alloc(..)` found no `alloc` at all
        // when it was instantiated.
        {
            let mut bindings: Vec<fallc_sema::ImportBinding> = Vec::new();
            let mut aliases: Vec<(String, String)> = Vec::new();
            let mut local_externals: std::collections::HashMap<
                String,
                fallc_ir::ExternalSignature,
            > = std::collections::HashMap::new();
            for &item in &parsed_ast.items {
                let fallc_ast::ItemKind::Import(import) = &parsed_ast.item(item).kind else {
                    continue;
                };
                let Some(exports) = imported.get(&import.module) else {
                    continue;
                };
                bind_import(
                    session,
                    import,
                    exports,
                    &mut bindings,
                    &mut aliases,
                    &mut local_externals,
                    &module_declared_types,
                );
            }
            for (alias, module) in aliases {
                if let Some(external) = imported.get(&module).cloned() {
                    resolver.add_module(alias, external);
                }
            }
            for binding in bindings {
                resolver.add_import(binding);
            }
        }
        // A declaration-only load resolves the bodies that an instantiation
        // will be lowered from, and nothing else: the ordinary implementation
        // of this module is already in its compiled object.
        resolver.set_declarations_only(mode == CheckMode::DeclarationsOnly);
        if std::env::var_os("FALLC_STAGES").is_some() {
            eprintln!("    {name:<32} resolve mode {mode:?}");
        }
        resolver.add_family_parameters(exports::family_parameter_names(&parsed_ast));
        let resolved = resolver.resolve(&parsed_ast);
        report_rss("resolved", name);
        session.report_all(resolved.diagnostics.clone());
        let mut checker = TypeChecker::new(fallc_ir::Backend::pointer_bits());
        checker.set_declarations_only(mode == CheckMode::DeclarationsOnly);
        checker.add_module(name.to_owned());
        // Every name the module is reachable by, so that `memory.malloc` is
        // recognised as a module member rather than a field of a value. Using
        // the full dotted names alone meant the short name was missing here
        // while it was present in the resolver, and the two disagreed.
        for dependency_name in &direct_imports {
            for key in module_keys(dependency_name) {
                checker.add_module(key);
            }
        }
        for dependency_name in &direct_imports {
            let Some(dependency) = imported.get(dependency_name) else {
                continue;
            };
            for (type_name, ty) in &dependency.types {
                let fields = ty
                    .fields
                    .iter()
                    .map(|field| {
                        // The declaring module's answer when it has one: a
                        // `[32]byte` has a length that is an expression in its
                        // tree, which this side cannot fold.
                        let ty = field
                            .resolved
                            .clone()
                            .unwrap_or_else(|| resolve_signature_type(&field.ty, false));
                        (field.name.clone(), ty)
                    })
                    .collect();
                let _ = dependency_name;
                // A module that uses another's family needs the parameters for
                // the same reason an importer does: `Array<T>` is written
                // inside the module that builds it.
                let field_alignments: Vec<(String, u64)> = ty
                    .fields
                    .iter()
                    .filter_map(|field| field.align.map(|align| (field.name.clone(), align)))
                    .collect();
                checker.add_exported_type(
                    type_name.clone(),
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
        }
        // The dependencies' enums. An enum is its base type under a name, so a
        // module that imports the one declaring it writes `OpenMode` -- and a
        // signature that a dependency exported may name it too, which is how a
        // module reached `File(.., mode OpenMode)` from `std.io` and was told
        // the type did not exist.
        for dependency_name in &direct_imports {
            let Some(dependency) = imported.get(dependency_name) else {
                continue;
            };
            for (enum_name, exported) in &dependency.enums {
                for key in module_keys(dependency_name) {
                    checker.add_imported_enum(
                        format!("{key}.{enum_name}"),
                        exported.base.as_ref(),
                        &exported.members,
                    );
                }
                checker.add_imported_enum(
                    enum_name.clone(),
                    exported.base.as_ref(),
                    &exported.members,
                );
            }
        }
        // A module reads its dependencies' variables and constants exactly as a
        // program does: `C.stdout` is a read of storage another module owns, and
        // without the entry the read reached code generation as a field of a
        // module name -- reported as an internal error, because nothing had
        // told it what the name stands for. Every spelling the module may write
        // is registered, the short name included.
        for dependency_name in &direct_imports {
            let Some(dependency) = imported.get(dependency_name) else {
                continue;
            };
            for key in module_keys(dependency_name) {
                for (member, variable) in &dependency.variables {
                    let ty = resolve_signature_type(&variable.ty, false);
                    checker.add_imported_variables(std::collections::HashMap::from([(
                        format!("{key}.{member}"),
                        ty,
                    )]));
                }
                for (member, global) in &dependency.globals {
                    checker.add_imported_variables(std::collections::HashMap::from([(
                        format!("{key}.{member}"),
                        global.ty.clone(),
                    )]));
                }
                for (member, value) in &dependency.constants {
                    if let Some(ty) = dependency.constant_types.get(member) {
                        checker.add_imported_constant(format!("{key}.{member}"), *value, ty);
                    }
                }
            }
        }
        // A module's declared methods have to be findable by *name*, not only by
        // an import that binds them: a body that calls a method on a field of an
        // imported type -- `e._type.try_init(..)` -- has no `from` import to
        // bind `try_init` with, and the lookup goes through the definition table.
        // The exported functions of the modules this one imports are added to it
        // for exactly that.
        let mut resolutions = resolved.resolutions.clone();
        // Only names the module does not already declare: its own `@extern_C`
        // declaration of `strlen` is one function, and importing the C module's
        // exported copy as well made every call to it ambiguous.
        let already: std::collections::HashSet<(String, String)> = resolutions
            .defs
            .iter()
            .filter_map(|def| {
                def.signature.as_ref().map(|signature| {
                    (
                        def.name.clone(),
                        fallc_ast::special::symbol_for(
                            &signature.written_name,
                            &signature
                                .param_types
                                .iter()
                                .map(|ty| {
                                    fallc_sema::mangle_type(&resolve_signature_type(ty, true))
                                })
                                .collect::<Vec<_>>(),
                        ),
                    )
                })
            })
            .collect();
        let mut seen: std::collections::HashSet<(String, String)> = already;
        for dependency in direct_imports.iter().filter_map(|name| imported.get(name)) {
            for (member, signatures) in &dependency.symbols {
                for signature in signatures {
                    let key = (
                        member.clone(),
                        fallc_ast::special::symbol_for(
                            &signature.written_name,
                            &signature
                                .param_types
                                .iter()
                                .map(|ty| {
                                    fallc_sema::mangle_type(&resolve_signature_type(ty, true))
                                })
                                .collect::<Vec<_>>(),
                        ),
                    );
                    if !seen.insert(key) {
                        continue;
                    }
                    let id = fallc_sema::DefId::from_raw(
                        u32::try_from(resolutions.defs.len())
                            .expect("definition count exceeds u32::MAX"),
                    );
                    resolutions.defs.push(fallc_sema::Def {
                        id,
                        kind: fallc_sema::DefKind::Function,
                        name: member.clone(),
                        node: None,
                        name_span: fallc_span::Span::SYNTHETIC,
                        span: fallc_span::Span::SYNTHETIC,
                        scope: fallc_sema::ScopeId::from_raw(0),
                        signature: Some(signature.clone()),
                        emitted_as: None,
                        imported_as: None,
                        imported_via: None,
                        is_builtin: false,
                        is_overridable: false,
                    });
                }
            }
        }
        register_traits(&mut checker, &parsed_ast, &resolutions, &[]);
        // The templates of the modules this one imports, for the same reason the
        // unit gets them: a body of this module may call a family method of an
        // imported type -- `into.push(x)` on an `Array<T>` -- and the body of
        // that method lives in the module that declared the family. Without the
        // bodies the request was dropped silently: the call was recorded with a
        // symbol, no function was ever declared for it, and code generation
        // reported a call to a function that did not exist.
        // Everything this module can reach by imports: a body it may legally
        // instantiate, and no other. Registering every body loaded so far put
        // another module's family under the same symbol -- `Box<int>` from
        // `std.memory` and a module's own `Box<int>` mangle alike -- and the
        // wrong declaration then met the body.
        let mut reachable: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        let mut pending_names: Vec<String> = direct_imports.clone();
        while let Some(dependency) = pending_names.pop() {
            if !reachable.insert(dependency.clone()) {
                continue;
            }
            if let Some(next) = module_imports.get(&dependency) {
                pending_names.extend(next.iter().cloned());
            }
        }
        for body in bodies
            .iter()
            .filter(|body| reachable.contains(&body.module))
        {
            let families = exports::family_parameter_names(&body.ast);
            for item in &body.ast.items {
                if let fallc_ast::ItemKind::Function(function) = &body.ast.item(*item).kind
                    && (!function.template_params.is_empty()
                        || function.has_pack()
                        || exports::method_of_family(function, &families))
                {
                    checker.add_template_definition(
                        exports::symbol_of_template(&body.ast, function),
                        fallc_sema::TemplateBody {
                            tree: body.ast.clone(),
                            resolutions: body.checker_resolutions.clone(),
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
        // The module's *own* templates, so that a body of its own -- its
        // `__setup__`, say, or a method that constructs a member of a family it
        // declared -- can be instantiated while it is checked. The importing
        // unit registers them for the bodies *it* instantiates; without this,
        // a module that instantiated one of its own templates found no body,
        // dropped the request silently, and the emitted program called a
        // constructor nothing defined.
        let families = exports::family_parameter_names(&parsed_ast);
        // One shared copy of the resolutions for every template this module
        // declares: cloning them per template made a module with 148 templates
        // hold 148 copies of its whole resolution table, which is what made
        // checking `std.tree` cost a hundred megabytes.
        let shared_resolutions = std::rc::Rc::new(resolutions.clone());
        for item in &parsed_ast.items {
            if let fallc_ast::ItemKind::Function(function) = &parsed_ast.item(*item).kind
                && (!function.template_params.is_empty()
                    || function.has_pack()
                    || exports::method_of_family(function, &families))
            {
                checker.add_template_definition(
                    exports::symbol_of_template(&parsed_ast, function),
                    fallc_sema::TemplateBody {
                        tree: std::rc::Rc::clone(&parsed_ast),
                        resolutions: std::rc::Rc::clone(&shared_resolutions),
                        // The module's own constants are the checker's, since
                        // this is its own tree being checked.
                        constants: std::collections::HashMap::new(),
                        constant_types: std::collections::HashMap::new(),
                        function: function.clone(),
                        attrs: parsed_ast.item(*item).attrs.clone(),
                        environment: None,
                        bindings: fallc_sema::BindingTypes::new(),
                    },
                );
            }
        }
        report_rss("pre-check", name);
    let checked = checker.check(&parsed_ast, &resolutions);
    report_rss("checked", name);
        session.report_all(checked.diagnostics.clone());
        // Every module-level constant, already folded by the module's own
        // check -- which is where the tree its expression ids belong to was in
        // hand.
        let module_constants = checked.constants_by_name.clone();
        let module_constant_types = checked.constant_types_by_name.clone();
        // Kept for `collect_exports`, which runs after `checked` is moved.
        let checked_constants = checked.constants_by_name.clone();
        let checked_declarations = checked.types_declarations.clone();
        let checked_bindings = checked.binding_types.clone();
        let checked_disabled = checked.disabled_functions.clone();
        // The values of the defaults the module's functions declare, folded
        // where their expressions live.
        let checked_defaults = checked.default_values.clone();
        // The folded value of every constant expression, which is how a type
        // member's answer crosses: `IsInteger<int>`'s `VALUE` is published as
        // the number the declaring module folded, not as an initialiser the
        // importer would have to fold in a tree it does not own.
        let checked_span_constants = checked.constant_values.clone();
        // The tables the module's own templated bodies were written against,
        // marked with the module's name so a clash can say where the other
        // declaration is.
        let mut module_environment = checked.environment.clone();
        module_environment.module = name.to_owned();
        let resolved = std::rc::Rc::new(fallc_ir::ModuleResolutions::of(&checked));
        report_rss("resolved", name);
        // Kept for the check an importer performs on this module's templates:
        // the importer resolves *this* module's tree, and the names in it are
        // this module's own imports -- `panic.panic` is a call into a module
        // the importing file may never have heard of. Taken *after* the
        // imported symbols were added, because a template body that writes
        // `util.__ft_moves<T>()` reaches a definition its module imported, and
        // the early copy did not have it.
        let resolved_for_checking = std::rc::Rc::new(resolutions.clone());
        report_rss("checker resolutions", name);
        report_rss("body stored", name);
        bodies.push(ModuleBody {
            module: name.to_owned(),
            cache_hash: source_hash,
            declarations: checked.types_declarations.clone(),
            ast: parsed_ast.clone(),
            types: std::rc::Rc::new(checked.types),
            binding_types: checked.binding_types,
            resolved,
            checker_resolutions: resolved_for_checking,
            external_global_refs: checked.external_global_refs.clone(),
            constant_values: module_constants,
            constant_types: module_constant_types,
            environment: std::rc::Rc::new(module_environment),
        });
        if std::env::var_os("FALLC_STAGES").is_some() {
            let resident = std::fs::read_to_string("/proc/self/statm")
                .ok()
                .and_then(|text| text.split_whitespace().nth(1).and_then(|p| p.parse::<u64>().ok()))
                .map(|p| p * 4096 / (1024 * 1024))
                .unwrap_or(0);
            eprintln!(
                "  module {:<32} {:>3}  {:>5} MB",
                name,
                bodies.len(),
                resident
            );
        }
        // The folded constants travel too, so an exported one can be published
        // by value: the expression that computed it belongs to this tree.
        collect_exports(
            &parsed_ast,
            &checked_constants,
            &checked_declarations,
            &checked.type_of_types,
            &checked_bindings,
            &checked_disabled,
            &checked_defaults,
            &checked_span_constants,
            name,
        )
    };

    let mut exports = module_exports;
    for dependency in &reexported {
        exports.merge(dependency);
    }

    // The module's interface is published as source: a file of the
    // declarations an importer may name, and nothing else. Writing it out here
    // -- from the export table that was just built -- is what lets an importer
    // read the interface as text instead of re-reading and re-checking the
    // module's implementation. See `interface`.
    crate::pipeline::interface::publish(&exports, name, &parsed_ast);

    // Every exported function is also recorded as an external symbol, keyed by
    // its qualified name, so the code generator can declare the ones that are
    // actually called.
    //
    // A re-exported name is registered under *this* module too. `C` exports
    // `printf` because `Stdio` does, and a program that writes `C.printf` is
    // the common case: the key has to exist for the umbrella's own name or the
    // call reaches codegen as an undeclared symbol.
    for (member, signatures) in &exports.symbols {
        for key in &module_keys(name) {
            // Keyed by the qualified name *and* the symbol, because the
            // mangling carries the parameter types: two overloads of one name
            // are two entries, not one that overwrites the other.
            for signature in signatures {
                let qualified = format!("{key}.{member}");
                let external = external_signature(signature, &module_declared_types);
                // Keyed by the qualified name, which is what a call site has.
                // The mangled symbol travels inside the signature, so two
                // overloads of one name are two entries under two *different*
                // keys only when their parameters differ -- which is exactly
                // what the symbol distinguishes.
                externals.entry(qualified.clone()).or_insert_with(|| external.clone());
                externals.insert(format!("{qualified}#{}", external.symbol), external);
            }
        }
    }
    module_links.extend(collect_links(&parsed_ast, &[]));
    imported.insert(name.to_owned(), exports.clone());
    Some(exports)
}

/// Selects what an `export from` republishes from the module it names.
///
/// `export from M import *;` publishes everything `M` exports, and
/// `export from M import a, b;` publishes exactly those. Either way only names
/// `M` itself exported are available: a name has to be published at every step
/// of the chain, which is what keeps `export` meaning one thing.
pub(super) fn bind_reexport(
    import: &fallc_ast::ImportItem,
    dependency: &fallc_sema::ExternalModule,
) -> fallc_sema::ExternalModule {
    let mut selected = fallc_sema::ExternalModule::default();
    match &import.members {
        Some(fallc_ast::ImportedMembers::All) => selected.merge(dependency),
        Some(fallc_ast::ImportedMembers::Named(names)) => {
            for name in names {
                if let Some(signatures) = dependency.symbols.get(&name.name) {
                    for signature in signatures {
                        selected.add_function(name.name.clone(), signature.clone());
                    }
                }
                if let Some(ty) = dependency.types.get(&name.name) {
                    selected.add_type(name.name.clone(), ty.clone());
                }
                // The other kinds are re-exported the same way: a name has to
                // be published at every step of the chain.
                if let Some(value) = dependency.constants.get(&name.name).copied() {
                    selected.add_constant(name.name.clone(), value);
                    if let Some(ty) = dependency.constant_types.get(&name.name) {
                        selected
                            .constant_types
                            .insert(name.name.clone(), ty.clone());
                    }
                }
                if let Some(variable) = dependency.variables.get(&name.name) {
                    selected
                        .variables
                        .insert(name.name.clone(), variable.clone());
                }
                if let Some(value) = dependency.enums.get(&name.name) {
                    selected.add_enum(name.name.clone(), value.clone());
                }
                if let Some(value) = dependency.interfaces.get(&name.name) {
                    selected.add_interface(name.name.clone(), value.clone());
                }
                if let Some(value) = dependency.globals.get(&name.name) {
                    selected.globals.insert(name.name.clone(), value.clone());
                }
                if dependency.traits.contains(&name.name) {
                    selected.traits.insert(name.name.clone());
                }
            }
        }
        // A re-export needs members to name; `export import C;` has none.
        None => {}
    }
    selected
}

/// Turns one import declaration into the local names it introduces.
///
/// The three spellings differ only here: a plain `import C` binds nothing on
/// its own, an `import C as Libc` binds the module under another name, and a
/// `from C import a, b` or `from C import *` binds the members themselves.
#[allow(clippy::too_many_arguments)]
pub(super) fn bind_import(
    session: &mut Session,
    import: &fallc_ast::ImportItem,
    exports: &fallc_sema::ExternalModule,
    bindings: &mut Vec<fallc_sema::ImportBinding>,
    aliases: &mut Vec<(String, String)>,
    externals: &mut std::collections::HashMap<String, fallc_ir::ExternalSignature>,
    declared_types: &[fallc_sema::NominalType],
) {
    let Some(members) = &import.members else {
        // A plain `import M` binds no bare name, but the module's members are
        // still recorded so the resolver can define them: a special method is
        // reached by its bound name rather than through the module, so
        // `String s = "hi";` needs `__init__String` to exist as a definition.
        bindings.push(fallc_sema::ImportBinding {
            local: import.module.clone(),
            module: import.module.clone(),
            member: None,
            span: import.name_span,
            reexported: false,
        });
        // `import C as Libc;` — the module is also reachable as `Libc.x`.
        if let Some(alias) = &import.alias {
            // A module alias is registered as a second name for the module, so
            // `Libc.printf` resolves exactly as `C.printf` does. The symbols
            // are filed under the alias too: the call site names `Libc.printf`,
            // and that is the key codegen looks the signature up by.
            aliases.push((alias.name.clone(), import.module.clone()));
            for (member, signatures) in &exports.symbols {
                for signature in signatures {
                    let qualified = format!("{}.{member}", alias.name);
                    let external = external_signature(signature, declared_types);
                    externals
                        .entry(qualified.clone())
                        .or_insert_with(|| external.clone());
                    externals.insert(format!("{qualified}#{}", external.symbol), external);
                }
            }
        }
        return;
    };
    // Whatever the form, the module's *functions* have to be defined, not only
    // the ones bound by name: a method is reached through the definition table
    // rather than through a name in scope, and an interface's satisfaction
    // check looks its methods up there. `from m import Square` used to leave
    // `Square`'s methods undefined, so the type satisfied no interface.
    bindings.push(fallc_sema::ImportBinding {
        local: import.module.clone(),
        module: import.module.clone(),
        member: None,
        span: import.name_span,
        reexported: false,
    });
    match members {
        fallc_ast::ImportedMembers::Named(names) => {
            for name in names {
                // Every kind of member may be imported by name -- a function,
                // a type, a constant, a variable, an enum, an interface. Asking
                // only about functions reported a name the module *does* export
                // as private.
                if !exports.exports(&name.name) {
                    session.report(
                        Diagnostic::error(format!(
                            "module `{}` does not export `{}`",
                            import.module, name.name
                        ))
                        .with_code(DiagnosticCode::sema(4))
                        .with_span(name.span)
                        .with_note("a module's declarations are private unless marked `export`"),
                    );
                    continue;
                }
                bindings.push(fallc_sema::ImportBinding {
                    local: name.name.clone(),
                    module: import.module.clone(),
                    member: Some(name.name.clone()),
                    span: name.span,
                    reexported: false,
                });
            }
        }
        fallc_ast::ImportedMembers::All => {
            // Every export, in a stable order so that two builds of the same
            // source bind the same names in the same sequence -- and every
            // *kind* of export: `from m import *` is the whole module, not its
            // functions.
            let names = exports.export_names();
            for name in names {
                bindings.push(fallc_sema::ImportBinding {
                    local: name.clone(),
                    module: import.module.clone(),
                    member: Some(name.clone()),
                    span: import.name_span,
                    reexported: true,
                });
            }
        }
    }
}

/// Installs everything the imported modules contribute into a resolver.
///
/// Both entry points must do exactly this. Doing it separately is how the two
/// came to disagree once already: `mark_overloads` was applied in `compile_file`
/// and not in `compile_text`, so the same source produced mangled symbols in one
/// and plain ones in the other.
pub(super) fn install_imports(
    resolver: &mut fallc_sema::Resolver,
    modules: std::collections::HashMap<String, fallc_sema::ExternalModule>,
    bindings: Vec<fallc_sema::ImportBinding>,
    aliases: &[(String, String)],
    direct: &[(String, String)],
) -> Vec<String> {
    let mut module_names: Vec<String> = Vec::new();
    // Which module each name is registered as, so that a name is never filed
    // twice under two different modules.
    let mut registered: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    // The file's own imports first, sorted: what a file asked for owns its
    // name. `import math;` in a program that also imports `C` must mean the
    // program's `math`, not `C.math` -- a module the program never named.
    let mut requested = direct.to_vec();
    requested.sort();
    let register = |resolver: &mut fallc_sema::Resolver,
                        module_names: &mut Vec<String>,
                        registered: &mut std::collections::HashMap<String, String>,
                        module: &str,
                        key: String,
                        short: bool| {
        match registered.get(&key) {
            Some(existing) if existing != module => {
                // A transitive module may not take a name the file asked for;
                // two *direct* imports sharing one are reported by the loader,
                // which is the part that knows both files.
                return;
            }
            Some(_) => return,
            None => {}
        }
        let Some(external) = modules.get(module).cloned() else {
            return;
        };
        registered.insert(key.clone(), module.to_owned());
        resolver.add_module(key.clone(), external);
        module_names.push(key.clone());
        let _ = short;
    };
    for (module, _path) in &requested {
        register(resolver, &mut module_names, &mut registered, module, module.clone(), false);
        if let Some((_, last)) = module.rsplit_once('.') {
            register(resolver, &mut module_names, &mut registered, module, last.to_owned(), true);
        }
    }
    // Then everything else by its full name, so that a module reached through
    // another is still addressable -- an instantiated `Array` body writes
    // `util.move(..)`, and the unit instantiating it imports only `std`.
    let mut loaded: Vec<&String> = modules.keys().collect();
    loaded.sort();
    let mut all_names: Vec<String> = Vec::new();
    for name in loaded {
        register(resolver, &mut module_names, &mut registered, name, name.clone(), false);
        // The *checker* knows every module by its last segment as well: a
        // body instantiated in this unit was written in another module and
        // says `util.move(..)`, so `util` has to be a module name here even
        // though this file never imported it. The resolver's table is what
        // restricts a program's own `X.y` spelling, and that stays direct-only.
        all_names.push(name.clone());
        if let Some((_, last)) = name.rsplit_once('.') {
            all_names.push(last.to_owned());
        }
    }
    module_names.extend(all_names);
    // An alias is a second way to write a module's name, so it joins the same
    // table under the name the file chose.
    for (alias, module) in aliases {
        if let Some(external) = resolver.module(module).cloned() {
            resolver.add_module(alias.clone(), external);
            module_names.push(alias.clone());
        }
    }
    for binding in bindings {
        resolver.add_import(binding);
    }
    module_names
}

/// Returns the names a module is reachable by.
///
/// A dotted module has two: its full path and its last segment. `import
/// std.string;` loads `standard/std/string.ftm`, and a program writes
/// `string.String` -- requiring `std.string.String` would make the directory
/// part of the *name* rather than of the path.
///
/// The rule lives here because three places need it -- the module loader, the
/// resolver's table, and the checker's set of module names -- and they were
/// written separately once already, which is why one of them did not know the
/// short name.
pub(super) fn module_keys(name: &str) -> Vec<String> {
    let mut keys = vec![name.to_owned()];
    if let Some((_, last)) = name.rsplit_once('.') {
        keys.push(last.to_owned());
    }
    keys
}

