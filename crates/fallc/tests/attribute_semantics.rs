//! The semantics of the new declaration attributes.
//!
//! These are checker-level rules, so the sources are compiled (or refused) with
//! `compile_text`; no program is run, which keeps the cases cheap.

use fallc::Outcome;
use fallc::cli::EmitKind;
use fallc::pipeline::compile_text;

fn compile(source: &str) -> (Outcome, String) {
    compile_text("test.ft", source, &[EmitKind::Llvm])
}

#[test]
fn a_constexpr_binding_needs_a_constant_value() {
    let source = "fn seven() int {\n    return 7;\n}\nfn main() int {\n    value int @constexpr = seven();\n    return value;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("is not a constant expression"), "{output}");
}

#[test]
fn a_constexpr_binding_with_a_constant_is_accepted() {
    let source = "fn main() int {\n    value int @constexpr = 7;\n    return value;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
}

#[test]
fn a_const_binding_may_be_initialised_at_run_time() {
    // Unlike `@constexpr`, `const` says the value cannot be written after the
    // declaration; it does not say the value is known before the program runs.
    let source = "fn seven() int {\n    return 7;\n}\nfn main() int {\n    value const int = seven();\n    return value;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
}

#[test]
fn uninitialized_rejects_an_initialiser() {
    let source = "fn main() int {\n    value int @uninitialized = 3;\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("cannot have an initialiser"), "{output}");
}

#[test]
fn uninitialized_skips_the_empty_constructor() {
    // The constructor may raise, so a caller that has to handle that would be
    // reported. `@uninitialized` means the constructor is not called at all,
    // which is why this compiles in a function without `except`.
    let source = "type Device {\n    n int;\n}\nfn __init__(this &Device) void except {\n    raise \"no device\";\n}\nfn main() int {\n    device Device @uninitialized @maybe_unused;\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
}

#[test]
fn uninitialized_is_refused_on_a_module_variable() {
    let source = "value int @uninitialized;\nfn main() int {\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("not a module-level variable"), "{output}");
}

#[test]
fn export_on_a_local_is_refused() {
    let source = "fn main() int {\n    export value int = 1;\n    return value;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("module-level declaration"), "{output}");
}

#[test]
fn maybe_unused_silences_the_never_read_warning() {
    let plain = "fn main() int {\n    value int = 1;\n    return 0;\n}\n";
    let (outcome, output) = compile(plain);
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(output.contains("never read"), "{output}");
    let waived = "fn main() int {\n    value int @maybe_unused = 1;\n    return 0;\n}\n";
    let (outcome, output) = compile(waived);
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(!output.contains("never read"), "{output}");
}

#[test]
fn branch_attributes_are_accepted_in_every_position() {
    let source = "fn main() int {\n    n int = 2;\n    if (n > 1) @likely {\n        n += 1;\n    } else @unlikely {\n        n = 0;\n    }\n    switch (n) {\n        case 3 @likely:\n            return 1;\n        default:\n            return 0;\n    }\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
}

#[test]
fn a_branch_attribute_belongs_to_its_branch() {
    // `export` is a keyword now, not an attribute; this checks that an
    // attribute is required in a branch position and that a non-attribute
    // there is reported as such.
    let source =
        "fn main() int {\n    if (true) @export {\n        return 1;\n    }\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(
        output.contains("not an attribute of this branch"),
        "{output}"
    );
}

#[test]
fn a_must_use_result_may_not_be_discarded() {
    let source = "@must_use\nfn compute() int {\n    return 1;\n}\nfn main() int {\n    compute();\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(output.contains("must_use"), "{output}");
}

#[test]
fn a_used_must_use_result_is_fine() {
    let source = "@must_use\nfn compute() int {\n    return 1;\n}\nfn main() int {\n    value int = compute();\n    return value;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(!output.contains("must_use"), "{output}");
}
