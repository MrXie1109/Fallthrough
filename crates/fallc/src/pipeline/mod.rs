//! The compilation pipeline: stage order, short-circuiting, exit status.
//!
//! Each stage is a function taking `&mut Session` plus the previous stage's
//! output and returning the next stage's output. A stage that reports a fatal
//! diagnostic stops the pipeline: later stages assume well-formed input, and
//! running them on broken input produces cascading nonsense rather than
//! additional useful errors.
//!
//! # Why an `Outcome` and not an exit code
//!
//! [`run`] is a library function and should not decide process exit status,
//! but "compilation succeeded", "the program is wrong", and "the compiler is
//! unfinished" are three genuinely different results that the caller must be
//! able to tell apart. [`Outcome`] carries that distinction; `main` maps it to
//! a code.

use std::fs;
use std::path::Path;
use std::path::PathBuf;

use fallc_ast::Ast;
use fallc_diag::{Diagnostic, DiagnosticCode};

use fallc_lexer::{Lexer, LexerConfig};
use fallc_parser::{Parser, ParserConfig, TokenStream};
use fallc_sema::{Resolutions, TypeChecker, TypeTable};
use fallc_span::FileId;

use crate::cli::{Command, EmitKind, Options};
use crate::emit;
use crate::modules;
use crate::session::Session;

/// The compiler-provided macros a file may expand.
///
/// The values are facts about this compilation, not about the source: the file's
/// own name, the platform and architecture the program is being built for, and
/// the date and time of the build. The compiler targets the machine it runs on,
/// so the platform is the host's.
pub(super) fn builtins_for(path: &Path, text: &str) -> fallc_lexer::macros::Builtins {
    let platform = crate::modules::target_platform();
    let windows = platform == "windows";
    let (date, time) = build_date_and_time();
    fallc_lexer::macros::Builtins {
        file: path.display().to_string(),
        source: text.to_owned(),
        platform: platform.to_owned(),
        arch: std::env::var("FALLC_TARGET_ARCH")
            .unwrap_or_else(|_| std::env::consts::ARCH.to_owned()),
        windows,
        date,
        time,
    }
}

/// The build's date and time, as `YYYY-MM-DD` and `HH:MM:SS`.
///
/// The calendar arithmetic is Howard Hinnant's `civil_from_days`: days since
/// 1970-01-01 in, a year, month and day out, with no dependency and no table.
fn build_date_and_time() -> (String, String) {
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let seconds = since % 86_400;
    let days = i64::try_from(since / 86_400).unwrap_or(0);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    (
        format!("{year:04}-{month:02}-{day:02}"),
        format!(
            "{:02}:{:02}:{:02}",
            seconds / 3_600,
            (seconds % 3_600) / 60,
            seconds % 60
        ),
    )
}

/// The declared type of each of a module's module-level variables, by the span
/// of its name in the module's tree.
///
/// A template body instantiated in another module still writes its module's
/// globals by their bare names -- the short `print` in `std.io` writes `out` --
/// and a variable's type lives in the binding table of the module that declared
/// it. Carrying it with the body is what lets the name be typed where the body
/// is instantiated; without it the member call on the global was never
/// resolved, and code generation had no record to emit.
///
/// Only module-level variables travel: a function's locals belong to the body
/// being instantiated, and that body records them itself.
pub(super) fn module_variable_bindings(
    ast: &Ast,
    binding_types: &fallc_sema::BindingTypes,
) -> fallc_sema::BindingTypes {
    let mut bindings = fallc_sema::BindingTypes::new();
    for item in &ast.items {
        if let fallc_ast::ItemKind::Variable(variable) = &ast.item(*item).kind
            && let Some(ty) = binding_types.get(variable.name_span)
        {
            bindings.insert(variable.name_span, ty.clone());
        }
    }
    bindings
}

/// How a compilation run ended.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// Every requested stage ran and produced no fatal diagnostic.
    Success,
    /// The program is invalid; diagnostics explain why.
    Failure,
    /// The compiler reached a construct that the language spec does not yet
    /// define. The program may be perfectly good.
    Unspecified,
}

impl Outcome {
    /// Maps this outcome to a process exit code.
    #[must_use]
    pub fn exit_code(self) -> u8 {
        match self {
            Outcome::Success => 0,
            Outcome::Failure => 1,
            Outcome::Unspecified => 3,
        }
    }
}

/// Type-checks one file and produces no output.
///
/// The language server needs the diagnostics and the spans they point at, but
/// nothing else a compile produces: no object, no link, no cache entry. This is
/// `--check` without the reporting, so a caller can read
/// [`Session::diagnostics`] itself.
pub fn check_file(session: &mut Session, path: &Path) -> Outcome {
    compile::compile_inputs(
        session,
        std::slice::from_ref(&path.to_path_buf()),
        &[],
        false,
        true,
        fallc_ir::BuildMode::Debug,
        fallc_ir::OptLevel::None,
        false,
        None,
    )
}

/// Runs `fallc` according to `options`.
pub fn run(options: &Options) -> std::process::ExitCode {
    match &options.command {
        Command::Version => {
            println!("fallc {}", env!("CARGO_PKG_VERSION"));
            std::process::ExitCode::SUCCESS
        }
        // The usage text goes to stdout, because it was asked for. The same
        // text is written to stderr when an argument is wrong, where it is an
        // explanation rather than a result.
        Command::Help => {
            print!("{}", crate::cli::USAGE);
            std::process::ExitCode::SUCCESS
        }
        // The language server owns stdin and stdout for the whole run.
        Command::LanguageServer => std::process::ExitCode::from(crate::lsp::run()),
        Command::Compile {
            inputs,
            emit,
            preserve_trivia,
            check_only,
            release,
            opt,
            keep_temporaries,
            output,
            warnings,
            color,
        } => {
            let mut session = Session::new();
            session.set_warnings(warnings.clone());
            session.set_input_files(inputs.iter().cloned());
            session.set_color(color.wants(
                std::io::IsTerminal::is_terminal(&std::io::stderr()),
                std::env::var_os("NO_COLOR").is_some(),
            ));
            let mode = if *release {
                fallc_ir::BuildMode::Release
            } else {
                fallc_ir::BuildMode::Debug
            };
            // Several inputs are one program, not several: each is compiled to
            // an object and they are linked together, so a function may be
            // defined in one file and called from another. A single input is
            // the same path with one element, which keeps the two cases from
            // drifting apart.
            let outcome = compile_inputs(
                &mut session,
                inputs,
                emit,
                *preserve_trivia,
                *check_only,
                mode,
                *opt,
                *keep_temporaries,
                output.as_deref(),
            );

            report(&session);
            std::process::ExitCode::from(outcome.exit_code())
        }
    }
}

impl PartialOrd for Outcome {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Outcome {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        fn rank(outcome: Outcome) -> u8 {
            match outcome {
                Outcome::Success => 0,
                Outcome::Unspecified => 1,
                Outcome::Failure => 2,
            }
        }
        rank(*self).cmp(&rank(*other))
    }
}

/// Everything the imported modules contribute to a compilation.
///
/// A record rather than a tuple: five values of four different shapes is past
/// the point where a tuple says what its parts are, and they are threaded
/// through two entry points that must agree.
#[derive(Default)]
struct LoadedImports {
    /// What each module exports, by module name.
    modules: std::collections::HashMap<String, fallc_sema::ExternalModule>,
    /// The umbrella each module is re-exported through, by module name.
    ///
    /// A program writes `std.Array`, never `std.array.Array`: the umbrella is
    /// what it imported, so the umbrella is the name a qualified reference
    /// starts with. Keeping the mapping is what lets `std.Array` and
    /// `std.String` resolve while `Array` and `String` do too.
    umbrellas: std::collections::HashMap<String, Vec<String>>,
    /// The external signatures codegen declares on demand, by qualified name.
    externals: std::collections::HashMap<String, fallc_ir::ExternalSignature>,
    /// The local names the imports introduce.
    bindings: Vec<fallc_sema::ImportBinding>,
    /// Module aliases, as (alias, module) pairs.
    aliases: Vec<(String, String)>,
    /// The modules *this* file imported, by the name it wrote.
    ///
    /// A short name -- the last segment of a dotted module -- exists only for
    /// what the file asked for. A transitive dependency keeps its full name: a
    /// program that imported `C` must write `C.math`, and one that did not
    /// import `C` at all must not find `math` in it.
    direct: Vec<(String, String)>,
    /// Link instructions the imported modules declare.
    links: Vec<LinkRequest>,
    /// The modules that define something, in load order.
    ///
    /// A module is not only a set of signatures: `export fn f() int { .. }`
    /// defines `f` here, and nothing else will emit it. Each entry carries what
    /// lowering a body needs -- its own tree, its own type table, and its own
    /// binding types -- because those are per-file and cannot be shared.
    #[allow(clippy::type_complexity)]
    bodies: Vec<ModuleBody>,
}

/// One imported module, kept so its function bodies can be lowered.
struct ModuleBody {
    /// The module's own name, as the loader knows it.
    ///
    /// Carried because a trait travels with the tree it was written in, and
    /// registering it under the names a program may write needs the module the
    /// tree belongs to.
    module: String,
    /// The nominal types the module declared, so code generation knows their
    /// layout while lowering its bodies.
    declarations: Vec<fallc_sema::NominalType>,
    /// The key the module-object cache stores this module's body under.
    ///
    /// Not a hash of its own text alone: the body is compiled against the
    /// interfaces of every module it imports, so the key covers the whole
    /// dependency closure. Editing `std.array` must not reuse the cached
    /// object of `std.string`, which calls it.
    cache_hash: u64,
    /// Its parsed tree.
    ///
    /// Shared: it is walked for template bodies, re-borrowed by code
    /// generation, and copied per template before this was an `Rc` -- a
    /// module's tree is the same tree for all of its templated functions.
    ast: std::rc::Rc<Ast>,
    /// The types its expressions were given.
    types: std::rc::Rc<fallc_sema::TypeTable>,
    /// The declared type of each of its local bindings.
    binding_types: fallc_sema::BindingTypes,
    /// Everything its own check resolved: which method each call is, which
    /// symbol each operator took, and so on.
    ///
    /// These are kept because the module's *bodies* are lowered by whichever
    /// module imports it, and lowering a body means reading those decisions.
    /// Discarding them meant a module's own private methods were unresolvable
    /// from inside that module: the call had been resolved during the module's
    /// own check, and the answer was thrown away before anything could read it.
    resolved: std::rc::Rc<fallc_ir::ModuleResolutions>,
    /// Which spans in the module's tree read storage another module owns.
    ///
    /// A span names a byte range in one file, so these stay per-module exactly
    /// as the resolutions do; code generation reads them while lowering the
    /// module's bodies.
    external_global_refs: std::collections::HashMap<fallc_span::Span, String>,
    /// The module's own resolutions, as the checker's own type.
    ///
    /// Kept beside the IR-shaped copy because a template declared in this
    /// module is *checked* by whichever file instantiates it, and that check
    /// needs to resolve names the module's own body uses -- `panic.panic` is a
    /// call into a module this one imported, and the instantiating file may
    /// never have heard of it.
    checker_resolutions: std::rc::Rc<fallc_sema::Resolutions>,
    /// The module's constants, already folded to their values.
    ///
    /// Folded while the module's own tree was in hand, because a constant is
    /// identified by an expression id and an id belongs to one tree.
    constant_values: std::collections::HashMap<String, i64>,
    /// The declared type of each of those constants, by name.
    ///
    /// Carried beside the values because the type is what code generation
    /// stores with: a constant whose type is unknown was stored as an `int`,
    /// which for a `usize` wrote four bytes of an eight-byte slot.
    constant_types: std::collections::HashMap<String, fallc_sema::Type>,
    /// Everything this module declared, for the template bodies it owns.
    ///
    /// A body is checked where it is instantiated, so the declarations it names
    /// travel with it -- including the ones the module kept private.
    environment: std::rc::Rc<fallc_sema::TypeEnvironment>,
}

impl ModuleBody {
    /// Drops the checker tables that only template registration needed.
    ///
    /// A module's templates are registered with their own resolutions,
    /// constants and environment; after that the checker holds what it needs,
    /// and code generation reads only the module's tree, types, bindings and
    /// IR-shaped resolutions. Keeping a second copy of every module's
    /// resolution table and environment alive through code generation was
    /// pure retention: the tables are large and nothing reads them again.
    pub(super) fn release_check_tables(&mut self) {
        self.checker_resolutions = std::rc::Rc::new(fallc_sema::Resolutions::new());
        self.constant_values = std::collections::HashMap::new();
        self.constant_types = std::collections::HashMap::new();
        self.environment = std::rc::Rc::new(fallc_sema::TypeEnvironment::default());
    }
}

impl LoadedImports {
    /// Returns every exported type of every loaded module.
    ///
    /// A type is named the same way wherever it came from, so the checker gets
    /// one flat list rather than a module-keyed one. Two modules exporting the
    /// same type name is a collision the language has no rule for yet; the
    /// first one registered wins.
    fn exported_types(&self) -> Vec<(String, fallc_sema::ExportType, String)> {
        let mut types: Vec<(String, fallc_sema::ExportType, String)> = Vec::new();
        let mut modules: Vec<&String> = self.modules.keys().collect();
        modules.sort();
        for module in modules {
            let Some(external) = self.modules.get(module) else {
                continue;
            };
            let mut names: Vec<&String> = external.types.keys().collect();
            names.sort();
            for name in names {
                if let Some(ty) = external.types.get(name) {
                    // Reported once per *name* a program may write. The bare
                    // name is what a sibling module uses; the umbrella's name
                    // is what a program that imported the umbrella uses --
                    // `std.String`, not `std.string.String`. Both are filed, so
                    // that `register_imported_types` can refuse to choose.
                    // Three names, all of which work: the bare one that a
                    // sibling module writes, the submodule's own
                    // (`std.string.String`), and the umbrella's
                    // (`std.String`) -- the last because the umbrella is what a
                    // program imported.
                    let mut candidates = vec![
                        (name.clone(), "".to_owned()),
                        (format!("{module}.{name}"), module.clone()),
                    ];
                    for umbrella in self.umbrellas.get(module).into_iter().flatten() {
                        candidates.push((format!("{umbrella}.{name}"), umbrella.clone()));
                    }
                    for candidate in candidates {
                        if !types
                            .iter()
                            .any(|(existing, _, _)| *existing == candidate.0)
                        {
                            types.push((candidate.0, ty.clone(), candidate.1));
                        }
                    }
                }
            }
        }
        types
    }

    /// Returns every `extern` variable the loaded modules exported, under each
    /// name a program may write to reach it.
    ///
    /// The names are the same three a type gets: the bare one a sibling module
    /// writes, the submodule's own (`C.stdio.stdin`), and the umbrella's
    /// (`C.stdin`), because the umbrella is what a program imported.
    fn exported_variables(&self) -> Vec<(String, fallc_sema::ExportedVariable)> {
        let mut variables: Vec<(String, fallc_sema::ExportedVariable)> = Vec::new();
        let mut modules: Vec<&String> = self.modules.keys().collect();
        modules.sort();
        for module in modules {
            let Some(external) = self.modules.get(module) else {
                continue;
            };
            let mut names: Vec<&String> = external.variables.keys().collect();
            names.sort();
            for name in names {
                let Some(variable) = external.variables.get(name) else {
                    continue;
                };
                let mut candidates = vec![name.clone(), format!("{module}.{name}")];
                for umbrella in self.umbrellas.get(module).into_iter().flatten() {
                    candidates.push(format!("{umbrella}.{name}"));
                }
                for candidate in candidates {
                    if !variables.iter().any(|(existing, _)| *existing == candidate) {
                        variables.push((candidate, variable.clone()));
                    }
                }
            }
        }
        variables
    }

    /// Returns every module-level variable the loaded modules exported, under
    /// each name a program may write for it.
    ///
    /// A variable is one object for the whole program, so what crosses is its
    /// symbol and its type; the importer reads and writes the same storage.
    fn exported_globals(&self) -> Vec<(String, fallc_sema::ExportedGlobal)> {
        let mut globals: Vec<(String, fallc_sema::ExportedGlobal)> = Vec::new();
        for (name, value) in self.exported_members(|external| &external.globals) {
            if !globals.iter().any(|(existing, _)| *existing == name) {
                globals.push((name, value));
            }
        }
        globals
    }

    /// Returns every constant the loaded modules exported, under each name a
    /// program may write for it, with its value and its declared type.
    ///
    /// The type travels because a value alone does not say how wide it is, and
    /// the same three names a type gets are what a program may write.
    fn exported_constants(&self) -> Vec<(String, i64, fallc_ast::TypeSyntax)> {
        let mut constants: Vec<(String, i64, fallc_ast::TypeSyntax)> = Vec::new();
        let mut modules: Vec<&String> = self.modules.keys().collect();
        modules.sort();
        for module in modules {
            let Some(external) = self.modules.get(module) else {
                continue;
            };
            let mut names: Vec<&String> = external.constants.keys().collect();
            names.sort();
            for name in names {
                let Some(value) = external.constants.get(name).copied() else {
                    continue;
                };
                let Some(ty) = external.constant_types.get(name).cloned() else {
                    continue;
                };
                for candidate in self.spellings(module, name) {
                    if !constants
                        .iter()
                        .any(|(existing, _, _)| *existing == candidate)
                    {
                        constants.push((candidate, value, ty.clone()));
                    }
                }
            }
        }
        constants
    }

    /// Returns every enum the loaded modules exported, under each name a
    /// program may write for it.
    ///
    /// The same three names a type gets: the bare one a sibling module writes,
    /// the submodule's own, and the umbrella's. An enum is a type whose members
    /// are named constants, and both halves have to be reachable under the name
    /// the program wrote.
    fn exported_enums(&self) -> Vec<(String, fallc_sema::ExportEnum)> {
        let mut enums: Vec<(String, fallc_sema::ExportEnum)> = Vec::new();
        for (name, value) in self.exported_members(|external| &external.enums) {
            if !enums.iter().any(|(existing, _)| *existing == name) {
                enums.push((name, value));
            }
        }
        enums
    }

    /// Returns every interface the loaded modules exported, under each name a
    /// program may write for it.
    fn exported_interfaces(&self) -> Vec<(String, fallc_sema::ExportInterface)> {
        let mut interfaces: Vec<(String, fallc_sema::ExportInterface)> = Vec::new();
        for (name, value) in self.exported_members(|external| &external.interfaces) {
            if !interfaces.iter().any(|(existing, _)| *existing == name) {
                interfaces.push((name, value));
            }
        }
        interfaces
    }

    /// Returns each exported member of one kind under the three names a program
    /// may write to reach it.
    ///
    /// The names are the bare one a sibling module writes, the module's own
    /// qualified name, and the umbrella's -- the last because the umbrella is
    /// what a program imported. Modules are visited in a stable order, so two
    /// builds of the same program file the same names in the same sequence.
    fn exported_members<T: Clone>(
        &self,
        of: impl Fn(&fallc_sema::ExternalModule) -> &std::collections::HashMap<String, T>,
    ) -> Vec<(String, T)> {
        let mut members: Vec<(String, T)> = Vec::new();
        let mut modules: Vec<&String> = self.modules.keys().collect();
        modules.sort();
        for module in modules {
            let Some(external) = self.modules.get(module) else {
                continue;
            };
            let mut names: Vec<&String> = of(external).keys().collect();
            names.sort();
            for name in names {
                let Some(value) = of(external).get(name) else {
                    continue;
                };
                for candidate in self.spellings(module, name) {
                    if !members.iter().any(|(existing, _)| *existing == candidate) {
                        members.push((candidate, value.clone()));
                    }
                }
            }
        }
        members
    }

    /// The names one exported member is reachable by.
    fn spellings(&self, module: &str, name: &str) -> Vec<String> {
        let mut names = vec![name.to_owned(), format!("{module}.{name}")];
        // A directly imported module is reachable by its short name too:
        // `import std.io;` is what makes `io.out` mean `std.io.out`. The
        // resolver only registers a short module name for a direct import, so
        // this spelling cannot reach a module the file did not ask for.
        if let Some((_, short)) = module.rsplit_once('.') {
            names.push(format!("{short}.{name}"));
        }
        for umbrella in self.umbrellas.get(module).into_iter().flatten() {
            names.push(format!("{umbrella}.{name}"));
        }
        names
    }

    /// Returns every trait the loaded modules exported, under each name a
    /// program may write for it.
    ///
    /// The same three names a type gets, for the same reason: a sibling module
    /// writes the bare name, a file that imported the module may qualify it
    /// with the module's own name, and a file that imported the umbrella may
    /// use the umbrella's -- `std.InputIterator`, not
    /// `std.iterator.InputIterator`.
    ///
    /// The traits come from the module *trees* rather than from the export
    /// table, because a requirement is an expression and only the tree it was
    /// written in can be checked.
    fn exported_traits(&self) -> Vec<(String, fallc_sema::TraitBody)> {
        let mut traits: Vec<(String, fallc_sema::TraitBody)> = Vec::new();
        for body in &self.bodies {
            let mut tree: Option<std::rc::Rc<Ast>> = None;
            let mut resolutions: Option<std::rc::Rc<fallc_sema::Resolutions>> = None;
            for &item in &body.ast.items {
                // An interface states its restrictions the way a trait does, so
                // a box in another module checks the same body: the export
                // table carries the signatures, and the tree is what the
                // restrictions need.
                let declaration = match &body.ast.item(item).kind {
                    fallc_ast::ItemKind::Trait(declaration) => declaration.clone(),
                    fallc_ast::ItemKind::Interface(interface)
                        if interface.exported && !interface.requirements.is_empty() =>
                    {
                        interface.as_trait()
                    }
                    _ => continue,
                };
                if !declaration.exported {
                    continue;
                }
                let tree = tree.get_or_insert_with(|| std::rc::Rc::clone(&body.ast));
                let resolutions = resolutions
                    .get_or_insert_with(|| std::rc::Rc::clone(&body.checker_resolutions));
                let mut names = vec![
                    declaration.name.clone(),
                    format!("{}.{}", body.module, declaration.name),
                ];
                for umbrella in self.umbrellas.get(&body.module).into_iter().flatten() {
                    names.push(format!("{umbrella}.{}", declaration.name));
                }
                let trait_body = fallc_sema::TraitBody {
                    tree: std::rc::Rc::clone(tree),
                    resolutions: std::rc::Rc::clone(resolutions),
                    declaration: declaration.clone(),
                };
                for name in names {
                    if !traits.iter().any(|(existing, _)| *existing == name) {
                        traits.push((name, trait_body.clone()));
                    }
                }
            }
        }
        traits
    }
}

/// One `link` instruction, in the form the linker needs.
#[derive(Clone, PartialEq, Eq, Debug)]
struct LinkRequest {
    /// The library's name, as written.
    library: String,
    /// Whether it is shared or an archive.
    kind: fallc_ast::LinkKind,
    /// Where to look, or `None` for the linker's own search path.
    directory: Option<String>,
}

impl LinkRequest {
    /// Returns the name to put after `-l`.
    ///
    /// A bare name is a library in the usual sense, so it is spelled the way a
    /// linker expects. A quoted name is passed through untouched, which is what
    /// lets `link "gcc_s" static from default;` name something the `lib`/`.a`
    /// convention does not fit.
    fn qualifier(&self) -> &str {
        &self.library
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_source_succeeds() {
        let (outcome, _) = compile_text("t.ft", "", &[]);
        assert_eq!(outcome, Outcome::Success);
    }

    #[test]
    fn lexical_error_fails() {
        // A backtick starts no token; `@` does now, since it begins an
        // attribute.
        let (outcome, text) = compile_text("t.ft", "`", &[]);
        assert_eq!(outcome, Outcome::Failure);
        assert!(text.contains("unrecognised character"), "{text}");
    }

    #[test]
    fn token_emission_lists_the_stream() {
        let (_, text) = compile_text("t.ft", "", &[EmitKind::Tokens]);
        assert!(text.contains("end of input"), "{text}");
        assert!(text.contains("t.ft:1:1"), "{text}");
    }

    #[test]
    fn outcome_ordering_prefers_failure() {
        assert!(Outcome::Failure > Outcome::Unspecified);
        assert!(Outcome::Unspecified > Outcome::Success);
        assert_eq!(Outcome::Success.exit_code(), 0);
        assert_eq!(Outcome::Failure.exit_code(), 1);
        assert_eq!(Outcome::Unspecified.exit_code(), 3);
    }

    #[test]
    fn module_name_comes_from_the_file_stem() {
        assert_eq!(module_name_for(Path::new("dir/hello.ft")), "hello");
        assert_eq!(module_name_for(Path::new("noext")), "noext");
    }
}

mod cache;
mod compile;
mod exports;
mod imports;
mod interface;
mod linking;
mod reporting;

pub use reporting::{
    compile_text, compile_text_in_mode, compile_text_in_mode_opt, compile_text_optimized,
    compile_text_with_warnings,
};

use compile::*;
use exports::*;
use imports::*;
use linking::*;
use reporting::*;
