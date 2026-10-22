//! `static_assert`: a claim the compiler checks.
//!
//! Outside a template it is checked where it is written; inside one it is
//! checked once per instantiation, so a template nobody instantiates and a body
//! nobody calls make no claim. The condition has to be a `bool` the compiler
//! knows, because there is nothing left to check when the program runs.

use fallc::Outcome;
use fallc::cli::EmitKind;
use fallc::pipeline::compile_text;

fn compile(source: &str) -> (Outcome, String) {
    compile_text("t.ft", source, &[EmitKind::Llvm])
}

#[test]
fn a_true_claim_compiles_at_module_scope_and_in_a_body() {
    let source = r#"import C;

static_assert(sizeof(int) == 4);

fn main() int {
    static_assert(sizeof(u8) == 1, "one byte");
    C.printf("ok\n");
    return 0;
}
"#;
    let (outcome, printed) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{printed}");
    assert!(printed.contains("define i32 @main"), "{printed}");
}

#[test]
fn a_false_claim_reports_the_message_it_was_given() {
    let source = "static_assert(sizeof(int) == 8, \"int is not eight bytes\");\n\nfn main() int {\n    return 0;\n}\n";
    let (outcome, printed) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{printed}");
    assert!(
        printed.contains("static assertion failed: int is not eight bytes"),
        "{printed}"
    );
}

#[test]
fn a_false_claim_without_a_message_still_says_what_happened() {
    let source = "static_assert(1 + 1 == 3);\n\nfn main() int {\n    return 0;\n}\n";
    let (outcome, printed) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{printed}");
    assert!(printed.contains("static assertion failed"), "{printed}");
}

#[test]
fn a_condition_that_is_not_a_bool_is_refused() {
    let source = "static_assert(1);\n\nfn main() int {\n    return 0;\n}\n";
    let (outcome, printed) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{printed}");
    assert!(printed.contains("must be a `bool`"), "{printed}");
}

#[test]
fn a_condition_the_compiler_cannot_decide_is_refused() {
    // A module-level variable is not a constant: its value is storage, not a
    // number the compiler has.
    let source = r#"import C;

n int = 1;

static_assert(n == 1);

fn main() int {
    C.printf("%d\n", n);
    return 0;
}
"#;
    let (outcome, printed) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{printed}");
    assert!(printed.contains("must be a constant"), "{printed}");
}

#[test]
fn a_claim_inside_a_template_is_checked_when_it_is_instantiated() {
    // The same template: `fits<int>()` holds, `fits<u8>()` does not.
    let failing = r#"import C;

@constexpr

template <T type>
fn fits() bool {
    static_assert(sizeof(T) == 4, "T is not four bytes");
    return true;
}

fn main() int {
    C.printf("%d\n", fits<u8>() as int);
    return 0;
}
"#;
    let (outcome, printed) = compile(failing);
    assert_eq!(outcome, Outcome::Failure, "{printed}");
    assert!(printed.contains("T is not four bytes"), "{printed}");

    let passing = failing.replace("fits<u8>", "fits<int>");
    let (outcome, printed) = compile(passing.as_str());
    assert_eq!(outcome, Outcome::Success, "{printed}");
}

#[test]
fn a_template_nobody_instantiates_makes_no_claim() {
    let source = r#"import C;

@constexpr

template <T type>
fn fits() bool {
    static_assert(sizeof(T) == 4, "T is not four bytes");
    return true;
}

fn main() int {
    C.printf("no instantiation\n");
    return 0;
}
"#;
    let (outcome, printed) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{printed}");
}
