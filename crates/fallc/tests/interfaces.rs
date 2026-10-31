//! Interfaces: structural satisfaction and virtual calls.
//!
//! An interface states restrictions about a type, exactly as a trait does. A
//! restriction that says what a call yields is also a method a box dispatches,
//! and a value of the interface type is a heap copy of a concrete value beside a
//! table of those methods. That copy is what a call reaches: a method that
//! writes its receiver writes the copy, not the object it came from.

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
            std::env::temp_dir().join(format!("fallc-iface-{label}-{}", std::process::id()));
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
fn a_value_is_boxed_and_its_methods_are_called() {
    let source = r#"import std;

type Counter {
    n int;
}

fn next(this &Counter) int {
    this.n = this.n + 1;
    return this.n;
}

fn value(this &const Counter) int {
    return this.n;
}

interface ICount(it T) {
    it.next() -> int,
    it.value() -> int,
}

fn main() int {
    c Counter = .{0};
    it ICount = c;
    C.printf("%d %d\n", it.next(), it.value());
    return 0;
}
"#;
    let scratch = Scratch::new("boxed");
    assert_eq!(scratch.run("boxed", source), "1 1\n");
}

#[test]
fn a_generic_interface_carries_its_argument() {
    let source = r#"import std;

type Numbers {
    value int;
}

fn value(this &const Numbers) int {
    return this.value;
}

type Other {
    tag int;
}

fn value(this &const Other) int {
    return this.tag * 100;
}

interface IValue(it S, value T) {
    it.value() -> T,
}

fn use(it IValue<int>) int {
    return it.value();
}

fn main() int {
    n Numbers = .{5};
    o Other = .{7};
    C.printf("%d %d\n", use(n), use(o));
    return 0;
}
"#;
    let scratch = Scratch::new("generic");
    assert_eq!(scratch.run("generic", source), "5 700\n");
}

#[test]
fn the_box_is_a_copy_of_the_value() {
    // A method that writes its receiver writes the box, not the original; the
    // two calls see the same box, which is what makes a counter through an
    // interface count.
    let source = r#"import std;

type Acc {
    base int;
}

fn bump(this &Acc) int {
    this.base = this.base + 1;
    return this.base;
}

interface IAcc(it T) {
    it.bump() -> int,
}

fn main() int {
    a Acc = .{10};
    x IAcc = a;
    C.printf("%d %d %d\n", x.bump(), x.bump(), a.base);
    return 0;
}
"#;
    let scratch = Scratch::new("copy");
    assert_eq!(scratch.run("copy", source), "11 12 10\n");
}

#[test]
fn an_interface_method_may_take_a_pointer() {
    let source = r#"import std;

type Acc {
    base int;
}

fn add(this &const Acc, p *int) int {
    return this.base + p[0];
}

interface IAcc(it T) {
    it.add(*int) -> int,
}

fn main() int {
    a Acc = .{10};
    x IAcc = a;
    n int = 5;
    C.printf("%d\n", x.add(&n));
    return 0;
}
"#;
    let scratch = Scratch::new("pointer");
    assert_eq!(scratch.run("pointer", source), "15\n");
}

#[test]
fn an_interface_value_may_be_returned_and_reassigned() {
    let source = r#"import std;

type A {
    v int;
}
fn value(this &const A) int {
    return this.v;
}

type B {
    w int;
}
fn value(this &const B) int {
    return this.w * 10;
}

interface IValue(it S, value T) {
    it.value() -> T,
}

fn make() IValue<int> {
    a A = .{3};
    return a;
}

fn main() int {
    x IValue<int> = make();
    a A = .{4};
    x = a;
    b B = .{5};
    x = b;
    C.printf("%d\n", x.value());
    return 0;
}
"#;
    let scratch = Scratch::new("return");
    assert_eq!(scratch.run("return", source), "50\n");
}

#[test]
fn an_interface_value_can_be_used_as_a_narrower_interface() {
    let source = r#"import std;

type Counter {
    n int;
}

fn next(this &Counter) int {
    this.n = this.n + 1;
    return this.n;
}

fn name(this &const Counter) *const c8 {
    return "counter";
}

interface IWide(it T) {
    it.next() -> int,
    it.name() -> *const c8,
}

interface INarrow(it T) {
    it.next() -> int,
}

fn main() int {
    c Counter = .{0};
    wide IWide = c;
    narrow INarrow = wide;
    C.printf("%d %d %s\n", narrow.next(), narrow.next(), wide.name());
    return 0;
}
"#;
    let scratch = Scratch::new("narrow");
    assert_eq!(
        scratch.run("narrow", source),
        "1 2 counter
"
    );
}

#[test]
fn a_wider_interface_cannot_be_made_from_a_narrower_one() {
    let source = r#"type A {
    v int;
}
fn value(this &const A) int {
    return this.v;
}

interface ISmall(it T) {
    it.value() -> int,
}

interface IBig(it T) {
    it.value() -> int,
    it.extra() -> int,
}

fn main() int {
    a A = .{1};
    small ISmall = a;
    big IBig = small;
    return big.value();
}
"#;
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("cannot be made from `ISmall`"),
        "{diagnostics}"
    );
}

#[test]
fn an_interface_may_be_used_inside_a_template() {
    let source = r#"import std;

type Counter {
    n int;
}

fn next(this &Counter) int {
    this.n = this.n + 1;
    return this.n;
}

fn value(this &const Counter) int {
    return this.n;
}

interface ICount(it T) {
    it.next() -> int,
    it.value() -> int,
}

template <T type>
fn via(c T) int {
    it ICount = c;
    return it.next() * 10 + it.value();
}

fn main() int {
    c Counter = .{0};
    C.printf("%d\n", via(c));
    return 0;
}
"#;
    let scratch = Scratch::new("template");
    assert_eq!(
        scratch.run("template", source),
        "11
"
    );
}

#[test]
fn an_interface_method_may_return_a_large_aggregate() {
    let source = r#"import std;

type Big {
    a [128]int;
    b [128]int;
}

fn make(this &const Big) Big {
    return this;
}

fn n(this &const Big) int {
    return this.a[0] + this.b[0];
}

interface IBig(it T) {
    it.make() -> Big,
    it.n() -> int,
}

fn main() int {
    x Big;
    x.a[0] = 20;
    x.b[0] = 22;
    v IBig = x;
    r Big = v.make();
    C.printf("%d %d\n", r.a[0] + r.b[0], v.n());
    return 0;
}
"#;
    let scratch = Scratch::new("sret");
    assert_eq!(
        scratch.run("sret", source),
        "42 42
"
    );
}

#[test]
fn an_interface_method_may_say_that_it_raises() {
    // `except` is part of the method's type, so an interface can declare it and
    // a method that may raise satisfies it. The virtual call is then a throwing
    // call: its exception is the call site's to handle.
    let source = r#"import std;

type Engine {
    n int;
}

fn start(this &Engine) int except {
    if (this.n == 0) {
        raise "E": "no fuel";
    }
    return this.n;
}

fn stop(this &Engine) int {
    return 0;
}

interface IEngine(it T) {
    it.start()?? -> int,
    it.stop() -> int,
}

fn main() int {
    e Engine = .{0};
    it IEngine = e;
    try {
        C.printf("%d\n", it.start());
    } except {
        C.printf("caught\n");
    }
    C.printf("%d\n", it.stop());
    e.n = 7;
    it2 IEngine = e;
    try {
        C.printf("%d\n", it2.start());
    } except {
        C.printf("caught 2\n");
    }
    return 0;
}
"#;
    let scratch = Scratch::new("throwing");
    assert_eq!(scratch.run("throwing", source), "caught\n0\n7\n");
}

#[test]
fn a_throwing_interface_call_must_be_handled() {
    let source = r#"type Engine {
    n int;
}

fn start(this &Engine) int except {
    return this.n;
}

interface IEngine(it T) {
    it.start()?? -> int,
}

fn main() int {
    e Engine = .{1};
    it IEngine = e;
    return it.start();
}
"#;
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("may raise, and nothing here handles it"),
        "{diagnostics}"
    );
}

#[test]
fn a_throwing_interface_call_can_be_passed_on() {
    let source = r#"import std;

type Engine {
    n int;
}

fn start(this &Engine) int except {
    if (this.n > 2) {
        raise "E": "too far";
    }
    return this.n;
}

interface IEngine(it T) {
    it.start()?? -> int,
}

fn run(it IEngine) int except {
    return it.start() ??;
}

fn main() int {
    e Engine = .{9};
    it IEngine = e;
    try {
        C.printf("%d\n", run(it));
    } except {
        C.printf("outer caught\n");
    }
    return 0;
}
"#;
    let scratch = Scratch::new("propagate");
    assert_eq!(scratch.run("propagate", source), "outer caught\n");
}

#[test]
fn a_method_must_agree_with_the_interface_about_raising() {
    // Both directions are a mismatch: the result shape differs, so neither
    // method can stand in for the other.
    let quiet = r#"type A {
    v int;
}

fn value(this &const A) int {
    return this.v;
}

interface IValue(it T) {
    it.value()?? -> int,
}

fn main() int {
    a A = .{1};
    x IValue = a;
    return 0;
}
"#;
    let (outcome, diagnostics) = compile_text("t.ft", quiet, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("does not hold") && diagnostics.contains("cannot raise"),
        "{diagnostics}"
    );

    let raising = r#"type A {
    v int;
}

fn value(this &const A) int except {
    return this.v;
}

interface IValue(it T) {
    it.value() -> int,
}

fn main() int {
    a A = .{1};
    x IValue = a;
    return 0;
}
"#;
    let (outcome, diagnostics) = compile_text("t.ft", raising, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("does not hold") && diagnostics.contains("may raise"),
        "{diagnostics}"
    );
}

#[test]
fn a_type_without_the_methods_does_not_satisfy_the_interface() {
    let source = r#"type A {
    v int;
}

interface IValue(it T) {
    it.value() -> int,
}

fn main() int {
    a A = .{1};
    x IValue = a;
    return x.value();
}
"#;
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("does not satisfy `IValue`"),
        "{diagnostics}"
    );
}

#[test]
fn a_type_with_the_wrong_signature_does_not_satisfy_the_interface() {
    let source = r#"type A {
    v int;
}

fn value(this &const A) *int {
    return &this.v;
}

interface IValue(it T) {
    it.value() -> int,
}

fn main() int {
    a A = .{1};
    x IValue = a;
    return x.value();
}
"#;
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("does not satisfy `IValue`"),
        "{diagnostics}"
    );
}

#[test]
fn an_interface_may_be_forward_declared_and_completed() {
    let source = r#"import std;

interface IValue;

interface IValue(it T) {
    it.value() -> int,
}

type A {
    v int;
}
fn value(this &const A) int {
    return this.v;
}

fn main() int {
    a A = .{42};
    x IValue = a;
    C.printf("%d\n", x.value());
    return 0;
}
"#;
    let scratch = Scratch::new("forward");
    assert_eq!(scratch.run("forward", source), "42\n");
}

#[test]
fn an_interface_forward_declaration_may_be_used_behind_a_pointer() {
    let source = r#"interface IValue;

fn main() int {
    p *IValue = nullptr;
    return 0;
}
"#;
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Success, "{diagnostics}");
}

#[test]
fn an_interface_is_declared_the_way_a_trait_is() {
    // One declaration surface for one idea: a contract stated about a type.
    // `traits Readable(it T) { .. }` and `interface ICount(it T) { .. }` take
    // the same parameter list -- the first parameter is the subject -- and the
    // parameters after it are type parameters, so `IValue<int>` reads as it
    // always did.
    let source = r#"import std;

interface ICount(it T) {
    it.next() -> int,
    it.value() -> int,
}

interface IValue(it S, value T) {
    it.get() -> T,
}

type Counter {
    n int;
}

fn next(this &Counter) int {
    this.n = this.n + 1;
    return this.n;
}

fn value(this &const Counter) int {
    return this.n;
}

type Number {
    v int;
}

fn get(this &const Number) int {
    return this.v;
}

fn main() int {
    c Counter = .{0};
    it ICount = c;
    n Number = .{7};
    v IValue<int> = n;
    C.printf("%d %d %d\n", it.next(), it.value(), v.get());
    return 0;
}
"#;
    let scratch = Scratch::new("declared-like-a-trait");
    assert_eq!(scratch.run("declared_like_a_trait", source), "1 1 7\n");
}

#[test]
fn a_restriction_without_a_result_is_not_a_method_of_the_box() {
    // `-> !void` says the call yields something without saying what, which is
    // enough to check and not enough to call: a virtual call has to know what
    // it gets back.
    let source = r#"type Range {
    at int;
}

fn begin(this &Range) *int {
    return &this.at;
}

interface IWalk(it T) {
    it.begin() -> !void,
}

fn main() int {
    r Range = .{0};
    it IWalk = r;
    return *it.begin();
}
"#;
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("without a result") && diagnostics.contains("it.begin(..) -> Type"),
        "{diagnostics}"
    );
}

#[test]
fn a_restriction_argument_states_the_parameter_type() {
    // The argument of a restriction is a type, and it is the parameter's type:
    // a method that takes a `c8` does not satisfy `it.push(int)`.
    let source = r#"type Buffer {
    at int;
}

fn push(this &Buffer, value c8) int {
    this.at = value as int;
    return this.at;
}

interface IPush(it T) {
    it.push(int) -> int,
}

fn main() int {
    b Buffer = .{0};
    it IPush = b;
    return it.push('a');
}
"#;
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("it.push(int) -> int") && diagnostics.contains("does not hold"),
        "{diagnostics}"
    );

    // The same method does satisfy the restriction that states its type.
    let matching = source.replace("it.push(int) -> int,", "it.push(c8) -> int,");
    let (outcome, diagnostics) = compile_text("t.ft", &matching, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Success, "{diagnostics}");
}

#[test]
fn a_virtual_call_passes_its_arguments_by_position() {
    // The interface states the types of the parameters, not their names, so
    // there is nothing for a named argument to name.
    let source = r#"type Buffer {
    at int;
}

fn push(this &Buffer, value int) int {
    this.at = value;
    return this.at;
}

interface IPush(it T) {
    it.push(int) -> int,
}

fn main() int {
    b Buffer = .{0};
    it IPush = b;
    return it.push(.value = 1);
}
"#;
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(diagnostics.contains("by position"), "{diagnostics}");
}

#[test]
fn a_type_is_not_a_value() {
    // A requirement's argument is a type; everywhere else a type name is not a
    // value, and saying so keeps it from reaching code generation as a name
    // with no storage.
    let source = r#"type Value {
    v int;
}

fn main() int {
    x int = int;
    return x;
}
"#;
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("a type is not a value") && diagnostics.contains("S0135"),
        "{diagnostics}"
    );
}

#[test]
fn an_interface_with_restrictions_answers_a_query() {
    // One body, two uses: the question a box answers is a question a program
    // may ask, and the answer is the same.
    let source = r#"import std;

type Counter {
    n int;
}

fn next(this &Counter) int {
    this.n = this.n + 1;
    return this.n;
}

interface ICount(it T) {
    it.next() -> int,
}

fn main() int {
    c Counter = .{0};
    it ICount = c;
    if (ICount(Counter)) @constexpr {
        C.printf("%d\n", it.next());
    }
    return 0;
}
"#;
    let scratch = Scratch::new("query");
    assert_eq!(scratch.run("query", source), "1\n");
}

#[test]
fn an_interface_body_states_methods_by_calling_them() {
    // A signature and a restriction would be two statements of one thing, and
    // they could disagree; the call states both, so a `fn` member is refused.
    let signed = r#"interface ICount(it T) {
    fn next() int;
}
"#;
    let (outcome, diagnostics) = compile_text("t.ft", signed, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("states a method by calling it")
            && diagnostics.contains("it.next() -> int"),
        "{diagnostics}"
    );

    // The subject needs a type variable, which the parameter list introduces:
    // the angle form states type parameters and no subject.
    let angle = r#"interface IValue<T> {
    it.value() -> T,
}
"#;
    let (outcome, diagnostics) = compile_text("t.ft", angle, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("writes its parameters as a trait does"),
        "{diagnostics}"
    );
}

#[test]
fn one_method_is_stated_once() {
    // The same method stated twice with one signature is harmless repetition.
    // Two signatures are a contradiction, and the table has room for one.
    let repeated = r#"interface ICount(it T) {
    it.next() -> int,
    it.next()  ->  int,
}
"#;
    let (outcome, diagnostics) = compile_text("t.ft", repeated, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Success, "{diagnostics}");

    let contradictory = r#"interface ICount(it T) {
    it.next() -> int,
    it.next() -> long,
}
"#;
    let (outcome, diagnostics) = compile_text("t.ft", contradictory, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("stated twice") && diagnostics.contains("different signatures"),
        "{diagnostics}"
    );
}

#[test]
fn a_reference_to_an_interface_dispatches_through_the_box() {
    // `&ICount` points at a box, so a call through it reaches the table's entry
    // -- not the concrete type's member, which the call has no receiver for.
    let source = r#"import std;

type Counter {
    n int;
}

fn next(this &Counter) int {
    this.n = this.n + 1;
    return this.n;
}

interface ICount(it T) {
    it.next() -> int,
}

fn bump(it &ICount) int {
    return it.next();
}

fn main() int except {
    c Counter = .{0};
    it ICount = c;
    std.out.print(bump(&it), "\n")??;
    return 0;
}
"#;
    let scratch = Scratch::new("ref-dispatch");
    assert_eq!(scratch.run("ref-dispatch", source), "1\n");
}

#[test]
fn a_concrete_value_is_not_a_reference_to_a_box() {
    // A reference to an interface is a reference to a box, and a concrete value
    // is not one. Accepting it handed the callee the value's own fields where a
    // method table was expected; the program boxes first.
    let source = r#"type Counter {
    n int;
}

fn next(this &Counter) int {
    this.n = this.n + 1;
    return this.n;
}

interface ICount(it T) {
    it.next() -> int,
}

fn bump(it &ICount) int {
    return it.next();
}

fn main() int {
    c Counter = .{0};
    return bump(&c);
}
"#;
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("cannot stand where") && diagnostics.contains("box"),
        "{diagnostics}"
    );
}
