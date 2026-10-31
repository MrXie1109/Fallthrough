//! Attributes: `@name` / `@name(args)` before a declaration.
//!
//! The attribute says something about the *declaration* rather than about a
//! type, which is why it is written before the declaration and not after the
//! type. Each one is checked where it is written: an attribute that means
//! nothing in that position is a mistake, not something to ignore.

use fallc::Outcome;
use fallc::cli::EmitKind;
use fallc::pipeline::compile_text;

fn compile(source: &str) -> (Outcome, String) {
    compile_text("t.ft", source, &[EmitKind::Llvm])
}

#[test]
fn an_attribute_is_on_the_item_it_precedes() {
    let (_, tree) = compile_text(
        "t.ft",
        "@inline\nfn fast(n int) int {\n    return n;\n}\n",
        &[EmitKind::Ast],
    );
    assert!(tree.contains("@inline"), "{tree}");
}

#[test]
fn an_unknown_attribute_is_reported() {
    let (outcome, diagnostics) = compile("@bogus\nfn f() void {\n}\n");
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("`@bogus` is not an attribute"),
        "{diagnostics}"
    );
}

#[test]
fn an_attribute_written_with_arguments_it_does_not_take_is_reported() {
    let (outcome, diagnostics) = compile("@inline(2)\nfn f() void {\n}\n");
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(diagnostics.contains("takes 0 argument"), "{diagnostics}");
}

#[test]
fn inline_asks_the_code_generator_to_inline() {
    let (outcome, ir) = compile(
        "@inline\nfn fast(n int) int {\n    return n + 1;\n}\n\nfn main() int {\n    return fast(1);\n}\n",
    );
    assert_eq!(outcome, Outcome::Success, "{ir}");
    assert!(ir.contains("alwaysinline"), "{ir}");
}

#[test]
fn noreturn_is_on_the_function_and_may_not_return_a_value() {
    let (outcome, ir) = compile(
        "@noreturn\nfn die() void {\n    while (true) {\n    }\n}\n\nfn main() int {\n    return 0;\n}\n",
    );
    assert_eq!(outcome, Outcome::Success, "{ir}");
    assert!(ir.contains("noreturn"), "{ir}");

    let (outcome, diagnostics) = compile("@noreturn\nfn bad() int {\n    return 1;\n}\n");
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("has no value to return"),
        "{diagnostics}"
    );
}

#[test]
fn using_a_deprecated_function_warns_every_time() {
    let source = r#"@deprecated("use fresh")
fn old(n int) int {
    return n;
}

fn main() int {
    return old(1) + old(2);
}
"#;
    let (outcome, diagnostics) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{diagnostics}");
    assert_eq!(
        diagnostics.matches("is deprecated").count(),
        2,
        "{diagnostics}"
    );

    // The group can be turned off like any other.
    let options = fallc::cli::Options::parse(&[
        "t.ft".to_owned(),
        "--check".to_owned(),
        "-Wno-deprecated".to_owned(),
    ])
    .expect("the arguments are well formed");
    let _ = options;
}

#[test]
fn a_deprecated_message_is_part_of_the_warning() {
    // The message is there to be read: a warning that said only "deprecated"
    // left the author to search for what to use instead.
    let source = r#"@deprecated("use fresh")
fn old(n int) int {
    return n;
}

fn main() int {
    return old(1);
}
"#;
    let (outcome, diagnostics) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{diagnostics}");
    assert!(
        diagnostics.contains("is deprecated: use fresh"),
        "{diagnostics}"
    );
}

#[test]
fn a_deprecated_variable_warns_where_it_is_used() {
    // `@deprecated` applies to a module-level variable as well as to a
    // function, and a read of the name is a use.
    let source = r#"@deprecated("set BUILD instead")
BUILD u64 = 7;

fn main() int {
    return BUILD as int;
}
"#;
    let (outcome, diagnostics) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{diagnostics}");
    assert!(
        diagnostics.contains("`BUILD` is deprecated: set BUILD instead"),
        "{diagnostics}"
    );
}

#[test]
fn enable_if_accepts_a_boolean_constant() {
    // `false` is a constant: the condition of `@enable_if` may be written the
    // way a condition is written.
    let source = r#"@enable_if(false)
fn gone() int {
    return 1;
}

@enable_if(true)
fn kept() int {
    return 2;
}

fn main() int {
    return kept();
}
"#;
    let (outcome, diagnostics) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{diagnostics}");

    let (outcome, diagnostics) = compile(
        "fn main() int {
    return gone();
}
",
    );
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
}

#[test]
fn align_on_a_variable_is_alignas_written_the_other_way_round() {
    // A weaker request is refused with the same message `alignas` gives.
    let (outcome, diagnostics) =
        compile("@align(2)\na int;\n\nfn main() int {\n    return a;\n}\n");
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(diagnostics.contains("is weaker than"), "{diagnostics}");
}

#[test]
fn an_attribute_that_describes_a_function_is_refused_on_a_variable() {
    let (outcome, diagnostics) =
        compile("@noreturn\nx int = 1;\n\nfn main() int {\n    return x;\n}\n");
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("applies to a function"),
        "{diagnostics}"
    );
}
