//! `type(auto)` — `decltype(auto)`: the expression's own type.
//!
//! `auto` deduces the *value* an initialiser or a `return` names, so a place
//! reads through it. `type(auto)` deduces the expression's own type, so a place
//! stays a reference: `r type(auto) = *p;` binds `r` to what `p` points at
//! rather than copying it. A `type(auto)` return keeps that reference, which is
//! how a function can hand back a place to write through.

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
            std::env::temp_dir().join(format!("fallc-decltype-{label}-{}", std::process::id()));
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
fn a_value_initialiser_gives_a_value() {
    let source = r#"import std;

fn main() int {
    v int = 7;
    r type(auto) = v;
    r = 9;
    C.printf("%d %d\n", v, r);
    return 0;
}
"#;
    let scratch = Scratch::new("value");
    assert_eq!(scratch.run("value", source), "7 9\n");
}

#[test]
fn a_place_initialiser_gives_a_reference() {
    // `type(auto)` keeps the place, so writing to `r` writes to `v`.
    let source = r#"import std;

fn main() int {
    v int = 3;
    r type(auto) = *&v;
    r = 4;
    C.printf("%d\n", v);
    return 0;
}
"#;
    let scratch = Scratch::new("place");
    assert_eq!(scratch.run("place", source), "4\n");
}

#[test]
fn a_type_auto_return_hands_back_the_place() {
    // The deduced return type is `&int`: the caller can write through it.
    let source = r#"import std;

fn first(p *int) type(auto) {
    return *p;
}

fn main() int {
    v int = 5;
    r type(auto) = first(&v);
    r = 6;
    C.printf("%d %d\n", v, r);
    return 0;
}
"#;
    let scratch = Scratch::new("return");
    assert_eq!(scratch.run("return", source), "6 6\n");
}

#[test]
fn an_auto_return_reads_through_a_place() {
    // `auto` is the value, not the place: the function returns an `int`, and a
    // caller writing to its result writes a copy. This used to deduce a
    // reference and emit a `ret` whose type did not match the function's.
    let source = r#"import std;

fn value_of(p *int) auto {
    return *p;
}

fn main() int {
    v int = 7;
    x auto = value_of(&v);
    x = 8;
    C.printf("%d %d\n", v, x);
    return 0;
}
"#;
    let scratch = Scratch::new("auto");
    assert_eq!(scratch.run("auto", source), "7 8\n");
}

#[test]
fn a_parameter_cannot_be_deduced() {
    // A parameter's type is part of the signature every caller matches, and
    // there is no initialiser or `return` to deduce from.
    for written in ["auto", "type(auto)"] {
        let source = format!(
            "fn f(n {written}) int {{\n    return 0;\n}}\n\nfn main() int {{\n    return 0;\n}}\n"
        );
        let (outcome, diagnostics) = compile_text("t.ft", source.as_str(), &[EmitKind::Llvm]);
        assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
        assert!(diagnostics.contains("cannot be deduced"), "{diagnostics}");
    }
}

#[test]
fn type_auto_needs_an_initialiser() {
    let source = "fn main() int {\n    x type(auto);\n    return 0;\n}\n";
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
}
