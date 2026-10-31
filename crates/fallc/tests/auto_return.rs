//! `auto` as a return type, deduced from the body.
//!
//! The rule is the one `?:` uses for its two branches: what comes out has one
//! type, so every `return` has to agree with the others. A function whose
//! `return` statements carry no value returns `void` -- that is a type, not a
//! mistake, because a function is allowed to return nothing.

use fallc::Outcome;
use fallc::cli::EmitKind;
use fallc::pipeline::compile_text;

fn compile(source: &str) -> (Outcome, String) {
    compile_text("test.ft", source, &[EmitKind::Llvm])
}

#[test]
fn the_return_type_is_what_the_body_returns() {
    let source = r#"fn twice(n int) auto {
    return n * 2;
}

fn main() int {
    return twice(3);
}
"#;
    let (outcome, printed) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{printed}");
    // `int`, so the emitted function returns `i32`.
    assert!(printed.contains("define i32 @twice"), "{printed}");
}

#[test]
fn every_return_has_to_agree() {
    let source = r#"fn mixed(flag bool) auto {
    if (flag) {
        return 1;
    }
    return "text";
}

fn main() int {
    return 0;
}
"#;
    let (outcome, printed) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{printed}");
    assert!(printed.contains("type mismatch"), "{printed}");
}

#[test]
fn an_auto_return_with_no_value_is_void() {
    // Nothing came back, so there is nothing to deduce and the answer is
    // `void`: a function is allowed to return no value, and `auto` has to say
    // so rather than demand a `return` that carries one.
    let source = r#"fn nothing() auto {
    return;
}

fn quiet() auto {
    x int = 1;
    x = 2;
}

fn main() int {
    nothing();
    quiet();
    return 0;
}
"#;
    let (outcome, printed) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{printed}");
    assert!(printed.contains("define void @nothing"), "{printed}");
    assert!(printed.contains("define void @quiet"), "{printed}");
}

#[test]
fn a_temporary_and_a_value_are_the_same_rule() {
    // The deduced type is the *value's* type, not a reference to the place it
    // came from: `return a[0]` in an `auto` function yields the element.
    let source = r#"fn first(p *int) auto {
    return p[1];
}

fn main() int {
    nums [3]int;
    nums[0] = 1;
    nums[1] = 2;
    nums[2] = 3;
    return first(&nums[0]);
}
"#;
    let (outcome, printed) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{printed}");
    assert!(printed.contains("define i32 @first"), "{printed}");
}

#[test]
fn a_definition_may_leave_the_return_type_out() {
    // Writing no return type on a *definition* means `auto`: the body decides.
    let source = r#"fn twice(n int) {
    return n * 2;
}

fn nothing() {
}

fn main() int {
    nothing();
    return twice(21);
}
"#;
    let (outcome, printed) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{printed}");
    assert!(printed.contains("define i32 @twice"), "{printed}");
    assert!(printed.contains("define void @nothing"), "{printed}");
}

#[test]
fn a_declaration_must_write_its_return_type() {
    // A declaration has no body to deduce from, so `auto` cannot be filled in.
    let source = "fn declared(n int);\n\nfn main() int {\n    return 0;\n}\n";
    let (outcome, printed) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{printed}");
    assert!(printed.contains("must write its return type"), "{printed}");
}
