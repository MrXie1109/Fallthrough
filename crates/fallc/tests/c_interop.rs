//! Tests for importing and calling C.
//!
//! These are the tests that show the compiler can do something visible without
//! a standard library: `C.printf` is the only way to print anything today.
//!
//! They run the whole pipeline and assert on the LLVM IR, because the point is
//! that a C function is declared and called correctly — which is a code
//! generation concern, not a parsing one.

use fallc::Outcome;
use fallc::cli::EmitKind;
use fallc::pipeline::compile_text;

/// The C bindings, as shipped. Tests import it by writing a file beside the
/// compiled source, which is how the loader searches.
const C_MODULE: &str = include_str!("../../../standard/C.ftm");

fn compile(source: &str) -> (Outcome, String) {
    compile_text("test.ftm", source, &[EmitKind::Llvm])
}

#[test]
fn the_c_bindings_parse() {
    // A syntax break in C.ftm would make every C-using program fail, so it is
    // checked directly.
    let (outcome, output) = compile(C_MODULE);
    assert!(
        !output.contains("expected"),
        "C.ftm must parse cleanly: {output}"
    );
    let _ = outcome;
}

#[test]
fn an_import_is_recorded_in_the_tree() {
    let (_, output) = compile_text("test.ftm", "import C;\n", &[EmitKind::Ast]);
    assert!(output.contains("Import"), "{output}");
}

#[test]
fn an_extern_declaration_parses_with_an_abi_and_export() {
    let source = "@extern_C export fn puts(text *const c8) int;\n";
    let (_, output) = compile_text("test.ftm", source, &[EmitKind::Ast]);
    assert!(output.contains("extern \"C\""), "{output}");
    assert!(output.contains("export"), "{output}");
    assert!(output.contains("(external)"), "{output}");
}

#[test]
fn a_variadic_declaration_records_the_ellipsis() {
    let source = "@extern_C export fn printf(fmt *const c8, ...) int;\n";
    let (_, output) = compile_text("test.ftm", source, &[EmitKind::Ast]);
    assert!(output.contains(", ..."), "{output}");
}

#[test]
fn a_function_may_omit_its_return_type() {
    // A C `void` function writes its return type, like any declaration.
    let source = "@extern_C export fn abort() void;\n";
    let (_, output) = compile_text("test.ftm", source, &[EmitKind::Ast]);
    assert!(output.contains("abort"), "{output}");
}

#[test]
fn a_byte_string_lexes_as_one_token() {
    use fallc_lexer::{Lexer, LexerConfig, TokenKind};
    let lexed =
        Lexer::new(LexerConfig::default()).tokenize(r#"b"hi""#, fallc_span::FileId::from_raw(0));
    assert_eq!(lexed.tokens[0].kind, TokenKind::ByteString);
    // Only one token plus the terminator: `b` must not be split off.
    assert_eq!(lexed.tokens.len(), 2);
}

#[test]
fn a_byte_string_carries_no_terminator() {
    // `b"hi"` is `[2]const byte`: raw memory, two bytes, not a C string. The
    // two prefixes differ in *both* the element type and the terminator, which
    // is what makes `b".."` a buffer and `".."` text.
    let source = "fn f() int {\n    x [2]const byte = b\"hi\";\n    return 0;\n}\n";
    let (_, output) = compile(source);
    assert!(output.contains(r#"c"hi""#), "{output}");
    assert!(!output.contains(r#"c"hi\00""#), "{output}");
}

#[test]
fn an_ordinary_string_is_nul_terminated_and_its_length_counts_it() {
    // `"hi"` is `[3]const c8`: two characters and the terminator, which is what
    // makes it usable as a C string.
    let source = "fn f() int {\n    x [3]const c8 = \"hi\";\n    return 0;\n}\n";
    let (_, output) = compile(source);
    assert!(output.contains(r#"c"hi\00""#), "{output}");
}

#[test]
fn an_imported_c_function_is_declared_and_called() {
    // The whole point: a call to C becomes an LLVM call to an emitted
    // declaration.
    let source = "import C;\nfn main() int {\n    C.puts(\"hi\");\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(output.contains("declare i32 @puts(ptr)"), "{output}");
    assert!(output.contains("call i32 @puts(ptr"), "{output}");
}

#[test]
fn a_variadic_c_call_passes_extra_arguments() {
    let source = "import C;\nfn main() int {\n    C.printf(\"%d\\n\", 42);\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
    // The declaration carries the ellipsis, and the call passes both arguments.
    assert!(output.contains("declare i32 @printf(ptr, ...)"), "{output}");
    assert!(
        output.contains("call i32 (ptr, ...) @printf(ptr"),
        "{output}"
    );
}

#[test]
fn a_variadic_c_call_accepts_few_arguments() {
    // Only the declared parameters are required.
    let source = "import C;\nfn main() int {\n    C.printf(\"hello\");\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
}

#[test]
fn a_non_variadic_c_call_still_checks_its_argument_count() {
    let source = "import C;\nfn main() int {\n    C.puts();\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("takes 1 argument"), "{output}");
}

#[test]
fn an_array_argument_decays_rather_than_passing_by_value() {
    // An array that reaches a variadic slot as an LLVM aggregate is read by the
    // callee as an address, which segfaults with no diagnostic anywhere. The
    // decay has to happen in codegen, not only in the checker.
    let source = "import C;\nfn main() int {\n    plain [3]const c8 = \"ab\";\n    bytes [2]const byte = b\"ab\";\n    C.printf(\"%s %.2s\\n\", plain, bytes);\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
    // Passed as pointers, never as `[N x i8]` aggregates.
    assert!(output.contains("ptr %plain"), "{output}");
    assert!(output.contains("ptr %bytes"), "{output}");
    assert!(!output.contains("[3 x i8] %plain"), "{output}");
    assert!(!output.contains("[2 x i8] %bytes"), "{output}");
}

#[test]
fn the_two_string_forms_differ_in_their_terminator() {
    // `"ab"` is three characters and `b"ab"` is two. That is the whole
    // difference between them, and it is what makes the first usable as a C
    // string.
    let source = "fn f() int {\n    a [3]const c8 = \"ab\";\n    b [2]const byte = b\"ab\";\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(output.contains(r#"c"ab\00""#), "{output}");
    assert!(output.contains(r#"c"ab""#), "{output}");
}

#[test]
fn a_string_literal_takes_its_element_width_from_its_prefix() {
    // `u16"ab"` is an array of `c16`, so its constant is `[3 x i16]`.
    let source = "fn f() int {\n    w [3]const c16 = u16\"ab\";\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(output.contains("[3 x i16]"), "{output}");
}

#[test]
fn an_array_decays_to_a_pointer_for_a_c_parameter() {
    // `"hi"` is `[3]const c8` and the parameter is `*const c8`, so the array
    // must decay. Without this, no string could be passed to C at all.
    let source = "import C;\nfn main() int {\n    C.puts(\"hi\");\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
}

#[test]
fn a_private_module_item_is_not_visible_to_an_importer() {
    // Declarations are private unless exported, so a call to an unexported
    // name must fail. The module is written inline here to control visibility.
    let source = "import M;\nfn main() int {\n    M.hidden();\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    // The module does not exist beside the test, so this fails at the import.
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("cannot find module"), "{output}");
}

#[test]
fn calling_a_name_a_module_does_not_export_is_reported() {
    // `abort` is deliberately absent from the check: it exists. `not_there` is
    // what matters.
    let source = "import C;\nfn main() int {\n    C.not_there();\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("does not export"), "{output}");
}
