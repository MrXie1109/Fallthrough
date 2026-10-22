//! `for (x T : range)`, which is C++'s range loop.
//!
//! The loop asks the range for `begin()` and `end()`, compares the position
//! with the end using `!=`, advances it with `++`, and reads it with `*`. Every
//! one of those is an operation the language already has, so the loop is a
//! *pattern of ordinary code* rather than a rule of its own -- which is what
//! makes any type iterable by providing five methods and nothing else.
//!
//! The programs are compiled and run rather than inspected, because what a loop
//! does is visible only when it runs: which elements it visits, in what order,
//! and whether a write through the loop variable reached the container.
//!
//! The `else` clause follows the rule the other loops use: it runs when the
//! loop ends by its condition rather than by `break`.

use std::path::PathBuf;

use fallc::cli::Options;
use fallc::pipeline::run;

/// A directory of scratch sources, removed when it goes out of scope.
struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-for-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a temporary directory can be created");
        Scratch { directory }
    }

    /// Compiles the source into an executable and runs it.
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
        assert!(
            output.status.success(),
            "`{label}` failed: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    /// Compiles the source in one file and returns what the compiler printed.
    fn check(&self, label: &str, source: &str) -> (fallc::Outcome, String) {
        fallc::pipeline::compile_text(
            &format!("{label}.ft"),
            source,
            &[fallc::cli::EmitKind::Llvm],
        )
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn an_array_is_iterated_in_order() {
    let source = r#"import std;

fn main() int {
    a Array<int> = Array<int>();
    a.push(10);
    a.push(20);
    a.push(30);
    for (x int : a) {
        C.printf("%d\n", x);
    }
    return 0;
}
"#;
    let scratch = Scratch::new("array");
    let output = scratch.run("array", source);
    assert_eq!(output, "10\n20\n30\n");
}

#[test]
fn an_empty_array_is_iterated_not_at_all() {
    // The end of a sequence is one past the last element, so an empty one
    // starts already finished: `begin() == end()`.
    let source = r#"import std;

fn main() int {
    a Array<int> = Array<int>();
    for (x int : a) {
        C.printf("never\n");
    } else {
        C.printf("done\n");
    }
    return 0;
}
"#;
    let scratch = Scratch::new("empty");
    let output = scratch.run("empty", source);
    assert_eq!(output, "done\n");
}

#[test]
fn a_string_yields_its_characters() {
    // The bytes of the text, including the inline case: a short string keeps
    // its bytes inside the object, and the iterator reads them there.
    let source = r#"import std;

fn main() int {
    s String = "hi!";
    for (c char : s) {
        C.printf("%c", c);
    }
    C.printf("\n");
    return 0;
}
"#;
    let scratch = Scratch::new("string");
    let output = scratch.run("string", source);
    assert_eq!(output, "hi!\n");
}

#[test]
fn a_reference_loop_variable_writes_the_element() {
    // `x &int` names the element rather than a copy of it, so assigning to `x`
    // writes the array. This is the same `&T` binding the language has
    // everywhere else, applied to what the iterator's `*` returned.
    let source = r#"import std;

fn main() int {
    a Array<int> = Array<int>();
    a.push(10);
    a.push(20);
    a.push(30);
    for (x &int : a) {
        x = x * 2;
    }
    for (x int : a) {
        C.printf("%d\n", x);
    }
    return 0;
}
"#;
    let scratch = Scratch::new("reference");
    let output = scratch.run("reference", source);
    assert_eq!(output, "20\n40\n60\n");
}

#[test]
fn break_continue_and_else_behave_as_they_do_in_any_loop() {
    let source = r#"import std;

fn main() int {
    a Array<int> = Array<int>();
    a.push(1);
    a.push(2);
    a.push(3);
    for (x int : a) {
        if (x == 2) {
            continue;
        }
        C.printf("kept %d\n", x);
    }
    for (x int : a) {
        if (x == 2) {
            break;
        }
        C.printf("before %d\n", x);
    } else {
        C.printf("never\n");
    }
    for (x int : a) {
        C.printf("all %d\n", x);
    } else {
        C.printf("finished\n");
    }
    return 0;
}
"#;
    let scratch = Scratch::new("control");
    let output = scratch.run("control", source);
    assert_eq!(
        output,
        "kept 1\nkept 3\nbefore 1\nall 1\nall 2\nall 3\nfinished\n"
    );
}

#[test]
fn a_loop_variable_without_a_written_type_is_a_value() {
    // `auto` deduces the initialiser's type, and a reference read as a value is
    // the value it names: `x auto = *it` copies the element, as `x int` does.
    // Deducting the *reference* would have made `x = 5` write the array, which
    // no one looking at `auto` expects.
    let source = r#"import std;

fn main() int {
    a Array<int> = Array<int>();
    a.push(4);
    for (x auto : a) {
        x = x + 1;
        C.printf("%d\n", x);
    }
    C.printf("%d\n", a[0]);
    return 0;
}
"#;
    let scratch = Scratch::new("auto");
    let output = scratch.run("auto", source);
    assert_eq!(output, "5\n4\n");
}

#[test]
fn a_type_may_be_made_iterable_by_five_methods() {
    // Nothing about `Array` is special: a program's own type is iterable by
    // declaring `begin`, `end`, `__inc__`, `__deref__` and `__ne__`. The loop
    // is the pattern those five form, and the language has no list of iterable
    // types.
    let source = r#"import std;

// Counts from `at` up to `stop`, one at a time.
type Countdown {
    _at int;
    _stop int;
}

fn __init__(this &Countdown, stop int) void {
    this._at = 0;
    this._stop = stop;
}

type CountdownPosition {
    _at int;
    _stop int;
}

fn begin(this &Countdown) CountdownPosition {
    return CountdownPosition{this._at, this._stop};
}

fn end(this &Countdown) CountdownPosition {
    return CountdownPosition{this._stop, this._stop};
}

fn __inc__(this &CountdownPosition) void {
    this._at = this._at + 1;
}

fn __deref__(this &CountdownPosition) &const int {
    return this._at;
}

fn __ne__(this &const CountdownPosition, other &const CountdownPosition) bool {
    return this._at != other._at;
}

fn main() int {
    for (n int : Countdown(4)) {
        C.printf("%d\n", n);
    }
    return 0;
}
"#;
    let scratch = Scratch::new("own");
    let output = scratch.run("own", source);
    assert_eq!(output, "0\n1\n2\n3\n");
}

#[test]
fn a_range_that_is_a_value_is_evaluated_once() {
    // The loop calls two operations on its range, so a range that is a *value*
    // is bound once rather than evaluated twice. The counter proves it: two
    // calls to `make()` would print `made` twice.
    let source = r#"import std;

fn make() Countdown {
    // Printed by the *caller's* loop if it is asked twice, which is what the
    // test is about.
    C.printf("made\n");
    return Countdown(2);
}

type Countdown {
    _at int;
    _stop int;
}

type CountdownPosition {
    _at int;
    _stop int;
}

fn __init__(this &Countdown, stop int) void {
    this._at = 0;
    this._stop = stop;
}

fn begin(this &Countdown) CountdownPosition {
    return CountdownPosition{this._at, this._stop};
}

fn end(this &Countdown) CountdownPosition {
    return CountdownPosition{this._stop, this._stop};
}

fn __inc__(this &CountdownPosition) void {
    this._at = this._at + 1;
}

fn __deref__(this &CountdownPosition) &const int {
    return this._at;
}

fn __ne__(this &const CountdownPosition, other &const CountdownPosition) bool {
    return this._at != other._at;
}

fn main() int {
    for (n int : make()) {
        C.printf("%d\n", n);
    }
    return 0;
}
"#;
    let scratch = Scratch::new("once");
    let output = scratch.run("once", source);
    assert_eq!(output, "made\n0\n1\n");
}

#[test]
fn two_loops_in_one_function_do_not_share_their_hidden_names() {
    // The names the loop introduces are made up by the compiler, so two loops
    // in one scope must each get their own; a collision would make the second
    // loop's position shadow the first's.
    let source = r#"import std;

fn main() int {
    a Array<int> = Array<int>();
    a.push(1);
    a.push(2);
    for (x int : a) {
        C.printf("a%d\n", x);
    }
    for (x int : a) {
        C.printf("b%d\n", x);
    }
    return 0;
}
"#;
    let scratch = Scratch::new("twice");
    let output = scratch.run("twice", source);
    assert_eq!(output, "a1\na2\nb1\nb2\n");
}

#[test]
fn a_range_without_the_operations_names_the_one_it_wants() {
    let source = r#"import std;

fn main() int {
    n int = 3;
    for (x int : n) {
        C.printf("%d\n", x);
    }
    return 0;
}
"#;
    let scratch = Scratch::new("not-iterable");
    let (outcome, output) = scratch.check("not-iterable", source);
    assert_eq!(outcome, fallc::Outcome::Failure, "{output}");
    assert!(output.contains("has no method `begin`"), "{output}");
    // The names the loop makes up are not the program's to see.
    assert!(!output.contains("$it"), "{output}");
    assert!(!output.contains("$end"), "{output}");
}

#[test]
fn a_const_container_can_be_walked() {
    // A function takes a container by `&const`, which is how a read-only view is
    // written. The loop still asks for `begin()`/`end()`, so the read-only pair
    // has to exist -- without it a helper that only reads its argument could not
    // iterate it at all.
    let source = r#"import std;

fn count_vowels(text &const String) int {
    total int = 0;
    for (c char : text) {
        if (c == 'a' || c == 'e' || c == 'i' || c == 'o' || c == 'u') {
            total += 1;
        }
    }
    return total;
}

fn sum(values &const Array<int>) int {
    total int = 0;
    for (value int : values) {
        total += value;
    }
    return total;
}

fn main() int {
    text String = "education";
    xs Array<int> = Array<int>(3, 1);
    C.printf("%d %d\n", count_vowels(text), sum(xs));
    return 0;
}
"#;
    let scratch = Scratch::new("const");
    assert_eq!(scratch.run("const", source), "5 3\n");
}

#[test]
fn an_array_walks_backwards_from_the_end() {
    // The reverse positions `collections.md` names: `rbegin()` is where a
    // forward walk stops, `*it` is the element before the position, and `++`
    // moves towards the front.
    let source = r#"import std;

fn main() int {
    xs Array<int> = Array<int>(3, 0);
    xs[0] = 1;
    xs[1] = 2;
    xs[2] = 3;
    it Array<int>.ReverseIterator = xs.rbegin();
    stop Array<int>.ReverseIterator = xs.rend();
    while (it != stop) {
        C.printf("%d ", *it);
        ++it;
    }
    C.printf("\n");
    return 0;
}
"#;
    let scratch = Scratch::new("reverse");
    assert_eq!(scratch.run("reverse", source), "3 2 1 \n");
}

#[test]
fn a_loop_over_strings_compares_positions_not_elements() {
    // The loop's condition is `$it != $end` over two `*String`s. Asking
    // `String`'s `__ne__` compared the *elements*, so the walk stopped as soon
    // as two of them happened to compare equal -- and an empty element compares
    // equal to whatever follows it. Two pointers compare as addresses.
    let source = r#"import std;

fn main() int {
    parts Array<String>;
    parts.push(String("a"));
    parts.push(String("b"));
    parts.push(String(""));
    parts.push(String("c"));
    count int = 0;
    for (piece String : parts) {
        C.printf("%d [%s]\n", count, piece.data());
        count += 1;
    }
    C.printf("count %d\n", count);
    return 0;
}
"#;
    let scratch = Scratch::new("strings");
    assert_eq!(
        scratch.run("strings", source),
        "0 [a]\n1 [b]\n2 []\n3 [c]\ncount 4\n"
    );
}
