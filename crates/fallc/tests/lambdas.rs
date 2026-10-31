//! Lambdas: `lambda(args) -> expr` and `lambda(args) { .. }`.
//!
//! A lambda is an anonymous function written where a value is expected. It is
//! called with parentheses around it -- `(lambda(x int) -> x)(1)` -- because
//! without them the call would be part of the body. A parameter's type may be
//! left out, in which case each call decides it; a name from the enclosing
//! function is captured by reference; and a lambda that captures nothing is a
//! function, so it may be stored in a function pointer.

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
            std::env::temp_dir().join(format!("fallc-lambda-{label}-{}", std::process::id()));
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
fn a_lambda_is_called_with_parentheses_around_it() {
    let source = r#"import std;

fn main() int {
    C.printf("%d\n", (lambda(x int) -> x * 2)(21));
    return 0;
}
"#;
    let scratch = Scratch::new("call");
    assert_eq!(scratch.run("call", source), "42\n");
}

#[test]
fn an_untyped_parameter_is_decided_by_each_call() {
    let source = r#"import std;

fn main() int {
    C.printf("%d\n", (lambda(x) -> x * 2)(21));
    C.printf("%.1f\n", (lambda(x) -> x * 2.0)(21.5));
    return 0;
}
"#;
    let scratch = Scratch::new("untyped");
    assert_eq!(scratch.run("untyped", source), "42\n43.0\n");
}

#[test]
fn a_lambda_captures_a_name_by_reference() {
    // The second lambda writes the captured variable, and the write is visible
    // afterwards: that is what "by reference" means.
    let source = r#"import std;

fn main() int {
    y int = 10;
    C.printf("%d\n", (lambda(x int) -> x + y)(5));
    y = 20;
    C.printf("%d\n", (lambda(x int) { y = y + x; return y; })(5));
    C.printf("%d\n", y);
    return 0;
}
"#;
    let scratch = Scratch::new("capture");
    assert_eq!(scratch.run("capture", source), "15\n25\n25\n");
}

#[test]
fn a_block_body_returns_from_the_lambda() {
    // `return` inside the body returns from the lambda, not from `main`; the
    // statement after the call still runs.
    let source = r#"import std;

fn main() int {
    v int = (lambda(x int) {
        if (x > 10) {
            return 100;
        }
        return x * 2;
    })(21);
    C.printf("%d %d\n", v, (lambda(x int) { return 7; })(0));
    return 0;
}
"#;
    let scratch = Scratch::new("block");
    assert_eq!(scratch.run("block", source), "100 7\n");
}

#[test]
fn a_lambda_that_captures_nothing_decays_to_a_function_pointer() {
    let source = r#"import std;

fn make() *fn(int) int {
    return lambda(x int) -> x + 1;
}

fn main() int {
    f *fn(int) int = lambda(x int) -> x + 1;
    C.printf("%d %d\n", f(41), make()(41));
    return 0;
}
"#;
    let scratch = Scratch::new("pointer");
    assert_eq!(scratch.run("pointer", source), "42 42\n");
}

#[test]
fn a_lambda_takes_a_reference_parameter() {
    let source = r#"import std;

type Point {
    x int;
    y int;
}

fn main() int {
    p Point = .{3, 4};
    C.printf("%d\n", (lambda(q &const Point) -> q.x + q.y)(p));
    return 0;
}
"#;
    let scratch = Scratch::new("reference");
    assert_eq!(scratch.run("reference", source), "7\n");
}

#[test]
fn a_lambda_takes_named_arguments() {
    let source = r#"import std;

fn main() int {
    C.printf("%d\n", (lambda(a int, b int) -> a * 10 + b)(.b = 2, .a = 1));
    return 0;
}
"#;
    let scratch = Scratch::new("named");
    assert_eq!(scratch.run("named", source), "12\n");
}

#[test]
fn a_lambda_body_may_call_another_lambda() {
    let source = r#"import std;

fn main() int {
    C.printf("%d\n", (lambda(x int) -> (lambda(y int) -> x + y)(1))(10));
    return 0;
}
"#;
    let scratch = Scratch::new("nested");
    assert_eq!(scratch.run("nested", source), "11\n");
}

#[test]
fn a_lambda_inside_a_method_of_a_family_works_for_each_member() {
    // Every member of the family checks the method's body again, and the
    // lambda inside it is instantiated once per member: the `int` member must
    // not be lowered with the `double` member's types.
    let source = r#"import std;

template <T type>
type Crate {
    v T;
}

fn twice(this &const Crate<T>) T {
    return (lambda(x T) -> x + x)(this.v);
}

fn main() int {
    a Crate<int> = .{21};
    b Crate<double> = .{2.5};
    C.printf("%d %.1f\n", a.twice(), b.twice());
    return 0;
}
"#;
    let scratch = Scratch::new("family");
    assert_eq!(scratch.run("family", source), "42 5.0\n");
}

#[test]
fn a_lambda_may_capture_the_receiver_of_a_method() {
    let source = r#"import std;

type Point {
    x int;
    y int;
}

fn offset(this &const Point, d int) int {
    return (lambda(v int) -> v + this.x + this.y)(d);
}

fn main() int {
    p Point = .{3, 4};
    C.printf("%d\n", p.offset(10));
    return 0;
}
"#;
    let scratch = Scratch::new("receiver");
    assert_eq!(scratch.run("receiver", source), "17\n");
}

#[test]
fn a_lambda_body_may_raise_and_its_call_is_caught() {
    // A raising lambda is a throwing function: the body's exception leaves the
    // call, so the call is a throwing call like any other.
    let source = r#"import std;

fn main() int {
    try {
        v int = (lambda(x int) {
            raise "E": "boom";
            return x;
        })(1);
        C.printf("no %d\n", v);
    } except {
        C.printf("caught\n");
    }
    return 0;
}
"#;
    let scratch = Scratch::new("raise");
    assert_eq!(
        scratch.run("raise", source),
        "caught
"
    );
}

#[test]
fn a_lambda_body_that_catches_its_own_raise_is_not_throwing() {
    // The `try` is inside the lambda, so nothing escapes the call and the call
    // needs no handling of its own.
    let source = r#"import std;

fn main() int {
    v int = (lambda(x int) {
        try {
            raise "E": "boom";
        } except {
            return x * 2;
        }
        return x;
    })(21);
    C.printf("%d\n", v);
    return 0;
}
"#;
    let scratch = Scratch::new("inner-try");
    assert_eq!(
        scratch.run("inner-try", source),
        "42
"
    );
}

#[test]
fn a_lambda_that_raises_must_have_its_call_handled() {
    let source = r#"fn main() int {
    v int = (lambda(x int) {
        raise "E": "boom";
        return x;
    })(1);
    return v;
}
"#;
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("this lambda may raise"),
        "{diagnostics}"
    );
}

#[test]
fn a_capturing_lambda_is_a_closure_value() {
    // A lambda that captures is a value like any other: stored, passed, and
    // called. Its captures travel with it, so the call reaches the same
    // variables.
    let source = r#"import std;

template <T type>
fn apply(f T, v int) int {
    return f(v);
}

fn main() int {
    y int = 10;
    f auto = lambda(x int) -> x + y;
    C.printf("%d %d\n", f(5), apply(f, 1));
    return 0;
}
"#;
    let scratch = Scratch::new("closure");
    assert_eq!(scratch.run("closure", source), "15 11\n");
}

#[test]
fn a_lambda_with_an_untyped_parameter_is_a_closure_value() {
    let source = r#"import std;

fn main() int {
    g auto = lambda(x) -> x * 2;
    C.printf("%d %d\n", g(21), g(2));
    return 0;
}
"#;
    let scratch = Scratch::new("untyped-value");
    assert_eq!(scratch.run("untyped-value", source), "42 4\n");
}

#[test]
fn a_lambda_body_binds_its_own_locals() {
    // A body is a function body: it declares locals, loops over them, and
    // returns one. Lowering it with no binding table said "no type recorded
    // for binding `total`", because the checker keeps a body's bindings with
    // the instantiation that checked it.
    let source = r#"import std;

fn main() int {
    f auto = lambda(n int) {
        total int = 0;
        i int = 0;
        while (i < n) {
            total += i;
            i += 1;
        }
        return total;
    };
    C.printf("%d\n", f(5));
    return 0;
}
"#;
    let scratch = Scratch::new("locals");
    assert_eq!(scratch.run("locals", source), "10\n");
}

#[test]
fn a_captured_variable_is_written_through_the_closure_value() {
    // Capture is by reference, and the reference is what the value holds: two
    // calls of the same stored closure write the same variable.
    let source = r#"import std;

fn main() int {
    n int = 0;
    bump auto = lambda() -> n = n + 1;
    bump();
    bump();
    C.printf("%d\n", n);
    return 0;
}
"#;
    let scratch = Scratch::new("write-back");
    assert_eq!(scratch.run("write-back", source), "2\n");
}

#[test]
fn a_throwing_closure_value_is_caught_where_it_is_called() {
    // A lambda that may raise is a throwing function. As a value it is a
    // closure, and the call still has to be handled: the exception belongs to
    // the call site, not to the assignment.
    let source = r#"import std;

fn main() int {
    g auto = lambda(x int) {
        raise "E": "from closure";
        return x;
    };
    try {
        C.printf("%d\n", g(1));
    } except {
        C.printf("caught\n");
    }
    return 0;
}
"#;
    let scratch = Scratch::new("throwing-value");
    assert_eq!(scratch.run("throwing-value", source), "caught\n");
}

#[test]
fn a_closure_returns_an_aggregate() {
    let source = r#"import std;

type Pair {
    a int;
    b int;
}

fn main() int {
    pair auto = lambda(n int) {
        q Pair = .{n, n * 2};
        return q;
    };
    p Pair = pair(4);
    C.printf("%d %d\n", p.a, p.b);
    return 0;
}
"#;
    let scratch = Scratch::new("aggregate");
    assert_eq!(scratch.run("aggregate", source), "4 8\n");
}

#[test]
fn a_closure_returned_by_a_call_keeps_its_capture() {
    // The returned value is a closure, and the call of it is in another
    // function -- where the captured declaration is not in scope, so the body
    // is checked from the capture's own record. What it captured is `main`'s
    // variable, which outlives both.
    let source = r#"import std;

fn main() int {
    y int = 3;
    choose auto = lambda() {
        inner auto = lambda(x int) -> x * y;
        return inner;
    };
    C.printf("%d\n", choose()(4));
    return 0;
}
"#;
    let scratch = Scratch::new("returned");
    assert_eq!(scratch.run("returned", source), "12\n");
}

#[test]
fn a_closure_crosses_a_function_boundary() {
    // A closure is a value, so it may be handed to a function and given back.
    // The captures travel with it, and the addresses they hold stay valid
    // because the captured variable outlives the call.
    let source = r#"import std;

template <T type>
fn give(f T) T {
    return f;
}

fn main() int {
    y int = 3;
    f auto = lambda(x int) -> x + y;
    g auto = give(f);
    C.printf("%d\n", g(5));
    return 0;
}
"#;
    let scratch = Scratch::new("forwarded");
    assert_eq!(scratch.run("forwarded", source), "8\n");
}

#[test]
fn an_untyped_closure_reaches_each_calls_type() {
    // One untyped parameter, two calls with different argument types: each call
    // decides the parameter, and neither instantiation affects the other.
    let source = r#"import std;

template <T type>
fn apply(f T, v int) int {
    return f(v);
}

template <T type>
fn apply_double(f T, v double) double {
    return f(v);
}

fn main() int {
    u auto = lambda(x) -> x + x;
    C.printf("%d %.1f\n", apply(u, 3), apply_double(u, 1.5));
    return 0;
}
"#;
    let scratch = Scratch::new("untyped-calls");
    assert_eq!(scratch.run("untyped-calls", source), "6 3.0\n");
}

#[test]
fn a_call_without_parentheses_belongs_to_the_body() {
    // `lambda(x int) -> x(1)` is a lambda whose body calls `x`, not a call of
    // the lambda: `x` is an `int`, and an `int` cannot be called.
    let source = r#"fn main() int {
    v int = lambda(x int) -> x(1);
    return 0;
}
"#;
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(diagnostics.contains("cannot be called"), "{diagnostics}");
}

#[test]
fn lambda_is_a_reserved_word() {
    let source = "fn main() int {\n    lambda int = 1;\n    return 0;\n}\n";
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(diagnostics.contains("expected `(`"), "{diagnostics}");
}
