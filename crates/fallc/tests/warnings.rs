//! Warnings: what the compiler noticed, without refusing the program.
//!
//! A warning is a remark about a program it accepts. The interesting parts are
//! therefore which remarks are made, and which of them the driver lets the user
//! turn off: `-Wno-<group>` silences a group, `-W<group>` brings it back, and
//! `-Werror` makes the remarks fatal so that a build can insist on them.

use fallc::Outcome;
use fallc::cli::{Command, EmitKind, Options};
use fallc::pipeline::compile_text_with_warnings;
use fallc_diag::WarningPolicy;

/// Compiles `source` and returns the outcome plus everything printed.
fn compile(source: &str) -> (Outcome, String) {
    compile_text_with_warnings(
        "test.ft",
        source,
        &[EmitKind::Llvm],
        &WarningPolicy::new(),
    )
}

/// Compiles `source` with `flags` applied, the way the driver would.
fn compile_with(source: &str, flags: &[&str]) -> (Outcome, String) {
    let options = Options::parse(&["test.ft".to_owned(), "--check".to_owned()]
        .into_iter()
        .chain(flags.iter().map(|flag| (*flag).to_owned()))
        .collect::<Vec<_>>())
    .expect("the flags are well formed");
    let Command::Compile { warnings, .. } = &options.command else {
        panic!("expected a compile command");
    };
    compile_text_with_warnings("test.ft", source, &[EmitKind::Llvm], warnings)
}

/// An explicit move, written here because the in-memory pipeline does not load
/// the module `std.move` is declared in -- and because a test that needs a move
/// should not depend on which module supplies it.
const MOVE: &str = r#"template <T type>
fn move(value &T) &&T {
    return value;
}

"#;

/// A program with one unused local, one intentionally unused one, and one that
/// is read.
const UNUSED: &str = r#"import std;

fn main() int {
    read int = 1;
    never int = 2;
    _intended int = 3;
    return read;
}
"#;

#[test]
fn a_local_that_is_never_read_is_reported() {
    let (outcome, printed) = compile(UNUSED);
    assert_eq!(outcome, Outcome::Success, "{printed}");
    assert!(printed.contains("S0140"), "{printed}");
    assert!(printed.contains("`never` is never read"), "{printed}");
    // The suffix `_intended` says the name is deliberately unused, and the
    // precedent for a name that is read is no warning at all.
    assert!(!printed.contains("`_intended`"), "{printed}");
    assert!(!printed.contains("`read` is never read"), "{printed}");
}

#[test]
fn a_parameter_that_is_never_read_is_not_reported() {
    // A signature is not a mistake: a function may have to accept an argument
    // it does not use, and every such interface would otherwise warn.
    let source = r#"fn ignore(n int) int {
    return 0;
}

fn main() int {
    return ignore(1);
}
"#;
    let (outcome, printed) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{printed}");
    assert!(!printed.contains("S0140"), "{printed}");
}

#[test]
fn a_statement_after_one_that_leaves_can_never_run() {
    let source = r#"import std;

fn main() int {
    return 0;
    C.printf("never\n");
    C.printf("also never\n");
}
"#;
    let (outcome, printed) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{printed}");
    assert!(printed.contains("S0141"), "{printed}");
    // One remark per block, not one per statement behind the `return`.
    assert_eq!(printed.matches("S0141").count(), 1, "{printed}");
    assert!(printed.contains("this statement can never run"), "{printed}");
}

#[test]
fn a_block_that_does_not_leave_says_nothing() {
    let source = r#"import std;

fn f(n int) int {
    if (n < 0) {
        return -n;
    }
    return n;
}

fn main() int {
    return f(-2);
}
"#;
    let (outcome, printed) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{printed}");
    assert!(!printed.contains("S0141"), "{printed}");
}

#[test]
fn a_group_can_be_turned_off_and_back_on() {
    let (outcome, printed) = compile_with(UNUSED, &["-Wno-unused"]);
    assert_eq!(outcome, Outcome::Success, "{printed}");
    assert!(!printed.contains("S0140"), "{printed}");
    // Every other group is still on: turning one off is not turning them all
    // off.
    let (outcome, printed) = compile_with(UNUSED, &["-Wno-unused", "-Wall"]);
    assert_eq!(outcome, Outcome::Success, "{printed}");
    assert!(printed.contains("S0140"), "{printed}");
}

#[test]
fn warnings_become_errors_when_asked() {
    let (outcome, printed) = compile_with(UNUSED, &["-Werror"]);
    assert_eq!(outcome, Outcome::Failure, "{printed}");
    // Reported as an error, which is what makes a build that refuses warnings
    // possible: the program is understood, and the remark is the reason.
    assert!(printed.contains("error[S0140]"), "{printed}");
    assert!(!printed.contains("warning[S0140]"), "{printed}");
}

#[test]
fn a_group_that_was_turned_off_stays_off_under_werror() {
    // `-Werror` is about the warnings that are reported, not about the ones the
    // user asked not to hear.
    let (outcome, printed) = compile_with(UNUSED, &["-Wno-unused", "-Werror"]);
    assert_eq!(outcome, Outcome::Success, "{printed}");
    assert!(!printed.contains("S0140"), "{printed}");
}

#[test]
fn an_unknown_warning_is_refused_and_the_known_ones_are_listed() {
    let error = Options::parse(&[
        "test.ft".to_owned(),
        "-Wno-nonsense".to_owned(),
    ])
    .expect_err("an unknown group is refused");
    assert!(error.contains("unknown warning"), "{error}");
    assert!(error.contains("unused"), "{error}");
    assert!(error.contains("unreachable"), "{error}");
    assert!(error.contains("useless-raise-mark"), "{error}");
}

/// Compiles `source` and returns just the warning sentences, so a test can
/// count them without matching the source echoes.
fn warnings_only(source: &str) -> Vec<String> {
    let (_, printed) = compile(source);
    printed
        .lines()
        .filter(|line| line.starts_with("warning["))
        .map(str::to_owned)
        .collect()
}

#[test]
fn a_parameter_that_is_never_read_is_its_own_group() {
    let source = r#"fn ignore(n int, _unused int) int {
    return 0;
}

fn main() int {
    return ignore(1, 2);
}
"#;
    let (outcome, printed) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{printed}");
    assert!(printed.contains("S0142"), "{printed}");
    assert!(printed.contains("`n` is never read"), "{printed}");
    // `_unused` is the convention for a parameter kept for its signature.
    assert!(!printed.contains("`_unused`"), "{printed}");

    // An `extern` function has no body, so its parameters are never read and
    // never could be: that is not a remark about this program.
    let external = "@extern_C fn puts(text *const c8) int;\n\nfn main() int {\n    return puts(\"hi\");\n}\n";
    let (outcome, printed) = compile(external);
    assert_eq!(outcome, Outcome::Success, "{printed}");
    assert!(!printed.contains("S0142"), "{printed}");
}

#[test]
fn a_local_that_hides_another_binding_is_reported() {
    let source = r#"import std;

fn main() int {
    x int = 1;
    {
        x int = 2;
        C.printf("%d\n", x);
    }
    return x;
}
"#;
    let (outcome, printed) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{printed}");
    assert!(printed.contains("S0143"), "{printed}");
    assert!(printed.contains("`x` shadows an outer binding"), "{printed}");
}

#[test]
fn a_module_level_name_shadowed_by_a_local_is_not_reported() {
    // A local called `printf` or `Array` hides a module's name, and that is
    // ordinary: the warning is about two bindings of one function.
    let source = r#"import std;

LIMIT const int = 4;

fn main() int {
    LIMIT int = 2;
    return LIMIT;
}
"#;
    let (outcome, printed) = compile(source);
    // The local hides a *constant*, which is a module-level name.
    assert!(!printed.contains("S0143"), "{printed}");
    assert_eq!(outcome, Outcome::Success, "{printed}");
}

#[test]
fn a_condition_written_as_a_literal_is_reported() {
    let source = r#"import std;

ON const bool = true;

fn main() int {
    if (true) {
        C.printf("always\n");
    }
    while (false) {
        C.printf("never\n");
    }
    if (ON) {
        return 1;
    }
    while (true) {
        break;
    }
    return 0;
}
"#;
    let warnings = warnings_only(source);
    // `if (true)` and `while (false)`: two remarks.
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    assert!(warnings.iter().any(|line| line.contains("always true")), "{warnings:?}");
    assert!(warnings.iter().any(|line| line.contains("loop can never run")), "{warnings:?}");
    // A `const` is a switch the author can flip, and `while (true)` is how an
    // infinite loop is written.
    assert!(
        !warnings.iter().any(|line| line.contains("S0144") && line.contains("ON")),
        "{warnings:?}"
    );
}

#[test]
fn an_assignment_where_a_condition_is_wanted_is_reported() {
    let source = r#"fn f() int {
    return 1;
}

fn main() int {
    x int = 0;
    if (x = f()) {
        return x;
    }
    return 0;
}
"#;
    let (outcome, printed) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{printed}");
    assert!(printed.contains("S0145"), "{printed}");
    assert!(printed.contains("this condition assigns"), "{printed}");
}

#[test]
fn a_value_compared_with_itself_is_reported() {
    let source = r#"fn main() int {
    a int = 1;
    if (a < a) {
        return 1;
    }
    return 0;
}
"#;
    let (outcome, printed) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{printed}");
    assert!(printed.contains("S0146"), "{printed}");
    assert!(printed.contains("compares a value with itself"), "{printed}");
}

#[test]
fn a_trait_requirement_may_compare_a_value_with_itself() {
    // `it < it` in a trait body is not a comparison anybody runs: it says the
    // two positions can be ordered, and the requirement would not compile
    // without it.
    let source = r#"type Down {
    _v int;
}

fn __lt__(this &const Down, other &const Down) bool {
    return this._v < other._v;
}

traits Ordered(it T) {
    it < it -> bool,
}

fn main() int {
    if (Ordered(Down)) @constexpr {
        return 1;
    }
    return 0;
}
"#;
    let (outcome, printed) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{printed}");
    assert!(!printed.contains("S0146"), "{printed}");
}

#[test]
fn a_division_by_a_constant_zero_is_reported() {
    let source = r#"import std;

fn main() int {
    C.printf("%d\n", 4 / 0);
    return 0;
}
"#;
    let (outcome, printed) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{printed}");
    assert!(printed.contains("S0147"), "{printed}");
    assert!(printed.contains("divides by a constant zero"), "{printed}");
}

#[test]
fn a_value_read_after_a_move_is_reported() {
    let source = &format!(
        r#"import std;

{MOVE}fn main() int {{
    a String = "text";
    b String = move(a);
    C.printf("%s\n", b.data());
    C.printf("%s\n", a.data());
    return 0;
}}
"#
    );
    let (outcome, printed) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{printed}");
    assert!(printed.contains("S0148"), "{printed}");
    assert!(printed.contains("`a` is read after it was moved from"), "{printed}");
    // The move itself is not a read, and `b` is not empty.
    assert!(!printed.contains("`b` is read"), "{printed}");
}

#[test]
fn a_moved_from_value_may_be_given_a_new_one() {
    // Assignment gives the name a value again: reading it after the assignment
    // is ordinary, and only the read *before* it is a mistake.
    let source = &format!(
        r#"import std;

{MOVE}fn main() int {{
    a String = "text";
    b String = move(a);
    a = String("again");
    C.printf("%s %s\n", a.data(), b.data());
    return 0;
}}
"#
    );
    let (outcome, printed) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{printed}");
    assert!(!printed.contains("S0148"), "{printed}");
}

#[test]
fn a_comment_can_silence_a_warning() {
    // `// no warning` on the statement, or on the line above it, is the
    // reader saying they know. Both forms are here, and the third binding has
    // no comment and is still reported.
    let source = r#"fn main() int {
    silenced int = 1;      // no warning
    // kept for the interface, not read -- no warning
    also int = 2;
    reported int = 3;
    return 0;
}
"#;
    let (outcome, printed) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{printed}");
    assert!(!printed.contains("`silenced`"), "{printed}");
    assert!(!printed.contains("`also`"), "{printed}");
    assert!(printed.contains("`reported` is never read"), "{printed}");
}

#[test]
fn only_a_comment_can_silence_a_warning() {
    // A string that happens to contain the words is not a comment: it does not
    // run to the end of the line, so the marker is not there.
    let source = r#"import std;

fn main() int {
    never int = 1;
    C.printf("text // no warning\n");
    return 0;
}
"#;
    let (outcome, printed) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{printed}");
    assert!(printed.contains("`never` is never read"), "{printed}");
}

#[test]
fn a_silenced_warning_is_not_an_error_under_werror() {
    // The comment is a request, and `-Werror` is a request too; the one about
    // this line is more specific.
    let source = r#"fn main() int {
    silenced int = 1;      // no warning
    return 0;
}
"#;
    let (outcome, printed) = compile_with(source, &["-Werror"]);
    assert_eq!(outcome, Outcome::Success, "{printed}");
    assert!(!printed.contains("S0140"), "{printed}");
}

#[test]
fn the_explanations_are_behind_a_flag() {
    // A diagnostic is read as "where" and "what" far more often than as "why",
    // so the why is what `-Wverbose` adds: the message, the location and the
    // labels are always there, the `= note:` lines are not.
    let source = r#"type A {
    a int @align(f(2));
}

fn f(n int) int {
    return n;
}

fn main() int {
    return 0;
}
"#;
    let (outcome, printed) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{printed}");
    assert!(printed.contains("needs a value the compiler knows"), "{printed}");
    assert!(!printed.contains("= note:"), "{printed}");

    let (outcome, verbose) = compile_with(source, &["-Wverbose"]);
    assert_eq!(outcome, Outcome::Failure, "{verbose}");
    assert!(verbose.contains("= note:"), "{verbose}");
    assert!(
        verbose.contains("so it is fixed before the program runs"),
        "{verbose}"
    );
}

#[test]
fn a_comment_can_name_what_it_silences() {
    // On its own the marker silences everything on the line; with a colon it
    // names a code or a group, and a name that matches neither silences
    // nothing.
    let source = r#"fn main() int {
    by_code int = 1;      // no warning: S0140
    by_group int = 2;     // no warning: unused
    wrong_code int = 3;   // no warning: S0145
    return 0;
}
"#;
    let (outcome, printed) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{printed}");
    assert!(!printed.contains("`by_code`"), "{printed}");
    assert!(!printed.contains("`by_group`"), "{printed}");
    assert!(printed.contains("`wrong_code` is never read"), "{printed}");
}

#[test]
fn a_comment_that_only_mentions_warnings_silences_nothing() {
    // A sentence about warnings is a sentence: the marker has to be the whole
    // comment, or follow `--`.
    let source = r#"fn main() int {
    prose int = 1;        // this mention of no warning is prose
    dashed int = 2;       // kept on purpose -- no warning
    return 0;
}
"#;
    let (outcome, printed) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{printed}");
    assert!(printed.contains("`prose` is never read"), "{printed}");
    assert!(!printed.contains("`dashed`"), "{printed}");
}

#[test]
fn a_trailing_comment_does_not_reach_the_next_line() {
    // The line-above form is a comment on a line of its own; a comment trailing
    // the statement before cannot silence the statement after it.
    let source = r#"fn main() int {
    first int = 1;        // no warning: unused
    second int = 2;
    return 0;
}
"#;
    let (outcome, printed) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{printed}");
    assert!(printed.contains("`second` is never read"), "{printed}");
}

#[test]
fn a_warning_carries_its_explanation_and_names_its_group() {
    // The line a warning is about is the same either way; what `-Wverbose` adds
    // is why the warning is made, and which group names it -- the group being
    // what `-Wno-<group>` and `// no warning: <group>` take.
    let source = r#"fn main() int {
    never int = 1;
    return 0;
}
"#;
    let (_, plain) = compile(source);
    assert!(plain.contains("warning[S0140]:"), "{plain}");
    assert!(!plain.contains("(unused)"), "{plain}");
    assert!(!plain.contains("= note:"), "{plain}");

    let (_, verbose) = compile_with(source, &["-Wverbose"]);
    assert!(verbose.contains("warning[S0140] (unused):"), "{verbose}");
    assert!(verbose.contains("= note:"), "{verbose}");
    // The explanation says what to do, not only that something is wrong.
    assert!(verbose.contains("write `_`"), "{verbose}");
}
