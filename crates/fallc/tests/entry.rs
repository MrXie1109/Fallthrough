//! The program's entry: what `main` may be, and what the C runtime sees.
//!
//! The C entry is `int main(int argc, char **argv)`. A `main` that returns a
//! status and takes no parameters is that entry, and is emitted as itself. The
//! other shapes a program may write -- returning nothing, taking the argument
//! vector, raising an exception -- are ordinary functions a generated wrapper
//! adapts, so that every shape has one meaning and one exit status.
//!
//! A shape the runtime cannot call is reported rather than linked: a wrong
//! `main` used to become "undefined reference to `main`" or, for a `void` one,
//! whatever the runtime found in the status register.

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
        let directory =
            std::env::temp_dir().join(format!("fallc-entry-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a temporary directory can be created");
        Scratch { directory }
    }

    /// Compiles the source into an executable, runs it with `arguments`, and
    /// returns its status and what it printed.
    fn run(&self, label: &str, source: &str, arguments: &[&str]) -> (i32, String) {
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
            .args(arguments)
            .output()
            .expect("the program can be run");
        (
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stdout).into_owned(),
        )
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

/// Compiles one file, returning the outcome and what the driver printed.
fn compile(source: &str) -> (Outcome, String) {
    compile_text("main.ft", source, &[EmitKind::Llvm])
}

#[test]
fn the_status_a_main_returns_is_the_exit_status() {
    let source = "fn main() int {\n    return 3;\n}\n";
    let scratch = Scratch::new("status");
    let (status, printed) = scratch.run("status", source, &[]);
    assert_eq!(status, 3, "{printed}");
}

#[test]
fn a_main_that_returns_nothing_exits_zero() {
    // A `void` entry was emitted *as* the C entry, so the runtime read a status
    // the function never wrote -- 16, on this machine, from whatever happened to
    // be in the register.
    let source = "fn main() void {\n    return;\n}\n";
    let scratch = Scratch::new("void");
    let (status, printed) = scratch.run("void", source, &[]);
    assert_eq!(status, 0, "{printed}");
}

#[test]
fn an_omitted_return_type_deduces_nothing_for_main() {
    // `fn main() { }` leaves the type to the body, and a body with no `return`
    // deduces "no value" -- which is a `main` that exits 0, not an error.
    let source = "fn main() {\n    return;\n}\n";
    let scratch = Scratch::new("deduced");
    let (status, printed) = scratch.run("deduced", source, &[]);
    assert_eq!(status, 0, "{printed}");
}

#[test]
fn a_main_may_take_the_argument_vector() {
    // `argv` is what the C runtime hands the program, unchanged: `argv[0]` is
    // the program's own path and the rest are its arguments.
    let source = r#"import std;

fn main(argc int, argv **c8) int {
    for (i usize = 1; i < argc as usize; i++) {
        C.printf("[%s]", argv[i]);
    }
    C.printf("\n");
    return argc;
}
"#;
    let scratch = Scratch::new("argv");
    let (status, printed) = scratch.run("argv", source, &["one", "two"]);
    assert_eq!(printed.trim(), "[one][two]");
    assert_eq!(status, 3, "the status is the argument count");
}

#[test]
fn a_main_that_raises_reports_the_exception_and_exits_seventy() {
    let source = r#"import std;

fn main() int except {
    raise "Boom": "nothing worked";
    return 0;
}
"#;
    let scratch = Scratch::new("raising");
    let (status, printed) = scratch.run("raising", source, &[]);
    assert_eq!(status, 70, "{printed}");
}

#[test]
fn a_main_that_returns_nothing_may_still_raise() {
    // The result of a `main` that may raise is `{ ok, error }` when it returns
    // nothing and `{ ok, value, error }` when it does not. Reading the value
    // field of the two-field shape produced IR that does not verify.
    let source = r#"import std;

fn main() void except {
    C.printf("ran\n");
    return;
}
"#;
    let scratch = Scratch::new("void-raising");
    let (status, printed) = scratch.run("void-raising", source, &[]);
    assert_eq!(status, 0, "{printed}");
    assert_eq!(printed.trim(), "ran");
}

#[test]
fn a_main_of_the_wrong_shape_is_reported() {
    // Every shape below cannot be the C entry, and none of them is adapted by
    // the wrapper either, so the mistake is the program's.
    for source in [
        "fn main() u8 {\n    return 0;\n}\n",
        "fn main(a int) int {\n    return a;\n}\n",
        "fn main(count int, name int) int {\n    return count;\n}\n",
        "fn main(text String) int {\n    return 0;\n}\n",
    ] {
        let (outcome, printed) = compile(source);
        assert_eq!(outcome, Outcome::Failure, "{source}\n{printed}");
        assert!(printed.contains("S0181"), "{source}\n{printed}");
    }
}
