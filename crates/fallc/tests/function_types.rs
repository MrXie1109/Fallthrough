//! Function types: `fn(int) int`, and `*fn(int) int` for a pointer to one.
//!
//! A function type is a type, so a pointer to a function is written like any
//! other pointer. A function name used as a value has the function's type and
//! decays to a pointer to it, which is why `&f` and a bare `f` both work where
//! a `*fn(..) ..` is wanted. Calling through a pointer is written the way a
//! call is written: `p(1)`.

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
            std::env::temp_dir().join(format!("fallc-function-{label}-{}", std::process::id()));
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
fn a_function_name_decays_to_a_pointer_and_an_address_is_the_same_pointer() {
    let source = r#"import std;

fn inc(n int) int {
    return n + 1;
}

fn main() int {
    by_name *fn(int) int = inc;
    by_address *fn(int) int = &inc;
    C.printf("%d %d\n", by_name(1), by_address(2));
    return 0;
}
"#;
    let scratch = Scratch::new("decay");
    assert_eq!(scratch.run("decay", source), "2 3\n");
}

#[test]
fn a_function_pointer_is_called_the_way_a_function_is() {
    let source = r#"import std;

fn add(a int, b int) int {
    return a + b;
}

fn main() int {
    p *fn(int, int) int = &add;
    C.printf("%d\n", p(3, 4));
    return 0;
}
"#;
    let scratch = Scratch::new("call");
    assert_eq!(scratch.run("call", source), "7\n");
}

#[test]
fn the_target_type_chooses_the_overload() {
    let source = r#"import std;

fn f(x int) int {
    return x + 1;
}

fn f(x double) int {
    return (x * 2.0) as int;
}

fn main() int {
    whole *fn(int) int = &f;
    fractional *fn(double) int = &f;
    C.printf("%d %d\n", whole(1), fractional(2.0));
    return 0;
}
"#;
    let scratch = Scratch::new("overload");
    assert_eq!(scratch.run("overload", source), "2 4\n");
}

#[test]
fn a_function_pointer_travels_as_an_argument_and_as_a_result() {
    let source = r#"import std;

fn add(a int, b int) int {
    return a + b;
}

fn apply(f *fn(int, int) int, a int, b int) int {
    return f(a, b);
}

fn pick() *fn(int, int) int {
    return &add;
}

fn main() int {
    C.printf("%d %d\n", apply(&add, 3, 4), pick()(10, 20));
    return 0;
}
"#;
    let scratch = Scratch::new("travel");
    assert_eq!(scratch.run("travel", source), "7 30\n");
}

#[test]
fn a_binding_may_have_a_function_type_without_a_pointer() {
    // A function *type* and a pointer to one are different types, and a
    // binding of the function type holds the function itself.
    let source = r#"import std;

fn inc(n int) int {
    return n + 1;
}

fn main() int {
    f fn(int) int = inc;
    C.printf("%d\n", f(41));
    return 0;
}
"#;
    let scratch = Scratch::new("plain");
    assert_eq!(scratch.run("plain", source), "42\n");
}

#[test]
fn an_abi_may_be_written_on_a_function_type() {
    let source = r#"import std;

fn add(a int, b int) int {
    return a + b;
}

fn main() int {
    // The ABI is part of the type, so a C-ABI pointer and the language's own
    // are different types; both spellings parse and either may be null.
    p *fn "C" (int, int) int = nullptr;
    q *fn(int, int) int = &add;
    if (p) {
        return 1;
    }
    C.printf("%d\n", q(1, 2));
    return 0;
}
"#;
    let scratch = Scratch::new("abi");
    assert_eq!(scratch.run("abi", source), "3\n");
}

#[test]
fn a_function_pointer_is_truthy_when_it_is_not_null() {
    let source = r#"import std;

fn inc(n int) int {
    return n + 1;
}

fn main() int {
    p *fn(int) int = &inc;
    empty *fn(int) int = nullptr;
    if (p) {
        C.printf("set\n");
    }
    if (!empty) {
        C.printf("unset\n");
    }
    return 0;
}
"#;
    let scratch = Scratch::new("truth");
    assert_eq!(scratch.run("truth", source), "set\nunset\n");
}

#[test]
fn calling_through_a_pointer_checks_the_arguments_against_its_type() {
    let source = r#"fn inc(n int) int {
    return n + 1;
}

fn main() int {
    p *fn(int) int = &inc;
    return p(1.5);
}
"#;
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(diagnostics.contains("type mismatch"), "{diagnostics}");
}

#[test]
fn a_call_through_a_pointer_checks_how_many_arguments_there_are() {
    let source = r#"fn inc(n int) int {
    return n + 1;
}

fn main() int {
    p *fn(int) int = &inc;
    return p(1, 2);
}
"#;
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(diagnostics.contains("argument"), "{diagnostics}");
}

#[test]
fn the_function_type_is_what_type_of_reports() {
    let (_, tree) = compile_text(
        "t.ft",
        "fn inc(n int) int {\n    return n + 1;\n}\n\nfn main() int {\n    p *fn(int) int = &inc;\n    return 0;\n}\n",
        &[EmitKind::Ast],
    );
    assert!(tree.contains("Function"), "{tree}");
}

#[test]
fn a_function_that_may_raise_is_not_a_plain_function_pointer() {
    // Its result is shaped by the exception ABI, so a call through a pointer
    // declared `*fn(int) int` reads an `int` where a struct comes back. The
    // assignment was accepted and the call segfaulted with no diagnostic.
    let source = r#"fn risky(n int) int except {
    return n * 2;
}

fn main() int {
    p *fn(int) int = risky;
    return p(1);
}
"#;
    let (outcome, printed) =
        fallc::pipeline::compile_text("throwing-pointer.ft", source, &[fallc::cli::EmitKind::Llvm]);
    assert_eq!(outcome, fallc::Outcome::Failure, "{printed}");
    assert!(printed.contains("fn(i32) i32 except"), "{printed}");
}

#[test]
fn a_function_that_cannot_raise_is_a_plain_function_pointer() {
    let source = r#"fn twice(n int) int {
    return n * 2;
}

fn main() int {
    p *fn(int) int = twice;
    return p(1);
}
"#;
    let (outcome, printed) =
        fallc::pipeline::compile_text("plain-pointer.ft", source, &[fallc::cli::EmitKind::Llvm]);
    assert_eq!(outcome, fallc::Outcome::Success, "{printed}");
}

#[test]
fn a_pointer_to_a_function_that_may_raise_is_its_own_type() {
    // `except` is part of a function's type, so a pointer to one is not a
    // pointer to a plain function: the two return their results through
    // different ABIs. The throwing spelling is what makes such a function
    // reachable through a pointer at all.
    let source = r#"import std;
import C;

fn risky(n int) int except {
    if (n < 0) {
        raise "Negative": "no";
    }
    return n * 2;
}

fn apply(f *fn(int) int except, v int) int except {
    return f(v)??;
}

fn main() int {
    p *fn(int) int except = risky;
    try {
        C.printf("%d\n", apply(p, 3));
    } except {
        C.printf("unexpected\n");
    }
    try {
        C.printf("%d\n", apply(p, -4));
    } except {
        e &const std.Exception = std.exception();
        C.printf("caught %s\n", e.name().data());
    }
    return 0;
}
"#;
    let scratch = Scratch::new("throwing-pointer-call");
    assert_eq!(
        scratch.run("throwing_pointer", source),
        "6\ncaught Negative\n"
    );
}

#[test]
fn a_throwing_pointer_and_a_plain_one_do_not_mix() {
    let source = r#"fn twice(n int) int {
    return n * 2;
}

fn risky(n int) int except {
    return n * 3;
}

fn main() int {
    plain *fn(int) int except = twice;
    raising *fn(int) int = risky;
    return 0;
}
"#;
    let (outcome, printed) =
        fallc::pipeline::compile_text("mixed-pointers.ft", source, &[fallc::cli::EmitKind::Llvm]);
    assert_eq!(outcome, fallc::Outcome::Failure, "{printed}");
    assert!(printed.contains("*fn(i32) i32 except"), "{printed}");
    assert!(printed.contains("*fn(i32) i32,"), "{printed}");
}

#[test]
fn a_call_through_a_throwing_pointer_is_a_call_site() {
    let source = r#"fn risky(n int) int except {
    return n * 2;
}

fn main() int {
    p *fn(int) int except = risky;
    return p(1);
}
"#;
    let (outcome, printed) = fallc::pipeline::compile_text(
        "unhandled-pointer.ft",
        source,
        &[fallc::cli::EmitKind::Llvm],
    );
    assert_eq!(outcome, fallc::Outcome::Failure, "{printed}");
    assert!(printed.contains("S0083"), "{printed}");
}
