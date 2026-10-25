//! Command-line parsing.
//!
//! Hand-rolled rather than derived: the driver's flag set is small, and a
//! generated parser would make it harder to give specific messages. `USAGE`
//! lists every accepted flag: `--emit`, `-o`, `-O`, `--check`,
//! `--keep-temporaries`, `--release`, the warning controls, `--color`, and
//! `--help`/`--version`.

use std::path::PathBuf;

/// The usage text printed for `--help` and on argument errors.
pub const USAGE: &str = "\
fallc — the Fallthrough compiler

USAGE:
    fallc <FILE>... [OPTIONS]

    Several files are compiled as one program: each becomes a translation
    unit, and they are linked together. A symbol may be defined once across
    the program, though declarations may be repeated.

OPTIONS:
    --emit=<KIND>   What to produce: exe (default), obj, static, shared,
                    llvm, tokens, ast, types, or all. Repeatable. `-o` names
                    the last one built, and the others are derived from it.
    -o <PATH>       Write the primary output to PATH
    -O<LEVEL>       Optimise: 0, 1, 2, 3, s, or z (default 0). A bare `-O` is
                    -O2. Independent of --release, which decides what the
                    source means rather than how hard LLVM works.
    --check         Lex, parse and type-check only; produce no output
    --no-trivia     Do not preserve comments/whitespace in tokens
    -Wall           Report every warning (the default)
    -W<name>        Report the warning group <name>
    -Wno-<name>     Do not report the warning group <name>
    -Werror         Report every warning as an error
    -Wverbose       Explain each diagnostic: why the rule is the rule, and
                    what to write instead
    --color=<when>  Colour the diagnostics: auto (default), always, or never.
                    `auto` colours a terminal only, and `NO_COLOR` turns it off
    --release       Release mode: overflow and division by zero are undefined
                    instead of panicking
    --keep-temporaries
                    Keep intermediate files. `--emit=all` implies this.
    --help, -h      Print this message
    --version, -V   Print the compiler version
    --lsp           Run the language server on stdin and stdout

A construct the compiler accepts but cannot yet turn into machine code stops
the compilation with an internal error that names it, instead of emitting
something that might not mean what the program says.
";

/// What the user asked the driver to do.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Command {
    /// Run the language server on stdin and stdout.
    LanguageServer,
    /// Compile the given input files.
    Compile {
        /// The input files, in the order given.
        inputs: Vec<PathBuf>,
        /// Which outputs to produce.
        emit: Vec<EmitKind>,
        /// Whether to preserve trivia while lexing.
        preserve_trivia: bool,
        /// Whether to stop after semantic analysis.
        check_only: bool,
        /// Whether to build for release.
        ///
        /// This changes semantics, not just optimisation: in debug builds the
        /// operations that can trap are guarded, and in release builds they are
        /// undefined behaviour.
        release: bool,
        /// How hard LLVM should optimise.
        ///
        /// Independent of `release`, which decides what the source *means*.
        /// `-O2` on a debug build gives fast code that still reports overflow.
        opt: fallc_ir::OptLevel,
        /// Whether to keep every intermediate file.
        ///
        /// `--emit=all` also implies it: keeping the files is the point of
        /// asking for everything.
        keep_temporaries: bool,
        /// Where to write the primary output, if the user named a path.
        output: Option<PathBuf>,
        /// Which warnings to report, and whether they are errors.
        warnings: fallc_diag::WarningPolicy,
        /// When to colour the diagnostics.
        color: ColorChoice,
    },
    /// Print the compiler version.
    Version,
    /// Print the usage text.
    Help,
}

/// A kind of output the driver can produce.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum EmitKind {
    /// The token stream.
    Tokens,
    /// The abstract syntax tree.
    Ast,
    /// The intermediate representation, as LLVM assembly (`.ll`).
    Llvm,
    /// The type table.
    Types,
    /// A native object file (`.o`).
    Object,
    /// A static library (`.a`), an archive of the unit's object and the
    /// module library objects it needs.
    Static,
    /// A shared library (`.so`, `.dylib` or `.dll`), linked position
    /// independently from the same objects.
    Shared,
    /// A linked executable.
    ///
    /// The only kind that produces something runnable, so it is the default
    /// when no `--emit` is given.
    Executable,
}

impl EmitKind {
    /// Parses an `--emit=` value.
    #[must_use]
    pub fn parse(text: &str) -> Option<EmitKind> {
        match text {
            "tokens" => Some(EmitKind::Tokens),
            "ast" => Some(EmitKind::Ast),
            "llvm" | "ll" => Some(EmitKind::Llvm),
            "obj" | "object" => Some(EmitKind::Object),
            "static" | "a" | "lib" => Some(EmitKind::Static),
            "shared" | "so" | "dylib" => Some(EmitKind::Shared),
            "exe" | "executable" | "bin" => Some(EmitKind::Executable),
            // `ir` is accepted as an alias and means the same thing: the
            // compiler has no IR other than LLVM's.
            "ir" => Some(EmitKind::Llvm),
            "types" => Some(EmitKind::Types),
            _ => None,
        }
    }

    /// Returns the spelling this kind is written with.
    ///
    /// The *canonical* spelling, which is what `parse` accepts and what a
    /// diagnostic should name: `ll` and `ir` are accepted aliases, but a
    /// message that offered them would be teaching two names for one thing.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            EmitKind::Tokens => "tokens",
            EmitKind::Ast => "ast",
            EmitKind::Types => "types",
            EmitKind::Llvm => "llvm",
            EmitKind::Object => "obj",
            EmitKind::Static => "static",
            EmitKind::Shared => "shared",
            EmitKind::Executable => "exe",
        }
    }

    /// Returns every kind of output, in the order they are produced.
    ///
    /// `--emit=all` is a shorthand for this list. It is a list rather than a
    /// special case so that everything downstream keeps working on kinds, and
    /// so that adding a kind adds it to `all` in one place.
    #[must_use]
    pub fn all() -> &'static [EmitKind] {
        &[
            EmitKind::Tokens,
            EmitKind::Ast,
            EmitKind::Types,
            EmitKind::Llvm,
            EmitKind::Object,
            EmitKind::Static,
            EmitKind::Shared,
            EmitKind::Executable,
        ]
    }
}

/// When the driver colours its diagnostics.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ColorChoice {
    /// Colour when the output is a terminal, and plain otherwise.
    #[default]
    Auto,
    /// Colour whether or not the output is a terminal.
    Always,
    /// Never colour.
    Never,
}

impl ColorChoice {
    /// Parses `auto`, `always`, or `never`.
    #[must_use]
    pub fn parse(value: &str) -> Option<ColorChoice> {
        match value {
            "auto" => Some(ColorChoice::Auto),
            "always" => Some(ColorChoice::Always),
            "never" => Some(ColorChoice::Never),
            _ => None,
        }
    }

    /// Returns whether diagnostics should be coloured, given the output.
    #[must_use]
    pub fn wants(self, is_terminal: bool, no_color_env: bool) -> bool {
        match self {
            ColorChoice::Always => true,
            ColorChoice::Never => false,
            // `NO_COLOR` is the convention a user sets once and expects every
            // program to honour, so it wins over `auto`.
            ColorChoice::Auto => is_terminal && !no_color_env,
        }
    }
}

/// Checks that `group` names a warning the compiler can report.
///
/// The set is small and closed, so an unknown name is refused with the list
/// rather than accepted and silently ignored -- a flag that does nothing is
/// worse than one that is rejected.
fn require_warning_group(group: &str) -> Result<(), String> {
    if fallc_diag::is_warning_group(group) {
        return Ok(());
    }
    let groups: Vec<&str> = fallc_diag::WARNING_GROUPS
        .iter()
        .map(|(name, _)| *name)
        .collect();
    Err(format!(
        "unknown warning `{group}`; expected one of {}, or `all`",
        groups.join(", ")
    ))
}

/// Parses the value of `-O`.
///
/// Kept next to the option rather than in the IR crate so that the message can
/// name the flag the user typed.
fn parse_opt_level(value: &str) -> Result<fallc_ir::OptLevel, String> {
    fallc_ir::OptLevel::parse(value).ok_or_else(|| {
        format!("`-O{value}` is not an optimisation level; expected 0, 1, 2, 3, s, or z")
    })
}

/// Parsed command-line options.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Options {
    /// What to do.
    pub command: Command,
}

impl Options {
    /// Parses `args`, excluding the program name.
    ///
    /// # Errors
    ///
    /// Returns a message describing the first malformed argument.
    pub fn parse(args: &[String]) -> Result<Options, String> {
        let mut inputs = Vec::new();
        let mut emit = Vec::new();
        let mut preserve_trivia = true;
        let mut check_only = false;
        let mut release = false;
        let mut opt = fallc_ir::OptLevel::None;
        let mut keep_temporaries = false;
        let mut output = None;
        let mut warnings = fallc_diag::WarningPolicy::new();
        let mut color = ColorChoice::Auto;
        // Peekable because `-O` may or may not be followed by its level.
        let mut args = args.iter().peekable();

        // A `while let` rather than a `for`, because `-o` needs to consume the
        // following argument.
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--help" | "-h" => {
                    return Ok(Options {
                        command: Command::Help,
                    });
                }
                "--version" | "-V" => {
                    return Ok(Options {
                        command: Command::Version,
                    });
                }
                // The language server: no file arguments, stdin and stdout.
                "--lsp" => {
                    return Ok(Options {
                        command: Command::LanguageServer,
                    });
                }
                "--check" => check_only = true,
                "--release" => release = true,
                "--keep-temporaries" => keep_temporaries = true,
                // `-O`, `-O2`, `-Os`, and the long spelling all name a level.
                // A bare `-O` is `-O2`, exactly as it is for a C compiler.
                "-O" => {
                    // `-O` alone means `-O2`; `-O 2` is accepted too, since a
                    // separate argument is how many build systems spell it.
                    let value = match args.peek() {
                        Some(next) if !next.starts_with('-') => {
                            let value = (*next).clone();
                            let _ = args.next();
                            value
                        }
                        _ => String::new(),
                    };
                    opt = parse_opt_level(&value)?;
                }
                other if other.starts_with("-O") => {
                    opt = parse_opt_level(&other["-O".len()..])?;
                }
                "-o" | "--output" => {
                    let value = args.next().ok_or_else(|| format!("`{arg}` needs a path"))?;
                    output = Some(PathBuf::from(value));
                }
                "--no-trivia" => preserve_trivia = false,
                "--color" => {
                    let value = args.next().ok_or_else(|| "`--color` needs a value".to_owned())?;
                    color = ColorChoice::parse(value).ok_or_else(|| {
                        "`--color` takes `auto`, `always`, or `never`".to_owned()
                    })?;
                }
                other if other.starts_with("--color=") => {
                    let value = &other["--color=".len()..];
                    color = ColorChoice::parse(value).ok_or_else(|| {
                        "`--color` takes `auto`, `always`, or `never`".to_owned()
                    })?;
                }
                other if other.starts_with("--emit=") => {
                    let value = &other["--emit=".len()..];
                    // `all` is the whole list, and asking for everything is
                    // asking to keep it: the intermediate files are most of
                    // what "everything" means.
                    if value == "all" {
                        emit = EmitKind::all().to_vec();
                        keep_temporaries = true;
                        continue;
                    }
                    // The valid kinds are named, because the set is small and
                    // a reader who mistyped one wants to see it rather than go
                    // back to `--help`.
                    let kind = EmitKind::parse(value).ok_or_else(|| {
                        let kinds: Vec<&str> =
                            EmitKind::all().iter().map(|kind| kind.name()).collect();
                        format!(
                            "unknown emit kind `{value}`; expected one of {}",
                            kinds.join(", ")
                        )
                    })?;
                    if !emit.contains(&kind) {
                        emit.push(kind);
                    }
                }
                // Warning control: `-W<group>`, `-Wno-<group>`, `-Wall`, and
                // `-Werror`, spelled as every C compiler spells them.
                "-Wall" => warnings.enable_all(),
                // `-Wverbose` is not a warning group but an answer to "where do
                // I read why": the explanation and the fix are behind it,
                // because a diagnostic line is read far more often than a rule
                // is looked up.
                "-Wverbose" => warnings.set_verbose(true),
                "-Werror" => warnings.deny(),
                other if other.starts_with("-Wno-") => {
                    let group = &other["-Wno-".len()..];
                    require_warning_group(group)?;
                    warnings.disable(group);
                }
                other if other.starts_with("-W") => {
                    let group = &other["-W".len()..];
                    require_warning_group(group)?;
                    warnings.enable(group);
                }
                other if other.starts_with('-') && other != "-" => {
                    return Err(format!("unrecognised option `{other}`"));
                }
                other => inputs.push(PathBuf::from(other)),
            }
        }

        if inputs.is_empty() {
            return Err("no input files".to_owned());
        }

        Ok(Options {
            command: Command::Compile {
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
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn single_input_is_required() {
        assert!(Options::parse(&args(&[])).is_err());
    }

    #[test]
    fn input_file_is_collected() {
        let options = Options::parse(&args(&["a.ft"])).expect("valid");
        match options.command {
            Command::Compile { inputs, .. } => assert_eq!(inputs, vec![PathBuf::from("a.ft")]),
            Command::Version | Command::Help => panic!("expected a compile command"),
        }
    }

    #[test]
    fn emit_kinds_accumulate_without_duplicates() {
        let options = Options::parse(&args(&["a.ft", "--emit=ast", "--emit=ast", "--emit=ir"]))
            .expect("valid");
        match options.command {
            Command::Compile { emit, .. } => {
                assert_eq!(emit, vec![EmitKind::Ast, EmitKind::Llvm]);
            }
            Command::Version | Command::Help => panic!("expected a compile command"),
        }
    }

    #[test]
    fn ir_is_an_alias_for_llvm() {
        let options = Options::parse(&args(&["a.ft", "--emit=ir", "--emit=llvm"])).expect("valid");
        match options.command {
            Command::Compile { emit, .. } => {
                // Both spellings name the same output, so they must not
                // produce two copies of it.
                assert_eq!(emit, vec![EmitKind::Llvm]);
            }
            Command::Version | Command::Help => panic!("expected a compile command"),
        }
    }

    #[test]
    fn unknown_emit_kind_is_rejected() {
        assert!(Options::parse(&args(&["a.ft", "--emit=wat"])).is_err());
    }

    #[test]
    fn warning_flags_are_collected() {
        let options = Options::parse(&args(&[
            "a.ft",
            "-Wno-unused",
            "-Werror",
            "-Wunreachable",
        ]))
        .expect("valid");
        match options.command {
            Command::Compile { warnings, .. } => {
                let mut expected = fallc_diag::WarningPolicy::new();
                expected.disable("unused");
                expected.enable("unreachable");
                expected.deny();
                assert_eq!(warnings, expected);
            }
            Command::Version | Command::Help => panic!("expected a compile command"),
        }
    }

    #[test]
    fn wall_turns_every_group_on_again() {
        let options =
            Options::parse(&args(&["a.ft", "-Wno-unused", "-Wno-unreachable", "-Wall"]))
                .expect("valid");
        match options.command {
            Command::Compile { warnings, .. } => {
                assert_eq!(warnings, fallc_diag::WarningPolicy::new());
            }
            Command::Version | Command::Help => panic!("expected a compile command"),
        }
    }

    #[test]
    fn an_unknown_warning_group_is_rejected() {
        let error = Options::parse(&args(&["a.ft", "-Wno-nonsense"]))
            .expect_err("the group does not exist");
        assert!(error.contains("unused"), "{error}");
    }

    #[test]
    fn unknown_option_is_rejected() {
        assert!(Options::parse(&args(&["a.ft", "--nope"])).is_err());
    }

    #[test]
    fn version_flag_short_circuits() {
        let options = Options::parse(&args(&["--version"])).expect("valid");
        assert_eq!(options.command, Command::Version);
    }

    #[test]
    fn flags_after_the_file_still_parse() {
        let options = Options::parse(&args(&["a.ft", "--no-trivia", "--check"])).expect("valid");
        match options.command {
            Command::Compile {
                preserve_trivia,
                check_only,
                ..
            } => {
                assert!(!preserve_trivia);
                assert!(check_only);
            }
            Command::Version | Command::Help => panic!("expected a compile command"),
        }
    }
}
