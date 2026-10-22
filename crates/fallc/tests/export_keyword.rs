//! `export` is a keyword, not an attribute.
//!
//! It introduces a declaration that importers can see: `export fn f()`,
//! `export type T`, `export NAME const T = ..;`, `export from M import *;` and
//! `export define NAME ..`. The attribute spelling `@export` is refused with
//! that rule rather than being silently accepted as a second way to write it.

use fallc::Outcome;
use fallc::cli::EmitKind;
use fallc::pipeline::compile_text;

fn compile(source: &str) -> (Outcome, String) {
    compile_text("t.ft", source, &[EmitKind::Llvm])
}

#[test]
fn export_prefixes_a_function() {
    let (outcome, diagnostics) = compile(
        "export fn twice(n int) int {\n    return n * 2;\n}\nfn main() int {\n    return twice(21) - 42;\n}\n",
    );
    assert_eq!(outcome, Outcome::Success, "{diagnostics}");
}

#[test]
fn export_prefixes_a_type_a_constant_and_a_variable() {
    let source = "export type Point {\n    x int;\n    y int;\n}\nexport LIMIT const int = 10;\nexport counter int = 0;\nfn main() int {\n    p Point = .{1, 2};\n    return p.x + p.y + LIMIT + counter - 13;\n}\n";
    let (outcome, diagnostics) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{diagnostics}");
}

#[test]
fn the_attribute_spelling_is_refused_with_the_rule() {
    let (outcome, diagnostics) = compile("@export fn f() int {\n    return 1;\n}\n");
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("`export` is a keyword, not an attribute"),
        "{diagnostics}"
    );
}

#[test]
fn export_on_a_local_is_refused() {
    let (outcome, diagnostics) =
        compile("fn main() int {\n    export x int = 1;\n    return x;\n}\n");
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("applies to a module-level declaration"),
        "{diagnostics}"
    );
}
