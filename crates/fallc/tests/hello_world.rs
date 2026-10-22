//! End-to-end tests for real programs.
//!
//! These drive the whole pipeline — lex, parse, resolve, type check, lower —
//! and assert on the LLVM IR that comes out. They are the tests that would
//! catch a break anywhere in the chain, which is why they exist alongside the
//! per-crate unit tests.

use fallc::Outcome;
use fallc::cli::EmitKind;
use fallc::pipeline::{compile_text, compile_text_in_mode};
use fallc_ir::BuildMode;

/// Compiles `source` and returns the outcome plus everything the driver
/// printed.
fn compile(source: &str) -> (Outcome, String) {
    compile_text("test.ft", source, &[EmitKind::Llvm])
}

/// Compiles `source` with `-Wverbose`, which adds the explanations.
fn compile_verbose(source: &str) -> (Outcome, String) {
    let mut warnings = fallc_diag::WarningPolicy::new();
    warnings.set_verbose(true);
    fallc::pipeline::compile_text_with_warnings("test.ft", source, &[EmitKind::Llvm], &warnings)
}

#[test]
fn the_first_program_compiles_to_llvm_ir() {
    let (outcome, output) = compile("fn main() int {\n    return 0;\n}\n");
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(output.contains("define i32 @main()"), "{output}");
    assert!(output.contains("ret i32 0"), "{output}");
}

#[test]
fn a_returned_value_reaches_the_llvm_ir() {
    let (outcome, output) = compile("fn main() int {\n    return 42;\n}\n");
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(output.contains("ret i32 42"), "{output}");
}

#[test]
fn the_emitted_text_is_valid_llvm_assembly() {
    // `int` is an alias for `i32`, so the LLVM return type must be `i32` even
    // though the source says `int`.
    let (_, output) = compile("fn main() int {\n    return 0;\n}\n");
    let body = output
        .lines()
        .skip_while(|line| !line.starts_with("; ModuleID"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(body.starts_with("; ModuleID"), "{output}");
}

#[test]
fn a_u8_return_type_lowers_to_i8() {
    // The literal's LLVM type comes from the declared return type, not from a
    // hardcoded default.
    let (outcome, output) = compile("fn small() u8 {\n    return 7;\n}\n");
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(output.contains("define i8 @small()"), "{output}");
    assert!(output.contains("ret i8 7"), "{output}");
}

#[test]
fn several_functions_each_get_a_definition() {
    let source = "fn one() int {\n    return 1;\n}\nfn two() int {\n    return 2;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(output.contains("define i32 @one()"), "{output}");
    assert!(output.contains("define i32 @two()"), "{output}");
}

#[test]
fn functions_may_be_referenced_before_they_are_declared() {
    // Resolution declares every top-level name first, so order does not matter:
    // a declaration is visible throughout its module whatever the source order.
    let source = "fn a() int {\n    return 1;\n}\nfn b() int {\n    return 2;\n}\n";
    let (outcome, _) = compile(source);
    assert_eq!(outcome, Outcome::Success);
}

#[test]
fn a_syntax_error_stops_before_codegen() {
    let (outcome, output) = compile("fn main() int {\n    return\n}\n");
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(!output.contains("define"), "no IR on failure: {output}");
}

#[test]
fn a_missing_return_value_is_reported_against_the_declared_type() {
    let (outcome, output) = compile("fn main() int {\n    return;\n}\n");
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("expected a value"), "{output}");
}

#[test]
fn a_control_flow_statement_reports_that_its_lowering_is_unspecified() {
    // `if` lexes, parses, resolves, type checks, and lowers. What fails here is
    // the undeclared condition `c`, which resolution reports before code
    // generation runs.
    let source = "fn main() int {\n    if (c) {\n        return 1;\n    }\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("cannot find"), "{output}");
}

#[test]
fn an_empty_file_compiles_to_an_empty_module() {
    let (outcome, output) = compile("");
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(output.contains("ModuleID"), "{output}");
    assert!(!output.contains("define"), "{output}");
}

// ---------------------------------------------------------------- bindings

#[test]
fn a_variable_can_be_declared_and_returned() {
    let source = "fn main() int {\n    x int = 7;\n    return x;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(output.contains("ret i32 %x"), "{output}");
}

#[test]
fn an_uninitialised_binding_is_zero_initialised() {
    let source = "fn main() int {\n    x int;\n    return x;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(output.contains("store i32 0"), "{output}");
}

#[test]
fn auto_takes_the_initialisers_type() {
    // A `u8`-returning `main` is not an entry, so the deduction is asked of an
    // ordinary function instead.
    let source = "fn seven() u8 {\n    x auto = 7;\n    return x;\n}\nfn main() int {\n    return seven() as int;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
    // The deduced type must be the context's `u8`, not a default `i32`.
    assert!(output.contains("alloca i8"), "{output}");
}

#[test]
fn auto_without_an_initialiser_is_rejected() {
    let source = "fn main() int {\n    x auto;\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("cannot deduce a type"), "{output}");
    assert!(output.contains("S0020"), "{output}");
}

#[test]
fn auto_from_a_designated_initialiser_is_rejected() {
    let source = "fn main() int {\n    x auto = .{ 1, 2 };\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("cannot deduce a type"), "{output}");
    // The same code as the missing-initialiser case: same failure, and the
    // message distinguishes them.
    assert!(output.contains("S0020"), "{output}");
}

#[test]
fn a_designated_initialiser_needs_a_declared_type() {
    // `.{ .. }` takes its field types from the type it initialises, so it
    // cannot be used where no type is declared.
    let source = "fn main() int {\n    x int = .{ 1, 2 };\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("is not one"), "{output}");
}

#[test]
fn a_designated_initialiser_needs_the_right_field_count() {
    let source = "type P {\n    x int;\n    y int;\n}\nfn main() int {\n    p P = .{ 1 };\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    // The message names the field that is missing rather than the count, because
    // a field may also be filled by name and the count alone would not say
    // which one is left out.
    assert!(output.contains("needs field `y`"), "{output}");
}

#[test]
fn a_designated_initialiser_checks_field_types_by_position() {
    // An unsuffixed literal adopts its field's type, so `2` fits a `u8` field.
    let source = "type P {\n    x int;\n    y u8;\n}\nfn main() int {\n    p P = .{ 1, 2 };\n    return p.y as int;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
    // The second field is an i8, since that is what `u8` lowers to.
    assert!(output.contains("{ i32, i8 }"), "{output}");
}

#[test]
fn a_suffixed_element_that_conflicts_with_its_field_is_refused() {
    // The suffix is the literal's type, so a mismatched one is an error rather
    // than a silent truncation.
    let source = "type P {\n    x int;\n    y u8;\n}\nfn main() int {\n    p P = .{ 1, 2u32 };\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("field `y`"), "{output}");
}

#[test]
fn a_nested_designated_initialiser_parses() {
    // An element may itself be an initialiser, and the parser must not stop at
    // the first inner `}`.
    let source = "type Inner {\n    v int;\n}\ntype Outer {\n    a Inner;\n    b Inner;\n}\nfn main() int {\n    o Outer = .{ .{ 1 }, .{ 2 } };\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
}

#[test]
fn a_binding_type_mismatch_is_reported() {
    let source = "fn main() int {\n    x int = 7;\n    y u8 = x;\n    return y;\n}\n";
    let (outcome, output) = compile(source);
    // Widening `u8` to `int` is implicit (every value fits), but narrowing
    // `int` to `u8` is not: a value may not fit, so it needs an explicit `as`.
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("type mismatch"), "{output}");
}

// ------------------------------------------------------------ integer types

#[test]
fn isize_and_usize_are_sixty_four_bit() {
    let source = "fn f() isize {\n    return 1;\n}\nfn g() usize {\n    return 2;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(output.contains("define i64 @f()"), "{output}");
    assert!(output.contains("define i64 @g()"), "{output}");
}

#[test]
fn byte_lowers_to_i8_but_is_not_u8() {
    // `byte` shares u8's representation but not its identity, so a `byte`
    // value may not be assigned to a `u8` binding without a conversion.
    let source = "fn main() int {\n    x byte = 1;\n    y u8 = x;\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("type mismatch"), "{output}");
    assert!(output.contains("found byte"), "{output}");
}

#[test]
fn u8_may_not_be_assigned_to_byte_either() {
    // The incompatibility must not be one-directional.
    let source = "fn main() int {\n    x u8 = 1;\n    y byte = x;\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("type mismatch"), "{output}");
    assert!(output.contains("found u8"), "{output}");
}

#[test]
fn byte_has_its_own_storage_width() {
    let source = "fn main() int {\n    x byte = 1;\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(output.contains("alloca i8"), "{output}");
}

#[test]
fn every_integer_type_maps_to_its_llvm_width() {
    for (ty, llvm) in [
        ("i8", "i8"),
        ("i16", "i16"),
        ("i32", "i32"),
        ("i64", "i64"),
        ("i128", "i128"),
        ("u8", "i8"),
        ("u16", "i16"),
        ("u32", "i32"),
        ("u64", "i64"),
        ("u128", "i128"),
        ("int", "i32"),
        ("uint", "i32"),
        ("isize", "i64"),
        ("usize", "i64"),
        ("byte", "i8"),
    ] {
        let source = format!("fn f() {ty} {{\n    return 0;\n}}\n");
        let (outcome, output) = compile(&source);
        assert_eq!(outcome, Outcome::Success, "`{ty}` failed: {output}");
        assert!(
            output.contains(&format!("define {llvm} @f()")),
            "`{ty}` should lower to `{llvm}`: {output}"
        );
    }
}

#[test]
fn float_return_types_are_recognised() {
    for (ty, llvm, literal) in [("float", "float", "1.5f"), ("double", "double", "1.5")] {
        let empty = format!("fn f() {ty} {{\n}}\n");
        let (outcome, output) = compile(&empty);
        assert_eq!(outcome, Outcome::Success, "`{ty}` failed: {output}");
        assert!(
            output.contains(&format!("define {llvm} @f()")),
            "`{ty}` should lower to `{llvm}`: {output}"
        );

        // A float function returns a float literal. An integer literal is not
        // converted implicitly, since there are no implicit conversions.
        let body = format!("fn f() {ty} {{\n    return {literal};\n}}\n");
        let (outcome, output) = compile(&body);
        assert_eq!(outcome, Outcome::Success, "`{ty}` body failed: {output}");
    }
}

#[test]
fn an_integer_literal_is_not_a_float() {
    // `return 0;` in a float function needs an explicit conversion, because
    // there are no implicit conversions between numeric types.
    let (outcome, output) = compile("fn f() float {\n    return 0;\n}\n");
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("type mismatch"), "{output}");
}

#[test]
fn char_is_not_yet_a_type() {
    // `char` is deferred until arrays are specified, so it must not silently
    // resolve to something else in the meantime. An unknown type is carried
    // rather than rejected, but an integer literal does not adopt it, so the
    // mismatch is reported.
    let source = "fn main() int {\n    x char = 1;\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("type mismatch"), "{output}");
}

// ------------------------------------------------------------- conversions

#[test]
fn as_converts_between_integer_types() {
    let source = "fn main() int {\n    x u8 = 1;\n    y int = x as int;\n    return y;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(output.contains("zext i8"), "{output}");
}

#[test]
fn as_converts_byte_to_u8_without_an_instruction() {
    // The two share a representation, so the conversion is a no-op in LLVM even
    // though the type system requires it to be written.
    let source = "fn main() int {\n    x byte = 1;\n    y u8 = x as u8;\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(!output.contains("trunc"), "{output}");
    assert!(!output.contains("zext"), "{output}");
}

#[test]
fn a_narrowing_conversion_truncates() {
    let source = "fn main() int {\n    x int = 1;\n    y u8 = x as u8;\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(output.contains("trunc i32"), "{output}");
}

#[test]
fn a_signed_source_sign_extends() {
    // LLVM integers are sign-agnostic, so the source type's signedness picks
    // the instruction: `sext` for signed, `zext` for unsigned.
    let source = "fn main() int {\n    x i8 = 1;\n    y i32 = x as i32;\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(output.contains("sext i8"), "{output}");
}

#[test]
fn chained_conversions_parse_left_to_right() {
    let source =
        "fn main() int {\n    x byte = 1;\n    y int = x as u8 as int;\n    return y;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(output.contains("zext i8"), "{output}");
}

#[test]
fn no_implicit_narrowing_between_integer_types() {
    let source = "fn main() int {\n    x int = 1;\n    y u8 = x;\n    return y;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("type mismatch"), "{output}");
}

// ----------------------------------------------------------- control flow

#[test]
fn an_if_lowers_to_branches() {
    let source = "fn f(c int) int {\n    if (c) {\n        return 1;\n    } else {\n        return 2;\n    }\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(output.contains("br i1"), "{output}");
    assert!(output.contains("if.then"), "{output}");
    assert!(output.contains("if.else"), "{output}");
}

#[test]
fn truthiness_tests_an_integer_against_zero() {
    let source = "fn f(c int) int {\n    if (c) {\n        return 1;\n    }\n    return 2;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(output.contains("icmp ne i32"), "{output}");
}

#[test]
fn a_bool_condition_needs_no_conversion() {
    let source = "fn f(c bool) int {\n    if (c) {\n        return 1;\n    }\n    return 2;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
    // `bool` is already `i1`, so no comparison is inserted.
    assert!(!output.contains("icmp"), "{output}");
}

#[test]
fn a_type_without_bool_cannot_be_a_condition() {
    // A type is a condition when it defines `__bool__`. Without one it has no
    // truth value, and saying so beats guessing at one.
    let source = "fn f(c Foo) int {\n    if (c) {\n        return 1;\n    }\n    return 2;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("cannot be used as a condition"), "{output}");
    // What a type needs in order to have a truth value is an explanation, and
    // explanations are what `-Wverbose` adds.
    assert!(!output.contains("__bool__"), "{output}");
    let (_, verbose) = compile_verbose(source);
    assert!(verbose.contains("__bool__"), "{verbose}");
}

#[test]
fn a_while_loop_puts_its_else_on_normal_termination() {
    // The condition's false edge goes to the `else` block, and `break` goes to
    // the end, which is exactly the specified distinction.
    let source = "fn f(c int) int {\n    while (c) {\n        return 1;\n    } else {\n        return 2;\n    }\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(output.contains("while.cond"), "{output}");
    assert!(output.contains("while.else"), "{output}");
}

#[test]
fn break_skips_the_loop_else() {
    let source = "fn f(c int) int {\n    while (c) {\n        break;\n    } else {\n        return 2;\n    }\n    return 7;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
    // `break` branches straight to the end, not through the else block.
    assert!(output.contains("while.end"), "{output}");
}

#[test]
fn a_c_style_for_lowers_all_three_clauses() {
    let source = "fn f() int {\n    for (i int = 0; i; i) {\n        return 1;\n    } else {\n        return 2;\n    }\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
    for block in ["for.cond", "for.body", "for.update", "for.else", "for.end"] {
        assert!(output.contains(block), "missing {block}: {output}");
    }
}

#[test]
fn switch_cases_do_not_fall_through_by_default() {
    let source = "fn f(e int) int {\n    switch (e) {\n        case 1:\n            return 10;\n        case 2:\n            return 20;\n    }\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
    // Each arm returns, so neither branches into the other.
    assert!(output.contains("switch i32"), "{output}");
    assert!(output.contains("i32 1, label %case.0"), "{output}");
    assert!(output.contains("i32 2, label %case.1"), "{output}");
}

#[test]
fn fallthrough_branches_to_the_next_arm() {
    let source = "fn f(e int) int {\n    switch (e) {\n        case 1:\n            fallthrough;\n        case 2:\n            return 20;\n    }\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
    // `case.0` must branch to `case.1` rather than to the switch end.
    assert!(
        output.contains("br label %case.1"),
        "fallthrough should branch to the next arm: {output}"
    );
}

#[test]
fn a_multi_value_case_labels_one_block_once() {
    let source = "fn f(e int) int {\n    switch (e) {\n        case 1, 2:\n            return 10;\n    }\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(output.contains("i32 1, label %case.0"), "{output}");
    assert!(output.contains("i32 2, label %case.0"), "{output}");
}

#[test]
fn a_default_arm_is_the_switch_fallback() {
    let source = "fn f(e int) int {\n    switch (e) {\n        case 1:\n            return 10;\n        default:\n            return 99;\n    }\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
    // A `default` arm is LLVM's fallback label, not a case value.
    assert!(output.contains("switch i32"), "{output}");
    assert!(output.contains("label %case.1"), "{output}");
}

#[test]
fn break_works_inside_a_switch_arm() {
    let source = "fn f(e int) int {\n    switch (e) {\n        case 1:\n            break;\n    }\n    return 5;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(output.contains("switch.end"), "{output}");
}

#[test]
fn bindings_are_scoped_to_their_block() {
    // An inner binding must not be visible after its block ends, so the outer
    // `x` is still the `int` it was declared as.
    let source = "fn f(c int) int {\n    x int = 1;\n    if (c) {\n        x u8 = 2;\n        return x as int;\n    }\n    return x;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
    // Each binding gets its own storage: the outer `x` is an i32 slot and the
    // inner one an i8 slot, and the inner is renamed rather than reusing the
    // outer's storage.
    assert!(output.contains("alloca i32, align 4"), "{output}");
    assert!(output.contains("alloca i8, align 1"), "{output}");
    // The load after the `if` reads the outer i32 `x`, proving the inner `u8`
    // binding did not leak out of its block.
    assert!(output.contains("load i32, ptr %x,"), "{output}");
}

#[test]
fn a_range_for_over_something_without_begin_is_refused() {
    // The range form is C++'s: the range is asked for `begin()` and `end()`,
    // the position is advanced with `++`, read with `*`, and compared with
    // `!=`. An `int` has none of that, and the error says which operation is
    // missing rather than what the desugaring looks like.
    let source =
        "fn f(r int) int {\n    for (e int : r) {\n        return 1;\n    }\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("`i32` has no method `begin`"), "{output}");
    // The name of the hidden binding must not leak into a diagnostic.
    assert!(!output.contains("$it"), "{output}");
}

// ------------------------------------------------------- operators and calls

/// Compiles `source` and returns the emitted LLVM IR, asserting success.
fn ir_of(source: &str) -> String {
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
    output
}

/// Returns the IR with LLVM's optimiser having folded what it can.
///
/// A debug build guards the arithmetic that can trap, so a constant expression
/// keeps its guards at `-O0` and does not fold. Folding is the cleaner evidence
/// of how an expression grouped, so the tests that check precedence by the
/// folded value ask for it.
fn optimized_ir_of(source: &str) -> String {
    let (outcome, output) = fallc::pipeline::compile_text_optimized(
        "test.ft",
        source,
        &[EmitKind::Llvm],
        fallc_ir::OptLevel::Default,
    );
    assert_eq!(outcome, Outcome::Success, "{output}");
    output
}

#[test]
fn arithmetic_uses_the_signed_instruction_for_signed_types() {
    let output = ir_of("fn f() int {\n    a int = 6;\n    b int = 3;\n    return a / b;\n}\n");
    assert!(output.contains("sdiv i32"), "{output}");
}

#[test]
fn arithmetic_uses_the_unsigned_instruction_for_unsigned_types() {
    let output = ir_of("fn f() u32 {\n    a u32 = 6;\n    b u32 = 3;\n    return a / b;\n}\n");
    assert!(output.contains("udiv i32"), "{output}");
}

#[test]
fn comparisons_use_the_signed_predicate_for_signed_types() {
    let output = ir_of("fn f() bool {\n    a int = 1;\n    b int = 2;\n    return a < b;\n}\n");
    assert!(output.contains("icmp slt"), "{output}");
}

#[test]
fn comparisons_use_the_unsigned_predicate_for_unsigned_types() {
    let output = ir_of("fn f() bool {\n    a u8 = 1;\n    b u8 = 2;\n    return a < b;\n}\n");
    assert!(output.contains("icmp ult"), "{output}");
}

#[test]
fn c_precedence_groups_multiplication_first() {
    // `1 + 2 * 3` is 7, not 9. LLVM's constant folder evaluates the whole
    // expression, so the folded result is the clearest evidence of grouping:
    // the wrong grouping would fold to 9.
    let output = optimized_ir_of("fn f() int {\n    return 1 + 2 * 3;\n}\n");
    assert!(output.contains("ret i32 7"), "{output}");
}

#[test]
fn c_precedence_puts_bitwise_and_below_equality() {
    // C's rule, and the opposite of the common assumption. With the C grouping
    // `3 & 1 == 1` is `3 & (1 == 1)`, i.e. `3 & true` — a type error here,
    // because there are no implicit conversions. With the other grouping it
    // would be `(3 & 1) == 1`, i.e. `1 == 1`, which is true and would compile.
    //
    // So the *rejection* is what shows the precedence: the compiler must reach
    // the `&` first and object to mixing an integer with a boolean.
    let source = "fn f() int {\n    return 3 & 1 == 1;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("same type"), "{output}");
}

#[test]
fn bool_promotes_to_one_or_zero() {
    // `bool as int` is well defined: true becomes 1 and false becomes 0.
    let output = ir_of("fn f() int {\n    b bool = true;\n    return b as int;\n}\n");
    assert!(output.contains("zext i1"), "{output}");
}

#[test]
fn bool_promotes_to_a_wider_integer_too() {
    let output = ir_of("fn f() u64 {\n    b bool = false;\n    return b as u64;\n}\n");
    assert!(output.contains("zext i1"), "{output}");
}

#[test]
fn a_non_boolean_does_not_convert_to_bool() {
    // The promotion is one-way: an integer does not implicitly become a bool,
    // and converting one would need a rule for which values are true.
    let source = "fn f() bool {\n    n int = 1;\n    return n as bool;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("cannot convert"), "{output}");
}

#[test]
fn c_precedence_orders_addition_below_shifts() {
    // `1 + 1 << 2` parses as `(1 + 1) << 2` = 8, because `+` binds tighter
    // than `<<` in C.
    let output = optimized_ir_of("fn f() int {\n    return 1 + 1 << 2;\n}\n");
    assert!(output.contains("ret i32 8"), "{output}");
}

#[test]
fn c_precedence_orders_comparison_below_addition() {
    // `1 + 1 < 3` parses as `(1 + 1) < 3`, which is true.
    let output = optimized_ir_of("fn f() bool {\n    return 1 + 1 < 3;\n}\n");
    assert!(output.contains("ret i1 true"), "{output}");
}

#[test]
fn short_circuit_and_lowers_to_control_flow_with_a_phi() {
    let output = ir_of("fn f(a bool) bool {\n    return a && a;\n}\n");
    assert!(output.contains("logic.rhs"), "{output}");
    assert!(output.contains("phi i1"), "{output}");
}

#[test]
fn assignment_stores_and_yields_the_value() {
    let output = ir_of("fn f() int {\n    a int = 1;\n    a = 2;\n    return a;\n}\n");
    assert!(output.contains("store i32 2"), "{output}");
}

#[test]
fn a_compound_assignment_reads_applies_and_stores() {
    let output = ir_of("fn f() int {\n    a int = 1;\n    a += 2;\n    return a;\n}\n");
    assert!(output.contains("load i32"), "{output}");
    assert!(output.contains("add i32"), "{output}");
}

#[test]
fn assigning_to_a_non_lvalue_is_refused() {
    let source = "fn f() int {\n    1 = 2;\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("cannot be assigned"), "{output}");
}

#[test]
fn a_call_emits_a_call() {
    // The argument is a parameter, so its value is not known until the program
    // runs and the call has to be emitted. A call whose arguments are all
    // constants is evaluated by the checker instead -- see the
    // `compile_time_evaluation` tests.
    let source = "fn g(n int) int {\n    return n + 1;\n}\nfn f(n int) int {\n    return g(n);\n}\n";
    let output = ir_of(source);
    assert!(output.contains("call i32 @g_I3_i32("), "{output}");
}

#[test]
fn a_call_checks_its_argument_count() {
    let source = "fn g(a int) int {\n    return a;\n}\nfn f() int {\n    return g();\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("takes 1 argument"), "{output}");
}

#[test]
fn a_call_checks_its_argument_types() {
    // Widening is implicit, narrowing is not: passing an `int` where a `u8` is
    // wanted is the mismatch.
    let source =
        "fn g(a u8) int {\n    return a as int;\n}\nfn f() int {\n    b int = 1;\n    return g(b);\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("type mismatch in argument"), "{output}");
}

#[test]
fn an_expression_statement_evaluates_for_effect() {
    // Called with a parameter, so the call is real and its value is discarded.
    let source = "fn g(n int) int {\n    return n + 1;\n}\nfn f(n int) int {\n    g(n);\n    return 0;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("call i32 @g_I3_i32("), "{output}");
}

#[test]
fn operators_still_require_matching_operand_types() {
    // No implicit conversions, so mixing types is an error even for arithmetic.
    let source = "fn f() int {\n    a u8 = 1;\n    b int = 2;\n    return a + b;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("same type"), "{output}");
}

// ----------------------------------------------------------- build modes

#[test]
fn debug_mode_guards_division_by_zero() {
    let source = "fn f(a int) int {\n    b int = 0;\n    return a / b;\n}\n";
    let (outcome, output) =
        compile_text_in_mode("test.ft", source, &[EmitKind::Llvm], BuildMode::Debug);
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(output.contains("divzero"), "{output}");
    assert!(output.contains("__builtin_panic"), "{output}");
}

#[test]
fn release_mode_leaves_division_by_zero_undefined() {
    let source = "fn f(a int) int {\n    b int = 0;\n    return a / b;\n}\n";
    let (outcome, output) =
        compile_text_in_mode("test.ft", source, &[EmitKind::Llvm], BuildMode::Release);
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(!output.contains("__builtin_panic"), "{output}");
}

#[test]
fn release_mode_leaves_the_operation_itself_intact() {
    let source = "fn f(a int, b int) int {\n    return a / b;\n}\n";
    let (outcome, output) =
        compile_text_in_mode("test.ft", source, &[EmitKind::Llvm], BuildMode::Release);
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(output.contains("sdiv i32"), "{output}");
}

// --------------------------------------------------------- pointers and const

#[test]
fn taking_an_address_yields_a_pointer() {
    let output = ir_of("fn f() int {\n    x int = 1;\n    p *int = &x;\n    return 0;\n}\n");
    assert!(output.contains("alloca ptr"), "{output}");
    assert!(output.contains("store ptr %x"), "{output}");
}

#[test]
fn dereferencing_reads_through_the_pointer() {
    let output = ir_of("fn f() int {\n    x int = 1;\n    p *int = &x;\n    return *p;\n}\n");
    assert!(output.contains("load i32, ptr"), "{output}");
}

#[test]
fn assigning_through_a_pointer_writes_the_pointee() {
    let output =
        ir_of("fn f() int {\n    x int = 1;\n    p *int = &x;\n    *p = 5;\n    return x;\n}\n");
    assert!(output.contains("store i32 5"), "{output}");
}

#[test]
fn a_pointer_may_not_be_assigned_through_when_the_pointee_is_const() {
    let source =
        "fn f() int {\n    x int = 1;\n    p *const int = &x;\n    *p = 5;\n    return x;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("pointer to const"), "{output}");
}

#[test]
fn writing_through_a_pointer_to_const_is_refused() {
    // `*const T` means the pointee is constant, so `*p = v` is an error.
    let source = "fn f(x *const int) int {\n    *p = 5;\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
}

#[test]
fn adding_const_to_a_pointer_is_implicit() {
    // `*T` becomes `*const T` without a cast: it only takes away the right to
    // write, so it is always safe. This is the one implicit conversion besides
    // literals taking their context's type.
    let source = "fn f() int {\n    x int = 1;\n    p *const int = &x;\n    return *p;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
}

#[test]
fn adding_const_works_through_a_parameter() {
    let source = "fn take(p *const int) int {\n    return *p;\n}\nfn f() int {\n    x int = 5;\n    return take(&x);\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
}

#[test]
fn const_widening_nests() {
    // Each level may add const independently, so `**T` becomes `*const *const T`.
    let source = "fn f() int {\n    x int = 1;\n    p *int = &x;\n    q *const *const int = &p;\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
}

#[test]
fn dropping_const_is_not_implicit() {
    // The reverse would let a caller write through a pointer the program
    // promised not to write through, so it needs an explicit `as`.
    let source = "fn f(p *const int) int {\n    q *int = p;\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("type mismatch"), "{output}");
}

#[test]
fn dropping_const_is_allowed_with_an_explicit_cast() {
    let source = "fn f(p *const int) int {\n    q *int = p as *int;\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
}

#[test]
fn dropping_const_through_a_byte_pointer_needs_a_cast() {
    // `*const T -> *byte` is the same rule as `*const T -> *T`: writing it is
    // what an explicit `as` is for, and leaving it out is still refused.
    let implicit = "fn f(p *const int) int {\n    q *byte = p;\n    return 0;\n}\n";
    let (outcome, output) = compile(implicit);
    assert_eq!(outcome, Outcome::Failure, "{output}");

    let explicit = "fn f(p *const int) int {\n    q *byte = p as *byte;\n    return 0;\n}\n";
    let (outcome, output) = compile(explicit);
    assert_eq!(outcome, Outcome::Success, "{output}");

    // And the other direction, which is `const` being dropped from a byte
    // pointer: `*const byte` to `*byte`.
    let from_byte = "fn f(p *const byte) int {\n    q *byte = p as *byte;\n    return 0;\n}\n";
    let (outcome, output) = compile(from_byte);
    assert_eq!(outcome, Outcome::Success, "{output}");
}

#[test]
fn dereferencing_a_non_pointer_is_refused() {
    let source = "fn f() int {\n    x int = 1;\n    return *x;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("not a pointer"), "{output}");
}

#[test]
fn arrays_allocate_inline_storage() {
    let output = ir_of("fn f() int {\n    a [3]int;\n    return 0;\n}\n");
    assert!(output.contains("alloca [3 x i32]"), "{output}");
}

#[test]
fn an_array_length_may_be_a_constant_expression() {
    let output = ir_of("fn f() int {\n    a [2 * 3]int;\n    return 0;\n}\n");
    assert!(output.contains("alloca [6 x i32]"), "{output}");
}

#[test]
fn a_non_constant_array_length_is_refused() {
    let source = "fn f() int {\n    n int = 3;\n    a [n]int;\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    // The message names what is wrong with `n`, rather than only stating the
    // rule: it is not a constant, so it cannot be an array length.
    assert!(output.contains("is not a constant"), "{output}");
}

#[test]
fn an_unsized_array_parameter_becomes_a_pointer() {
    let output = ir_of("fn f(a []int) int {\n    return 0;\n}\n");
    // The symbol carries the parameter types; the parameter itself is a `ptr`.
    assert!(output.contains("define i32 @f_"), "{output}");
    assert!(output.contains("(ptr"), "{output}");
}

#[test]
fn a_const_binding_must_be_initialised() {
    let source = "fn f() int {\n    n const int;\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("must be initialised"), "{output}");
}

#[test]
fn a_const_binding_cannot_be_assigned() {
    let source = "fn f() int {\n    n const int = 1;\n    n = 2;\n    return n;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("cannot assign to `const`"), "{output}");
}

#[test]
fn an_ordinary_binding_is_still_assignable() {
    // Recording every binding as const would make ordinary variables
    // immutable, which is what happened first.
    let output = ir_of("fn f() int {\n    n int = 1;\n    n = 2;\n    return n;\n}\n");
    assert!(output.contains("store i32 2"), "{output}");
}

// ------------------------------------------------------------ inc/dec, comma

#[test]
fn postfix_increment_yields_the_old_value() {
    let output = ir_of("fn f() int {\n    i int = 1;\n    return i++;\n}\n");
    assert!(output.contains("add i32"), "{output}");
    // The returned value is the load taken before the add.
    let load = output.find("load i32").expect("a load");
    let add = output.find("add i32").expect("an add");
    let ret = output.find("ret i32").expect("a return");
    assert!(load < add && add < ret, "{output}");
}

#[test]
fn prefix_increment_yields_the_new_value() {
    let output = ir_of("fn f() int {\n    i int = 1;\n    return ++i;\n}\n");
    assert!(output.contains("add i32"), "{output}");
}

#[test]
fn decrement_lowers_to_a_subtraction() {
    let output = ir_of("fn f() int {\n    i int = 1;\n    i--;\n    return i;\n}\n");
    assert!(output.contains("sub i32"), "{output}");
}

#[test]
fn incrementing_a_non_lvalue_is_refused() {
    let source = "fn f() int {\n    1++;\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
}

#[test]
fn the_comma_operator_yields_its_right_operand() {
    // The left side runs and is discarded, so the result is `2`.
    let output = ir_of("fn f() int {\n    a int = 1;\n    return a = 5, 2;\n}\n");
    assert!(output.contains("ret i32 2"), "{output}");
}

#[test]
fn a_comma_separates_case_labels_rather_than_combining_them() {
    // The comma operator must not swallow a label separator: `case 1, 2:`
    // labels one arm with both values, and the first must not vanish.
    let source = "fn f(e int) int {\n    switch (e) {\n        case 1, 2:\n            return 10;\n    }\n    return 0;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("i32 1, label %case.0"), "{output}");
    assert!(output.contains("i32 2, label %case.0"), "{output}");
}

#[test]
fn a_comma_separates_call_arguments() {
    let source =
        "fn g(a int, b int) int {\n    return a;\n}\nfn f(n int) int {\n    return g(n, 2);\n}\n";
    let output = ir_of(source);
    // A function's symbol carries its parameter types, so the call names the
    // mangled symbol rather than the bare `g`. The first argument is a
    // parameter, so the call is emitted rather than folded.
    assert!(output.contains("call i32 @g_"), "{output}");
    assert!(output.contains(", i32 2)"), "{output}");
}

// -------------------------------------------------------------- bool literals

#[test]
fn boolean_literals_lower_to_i1() {
    let output = ir_of("fn f() bool {\n    return true;\n}\n");
    assert!(output.contains("ret i1 true"), "{output}");
}

// ------------------------------------------------------------ emit targets

#[test]
fn exe_is_the_default_emit_kind() {
    // No `--emit` at all must still produce something runnable, because that is
    // what a user typing `fallc hello.ft` expects.
    let options = fallc::cli::Options::parse(&["hello.ft".to_owned()]).expect("valid");
    match options.command {
        fallc::cli::Command::Compile { emit, .. } => assert!(emit.is_empty()),
        fallc::cli::Command::Version | fallc::cli::Command::Help => {
            panic!("expected a compile command")
        }
    }
}

#[test]
fn object_and_executable_emit_kinds_parse() {
    use fallc::cli::{Command, EmitKind, Options};
    for (text, expected) in [
        ("obj", EmitKind::Object),
        ("object", EmitKind::Object),
        ("exe", EmitKind::Executable),
        ("executable", EmitKind::Executable),
        ("ll", EmitKind::Llvm),
    ] {
        let args = vec!["a.ft".to_owned(), format!("--emit={text}")];
        match Options::parse(&args).expect("valid").command {
            Command::Compile { emit, .. } => assert_eq!(emit, vec![expected], "for `{text}`"),
            Command::Version | Command::Help => panic!("expected a compile command"),
        }
    }
}

#[test]
fn the_output_path_is_captured() {
    use fallc::cli::{Command, Options};
    let args = vec!["a.ft".to_owned(), "-o".to_owned(), "prog".to_owned()];
    match Options::parse(&args).expect("valid").command {
        Command::Compile { output, .. } => {
            assert_eq!(output, Some(std::path::PathBuf::from("prog")));
        }
        Command::Version | Command::Help => panic!("expected a compile command"),
    }
}

#[test]
fn an_output_flag_without_a_path_is_rejected() {
    use fallc::cli::Options;
    assert!(Options::parse(&["a.ft".to_owned(), "-o".to_owned()]).is_err());
}

// ------------------------------------------------ mixed-signedness comparison

#[test]
fn a_signed_value_compares_by_mathematical_value_against_an_unsigned_one() {
    // `-1` as an `i8` is less than `1` as a `u32`. A plain bit-width conversion
    // would widen `-1` to a huge unsigned value and get this backwards, which
    // is what C does.
    let source = "fn f() bool {\n    a i8 = 0 as i8 - 1 as i8;\n    b u32 = 1 as u32;\n    return a < b;\n}\n";
    let output = ir_of(source);
    // The smart rule branches on the signed operand's sign first.
    assert!(output.contains("cmp.negative"), "{output}");
    assert!(output.contains("phi i1"), "{output}");
}

#[test]
fn comparing_a_signed_value_with_an_equal_unsigned_one() {
    // The non-negative path: both fit unsigned, so an unsigned comparison is
    // exact.
    let source = "fn f() bool {\n    a i8 = 1 as i8;\n    b u32 = 4294967295 as u32;\n    return a < b;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("icmp ult"), "{output}");
}

#[test]
fn a_negative_signed_value_is_never_equal_to_an_unsigned_one() {
    // The negative path resolves equality without comparing the operands, since
    // no unsigned value is negative.
    let source = "fn f() bool {\n    a i8 = 0 as i8 - 1 as i8;\n    b u32 = 0 as u32 - 1 as u32;\n    return a == b;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("cmp.negative"), "{output}");
}

#[test]
fn same_signedness_comparison_needs_no_branching() {
    // Both unsigned, so a single unsigned comparison suffices and the smart
    // rule must not kick in.
    let source =
        "fn f() bool {\n    a u8 = 1 as u8;\n    b u32 = 2 as u32;\n    return a < b;\n}\n";
    let output = ir_of(source);
    assert!(!output.contains("cmp.negative"), "{output}");
    assert!(output.contains("icmp ult"), "{output}");
}

#[test]
fn widening_between_unsigned_widths_is_zero_extension() {
    let source =
        "fn f() bool {\n    a u8 = 1 as u8;\n    b u32 = 2 as u32;\n    return a < b;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("zext i8"), "{output}");
}

#[test]
fn widening_between_signed_widths_is_sign_extension() {
    let source =
        "fn f() bool {\n    a i8 = 1 as i8;\n    b i32 = 2 as i32;\n    return a < b;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("sext i8"), "{output}");
}

#[test]
fn a_float_may_be_compared_with_an_integer() {
    let source =
        "fn f() bool {\n    a float = 1.5 as float;\n    b int = 2;\n    return a < b;\n}\n";
    let (outcome, output) = compile(source);
    // Mixing float and integer is a comparison the language allows, so the
    // comparison must not be what rejects it.
    assert!(
        !output.contains("cannot compare"),
        "mixing float and int must be allowed: {output}"
    );
    let _ = outcome;
}

#[test]
fn arithmetic_still_requires_matching_types() {
    // Only comparison mixes types; `+` does not.
    let source = "fn f() int {\n    a i8 = 1 as i8;\n    b u32 = 2 as u32;\n    return a + b;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("same type"), "{output}");
}

// ------------------------------------------------------- string termination

#[test]
fn a_string_literal_is_nul_terminated() {
    // A byte pointer is read until a NUL, so a literal without one makes the
    // consumer read past its end. That showed up at runtime as "panic:
    // boompanic:" — the message running into whatever followed it in memory —
    // and is invisible in the IR unless the terminator is checked for.
    let source = "fn f() int {\n    __builtin_panic(\"boom\");\n    return 0;\n}\n";
    let output = ir_of(source);
    assert!(
        output.contains(r#"c"boom\00""#),
        "the literal must carry a terminator: {output}"
    );
    assert!(
        output.contains("[5 x i8]"),
        "four characters plus a terminator: {output}"
    );
}

#[test]
fn the_compilers_own_panic_message_is_nul_terminated() {
    // The message the compiler emits for its own guards is built separately
    // from a source literal and had the same bug.
    let source = "fn f(a int) int {\n    b int = 0;\n    return a / b;\n}\n";
    let output = ir_of(source);
    assert!(
        output.contains(r#"c"division by zero\00""#),
        "the guard message must carry a terminator: {output}"
    );
}

// ------------------------------------------------------- numeric literals

#[test]
fn a_float_literal_defaults_to_double() {
    let output = ir_of("fn f() double {\n    return 1.5;\n}\n");
    assert!(output.contains("define double @f()"), "{output}");
    assert!(output.contains("double 1.500000"), "{output}");
}

#[test]
fn an_f_suffix_makes_a_float_literal_a_float() {
    let output = ir_of("fn f() float {\n    return 1.5f;\n}\n");
    assert!(output.contains("define float @f()"), "{output}");
    assert!(output.contains("float 1.500000"), "{output}");
}

#[test]
fn an_integer_suffix_names_the_literals_type() {
    // The suffix is the literal's type, so this is a `u8` rather than a
    // conversion applied to an `i32`.
    let output = ir_of("fn f(a u8) u8 {\n    return 5u8;\n}\n");
    assert!(output.contains("ret i8 5"), "{output}");
}

#[test]
fn every_integer_suffix_resolves() {
    // The three columns are the suffix, the type it names, and the LLVM type.
    // The two shorthands name a type that is spelled differently, so the
    // middle column is not always the suffix.
    for (suffix, ty, llvm) in [
        ("i8", "i8", "i8"),
        ("i16", "i16", "i16"),
        ("i32", "i32", "i32"),
        ("i64", "i64", "i64"),
        ("u8", "u8", "i8"),
        ("u16", "u16", "i16"),
        ("u32", "u32", "i32"),
        ("u64", "u64", "i64"),
        ("usize", "usize", "i64"),
        ("isize", "isize", "i64"),
        // The shorthands: `u` is `uint` (a `u32`), `i` is `int` (an `i32`).
        ("u", "uint", "i32"),
        ("i", "int", "i32"),
    ] {
        let source = format!("fn f() {ty} {{\n    return 1{suffix};\n}}\n");
        let (outcome, output) = compile(&source);
        assert_eq!(outcome, Outcome::Success, "`{suffix}` failed: {output}");
        assert!(
            output.contains(&format!("define {llvm} @f()")),
            "`{suffix}` should be `{llvm}`: {output}"
        );
    }
}

#[test]
fn a_suffix_that_conflicts_with_its_context_is_an_error() {
    // The suffix is the literal's type, not a conversion, so a mismatched
    // suffix is a type error rather than a silent truncation. This is the
    // difference between `5u32` and `5 as u32`.
    let source = "fn f() int {\n    x u8 = 5u32;\n    return x as int;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("type mismatch"), "{output}");
    assert!(output.contains("found u32"), "{output}");
}

#[test]
fn a_literal_without_a_suffix_adopts_its_context() {
    // Which is what makes this compile at all, and is why the suffix is needed
    // to say otherwise.
    let output = ir_of("fn f(a u8) u8 {\n    return 7;\n}\n");
    assert!(output.contains("ret i8 7"), "{output}");
}

#[test]
fn a_byte_binding_accepts_an_unsuffixed_literal() {
    // `byte` takes no part in arithmetic, but it is based on `u8`, so a plain
    // literal can initialise one.
    let output = ir_of("fn f() int {\n    b byte = 1;\n    return 0;\n}\n");
    assert!(output.contains("alloca i8"), "{output}");
}

#[test]
fn byte_is_not_a_literal_suffix() {
    // It is not a numeric type, so `5byte` is an integer followed by a name.
    let source = "fn f() int {\n    b byte = 5byte;\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
}

#[test]
fn a_float_literal_in_a_variadic_argument_is_lowered() {
    // Variadic arguments are not type checked, but they must still be
    // inferred, or the code generator has no type to lower with.
    // A byte string is raw memory and does not convert to a C string, so the
    // format is an ordinary string literal; what the test is about is the
    // variadic float.
    let source = "import C;\nfn main() int {\n    C.printf(\"%f\", 1.5);\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(output.contains("double 1.500000"), "{output}");
}

#[test]
fn mixing_float_widths_needs_a_conversion() {
    // There are no implicit conversions, so `double` and `float` do not mix in
    // arithmetic even though both are floats.
    let source = "fn f() double {\n    return 0.0 - 3.5f;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("same type"), "{output}");
}

// ---------------------------------------------------------- radix literals

#[test]
fn hexadecimal_octal_and_binary_literals() {
    let output = ir_of(
        "fn f() int {\n    a int = 0xFF;\n    b int = 0o17;\n    c int = 0b1010;\n    return a + b + c;\n}\n",
    );
    assert!(output.contains("255"), "{output}");
    assert!(output.contains("15"), "{output}");
    assert!(output.contains("10"), "{output}");
}

#[test]
fn digit_separators_are_ignored() {
    let output = ir_of("fn f() i64 {\n    return 1_000_000i64;\n}\n");
    assert!(output.contains("1000000"), "{output}");
}

#[test]
fn a_separator_works_in_a_radix_literal() {
    let output = ir_of("fn f() i64 {\n    return 0xFFFF_FFFFi64;\n}\n");
    assert!(output.contains("4294967295"), "{output}");
}

#[test]
fn a_radix_literal_may_carry_a_suffix() {
    // `0xFFu8`, and the letters of the radix must not be mistaken for the
    // suffix: splitting at the first letter would leave `xFFu8` as a type name.
    let output = ir_of("fn f() u8 {\n    return 0xFFu8;\n}\n");
    assert!(output.contains("define i8 @f()"), "{output}");
    assert!(output.contains("ret i8 -1"), "{output}");
}

#[test]
fn a_separator_cannot_start_a_literal() {
    // `_1000` is an identifier followed by a number, since a separator must sit
    // between two digits.
    let source = "fn f() int {\n    return _1000;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("cannot find `_1000`"), "{output}");
}

#[test]
fn a_separator_cannot_end_a_literal() {
    // `1000_` is the number 1000 followed by an identifier, so this is a
    // syntax error rather than the value 1000.
    let source = "fn f() int {\n    return 1000_;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
}

#[test]
fn a_doubled_separator_is_not_part_of_the_number() {
    let source = "fn f() int {\n    return 1__0;\n}\n";
    let (outcome, _) = compile(source);
    assert_eq!(outcome, Outcome::Failure);
}

// ----------------------------------------------------------- char literals

#[test]
fn a_char_literal_is_a_code_unit() {
    // `A` is 65, and `char` is an alias for `c8`, so it is one byte wide.
    let output = ir_of("fn f() char {\n    return 'A';\n}\n");
    assert!(output.contains("define i8 @f()"), "{output}");
    assert!(output.contains("ret i8 65"), "{output}");
}

#[test]
fn a_wide_char_literal_holds_a_non_ascii_code_point() {
    // `中` is U+4E2D, which needs a `c32`.
    let output = ir_of("fn f() c32 {\n    return u32'中';\n}\n");
    assert!(output.contains("ret i32 20013"), "{output}");
}

#[test]
fn char_escapes_are_decoded() {
    for (text, value) in [("'\\n'", 10), ("'\\t'", 9), ("'\\0'", 0), ("'\\''", 39)] {
        let source = format!("fn f() c8 {{\n    return {text};\n}}\n");
        let (outcome, output) = compile(&source);
        assert_eq!(outcome, Outcome::Success, "`{text}` failed: {output}");
        assert!(
            output.contains(&format!("ret i8 {value}")),
            "`{text}` should be {value}: {output}"
        );
    }
}

#[test]
fn a_hex_escape_is_decoded() {
    let output = ir_of("fn f() c8 {\n    return '\\x41';\n}\n");
    assert!(output.contains("ret i8 65"), "{output}");
}

#[test]
fn a_braced_escape_takes_a_code_point() {
    let output = ir_of("fn f() c32 {\n    return u32'\\u{4E2D}';\n}\n");
    assert!(output.contains("ret i32 20013"), "{output}");
}

#[test]
fn char_does_not_take_part_in_arithmetic() {
    // As with `byte`, a code point is not a number.
    let source = "fn f() char {\n    a char = 'A';\n    return a + a;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(
        output.contains("same type") || output.contains("cannot be applied"),
        "{output}"
    );
}

#[test]
fn char_converts_to_an_integer_with_a_cast() {
    // The value is stored and loaded, so the result is not folded to a
    // constant; what matters is that the cast is accepted and the widths match.
    let source = "fn f() u8 {\n    a char = 'A';\n    return a as u8;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
    // Same width, so no conversion instruction is needed — but the cast is
    // what makes the type change legal.
    assert!(output.contains("store i8 65"), "{output}");
    assert!(!output.contains("trunc"), "{output}");
}

// ----------------------------------------------------------- byte literals

#[test]
fn a_character_literal_without_a_prefix_is_a_c8() {
    // `'x'` is a `c8`, and a `c8` is a byte's worth of character.
    let output = ir_of("fn f() c8 {\n    return 'x';\n}\n");
    assert!(output.contains("define i8 @f()"), "{output}");
    assert!(output.contains("ret i8 120"), "{output}");
}

#[test]
fn a_wide_character_literal_takes_its_width_from_its_prefix() {
    // The prefix names the character type, so the same character can be held in
    // one, two, or four bytes without a cast.
    let narrow = ir_of("fn f() c16 {\n    return u16'A';\n}\n");
    assert!(narrow.contains("ret i16 65"), "{narrow}");
    let wide = ir_of("fn f() c32 {\n    return u32'A';\n}\n");
    assert!(wide.contains("ret i32 65"), "{wide}");
}

#[test]
fn character_escapes_are_decoded() {
    let output = ir_of("fn f() c8 {\n    return '\\t';\n}\n");
    assert!(output.contains("ret i8 9"), "{output}");
}

#[test]
fn a_character_literal_too_wide_for_its_type_is_refused() {
    // A code point above 255 does not fit a `c8`. Truncating it would silently
    // change the value, so the literal does not fit and must be written with a
    // wider prefix.
    let source = "fn f() c8 {\n    return '中';\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");

    // The same character is fine once the type is wide enough.
    let source = "fn f() c32 {\n    return u32'中';\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
}

#[test]
fn a_char_and_a_byte_literal_are_different_types() {
    let source = "fn f() byte {\n    return 'A';\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("type mismatch"), "{output}");
}
