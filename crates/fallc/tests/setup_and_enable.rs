//! `__setup__ { .. }` and `@enable_if(cond)`.
//!
//! A module's `__setup__` block runs before `main`, after the setups of every
//! module it imports -- so a module may use what it imports, and the program's
//! own setup runs last. `@enable_if(cond)` decides whether a declaration
//! exists: a false condition removes it, so a call finds whatever else is there
//! or says that nothing matches. On a template the condition is checked per
//! instantiation, with the arguments substituted.

use std::path::PathBuf;

use fallc::Outcome;
use fallc::cli::{EmitKind, Options};
use fallc::pipeline::{compile_text, run};

/// A directory of scratch sources, removed when it goes out of scope.
struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory = std::env::temp_dir().join(format!("fallc-setup-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a temporary directory can be created");
        Scratch { directory }
    }

    fn write(&self, name: &str, source: &str) -> PathBuf {
        let path = self.directory.join(name);
        std::fs::write(&path, source).expect("the source can be written");
        path
    }

    fn build(&self, sources: &[PathBuf], output: &str) -> (bool, String) {
        let executable = self.directory.join(output);
        let mut args: Vec<String> = sources.iter().map(|p| p.display().to_string()).collect();
        args.push("-o".to_owned());
        args.push(executable.display().to_string());
        let options = Options::parse(&args).expect("the arguments are well formed");
        if run(&options) != std::process::ExitCode::SUCCESS {
            return (false, String::new());
        }
        let run_output = std::process::Command::new(&executable)
            .output()
            .expect("the program can be run");
        (true, String::from_utf8_lossy(&run_output.stdout).into_owned())
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn a_setup_block_runs_before_main() {
    let source = r#"import std;

value int = 0;

__setup__ {
    value = 41;
    C.printf("setup ");
}

fn main() int {
    C.printf("%d\n", value + 1);
    return 0;
}
"#;
    let scratch = Scratch::new("before");
    let main = scratch.write("main.ft", source);
    let (built, output) = scratch.build(&[main], "app");
    assert!(built, "the program did not compile");
    assert_eq!(output, "setup 42\n");
}

#[test]
fn a_setup_may_return_early() {
    let source = r#"import std;

__setup__ {
    C.printf("a ");
    if (true) {
        return;
    }
    C.printf("b ");
}

fn main() int {
    C.printf("main\n");
    return 0;
}
"#;
    let scratch = Scratch::new("early");
    let main = scratch.write("main.ft", source);
    let (built, output) = scratch.build(&[main], "app");
    assert!(built, "the program did not compile");
    assert_eq!(output, "a main\n");
}

#[test]
fn setup_blocks_run_in_dependency_order_with_the_main_module_last() {
    let scratch = Scratch::new("order");
    scratch.write(
        "inner.ftm",
        "import C;\n\n__setup__ {\n    C.printf(\"inner \");\n}\n",
    );
    scratch.write(
        "outer.ftm",
        "import C;\nimport inner;\n\n__setup__ {\n    C.printf(\"outer \");\n}\n",
    );
    let main = scratch.write(
        "main.ft",
        "import C;\nimport outer;\n\n__setup__ {\n    C.printf(\"unit \");\n}\n\nfn main() int {\n    C.printf(\"run\\n\");\n    return 0;\n}\n",
    );
    let (built, output) = scratch.build(&[main], "app");
    assert!(built, "the program did not compile");
    assert_eq!(output, "inner outer unit run\n");
}

#[test]
fn a_module_may_have_only_one_setup() {
    let source = "__setup__ {\n}\n\n__setup__ {\n}\n\nfn main() int {\n    return 0;\n}\n";
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("only one `__setup__` block"),
        "{diagnostics}"
    );
}

#[test]
fn a_setup_block_cannot_return_a_value() {
    let source = "__setup__ {\n    return 5;\n}\n\nfn main() int {\n    return 0;\n}\n";
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(diagnostics.contains("expected void"), "{diagnostics}");
}

#[test]
fn an_enable_if_declaration_that_is_false_does_not_exist() {
    let source = r#"@enable_if(sizeof(int) == 8)
fn eight() int {
    return 8;
}

fn main() int {
    return eight();
}
"#;
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("`eight` is disabled by `@enable_if`"),
        "{diagnostics}"
    );
}

#[test]
fn an_enable_if_declaration_that_is_true_is_usable() {
    let source = r#"import std;

@enable_if(sizeof(int) == 4)
fn four() int {
    return 4;
}

fn main() int {
    C.printf("%d\n", four());
    return 0;
}
"#;
    let scratch = Scratch::new("true");
    let main = scratch.write("main.ft", source);
    let (built, output) = scratch.build(&[main], "app");
    assert!(built, "the program did not compile");
    assert_eq!(output, "4\n");
}

#[test]
fn a_template_is_enabled_for_each_set_of_arguments() {
    let disabled = r#"@enable_if(sizeof(T) == 4)
template <T type>
fn wide() int {
    return sizeof(T) as int;
}

fn main() int {
    return wide<double>();
}
"#;
    let (outcome, diagnostics) = compile_text("t.ft", disabled, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("`wide` is disabled for these arguments"),
        "{diagnostics}"
    );

    let enabled = r#"import std;

@enable_if(sizeof(T) == 4)
template <T type>
fn wide() int {
    return sizeof(T) as int;
}

fn main() int {
    C.printf("%d\n", wide<int>());
    return 0;
}
"#;
    let scratch = Scratch::new("template");
    let main = scratch.write("main.ft", enabled);
    let (built, output) = scratch.build(&[main], "app");
    assert!(built, "the program did not compile");
    assert_eq!(output, "4\n");
}

#[test]
fn a_family_method_is_enabled_for_each_member() {
    let disabled = r#"template <T type>
type Crate {
    v T;
}

@enable_if(sizeof(T) == 4)
fn wide(this &const Crate<T>) int {
    return 4;
}

fn main() int {
    b Crate<double> = .{2.5};
    return b.wide();
}
"#;
    let (outcome, diagnostics) = compile_text("t.ft", disabled, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("disabled for this member"),
        "{diagnostics}"
    );

    let enabled = r#"import std;

template <T type>
type Crate {
    v T;
}

@enable_if(sizeof(T) == 4)
fn wide(this &const Crate<T>) int {
    return 4;
}

fn main() int {
    a Crate<int> = .{1};
    C.printf("%d\n", a.wide());
    return 0;
}
"#;
    let scratch = Scratch::new("family");
    let main = scratch.write("main.ft", enabled);
    let (built, output) = scratch.build(&[main], "app");
    assert!(built, "the program did not compile");
    assert_eq!(output, "4\n");
}

#[test]
fn an_enable_if_condition_must_be_a_constant() {
    let source = r#"value int = 1;

@enable_if(value == 1)
fn f() int {
    return 1;
}

fn main() int {
    return f();
}
"#;
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("must be a constant"),
        "{diagnostics}"
    );
}
