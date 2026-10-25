//! Traits: a named set of requirements a type either has or does not.
//!
//! A trait declares no code and no storage. It says which operations a type
//! must have -- written as expressions against a parameter -- and the compiler
//! answers whether a given type has them. Nothing declares conformance, so
//! there is no list to keep in step with a type's methods.
//!
//! A query is an ordinary expression of type `bool`: `Incrementable(it)` is
//! true when `it`'s type has the operations the trait names. That makes
//! `if constexpr` the way to act on the answer, and it makes a query about a
//! type the program has not used yet answerable at all.

use fallc::Outcome;
use fallc::cli::EmitKind;
use fallc::pipeline::compile_text;

/// Compiles `source` and returns the outcome plus everything the driver printed.
fn compile(source: &str) -> (Outcome, String) {
    compile_text("test.ft", source, &[EmitKind::Llvm])
}

/// Compiles `source` into an executable, runs it, and returns what it printed.
fn run_program(label: &str, source: &str) -> String {
    let directory =
        std::env::temp_dir().join(format!("fallc-traits-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("a temporary directory can be created");
    let main = directory.join("main.ft");
    std::fs::write(&main, source).expect("the source can be written");
    let executable = directory.join(label);
    let options = fallc::cli::Options::parse(&[
        main.display().to_string(),
        "-o".to_owned(),
        executable.display().to_string(),
    ])
    .expect("the arguments are well formed");
    assert_eq!(
        fallc::pipeline::run(&options),
        std::process::ExitCode::SUCCESS,
        "`{label}` did not compile"
    );
    let output = std::process::Command::new(&executable)
        .output()
        .expect("the program can be run");
    let _ = std::fs::remove_dir_all(&directory);
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// A program that prints the answer for one query, plus the declarations it
/// asks about.
///
/// The declarations come first: a type is named after its declaration, not
/// before it.
fn does(declarations: &str, query: &str) -> (Outcome, String) {
    // The answer is the program's *return value*: `if constexpr` keeps one
    // branch, so what the IR returns is what the compiler decided. Printing
    // would not show it -- both string literals are emitted either way.
    let program = format!(
        "import std;\n{declarations}\n\nfn main() int {{\n    if ({query}) @constexpr {{\n        return 1;\n    }}\n    return 0;\n}}\n"
    );
    compile(&program)
}

/// Whether the program's `main` returns `value`.
///
/// Checked inside `main` only: the standard library's own functions return
/// plenty of `0`s and `1`s, and a test that looked at the whole module would
/// pass whichever answer the query gave.
///
/// A program whose modules bring a `__setup__` block -- `std` has one, because
/// its devices are connected before anything runs -- has the `main` it wrote
/// emitted as `__ft_user_main`, with a wrapper under the name `main` around it.
/// The return value these tests are about is the one the program wrote.
fn main_returns(output: &str, value: i32) -> bool {
    let marker = if output.contains("define i32 @__ft_user_main()") {
        "define i32 @__ft_user_main()"
    } else {
        "define i32 @main()"
    };
    let Some(start) = output.find(marker) else {
        return false;
    };
    let body = &output[start..];
    let end = body.find("\n}").unwrap_or(body.len());
    body[..end].contains(&format!("ret i32 {value}"))
}

/// A type with `__inc__`, one without, and a trait that asks for it.
const INCREMENTABLE: &str = r#"
type Counts {
    _at int;
}

fn __init__(this &Counts, at int) void {
    this._at = at;
}

fn __inc__(this &Counts) void {
    this._at = this._at + 1;
}

type Plain {
    _v int;
}

traits Incrementable(it T) {
    ++it,
}
"#;

#[test]
fn a_trait_holds_when_every_requirement_compiles() {
    let (outcome, output) = does(INCREMENTABLE, "Incrementable(Counts)");
    assert_eq!(outcome, Outcome::Success, "{output}");
    // Only the live branch is emitted, so the answer is what the program
    // prints when it runs.
    assert!(main_returns(&output, 1), "{output}");
    assert!(!main_returns(&output, 0), "{output}");
}

#[test]
fn a_trait_does_not_hold_when_a_requirement_does_not() {
    let (outcome, output) = does(INCREMENTABLE, "Incrementable(Plain)");
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(main_returns(&output, 0), "{output}");
    assert!(!main_returns(&output, 1), "{output}");
}

#[test]
fn a_query_is_a_compile_time_constant() {
    // `if constexpr` needs a constant, so a query that folded to a run-time
    // value would not compile at all.
    let (outcome, output) = does(INCREMENTABLE, "Incrementable(Plain)");
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(!main_returns(&output, 1), "{output}");
    assert!(main_returns(&output, 0), "{output}");
}

#[test]
fn a_requirement_may_say_what_the_operation_yields() {
    // `*it -> !void` is "and it yields something". A type whose `__deref__`
    // returns a reference yields a value; a type without one leaves the
    // requirement unmet.
    let source = r#"
type Position {
    _at int;
}

fn __deref__(this &Position) &const int {
    return this._at;
}

traits Readable(it T) {
    *it -> !void,
}
"#;
    let (outcome, output) = does(source, "Readable(Position)");
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(main_returns(&output, 1), "{output}");

    let (outcome, output) = does(source, "Readable(int)");
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(!main_returns(&output, 1), "{output}");
    assert!(main_returns(&output, 0), "{output}");
}

#[test]
fn a_requirement_may_say_the_result_has_a_type() {
    // `-> bool` is the exact type: a comparison that returns something else
    // does not satisfy it.
    let source = r#"
type Lying {
    _v int;
}

fn __ne__(this &const Lying, other &const Lying) int {
    return 1;
}

type Truthful {
    _v int;
}

fn __ne__(this &const Truthful, other &const Truthful) bool {
    return false;
}

traits Comparable(it T) {
    it != it -> bool,
}
"#;
    let (outcome, output) = does(source, "Comparable(Truthful)");
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(main_returns(&output, 1), "{output}");

    let (outcome, output) = does(source, "Comparable(Lying)");
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(!main_returns(&output, 1), "{output}");
    assert!(main_returns(&output, 0), "{output}");
}

#[test]
fn a_trait_may_require_another() {
    let source = r#"
type Walks {
    _at int;
}

fn __init__(this &Walks, at int) void {
    this._at = at;
}

fn __inc__(this &Walks) void {
    this._at = this._at + 1;
}

traits Incrementable(it T) {
    ++it,
}

traits Walkable(it T) {
    it != it -> bool,
    Incrementable(it) -> true,
}
"#;
    // `Walkable` needs `!=` as well, which `Walks` does not have, so the
    // requirement that does hold is not enough.
    let (outcome, output) = does(source, "Walkable(Walks)");
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(!main_returns(&output, 1), "{output}");
    assert!(main_returns(&output, 0), "{output}");

    let (outcome, output) = does(source, "Incrementable(Walks)");
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(main_returns(&output, 1), "{output}");
}

#[test]
fn the_standard_library_provides_the_iterator_traits() {
    // `std/iterator.ftm` declares `Iterable` and `InputIterator`, and a query
    // reaches them under the umbrella's name as well as the bare one and the
    // submodule's.
    let source = r#"
type Walk {
    _at usize;
}

fn __inc__(this &Walk) void {
    this._at = this._at + 1;
}

fn __deref__(this &Walk) &const usize {
    return this._at;
}

fn __ne__(this &const Walk, other &const Walk) bool {
    return this._at != other._at;
}
"#;
    for query in [
        "InputIterator(Walk)",
        "std.InputIterator(Walk)",
        "std.iterator.InputIterator(Walk)",
    ] {
        let (outcome, output) = does(source, query);
        assert_eq!(outcome, Outcome::Success, "{query}: {output}");
        assert!(main_returns(&output, 1), "{query}: {output}");
    }
    // An `int` is not an iterator.
    let (outcome, output) = does(source, "InputIterator(int)");
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(!main_returns(&output, 1), "{output}");
    assert!(main_returns(&output, 0), "{output}");
}

#[test]
fn a_query_is_an_ordinary_bool() {
    // A query's value is known before the program runs, but its *type* is an
    // ordinary `bool`: it may stand in a runtime `if`, and in a `bool` binding,
    // not only in `if constexpr`. The type table is what a condition is lowered
    // from, and the query used to be missing from it.
    let source = r#"import std;

traits Incrementable(it T) {
    ++it,
}

type Counts {
    n int;
}

fn __inc__(this &Counts) Counts {
    this.n += 1;
    return this;
}

fn main() int {
    if (Incrementable(Counts)) {
        C.printf("yes\n");
    } else {
        C.printf("no\n");
    }
    answer bool = Incrementable(Counts);
    C.printf("%d\n", answer ? 1 : 0);
    return 0;
}
"#;
    assert_eq!(run_program("query-bool", source), "yes\n1\n");
}

#[test]
fn a_query_about_too_many_types_is_refused() {
    let source = r#"
traits One(it T) {
    ++it,
}
"#;
    let (outcome, output) = does(source, "One(int, int)");
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("is about 1 type"), "{output}");
}

#[test]
fn a_trait_is_not_checked_where_it_is_declared() {
    // A requirement is checked when a type is asked about it, so a trait may
    // name operations nothing has -- including one nothing asks about. That is
    // the price of not demanding a separate declaration of conformance: the
    // question is answered where it is asked.
    let source = r#"
traits Unused(it T) {
    it.frobnicate(),
}
"#;
    let (outcome, output) = does(source, "Unused(int)");
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(main_returns(&output, 0), "{output}");
}

#[test]
fn a_query_may_ask_about_the_type_of_a_value() {
    // A type with no name of its own -- `a.begin()` returns an
    // `*int`, the type `a.begin()` returns -- is asked about through a value of
    // it. The program
    // is built and run because `Array`'s constructor lives in the standard
    // library, which is a module like any other.
    let source = r#"import std;

fn main() int {
    a Array<int> = Array<int>();
    a.push(1);
    it auto = a.begin();
    if (InputIterator(type(it))) @constexpr {
        C.printf("input\n");
    }
    if (Iterable(type(a))) @constexpr {
        C.printf("iterable\n");
    }
    return 0;
}
"#;
    let output = run_program("value-query", source);
    assert_eq!(output, "input\niterable\n");
}

#[test]
fn the_iterator_hierarchy_is_a_chain_of_answers() {
    // Each trait says what the one above it can already do. An `Array`'s
    // iterator is a pointer, so it is the strongest member of the chain; a
    // minimal iterator -- `++`, `*`, `!=` and nothing else -- is only an input
    // iterator, and asking for more is answered with `false` rather than with
    // an error.
    let source = r#"import std;

type Walk {
    _at usize;
}

fn __inc__(this &Walk) void {
    this._at = this._at + 1;
}

fn __deref__(this &Walk) &const usize {
    return this._at;
}

fn __ne__(this &const Walk, other &const Walk) bool {
    return this._at != other._at;
}

fn main() int {
    a Array<int> = Array<int>();
    a.push(1);
    it auto = a.begin();
    p *int = &a[0];
    w Walk;
    if (InputIterator(type(it))) @constexpr {
        C.printf("array input\n");
    }
    if (ForwardIterator(type(it))) @constexpr {
        C.printf("array forward\n");
    }
    if (BidirectionalIterator(type(it))) @constexpr {
        C.printf("array bidirectional\n");
    }
    if (RandomAccessIterator(type(it))) @constexpr {
        C.printf("array random\n");
    }
    if (InputIterator(type(p))) @constexpr {
        C.printf("pointer input\n");
    }
    if (InputIterator(type(w))) @constexpr {
        C.printf("walk input\n");
    }
    if (ForwardIterator(type(w))) @constexpr {
        C.printf("walk forward\n");
    }
    return 0;
}
"#;
    let output = run_program("hierarchy", source);
    assert_eq!(
        output,
        "array input\narray forward\narray bidirectional\narray random\n\
         pointer input\nwalk input\n"
    );
}

/// A type whose positions can be walked, one whose `begin()` is a plain number,
/// and traits that ask which is which.
///
/// `-> true` is how a requirement that is itself a question is written: the
/// expression already folds to an answer, and the author states which one it
/// must be.
const VALUE_REQUIREMENT: &str = r#"
traits Walkable(it R) {
    it.begin() -> !void,
    it.end() -> !void,
    InputIterator(type(it.begin())) -> true,
}

traits NeverWalkable(it R) {
    it.begin() -> !void,
    InputIterator(type(it.begin())) -> false,
}

type Cells {
    _cells [2]int;
}

fn begin(this &Cells) *int {
    return &this._cells[0];
}

fn end(this &Cells) *int {
    return &this._cells[0] + 2;
}

type Num {
    _v int;
}

fn begin(this &Num) int {
    return this._v;
}

fn end(this &Num) int {
    return this._v;
}
"#;

#[test]
fn a_requirement_may_ask_for_a_value() {
    // `Cells.begin()` yields `*int`, which can be advanced, read and compared;
    // `Num.begin()` yields an `int`, which cannot be a position.
    let (outcome, output) = does(VALUE_REQUIREMENT, "Walkable(Cells)");
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(main_returns(&output, 1), "{output}");
    let (outcome, output) = does(VALUE_REQUIREMENT, "Walkable(Num)");
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(main_returns(&output, 0), "{output}");
}

#[test]
fn a_value_requirement_compares_the_folded_value() {
    // The same question written the other way round, so the comparison shows in
    // both directions: `-> false` holds for exactly the types `-> true` does
    // not.
    let (outcome, output) = does(VALUE_REQUIREMENT, "NeverWalkable(Cells)");
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(main_returns(&output, 0), "{output}");
    let (outcome, output) = does(VALUE_REQUIREMENT, "NeverWalkable(Num)");
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(main_returns(&output, 1), "{output}");
}

#[test]
fn the_standard_library_requires_the_position_to_be_an_iterator() {
    // What a range loop needs is not just `begin()` and `end()` but a position
    // it can walk, which `std.Iterable` states as a question about the type
    // `begin()` yields.
    let (outcome, output) = does(VALUE_REQUIREMENT, "Iterable(Cells)");
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(main_returns(&output, 1), "{output}");
    let (outcome, output) = does(VALUE_REQUIREMENT, "Iterable(Num)");
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(main_returns(&output, 0), "{output}");
}

#[test]
fn a_query_may_name_a_written_type() {
    // `type(expr)` asks what a value's type is. A type that no value has, or
    // that cannot be written as an expression at all -- `Array<int>` -- is
    // written directly: it is what the query is about, so it is a type in a
    // place that wants one.
    let (outcome, output) = does(VALUE_REQUIREMENT, "Iterable(Array<int>)");
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(main_returns(&output, 1), "{output}");
    let (outcome, output) = does(VALUE_REQUIREMENT, "Iterable(String)");
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(main_returns(&output, 1), "{output}");
    // A range is not a position, however its positions are laid out.
    let (outcome, output) = does(VALUE_REQUIREMENT, "RandomAccessIterator(Array<int>)");
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(main_returns(&output, 0), "{output}");
}

#[test]
fn a_written_type_is_not_a_value() {
    // The same node is refused where a value belongs: a type is a type.
    let source = r#"import std;

fn twice(n int) int {
    return n * 2;
}

fn main() int {
    return twice(Array<int>);
}
"#;
    let (outcome, printed) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{printed}");
    assert!(printed.contains("a type is not a value"), "{printed}");
}

#[test]
fn an_ordinary_argument_that_looks_like_a_type_is_still_an_expression() {
    // `f(a < b)` begins the same way a written type does, so the type grammar
    // is only a guess that must end where an argument ends: `a < b` is a
    // comparison, and `a < b > (c)` is one too.
    let source = r#"import std;

fn positive(n bool) int {
    if (n) {
        C.printf("less\n");
        return 1;
    }
    return 0;
}

fn main() int {
    a int = 1;
    b int = 2;
    return positive(a < b);
}
"#;
    let (outcome, printed) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{printed}");
    // And it means what it says: the argument is the comparison, which is
    // true, so the branch in `positive` runs.
    assert_eq!(run_program("comparison-argument", source), "less\n");
}

#[test]
fn a_reverse_position_is_a_random_access_position() {
    // Walking backwards does not make a position weaker: it can still be moved
    // by a count in one step, read at an offset, and ordered -- all of it
    // mirrored, so that `it + n` is `n` steps towards the front. What it does
    // *not* have is an `it++` that the loop can rely on copying, so it is a
    // random-access position without being a forward one, which is what C++'s
    // reverse iterators are as well.
    let source = r#"import std;

fn main() int {
    a Array<int> = Array<int>();
    a.push(10);
    a.push(20);
    a.push(30);
    r Array<int>.ReverseIterator = a.rbegin();
    C.printf("random %d\n", RandomAccessIterator(type(r)));
    C.printf("%d %d %d %d\n", r[0], r[1], (r + 1)[0], (r - 1)[0]);
    C.printf("ordered %d %d\n", (r < r + 1) as int, (r > r + 1) as int);
    C.printf("distance %ld\n", (r - (r + 2)) as isize);
    r += 1;
    C.printf("after += 1: %d\n", r[0]);
    r -= 1;
    C.printf("after -= 1: %d\n", r[0]);
    return 0;
}
"#;
    let output = run_program("reverse", source);
    assert_eq!(
        output,
        "random 1\n30 20 20 0\nordered 1 0\ndistance -2\nafter += 1: 20\nafter -= 1: 30\n"
    );
}

#[test]
fn an_interface_may_declare_requirements_instead_of_signatures() {
    // One construct, one spelling: a contract stated as requirements is what a
    // trait was, and a program writes it with `interface` now. Swapping the
    // word in a trait declaration is legal and means the same thing.
    let declarations = r#"type Counts {
    n int;
}

fn __inc__(this &Counts) Counts {
    this.n += 1;
    return this;
}

interface Incrementable(it T) {
    ++it,
}
"#;
    let (outcome, output) = does(declarations, "Incrementable(Counts)");
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(main_returns(&output, 1), "{output}");
}

/// A type whose `push` takes a `c8`, and one whose `save` may raise.
const ARGUMENTS_AND_RAISES: &str = r#"
type Buffer {
    at int;
}

fn push(this &Buffer, byte c8) int {
    this.at = byte as int;
    return this.at;
}

type File {
    at int;
}

fn save(this &File, amount int) void except (amount < 0) {
    this.at = amount;
}
"#;

#[test]
fn a_requirement_argument_is_a_type_that_has_to_match() {
    // The argument of a restriction states the parameter's type, so a `c8`
    // method does not satisfy `it.push(int)`. The requirement fails because the
    // call does not compile, which is what a restriction means.
    let (outcome, output) = does(
        &format!("{ARGUMENTS_AND_RAISES}\ntraits P(it T) {{ it.push(int) -> int, }}"),
        "P(Buffer)",
    );
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(main_returns(&output, 0), "{output}");

    let (outcome, output) = does(
        &format!("{ARGUMENTS_AND_RAISES}\ntraits Q(it T) {{ it.push(c8) -> int, }}"),
        "Q(Buffer)",
    );
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(main_returns(&output, 1), "{output}");
}

#[test]
fn a_restriction_says_whether_the_operation_may_raise() {
    // `??` is how a restriction says the operation returns through the
    // exception path. Both directions are a mismatch, because whether a call
    // may raise is part of the call's type.
    let raising = format!("{ARGUMENTS_AND_RAISES}\ntraits R(it T) {{ it.save(int)?? -> void, }}");
    let (outcome, output) = does(&raising, "R(File)");
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(main_returns(&output, 1), "{output}");

    let quiet = format!("{ARGUMENTS_AND_RAISES}\ntraits R(it T) {{ it.save(int) -> void, }}");
    let (outcome, output) = does(&quiet, "R(File)");
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(main_returns(&output, 0), "{output}");

    let marked = format!("{ARGUMENTS_AND_RAISES}\ntraits R(it T) {{ it.save(int)?? -> void, }}");
    let (outcome, output) = does(&marked, "R(Buffer)");
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(main_returns(&output, 0), "{output}");
}
