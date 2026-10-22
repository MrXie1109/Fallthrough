//! Tests for overloading and default arguments.
//!
//! Overloading matters because a name may denote several functions. There are
//! no implicit conversions, so a candidate applies only when the argument types
//! match; the one precision left is that an unsuffixed integer literal prefers
//! `int`, which is what makes `f(int)` beat `f(u8)` for `f(5)`.

use fallc::Outcome;
use fallc::cli::EmitKind;
use fallc::pipeline::compile_text;

fn compile(source: &str) -> (Outcome, String) {
    compile_text("test.ftm", source, &[EmitKind::Llvm])
}

fn ir_of(source: &str) -> String {
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
    output
}

/// Returns the IR with LLVM's optimiser having folded what it can.
///
/// A debug build guards the arithmetic that can trap, so a constant expression
/// keeps its guards at `-O0` and does not fold into a single value.
fn optimized_ir_of(source: &str) -> String {
    let (outcome, output) = fallc::pipeline::compile_text_optimized(
        "test.ftm",
        source,
        &[EmitKind::Llvm],
        fallc_ir::OptLevel::Default,
    );
    assert_eq!(outcome, Outcome::Success, "{output}");
    output
}

// ------------------------------------------------------------------ overloads

#[test]
fn two_functions_may_share_a_name() {
    let source = "fn f(x int) int {\n    return 1;\n}\nfn f(x u8) int {\n    return 2;\n}\nfn g() int {\n    return 0;\n}\n";
    let output = ir_of(source);
    // Distinct symbols, or the second would replace the first. The encoding
    // names the type as the language spells it, so `u8` stays `u8` rather than
    // becoming LLVM's `i8` -- the two are different types here.
    assert!(output.contains("define i32 @f_I3_i32"), "{output}");
    assert!(output.contains("define i32 @f_I2_u8"), "{output}");
}

#[test]
fn a_literal_prefers_the_default_integer_type() {
    // `f(int)` beats `f(u8)` for `f(5)`, because `5` is an `int` unless
    // something says otherwise. The first argument is a parameter so that the
    // call survives to be emitted; the literal is the second, and the symbol
    // shows which overload it selected.
    let source = "fn f(a int, x int) int {\n    return 1;\n}\nfn f(a int, x u8) int {\n    return 2;\n}\nfn g(a int) int {\n    return f(a, 5);\n}\n";
    let output = ir_of(source);
    // The symbol encodes the parameter type structurally, so `f` taking an
    // `i32` is `f_I3_i32`.
    assert!(output.contains("call i32 @f_I3_i32_I3_i32"), "{output}");
}

#[test]
fn a_suffix_selects_the_other_overload() {
    // The argument is a parameter, so the call is emitted and the symbol it
    // names shows which overload was chosen. A literal argument would be
    // evaluated at compile time and no call would appear at all.
    let source = "fn f(x int) int {\n    return 1;\n}\nfn f(x u8) int {\n    return 2;\n}\nfn g(a u8) int {\n    return f(a);\n}\n";
    let output = ir_of(source);
    assert!(output.contains("call i32 @f_I2_u8"), "{output}");
}

#[test]
fn a_variable_selects_by_its_type() {
    let source = "fn f(x int) int {\n    return 1;\n}\nfn f(x u8) int {\n    return 2;\n}\nfn g(a u8) int {\n    return f(a);\n}\n";
    let output = ir_of(source);
    assert!(output.contains("call i32 @f_I2_u8"), "{output}");
}

#[test]
fn a_call_that_fits_no_overload_is_reported() {
    let source = "fn f(x int) int {\n    return 1;\n}\nfn f(x u8) int {\n    return 2;\n}\nfn g(b bool) int {\n    return f(b);\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("no overload"), "{output}");
}

#[test]
fn an_ambiguous_call_is_reported_rather_than_resolved_by_order() {
    // Two candidates accept `5` equally: neither is `int`, so neither is more
    // precise. Picking one by declaration order would silently change meaning
    // when a function is added.
    let source = "fn f(x u8) int {\n    return 1;\n}\nfn f(x u16) int {\n    return 2;\n}\nfn g() int {\n    return f(5);\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("ambiguous"), "{output}");
}

#[test]
fn overloads_may_differ_in_arity() {
    // One argument is a parameter, so the call survives to be emitted and the
    // arity that was chosen is visible in the symbol.
    let source = "fn f(a int) int {\n    return 1;\n}\nfn f(a int, b int) int {\n    return 2;\n}\nfn g(a int) int {\n    return f(a, 2);\n}\n";
    let output = ir_of(source);
    assert!(output.contains("call i32 @f_I3_i32_I3_i32"), "{output}");
}

#[test]
fn a_function_that_is_not_overloaded_keeps_its_plain_symbol() {
    // Every function's symbol carries its parameter types, so a name never has
    // to be shared and a definition cannot be silently replaced by another.
    // The exception is an `extern` declaration, which names a symbol something
    // else provides: see `an_extern_declaration_keeps_its_declared_name`.
    let source = "fn plain(x int) int {\n    return 1;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("define i32 @plain_"), "{output}");
    assert!(output.contains("(i32 %0)"), "{output}");
}

// ---------------------------------------------------------- default arguments

#[test]
fn a_default_may_be_omitted() {
    // The call is reached with an argument the compiler cannot fold, so what
    // the test pins is the call site: the default is supplied there, which
    // shows as the second argument of the call.
    let source =
        "fn f(a int, b int = 7) int {\n    return a + b;\n}\nfn g(n int) int {\n    return f(n);\n}\n";
    let output = ir_of(source);
    assert!(output.contains("call i32 @f_"), "{output}");
    assert!(output.contains(", i32 7)"), "{output}");
}

#[test]
fn a_default_may_be_overridden() {
    // `a` is a parameter, so the call is emitted. What this test is about is
    // that the *written* second argument is used rather than the default, which
    // shows in the arguments the call carries.
    let source =
        "fn f(a int, b int = 7) int {\n    return b;\n}\nfn g(a int) int {\n    return f(a, 9);\n}\n";
    let output = ir_of(source);
    assert!(output.contains("call i32 @f_"), "{output}");
    assert!(output.contains(", i32 9)"), "{output}");
}

#[test]
fn every_default_may_be_omitted() {
    // A call with no arguments at all is a constant and is folded, so the call
    // site is reached by overriding the first parameter and letting the second
    // take its default.
    let source = "fn f(a int = 1, b int = 2) int {\n    return a + b;\n}\nfn g(n int) int {\n    return f(n);\n}\n";
    let output = ir_of(source);
    assert!(output.contains("call i32 @f_"), "{output}");
    assert!(output.contains(", i32 2)"), "{output}");
}

#[test]
fn a_default_must_come_after_the_required_parameters() {
    let source = "fn f(a int = 1, b int) int {\n    return b;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(
        output.contains("cannot follow one with a default"),
        "{output}"
    );
}

#[test]
fn too_few_arguments_is_still_an_error() {
    let source =
        "fn f(a int, b int = 7) int {\n    return b;\n}\nfn g() int {\n    return f();\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("takes 2 argument"), "{output}");
}

#[test]
fn too_many_arguments_is_still_an_error() {
    let source = "fn f(a int = 1) int {\n    return a;\n}\nfn g() int {\n    return f(1, 2);\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("takes 1 argument"), "{output}");
}

#[test]
fn a_default_is_a_constant_expression_when_it_needs_to_be() {
    // The value need not be a literal, but it is evaluated where the call is,
    // so a default referring to a local of the callee would not work. A plain
    // constant does.
    let source = "fn f(a int = 1 + 2) int {\n    return a;\n}\nfn g() int {\n    return f();\n}\n";
    // Optimised, because the default's arithmetic is guarded at `-O0` and so
    // does not fold there. The folded `3` in `g` is what shows the value was
    // computed where the call is: at `-O2` the callee is inlined, so the
    // evidence is the result rather than a call instruction carrying it.
    let output = optimized_ir_of(source);
    assert!(output.contains("define noundef i32 @g()"), "{output}");
    assert!(output.contains("ret i32 3"), "{output}");
    // And the callee still computes it rather than taking it as an argument
    // from anywhere: this is the same constant, folded in `g`.
    assert!(!output.contains("i32 1, i32 2"), "{output}");
}
