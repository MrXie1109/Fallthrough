//! The declaration modifiers written after the type: `static` and `@align(..)`.
//!
//! They describe the *storage* a declaration asks for, which is why they come
//! after the type while the qualifiers that describe the value come before it.
//! `static` gives the binding one object for the whole run; `alignas` raises
//! the alignment of that object, and only ever raises it.

use std::path::PathBuf;

use fallc::Outcome;
use fallc::cli::{EmitKind, Options};
use fallc::pipeline::{compile_text, run};

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-modifier-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a temporary directory can be created");
        Scratch { directory }
    }

    fn run(&self, label: &str, source: &str) -> String {
        let main = self.directory.join("main.ft");
        std::fs::write(&main, source).expect("the source can be written");
        let executable = self.directory.join(label);
        let options = Options::parse(&[
            main.display().to_string(),
            "-o".to_owned(),
            executable.display().to_string(),
        ])
        .expect("the arguments are well formed");
        assert_eq!(
            run(&options),
            std::process::ExitCode::SUCCESS,
            "`{label}` did not compile"
        );
        let output = std::process::Command::new(&executable)
            .output()
            .expect("the program can be run");
        String::from_utf8_lossy(&output.stdout).into_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.directory.as_path());
    }
}

#[test]
fn a_static_binding_keeps_its_value_between_calls() {
    let source = r#"import std;

fn counter() int {
    n int @static = 0;
    n += 1;
    return n;
}

fn main() int {
    counter();
    counter();
    C.printf("%d\n", counter());
    return 0;
}
"#;
    let scratch = Scratch::new("counter");
    assert_eq!(scratch.run("counter", source), "3\n");
}

#[test]
fn a_static_binding_without_an_initialiser_starts_at_zero() {
    let source = r#"import std;

fn bump() int {
    seen int @static;
    seen += 1;
    return seen;
}

fn main() int {
    C.printf("%d %d %d\n", bump(), bump(), bump());
    return 0;
}
"#;
    let scratch = Scratch::new("zero");
    assert_eq!(scratch.run("zero", source), "1 2 3\n");
}

#[test]
fn a_static_binding_may_not_be_initialised_at_run_time() {
    // A call the compiler can evaluate is a constant; one that reads state
    // the program has not created yet is not.
    let source = r#"import std;

count int = 0;

fn bump() int {
    count += 1;
    return count;
}

fn g() int {
    n int @static = bump();
    return n;
}

fn main() int {
    return g();
}
"#;
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("is not a constant expression"),
        "{diagnostics}"
    );
}

#[test]
fn a_module_level_variable_may_not_be_initialised_at_run_time() {
    let source = r#"import std;

count int = 0;

fn bump() int {
    count += 1;
    return count;
}

g int = bump();

fn main() int {
    return g;
}
"#;
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("is not a constant expression"),
        "{diagnostics}"
    );
}

#[test]
fn alignas_raises_the_alignment_of_a_binding() {
    // The address is what alignment is about, so the program prints it modulo
    // the alignment it asked for: zero means the request was honoured.
    let source = r#"import std;

fn main() int {
    aligned [4]byte @align(64);
    C.printf("%p\n", &aligned);
    return 0;
}
"#;
    let scratch = Scratch::new("raise");
    let printed = scratch.run("raise", source);
    let address = usize::from_str_radix(
        printed.trim().trim_start_matches("0x"),
        16,
    )
    .expect("`%p` prints a hexadecimal address");
    assert_eq!(address % 64, 0, "printed `{printed}`");
}

#[test]
fn alignas_of_a_type_is_alignas_of_its_alignment() {
    let source = r#"import std;

type Wide {
    a int;
    b int;
}

fn main() int {
    by_type byte @align(Wide);
    C.printf("%p\n", &by_type);
    return 0;
}
"#;
    let scratch = Scratch::new("type");
    let printed = scratch.run("type", source);
    let address = usize::from_str_radix(
        printed.trim().trim_start_matches("0x"),
        16,
    )
    .expect("`%p` prints a hexadecimal address");
    // `Wide` is two `int`s, so it is four-byte aligned: `@align(Wide)` is
    // `@align(alignof(Wide))`.
    assert_eq!(address % 4, 0, "printed `{printed}`");
}

#[test]
fn an_alignment_that_would_pack_a_value_tighter_is_refused() {
    let source = "fn main() int {\n    a int @align(2);\n    return 0;\n}\n";
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(diagnostics.contains("is weaker than"), "{diagnostics}");
}

#[test]
fn mutable_belongs_to_a_field_and_not_to_a_binding() {
    let source = "fn main() int {\n    a int mutable;\n    return 0;\n}\n";
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("property of a field"),
        "{diagnostics}"
    );
}
