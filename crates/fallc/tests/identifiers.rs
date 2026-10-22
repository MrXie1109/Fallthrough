//! Identifiers: the Unicode letter classes, and the two contextual keywords.
//!
//! A name may be written in any script UAX #31 allows, and no normalization is
//! applied: two names that look alike are the same name only if they are the
//! same characters. `dynamic` and `delete` are not reserved words -- they mean
//! something in exactly one position each -- so they remain usable as names.

use fallc::Outcome;
use fallc::cli::EmitKind;
use fallc::pipeline::compile_text;

/// Compiles `source` and returns the outcome plus everything printed.
fn compile(source: &str) -> (Outcome, String) {
    compile_text("test.ft", source, &[EmitKind::Llvm])
}

#[test]
fn a_name_may_be_written_in_any_script() {
    let source = r#"fn 平方(n int) int {
    return n * n;
}

fn main() int {
    数 int = 平方(3);
    数值 int = 数 + 1;
    δelta int = 2;
    café int = 3;
    return 数值 + δelta + café;
}
"#;
    let (outcome, printed) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{printed}");
}

#[test]
fn a_letter_with_a_combining_mark_still_continues_a_name() {
    // `é` written as `e` plus a combining acute is a valid identifier: the
    // mark is `XID_Continue`.
    let source = "fn main() int {\n    cafe\u{301} int = 4;\n    return cafe\u{301};\n}\n";
    let (outcome, printed) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{printed}");
}

#[test]
fn two_spellings_that_look_alike_are_two_names() {
    // No normalization: `é` (U+00E9) and `e` + U+0301 are different names, so
    // naming one and reading the other is an unknown name rather than a hit.
    let source = "fn main() int {\n    caf\u{e9} int = 4;\n    return cafe\u{301};\n}\n";
    let (outcome, printed) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{printed}");
    assert!(printed.contains("cannot find"), "{printed}");
}

#[test]
fn dynamic_and_delete_are_reserved_words() {
    // Both are reserved: `dynamic` is the link kind, `delete` the deleted
    // definition marker, and the language has no contextual keywords.
    let source = "fn dynamic(n int) int {\n    return n + 1;\n}\nfn main() int {\n    return dynamic(1);\n}\n";
    let (outcome, printed) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{printed}");
}

#[test]
fn the_removed_keywords_are_ordinary_names() {
    // `alignas`, `constexpr`, `extern`, `thread_local` and `static` stopped
    // being reserved when their declarations became attributes, so a program
    // may use them as names. `export` is a keyword again: it introduces a
    // declaration, so it is not in this list.
    let source = "fn main() int {\n    alignas int = 1;\n    constexpr int = 2;\n    extern int = 4;\n    thread_local int = 5;\n    static int = 6;\n    return alignas + constexpr + extern + thread_local + static;\n}\n";
    let (outcome, printed) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{printed}");
}
