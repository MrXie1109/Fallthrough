//! Keyword arguments: `.name = value` at a call and in an initialiser.
//!
//! A call writes its arguments as a positional prefix followed by any number
//! of named ones, so `.name = value` only ever appears after the arguments it
//! follows. A named argument goes to the parameter it names, whatever its
//! position in the list: `f(.c = 3, .a = 1, .b = 2)` and `f(1, 2, 3)` call the
//! same function with the same values. A parameter the call leaves out is
//! filled from its default, exactly as an omitted positional argument is.
//!
//! The same syntax fills fields in a `.{ .. }` initialiser, where a name says
//! which field a value is for rather than relying on declaration order.

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
            std::env::temp_dir().join(format!("fallc-named-{label}-{}", std::process::id()));
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

/// A record and a few functions, which most tests below share.
const PRELUDE: &str = r#"import std;

fn f(a int, b int, c int) int {
    return a * 100 + b * 10 + c;
}

fn g(a int, b int = 2, c int = 3) int {
    return a * 100 + b * 10 + c;
}

type Point {
    x int;
    y int;
}

fn sum(this &const Point) int {
    return this.x + this.y;
}

fn moved(this &const Point, dx int, dy int) Point {
    return .{ .y = this.y + dy, .x = this.x + dx };
}
"#;

#[test]
fn a_named_argument_goes_to_the_parameter_its_name_says() {
    let source = format!(
        "{PRELUDE}\nfn main() int {{\n    C.printf(\"%d %d\\n\", f(1, 2, 3), f(.c = 3, .b = 2, .a = 1));\n    return 0;\n}}\n"
    );
    let scratch = Scratch::new("order");
    assert_eq!(scratch.run("order", &source), "123 123\n");
}

#[test]
fn a_named_argument_follows_the_positional_prefix() {
    let source = format!(
        "{PRELUDE}\nfn main() int {{\n    C.printf(\"%d %d\\n\", f(1, .c = 3, .b = 2), g(1, .c = 30));\n    return 0;\n}}\n"
    );
    let scratch = Scratch::new("prefix");
    // `g(1, .c = 30)` leaves `b` to its default, so the middle of the list is a
    // hole filled from the declaration.
    assert_eq!(scratch.run("prefix", &source), "123 150\n");
}

#[test]
fn a_named_argument_may_fill_a_hole_a_default_would_have_filled() {
    let source = r#"import std;

fn join(a int, b int = a + 1, c int = a + b) int {
    return a * 1000 + b * 100 + c;
}

fn main() int {
    C.printf("%d %d %d\n", join(1), join(1, .c = 7), join(.c = 7, .a = 1, .b = 2));
    return 0;
}
"#;
    let scratch = Scratch::new("hole");
    // `join(1)`: b = 2, c = 3. `join(1, .c = 7)`: b = 2, c = 7. The third
    // names every parameter, so no default is evaluated at all.
    assert_eq!(scratch.run("hole", source), "1203 1207 1207\n");
}

#[test]
fn a_method_may_be_given_its_arguments_by_name() {
    let source = format!(
        "{PRELUDE}\nfn main() int {{\n    p Point = .{{1, 2}};\n    q Point = p.moved(.dy = 20, .dx = 10);\n    C.printf(\"%d %d %d\\n\", p.sum(), q.x, q.y);\n    return 0;\n}}\n"
    );
    let scratch = Scratch::new("method");
    assert_eq!(scratch.run("method", &source), "3 11 22\n");
}

#[test]
fn a_named_field_fills_the_field_its_name_says() {
    let source = format!(
        "{PRELUDE}\nfn main() int {{\n    a Point = .{{ .y = 2, .x = 1 }};\n    b Point = .{{ .x = 1, .y = 2 }};\n    C.printf(\"%d %d\\n\", a.sum(), b.sum());\n    return 0;\n}}\n"
    );
    let scratch = Scratch::new("fields");
    assert_eq!(scratch.run("fields", &source), "3 3\n");
}

#[test]
fn a_named_field_works_in_a_global_and_nested_in_another_initialiser() {
    let source = r#"import std;

type Point {
    x int;
    y int;
}

type Pair {
    a Point;
    b int;
}

origin Point = .{ .y = 2, .x = 1 };

fn main() int {
    pair Pair = .{ .b = 7, .a = .{ .y = 4, .x = 3 } };
    C.printf("%d %d %d\n", origin.x, origin.y, pair.a.x);
    C.printf("%d %d\n", pair.a.y, pair.b);
    return 0;
}
"#;
    let scratch = Scratch::new("nested");
    assert_eq!(scratch.run("nested", source), "1 2 3\n4 7\n");
}

#[test]
fn a_named_argument_reaches_a_function_of_another_module() {
    let source = r#"import std;

fn main() int {
    raw *byte = std.calloc(.size = 8, .count = 2);
    bytes *byte = raw;
    bytes[7] = 42;
    C.printf("%d\n", bytes[7] as int);
    std.free(raw);
    return 0;
}
"#;
    let scratch = Scratch::new("module");
    assert_eq!(scratch.run("module", source), "42\n");
}

#[test]
fn a_named_argument_is_deduced_by_a_template() {
    let source = r#"import std;

template <T type>
fn pair(a T, b T) int {
    return (a * 10 + b) as int;
}

fn main() int {
    C.printf("%d %d\n", pair(.b = 2, .a = 1), pair(3, .b = 4));
    return 0;
}
"#;
    let scratch = Scratch::new("template");
    assert_eq!(scratch.run("template", source), "12 34\n");
}

#[test]
fn a_constructor_may_be_given_its_arguments_by_name() {
    let source = r#"import std;

type Crate {
    a int;
    b int;
}

fn __init__(this &Crate, a int, b int) void {
    this.a = a * 10;
    this.b = b;
}

fn main() int {
    value Crate = Crate(.b = 2, .a = 1);
    C.printf("%d %d\n", value.a, value.b);
    return 0;
}
"#;
    let scratch = Scratch::new("constructor");
    assert_eq!(scratch.run("constructor", source), "10 2\n");
}

#[test]
fn a_named_field_works_with_the_type_written_out() {
    let source = r#"import std;

type Point {
    x int;
    y int;
}

fn main() int {
    p Point = Point{ .y = 20, .x = 10 };
    C.printf("%d %d\n", p.x, p.y);
    return 0;
}
"#;
    let scratch = Scratch::new("braces");
    assert_eq!(scratch.run("braces", source), "10 20\n");
}

#[test]
fn a_name_used_only_as_a_named_argument_is_a_use() {
    // The walker that decides which bindings are never read has to reach a
    // named argument's value, or a variable passed only that way looks unused.
    let source = r#"fn f(a int, b int) int {
    return a * 10 + b;
}

fn main() int {
    x int = 2;
    return f(1, .b = x);
}
"#;
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Success, "{diagnostics}");
    assert!(!diagnostics.contains("`x` is never read"), "{diagnostics}");
}

#[test]
fn an_unknown_name_is_reported_with_the_parameters() {
    let source = format!("{PRELUDE}\nfn main() int {{\n    return f(1, 2, .d = 3);\n}}\n");
    let (outcome, diagnostics) = compile_text("t.ft", &source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("has no parameter named `d`"),
        "{diagnostics}"
    );
}

#[test]
fn a_parameter_given_twice_is_reported() {
    let source = format!("{PRELUDE}\nfn main() int {{\n    return f(1, .b = 2, .b = 3);\n}}\n");
    let (outcome, diagnostics) = compile_text("t.ft", &source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(diagnostics.contains("is given `b` twice"), "{diagnostics}");
}

#[test]
fn a_parameter_written_by_position_and_by_name_is_reported() {
    let source = format!("{PRELUDE}\nfn main() int {{\n    return f(1, 2, .b = 3);\n}}\n");
    let (outcome, diagnostics) = compile_text("t.ft", &source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(diagnostics.contains("is given `b` twice"), "{diagnostics}");
}

#[test]
fn a_required_parameter_a_named_call_leaves_out_is_reported() {
    let source = format!("{PRELUDE}\nfn main() int {{\n    return f(.c = 3);\n}}\n");
    let (outcome, diagnostics) = compile_text("t.ft", &source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(diagnostics.contains("needs `a`"), "{diagnostics}");
}

#[test]
fn a_field_the_record_does_not_have_is_reported() {
    let source = format!(
        "{PRELUDE}\nfn main() int {{\n    p Point = .{{ .z = 1, .x = 2, .y = 3 }};\n    return p.sum();\n}}\n"
    );
    let (outcome, diagnostics) = compile_text("t.ft", &source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("has no field named `z`"),
        "{diagnostics}"
    );
}

#[test]
fn an_initialiser_may_not_leave_a_field_out() {
    let source = format!(
        "{PRELUDE}\nfn main() int {{\n    p Point = .{{ .x = 1 }};\n    return p.sum();\n}}\n"
    );
    let (outcome, diagnostics) = compile_text("t.ft", &source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(diagnostics.contains("needs field `y`"), "{diagnostics}");
}

#[test]
fn a_named_argument_keeps_its_type_checking() {
    let source = format!("{PRELUDE}\nfn main() int {{\n    return f(1, 2, .c = \"text\");\n}}\n");
    let (outcome, diagnostics) = compile_text("t.ft", &source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(diagnostics.contains("type mismatch"), "{diagnostics}");
}
