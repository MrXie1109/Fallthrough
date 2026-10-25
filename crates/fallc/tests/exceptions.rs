//! Raising, catching, and passing an exception on.
//!
//! A `??` function returns its value *or* an exception, so these tests can only
//! be about behaviour: what a handler sees, which clauses run, and where an
//! exception that nobody catches ends up. The programs are compiled and run
//! rather than inspected, because the protocol is about what happens.
//!
//! The compiler's rules -- where `raise` is legal, which calls need guarding --
//! are checked separately, by compiling programs that must be refused.

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
            std::env::temp_dir().join(format!("fallc-exc-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a temporary directory can be created");
        Scratch { directory }
    }

    /// Compiles the source into an executable and runs it.
    fn run(&self, label: &str, source: &str) -> String {
        let main = self.directory.join("main.ft");
        std::fs::write(&main, source).expect("the source can be written");
        let executable = self.directory.join(label);
        let args = [
            main.display().to_string(),
            "-o".to_owned(),
            executable.display().to_string(),
        ];
        let options = Options::parse(&args).expect("the arguments are well formed");
        assert_eq!(
            run(&options),
            std::process::ExitCode::SUCCESS,
            "`{label}` did not compile"
        );
        let output = std::process::Command::new(&executable)
            .output()
            .expect("the program can be run");
        assert!(
            output.status.success(),
            "`{label}` failed: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    /// Compiles the source, which must be refused.
    fn refused(&self, label: &str, source: &str) -> String {
        let main = self.directory.join("main.ft");
        std::fs::write(&main, source).expect("the source can be written");
        let executable = self.directory.join(label);
        let args = [
            main.display().to_string(),
            "--check".to_owned(),
            "-o".to_owned(),
            executable.display().to_string(),
        ];
        let options = Options::parse(&args).expect("the arguments are well formed");
        assert_ne!(
            run(&options),
            std::process::ExitCode::SUCCESS,
            "`{label}` compiled, and it should not have"
        );
        String::new()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn a_raise_is_caught_by_the_enclosing_except() {
    let source = r#"import std;

fn risky(n int) int except {
    if (n < 0) {
        raise "ValueError": "negative";
    }
    return n * 2;
}

fn main() int {
    try {
        C.printf("ok %d\n", risky(3));
    } except {
        C.printf("never\n");
    }
    try {
        C.printf("bad %d\n", risky(-1));
    } except {
        e &const std.Exception = std.exception();
        C.printf("caught %s: %s\n", e.name().data(), e.what().data());
    }
    return 0;
}
"#;
    let scratch = Scratch::new("catch");
    let output = scratch.run("catch", source);
    assert!(output.contains("ok 6"), "{output}");
    assert!(output.contains("caught ValueError: negative"), "{output}");
    assert!(!output.contains("never"), "{output}");
    assert!(!output.contains("bad "), "the call ran past the raise:\n{output}");
}

#[test]
fn a_raise_without_a_message_has_an_empty_what() {
    let source = r#"import std;

fn failing() void except {
    raise "ZeroError";
}

fn main() int {
    try {
        failing();
    } except {
        e &const std.Exception = std.exception();
        C.printf("name [%s] what [%s] len %lu\n", e.name().data(), e.what().data(), e.what().length());
    }
    return 0;
}
"#;
    let scratch = Scratch::new("no-message");
    let output = scratch.run("no-message", source);
    assert!(output.contains("name [ZeroError] what [] len 0"), "{output}");
}

#[test]
fn else_runs_only_when_nothing_was_raised() {
    let source = r#"import std;

fn maybe(n int) int except {
    if (n < 0) {
        raise "Bad";
    }
    return n;
}

fn main() int {
    try {
        C.printf("a %d\n", maybe(1));
    } except {
        C.printf("a caught\n");
    } else {
        C.printf("a else\n");
    }
    try {
        C.printf("b %d\n", maybe(-1));
    } except {
        C.printf("b caught\n");
    } else {
        C.printf("b else\n");
    }
    return 0;
}
"#;
    let scratch = Scratch::new("else");
    let output = scratch.run("else", source);
    assert!(output.contains("a else"), "{output}");
    assert!(!output.contains("a caught"), "{output}");
    assert!(output.contains("b caught"), "{output}");
    assert!(!output.contains("b else"), "{output}");
}

#[test]
fn finally_runs_on_both_paths() {
    let source = r#"import std;

fn maybe(n int) int except {
    if (n < 0) {
        raise "Bad";
    }
    return n;
}

fn main() int {
    try {
        maybe(1);
        C.printf("clean path\n");
    } except {
        C.printf("caught\n");
    } finally {
        C.printf("finally one\n");
    }
    try {
        maybe(-1);
        C.printf("not reached\n");
    } except {
        C.printf("caught two\n");
    } finally {
        C.printf("finally two\n");
    }
    return 0;
}
"#;
    let scratch = Scratch::new("finally");
    let output = scratch.run("finally", source);
    assert!(output.contains("clean path"), "{output}");
    assert!(output.contains("finally one"), "{output}");
    assert!(output.contains("caught two"), "{output}");
    assert!(output.contains("finally two"), "{output}");
    assert!(!output.contains("not reached"), "{output}");
    // Each `finally` runs exactly once.
    assert_eq!(output.matches("finally one").count(), 1, "{output}");
    assert_eq!(output.matches("finally two").count(), 1, "{output}");
}

#[test]
fn a_propagated_exception_reaches_the_outer_handler() {
    let source = r#"import std;

fn inner(n int) int except {
    if (n < 0) {
        raise "DeepError": "from inner";
    }
    return n;
}

fn middle(n int) int except {
    // `??` re-raises at the mark, so the `try` here is what takes it: that is
    // the meaning `x??` has -- `try { x } except { raise; }`. The handler
    // passes it on again, which is how a layer adds something and re-raises.
    try {
        return inner(n)??;
    } except {
        C.printf("middle caught %d\n", n);
        raise;
    }
}

fn main() int {
    try {
        C.printf("value %d\n", middle(5));
    } except {
        C.printf("main caught\n");
    }
    try {
        C.printf("value %d\n", middle(-1));
    } except {
        e &const std.Exception = std.exception();
        C.printf("main saw %s: %s\n", e.name().data(), e.what().data());
    }
    return 0;
}
"#;
    let scratch = Scratch::new("propagate");
    let output = scratch.run("propagate", source);
    assert!(output.contains("value 5"), "{output}");
    assert!(output.contains("main saw DeepError: from inner"), "{output}");
    // The `try` in `middle` catches the re-raise `??` produced, and its
    // handler raises again to pass it on.
    assert!(output.contains("middle caught -1"), "{output}");
}

#[test]
fn a_bare_raise_passes_on_the_same_exception() {
    let source = r#"import std;

fn inner(n int) int except {
    if (n < 0) {
        raise "LayerError": "kept";
    }
    return n;
}

fn middle(n int) int except {
    try {
        return inner(n);
    } except {
        C.printf("middle saw %s\n", std.exception().name().data());
        raise;
    }
    return 0;
}

fn main() int {
    try {
        middle(-1);
    } except {
        e &const std.Exception = std.exception();
        C.printf("main saw %s: %s\n", e.name().data(), e.what().data());
    }
    return 0;
}
"#;
    let scratch = Scratch::new("reraise");
    let output = scratch.run("reraise", source);
    assert!(output.contains("middle saw LayerError"), "{output}");
    // The same exception, not a new one: the message survived the re-raise.
    assert!(output.contains("main saw LayerError: kept"), "{output}");
}

#[test]
fn a_return_inside_a_try_still_runs_the_finally() {
    let source = r#"import std;

fn with_finally(n int) int except {
    try {
        return n * 2;
    } finally {
        C.printf("finally ran\n");
    }
}

fn main() int {
    try {
        C.printf("value %d\n", with_finally(4));
    } except {
        C.printf("never\n");
    }
    return 0;
}
"#;
    let scratch = Scratch::new("return-finally");
    let output = scratch.run("return-finally", source);
    assert!(output.contains("value 8"), "{output}");
    assert_eq!(output.matches("finally ran").count(), 1, "{output}");
}

#[test]
fn raise_is_refused_outside_a_throwing_function() {
    let bare = r#"import std;

fn plain() int {
    raise "Nope";
    return 0;
}

fn main() int {
    return plain();
}
"#;
    let scratch = Scratch::new("raise-refused");
    scratch.refused("raise-plain", bare);

    let unprotected = r#"import std;

fn risky() int except {
    raise "Nope";
}

fn main() int {
    return risky();
}
"#;
    scratch.refused("unprotected", unprotected);

    let bad_reraise = r#"import std;

fn f() void except {
    raise;
}

fn main() int {
    return 0;
}
"#;
    scratch.refused("reraise", bad_reraise);
}

#[test]
fn a_propagation_must_be_inside_a_throwing_function() {
    let source = r#"import std;

fn risky() int except {
    raise "Nope";
}

fn plain() int {
    return risky()??;
}

fn main() int {
    return 0;
}
"#;
    let scratch = Scratch::new("propagate-refused");
    scratch.refused("propagate-plain", source);
}

#[test]
fn a_region_may_share_one_propagation() {
    // The same protocol, chosen at a coarser grain: one `try` passes on
    // whatever any call in it raises, in order, with the exception intact.
    let source = r#"import std;

fn a(n int) void except {
    if (n == 1) {
        raise "AError": "from a";
    }
}

fn b(n int) void except {
    if (n == 2) {
        raise "BError": "from b";
    }
}

fn c(n int) void except {
    if (n == 3) {
        raise "CError": "from c";
    }
}

fn work(n int) void except {
    try {
        a(n);
        b(n);
        c(n);
        C.printf("all three ran with %d\n", n);
    } except {
        raise;
    }
}

fn main() int {
    try {
        work(0);
    } except {
        C.printf("never\n");
    }
    try {
        work(2);
    } except {
        e &const std.Exception = std.exception();
        C.printf("caught %s: %s\n", e.name().data(), e.what().data());
    }
    return 0;
}
"#;
    let scratch = Scratch::new("region");
    let output = scratch.run("region", source);
    assert!(output.contains("all three ran with 0"), "{output}");
    assert!(output.contains("caught BError: from b"), "{output}");
    // `b` raising means `c` never runs.
    assert!(!output.contains("all three ran with 2"), "{output}");
}

#[test]
fn a_conditional_claim_is_decided_at_the_call() {
    // `??(n < 0)` says the function raises only when the argument is negative.
    // With a constant argument that is a fact the checker can establish, so
    // `check(3)` needs no protection; a variable argument leaves it undecided,
    // and then it does.
    let decided = r#"import std;

fn check(n int) int except(n < 0) {
    if (n < 0) {
        raise "NegativeError": "negative";
    }
    return n;
}

fn main() int {
    C.printf("%d\n", check(3));
    try {
        C.printf("%d\n", check(-1));
    } except {
        C.printf("caught %s\n", std.exception().name().data());
    }
    return 0;
}
"#;
    let scratch = Scratch::new("cond");
    let output = scratch.run("cond", decided);
    assert!(output.contains("3"), "{output}");
    assert!(output.contains("caught NegativeError"), "{output}");

    let undecided = r#"import std;

fn check(n int) int except(n < 0) {
    if (n < 0) {
        raise "NegativeError";
    }
    return n;
}

fn main() int {
    n int = 3;
    C.printf("%d\n", check(n));
    return 0;
}
"#;
    scratch.refused("cond-variable", undecided);
}

#[test]
fn an_exception_that_leaves_main_is_reported() {
    // `main` may raise too. What comes out of the program is a report on
    // standard error and an exit code of its own, so an uncaught exception is
    // never mistaken for a program that returned normally.
    let source = r#"import std;

fn main() int except {
    C.printf("before\n");
    raise "Boom": "from main";
    return 0;
}
"#;
    let scratch = Scratch::new("uncaught");
    let main = scratch.directory.join("main.ft");
    std::fs::write(&main, source).expect("the source can be written");
    let executable = scratch.directory.join("uncaught");
    let args = [
        main.display().to_string(),
        "-o".to_owned(),
        executable.display().to_string(),
    ];
    let options = Options::parse(&args).expect("the arguments are well formed");
    assert_eq!(run(&options), std::process::ExitCode::SUCCESS, "did not compile");
    let output = std::process::Command::new(&executable)
        .output()
        .expect("the program can be run");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.contains("before"), "{stdout}");
    assert!(stderr.contains("uncaught exception: Boom: from main"), "{stderr}");
    assert_eq!(output.status.code(), Some(70), "{output:?}");
}

#[test]
fn a_constructor_that_raises_is_caught_where_it_was_written() {
    // A construction is a call site even though it is written as an
    // initialiser: `bad Ratio = 0;` runs `__init__`, so a raise from it is
    // handled by the `try` the initialiser stands in.
    let source = r#"import std;

type Ratio {
    _value int;
}

fn __im_init__(this &Ratio, value int) void except {
    if (value == 0) {
        raise "ZeroError": "zero";
    }
    this._value = value;
}

fn get(this &const Ratio) int {
    return this._value;
}

fn main() int {
    try {
        good Ratio = 7;
        C.printf("good %d\n", good.get());
        bad Ratio = 0;
        C.printf("never\n");
    } except {
        e &const std.Exception = std.exception();
        C.printf("caught %s: %s\n", e.name().data(), e.what().data());
    }
    return 0;
}
"#;
    let scratch = Scratch::new("throwing-init");
    let output = scratch.run("throwing-init", source);
    assert!(output.contains("good 7"), "{output}");
    assert!(output.contains("caught ZeroError: zero"), "{output}");
    assert!(!output.contains("never"), "{output}");
}

#[test]
fn a_constructor_that_may_raise_needs_handling() {
    // Nothing here catches the raise and `plain` does not throw, so the
    // construction is refused where it is written.
    let source = r#"import std;

type Ratio {
    _value int;
}

fn __im_init__(this &Ratio, value int) void except {
    if (value == 0) {
        raise "ZeroError": "zero";
    }
    this._value = value;
}

fn plain() int {
    bad Ratio = 0;
    return 1;
}

fn main() int {
    return plain();
}
"#;
    let scratch = Scratch::new("throwing-init-unprotected");
    scratch.refused("throwing-init-unprotected", source);
}

#[test]
fn a_constructor_that_may_raise_propagates_from_a_throwing_function() {
    let source = r#"import std;

type Ratio {
    _value int;
}

fn __im_init__(this &Ratio, value int) void except {
    if (value == 0) {
        raise "ZeroError": "zero";
    }
    this._value = value;
}

fn get(this &const Ratio) int {
    return this._value;
}

// A construction has no call to write `??` on, so the marker goes on the
// initialiser itself.
fn build(value int) int except {
    made Ratio = value??;
    return made.get();
}

fn main() int {
    try {
        C.printf("%d\n", build(5));
        C.printf("%d\n", build(0));
    } except {
        e &const std.Exception = std.exception();
        C.printf("caught %s\n", e.name().data());
    }
    return 0;
}
"#;
    let scratch = Scratch::new("throwing-init-propagate");
    let output = scratch.run("throwing-init-propagate", source);
    assert!(output.contains("5"), "{output}");
    assert!(output.contains("caught ZeroError"), "{output}");
}

#[test]
fn an_operator_that_may_raise_is_caught_where_it_is_written() {
    // `a + b` is a call of `__add__`, so a raise from it belongs to the `try`
    // the expression stands in -- and to nothing else, which the refusal below
    // checks.
    let source = r#"import std;

type Limited {
    _value int;
}

fn __im_init__(this &Limited, value int) void {
    this._value = value;
}

fn __add__(this &const Limited, other &const Limited) Limited except {
    if (this._value + other._value > 100) {
        raise "OverflowError": "too large";
    }
    return Limited(this._value + other._value);
}

fn __init__(this &Limited, other &const Limited) void {
    this._value = other._value;
}

fn main() int {
    try {
        a Limited = 40;
        b Limited = 50;
        c Limited = a + b;
        C.printf("%d\n", c._value);
        d Limited = 80;
        e Limited = a + d;
        C.printf("never\n");
    } except {
        err &const std.Exception = std.exception();
        C.printf("caught %s: %s\n", err.name().data(), err.what().data());
    }
    return 0;
}
"#;
    let scratch = Scratch::new("throwing-operator");
    let output = scratch.run("throwing-operator", source);
    assert!(output.contains("90"), "{output}");
    assert!(output.contains("caught OverflowError: too large"), "{output}");
    assert!(!output.contains("never"), "{output}");
}

#[test]
fn an_initialiser_marker_is_only_legal_in_a_throwing_function() {
    // `??` passes the raise on, so it needs a function that may raise: a plain
    // one has nowhere to pass it to.
    let source = r#"import std;

type Ratio {
    _value int;
}

fn __im_init__(this &Ratio, value int) void except {
    if (value == 0) {
        raise "ZeroError": "zero";
    }
    this._value = value;
}

fn plain() int {
    made Ratio = 0??;
    return 1;
}

fn main() int {
    return plain();
}
"#;
    let scratch = Scratch::new("initialiser-marker");
    scratch.refused("initialiser-marker", source);
}

#[test]
fn a_function_may_swallow_what_it_cannot_do() {
    // A layer that can decide what a raise means does not propagate it: the
    // `try` is inside an ordinary function, so its callers see a plain
    // function and no `except` mark, and no `??` is needed anywhere above.
    let source = r#"import std;

fn risky(n int) int except {
    if (n < 0) {
        raise "Negative";
    }
    return n * 2;
}

fn doubled_or_zero(n int) int {
    try {
        return risky(n);
    } except {
        return 0;
    }
}

fn main() int {
    C.printf("%d %d\n", doubled_or_zero(3), doubled_or_zero(-1));
    return 0;
}
"#;
    let scratch = Scratch::new("swallow");
    let output = scratch.run("swallow", source);
    assert!(output.contains("6 0"), "{output}");
}

#[test]
fn an_uncaught_exception_without_a_message_prints_only_the_name() {
    // `raise "Name";` has no message, so the separator has nothing to
    // introduce: the report is the name alone, on its own line.
    let source = "fn main() int except {\n    raise \"Boom\";\n    return 0;\n}\n";
    let scratch = Scratch::new("uncaught-empty");
    let main = scratch.directory.join("main.ft");
    std::fs::write(&main, source).expect("the source can be written");
    let executable = scratch.directory.join("uncaught-empty");
    let args = [
        main.display().to_string(),
        "-o".to_owned(),
        executable.display().to_string(),
    ];
    let options = Options::parse(&args).expect("the arguments are well formed");
    assert_eq!(run(&options), std::process::ExitCode::SUCCESS, "did not compile");
    let output = std::process::Command::new(&executable)
        .output()
        .expect("the program can be run");
    assert_eq!(String::from_utf8_lossy(&output.stderr), "uncaught exception: Boom\n");
    assert_eq!(output.status.code(), Some(70), "{output:?}");
}

#[test]
fn the_exception_runtime_comes_without_an_import() {
    // Exceptions are the language's, not the library's: a file that raises gets
    // the runtime it needs. Naming the standard library is not a rule a
    // program should have to know.
    let source = "fn bad() int except {\n    raise \"Unimported\";\n}\nfn main() int except {\n    return bad()??;\n}\n";
    let scratch = Scratch::new("no-import");
    let main = scratch.directory.join("main.ft");
    std::fs::write(&main, source).expect("the source can be written");
    let executable = scratch.directory.join("no-import");
    let args = [
        main.display().to_string(),
        "-o".to_owned(),
        executable.display().to_string(),
    ];
    let options = Options::parse(&args).expect("the arguments are well formed");
    assert_eq!(run(&options), std::process::ExitCode::SUCCESS, "did not compile");
    let output = std::process::Command::new(&executable)
        .output()
        .expect("the program can be run");
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "uncaught exception: Unimported\n"
    );
}

#[test]
fn a_mark_on_a_call_that_cannot_raise_is_a_warning() {
    // `??` says "pass on whatever this raised". A call that cannot raise has
    // nothing to pass on, so the mark does nothing -- worth saying, but not
    // worth refusing: inside a template the same `??` may stand for calls that
    // do raise, and an error would forbid the spelling the marker exists for.
    let source = r#"import std;

fn twice(n int) int {
    return n * 2;
}

fn main() int except {
    return twice(2)??;
}
"#;
    let (outcome, printed) =
        fallc::pipeline::compile_text("warn.ft", source, &[fallc::cli::EmitKind::Llvm]);
    assert_eq!(outcome, fallc::Outcome::Success, "{printed}");
    assert!(printed.contains("warning[S0083]"), "{printed}");
    assert!(printed.contains("cannot raise"), "{printed}");
}

#[test]
fn a_mark_on_a_call_that_can_raise_is_silent() {
    let source = r#"import std;

fn risky() int except {
    raise "Error";
}

fn main() int except {
    return risky()??;
}
"#;
    let (outcome, printed) = fallc::pipeline::compile_text("quiet.ft", source, &[fallc::cli::EmitKind::Llvm]);
    assert_eq!(outcome, fallc::Outcome::Success, "{printed}");
    assert!(!printed.contains("S0083"), "{printed}");
}

#[test]
fn a_try_body_runs_its_destructors_before_the_handler() {
    // The body is a scope of its own, and an exception leaving it leaves that
    // scope -- so every value declared in it is destroyed before the handler
    // runs, exactly as a `return` would destroy them. The handler used to be
    // branched to directly, which left whatever the body had declared alive and
    // unreachable.
    let source = r#"import std;

type Loud {
    v int;
}

fn __drop__(this &Loud) void {
    C.printf("drop %d\n", this.v);
}

fn boom() int except {
    raise "E": "boom";
    return 0;
}

fn main() int {
    try {
        loud Loud = .{7};
        boom();
    } except {
        C.printf("except\n");
    }
    return 0;
}
"#;
    let scratch = Scratch::new("drops-before-handler");
    // Declared in the body, destroyed before the handler; the value outside is
    // not in the body's scope and outlives it.
    assert_eq!(scratch.run("drops", source), "drop 7\nexcept\n");
}

#[test]
fn an_assignment_through_a_reference_calls_the_types_assign() {
    // `t = u` with `t` a `&Thing` writes *through* the reference, so the type
    // that answers the assignment is `Thing`, not `&Thing`. Asking the
    // reference found no `__assign__`, and the store fell back to member-wise
    // copy -- which for a type that owns a buffer frees it twice, and for a
    // throwing `__assign__` dropped the exception on the floor.
    let source = r#"import std;

type Thing {
    v int;
}

fn __assign__(this &Thing, other &const Thing) void except {
    raise "AssignFailed": "no";
}

fn copy_into(t &Thing, u &const Thing) void except {
    try {
        t = u;
    } except {
        raise;
    }
}

fn main() int {
    a Thing = .{1};
    b Thing = .{2};
    try {
        copy_into(&a, &b);
    } except {
        e &const std.Exception = std.exception();
        C.printf("caught %s\n", e.name().data());
    }
    C.printf("%d\n", a.v);
    return 0;
}
"#;
    let scratch = Scratch::new("assign-through-ref");
    assert_eq!(scratch.run("assign_ref", source), "caught AssignFailed\n1\n");
}

#[test]
fn a_throwing_assignment_is_a_call_site() {
    // An `__assign__` that may raise needs a `try` or a `??` at the assignment,
    // exactly as a call does. It was not reported at all, so an exception could
    // be raised and lost: the target kept its old value and nothing said so.
    let source = r#"type Thing {
    v int;
}

fn __assign__(this &Thing, other &const Thing) void except {
    raise "AssignFailed": "no";
}

fn main() int except {
    a Thing = .{1};
    b Thing = .{2};
    a = b;
    return 0;
}
"#;
    let (outcome, printed) =
        fallc::pipeline::compile_text("throwing_assign.ft", source, &[fallc::cli::EmitKind::Llvm]);
    assert_eq!(outcome, fallc::Outcome::Failure, "{printed}");
    assert!(printed.contains("S0083"), "{printed}");
}

#[test]
fn an_except_condition_reads_its_own_functions_parameters() {
    // The condition is an expression written against the parameters, so it is
    // resolved in the function's own scope. Without that, a name it mentions
    // was resolved by falling back to the first definition in the program: a
    // parameter of the same name in another function, and of another type.
    let source = r#"import C;

type A {
    n int;
}

type B {
    n int;
}

fn tag(this &A, value c8) int {
    return value as int;
}

fn save(this &B, value int) int except (value < 0) {
    this.n = value;
    return this.n;
}

fn main() int {
    b B = .{0};
    try {
        C.printf("%d\n", save(b, 4));
    } except {
        C.printf("caught\n");
    }
    return 0;
}
"#;
    let scratch = Scratch::new("except-condition-scope");
    assert_eq!(scratch.run("except-condition-scope", source), "4\n");
}

#[test]
fn a_call_of_a_template_or_a_pack_obeys_the_raise_rule() {
    // A template is not a function until a call instantiates it, but the call
    // knows the declaration says `except`, so the duty to handle it is the same
    // as for an ordinary function. It used to be skipped, and a template that
    // always raises could be called with nothing handling it.
    let source = r#"import std;

template <T type>
fn noisy(value T) void except {
    raise "Boom": "always";
}

template <Ts ...type>
fn louder(values ...Ts) void except {
    raise "Boom": "always";
}

fn main() int except {
    noisy(1);
    louder(1, 2);
    return 0;
}
"#;
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("`noisy` may raise"),
        "{diagnostics}"
    );
    assert!(
        diagnostics.contains("`louder` may raise"),
        "{diagnostics}"
    );
}

#[test]
fn a_named_except_catches_only_the_exception_it_names() {
    // The filter compares the exception's `name()`: an exception with another
    // name is not caught here and keeps going outward, through the `finally`.
    let source = r#"import std;

fn risky(which int) int except {
    if (which == 1) {
        raise "ZeroError": "zero";
    }
    raise "OtherError": "other";
}

fn matching() int except {
    try {
        C.printf("risky %d\n", risky(2));
    } except ("OtherError") {
        e &const std.Exception = std.exception();
        C.printf("matched %s\n", e.name().data());
    }
    return 0;
}

fn mismatched() int except {
    try {
        risky(1)??;
    } except ("OtherError") {
        C.printf("wrong handler\n");
    } finally {
        C.printf("inner finally\n");
    }
    return 0;
}

fn run() int except {
    matching()??;
    mismatched()??;
    return 0;
}

fn main() int {
    try {
        run()??;
    } except {
        e &const std.Exception = std.exception();
        C.printf("escaped %s\n", e.name().data());
    }
    return 0;
}
"#;
    let scratch = Scratch::new("named-except");
    let output = scratch.run("named-except", source);
    assert!(output.contains("matched OtherError"), "{output}");
    assert!(output.contains("inner finally"), "{output}");
    assert!(output.contains("escaped ZeroError"), "{output}");
    assert!(!output.contains("wrong handler"), "{output}");
}

#[test]
fn a_named_except_needs_a_function_that_can_raise() {
    // A handler that names exceptions lets the others through, so the enclosing
    // function has to be able to raise them. Names are values, so there is no
    // way to prove a list complete.
    let source = r#"import std;

fn risky() int except {
    raise "ZeroError";
}

fn main() int {
    try {
        risky()??;
    } except ("ZeroError") {
        C.printf("caught\n");
    }
    return 0;
}
"#;
    let scratch = Scratch::new("named-except-refused");
    let diagnostics = scratch.refused("named-except-refused", source);
    let _ = diagnostics;
}

#[test]
fn except_clauses_are_tried_in_order() {
    // Several `except` clauses, named and bare, run top to bottom: the first
    // one that matches handles the exception, and a bare clause catches
    // everything after it.
    let source = r#"import std;

fn classify(n int) int except {
    if (n == 1) {
        raise "A": "one";
    }
    if (n == 2) {
        raise "B": "two";
    }
    raise "C": "three";
}

fn order() int except {
    classify(2)??;
    return 0;
}

fn fallback() int except {
    classify(3)??;
    return 0;
}

fn run() int except {
    try {
        order()??;
    } except ("A") {
        C.printf("A\n");
    } except ("B") {
        C.printf("B\n");
    } except {
        C.printf("other\n");
    }
    try {
        fallback()??;
    } except ("A") {
        C.printf("A again\n");
    } except ("B") {
        C.printf("B again\n");
    } except {
        e &const std.Exception = std.exception();
        C.printf("fallback %s\n", e.name().data());
    }
    return 0;
}

fn main() int {
    try {
        run()??;
    } except {
        C.printf("escaped\n");
    }
    return 0;
}
"#;
    let scratch = Scratch::new("except-order");
    let output = scratch.run("except-order", source);
    assert!(output.contains("B"), "{output}");
    assert!(output.contains("fallback C"), "{output}");
    assert!(!output.contains("A\n"), "{output}");
    assert!(!output.contains("escaped"), "{output}");
}
