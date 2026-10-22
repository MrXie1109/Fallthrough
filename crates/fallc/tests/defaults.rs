//! Defaults: template parameters and function parameters.
//!
//! A template parameter may write a default (`template <T type = int, n int = 4>`),
//! and a function parameter may write one too (`b int = a + 1`). Two rules run
//! through both: a default comes last -- a caller leaves arguments out from the
//! right -- and a default may use the parameters written before it, which is why
//! `n int = sizeof(T)` needs no second spelling.

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
            std::env::temp_dir().join(format!("fallc-defaults-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a temporary directory can be created");
        Scratch { directory }
    }

    /// Writes a source file beside `main.ft`, and returns its path.
    fn write_file(&self, name: &str, source: &str) -> PathBuf {
        let path = self.directory.join(name);
        std::fs::write(&path, source).expect("the source can be written");
        path
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
fn a_template_type_parameter_may_have_a_default() {
    let source = r#"import std;

@constexpr

template <T type = int>
fn twice(v T) int {
    return (v + v) as int;
}

fn main() int {
    C.printf("%d %d\n", twice(3), twice<u8>(3u8));
    return 0;
}
"#;
    let scratch = Scratch::new("type");
    assert_eq!(scratch.run("type", source), "6 6\n");
}

#[test]
fn a_template_value_parameter_may_have_a_default() {
    let source = r#"import std;

@constexpr

template <n int = 4>
fn number() int {
    return n;
}

fn main() int {
    C.printf("%d %d\n", number(), number<9>());
    return 0;
}
"#;
    let scratch = Scratch::new("value");
    assert_eq!(scratch.run("value", source), "4 9\n");
}

#[test]
fn a_default_may_use_the_parameters_before_it() {
    // `n` is `sizeof(T)`, and `m` is `n * 3`: each default is resolved against
    // the arguments gathered so far.
    let source = r#"import std;

@constexpr

template <T type, n int = sizeof(T), m int = n * 3>
fn widths() int {
    return (n + m) as int;
}

fn main() int {
    C.printf("%d %d %d\n", widths<u8>(), widths<u32>(), widths<u32, 2>());
    return 0;
}
"#;
    let scratch = Scratch::new("earlier");
    // u8: n=1, m=3 -> 4; u32: n=4, m=12 -> 16; u32 with n=2: 2 + 6 = 8.
    assert_eq!(scratch.run("earlier", source), "4 16 8\n");
}

#[test]
fn an_argument_that_differs_only_in_a_value_still_makes_its_own_function() {
    // The mangled symbol has to include the value argument, or two
    // instantiations of one template share a body and the last one wins.
    let source = r#"import std;

@constexpr

template <T type = int, n int = 4>
fn scaled(v T) int {
    return (v * n) as int;
}

fn main() int {
    C.printf("%d %d %d\n", scaled(2), scaled<int, 3>(2), scaled<int, 5>(2));
    return 0;
}
"#;
    let scratch = Scratch::new("mangle");
    assert_eq!(scratch.run("mangle", source), "8 6 10\n");
}

#[test]
fn a_type_family_may_have_defaults() {
    let source = r#"import std;

template <T type = int>
type Crate {
    value T;
}

fn main() int {
    a Crate = .{7};
    b Crate<u8> = .{5u8};
    C.printf("%d %d\n", a.value, b.value as int);
    return 0;
}
"#;
    let scratch = Scratch::new("family");
    assert_eq!(scratch.run("family", source), "7 5\n");
}

#[test]
fn a_parameter_without_a_default_may_not_follow_one_that_has_it() {
    let source = "template <T type = int, U type>\nfn f(a T, b U) int {\n    return 0;\n}\n\nfn main() int {\n    return 0;\n}\n";
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("must also have a default"),
        "{diagnostics}"
    );
}

#[test]
fn a_pack_cannot_have_a_default() {
    let source = "template <Args ...type = int>\nfn f() int {\n    return 0;\n}\n\nfn main() int {\n    return 0;\n}\n";
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("pack cannot have a default"),
        "{diagnostics}"
    );
}

#[test]
fn a_function_parameter_default_may_use_the_ones_before_it() {
    let source = r#"import std;

fn add(a int, b int = a + 1, c int = a + b) int {
    return a + b + c;
}

fn main() int {
    C.printf("%d %d %d\n", add(1), add(1, 10), add(1, 10, 100));
    return 0;
}
"#;
    let scratch = Scratch::new("function");
    assert_eq!(scratch.run("function", source), "6 22 111\n");
}

#[test]
fn a_default_is_evaluated_only_when_the_argument_is_left_out() {
    // The default calls a function, so whether it ran is observable.
    let source = r#"import std;

calls int = 0;

fn bump() int {
    calls += 1;
    return 10;
}

fn f(a int, b int = bump()) int {
    return a + b;
}

fn main() int {
    C.printf("%d ", f(1, 5));
    C.printf("%d %d\n", calls, f(1));
    return 0;
}
"#;
    let scratch = Scratch::new("lazy");
    // The first call supplies `b`, so `bump` has not run; the second uses it.
    assert_eq!(scratch.run("lazy", source), "6 0 11\n");
}

#[test]
fn a_default_has_to_have_the_parameter_type() {
    let source = "fn f(a int, b int = \"text\") int {\n    return a;\n}\n\nfn main() int {\n    return f(1);\n}\n";
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(diagnostics.contains("type mismatch"), "{diagnostics}");
}

#[test]
fn a_template_function_default_works_with_deduction() {
    let source = r#"import std;

@constexpr

template <T type>
fn scaled(a T, b T = a + 1) int {
    return (a + b) as int;
}

fn main() int {
    C.printf("%d %d\n", scaled(1), scaled<int>(1, 10));
    return 0;
}
"#;
    let scratch = Scratch::new("template-function");
    assert_eq!(scratch.run("template-function", source), "3 11\n");
}


#[test]
fn a_default_crosses_a_module_as_a_value_or_as_an_expression() {
    // A default is an expression of the module that wrote it, so there is no
    // tree to evaluate it in at the call site. What can cross is its *value*:
    // the module folds it where it is written and publishes the number, so
    // `greet(1)` reaches `name = 1`. A default the compiler could not work out
    // stays behind, and the call has to give it.
    let scratch = Scratch::new("default-across");
    scratch.write_file(
        "greet.ftm",
        "export fn greet(times int, name int = 1) int {\n    return times + name;\n}\n",
    );
    let qualified = scratch.write_file(
        "qualified.ft",
        "import C;\nimport greet;\n\nfn main() int {\n    C.printf(\"%d %d\\n\", greet.greet(1), greet.greet(2, 5));\n    return 0;\n}\n",
    );
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_fallc"))
        .arg(&qualified)
        .arg("-o")
        .arg(scratch.directory.join("qualified"))
        .output()
        .expect("the compiler can be run");
    assert!(
        output.status.success(),
        "a constant default must cross a module boundary:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let run = std::process::Command::new(scratch.directory.join("qualified"))
        .output()
        .expect("the program can be run");
    assert_eq!(String::from_utf8_lossy(&run.stdout).into_owned(), "2 7\n");

    // A name an import bound takes the same route.
    let bound = scratch.write_file(
        "bound.ft",
        "import C;\nfrom greet import greet;\n\nfn main() int {\n    C.printf(\"%d\\n\", greet(4));\n    return 0;\n}\n",
    );
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_fallc"))
        .arg(&bound)
        .arg("-o")
        .arg(scratch.directory.join("bound"))
        .output()
        .expect("the compiler can be run");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let run = std::process::Command::new(scratch.directory.join("bound"))
        .output()
        .expect("the program can be run");
    assert_eq!(String::from_utf8_lossy(&run.stdout).into_owned(), "5\n");

    // A default that is an *expression* is lowered where it is written -- in
    // the declaring module's tree, with the arguments the call did give -- so
    // `step int = times + 1` is answered for the caller too.
    scratch.write_file(
        "made.ftm",
        "export fn twice(times int, step int = times + 1) int {\n    return times * step;\n}\n\nexport fn label(value int) int {\n    return value;\n}\n",
    );
    let unfolded = scratch.write_file(
        "unfolded.ft",
        "import C;\nimport made;\n\nfn main() int {\n    C.printf(\"%d %d %d\\n\", made.twice(2), made.twice(3, 10), made.twice(made.label(4)));\n    return 0;\n}\n",
    );
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_fallc"))
        .arg(&unfolded)
        .arg("-o")
        .arg(scratch.directory.join("unfolded"))
        .output()
        .expect("the compiler can be run");
    assert!(
        output.status.success(),
        "a default written as an expression must travel:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let run = std::process::Command::new(scratch.directory.join("unfolded"))
        .output()
        .expect("the program can be run");
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).into_owned(),
        "6 30 20\n"
    );

    // A method's default crosses the same way: the expression is written
    // against the method's own parameters, and the receiver is one of them.
    scratch.write_file(
        "shapes.ftm",
        "export type Shape {\n    side int;\n}\n\nexport fn area(this &Shape, base int, factor int = base * 2) int {\n    return this.side * factor;\n}\n",
    );
    let method = scratch.write_file(
        "method.ft",
        "import C;\nimport shapes;\n\nfn main() int {\n    s Shape = .{3};\n    C.printf(\"%d %d\\n\", s.area(2), s.area(2, 5));\n    return 0;\n}\n",
    );
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_fallc"))
        .arg(&method)
        .arg("-o")
        .arg(scratch.directory.join("method"))
        .output()
        .expect("the compiler can be run");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let run = std::process::Command::new(scratch.directory.join("method"))
        .output()
        .expect("the program can be run");
    assert_eq!(String::from_utf8_lossy(&run.stdout).into_owned(), "12 15\n");
}

#[test]
fn a_folded_call_fills_the_parameters_it_left_out() {
    // `clamp(99)` folds to a constant, and the fold has to know that `high` is
    // `low + 10`. It did not: the omitted parameter was simply unbound, the
    // `if` that mentioned it took the other branch, and the call folded to 99 --
    // a *wrong value*, which is the worst thing a fold can be. The answer is
    // 10, and the same answer comes out when the call cannot be folded.
    let folded = r#"import C;

fn clamp(value int, low int = 0, high int = low + 10) int {
    if (value < low) {
        return low;
    }
    if (value > high) {
        return high;
    }
    return value;
}

fn main() int {
    C.printf("%d %d %d\n", clamp(99), clamp(99, 2), clamp(3, 2));
    return 0;
}
"#;
    let scratch = Scratch::new("fold-defaults");
    assert_eq!(scratch.run("fold_defaults", folded), "10 12 3\n");

    // The same call with a value the compiler does not know, so nothing folds
    // and the answer comes from the code.
    let unfolded = r#"import C;

n int = 99;

fn clamp(value int, low int = 0, high int = low + 10) int {
    if (value < low) {
        return low;
    }
    if (value > high) {
        return high;
    }
    return value;
}

fn main() int {
    C.printf("%d\n", clamp(n));
    return 0;
}
"#;
    let scratch = Scratch::new("unfolded-defaults");
    assert_eq!(scratch.run("unfolded_defaults", unfolded), "10\n");
}
