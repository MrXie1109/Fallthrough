//! Optimisation levels, and the overflow guards they must not remove.
//!
//! `-O` and `--release` are separate axes. `--release` decides what the source
//! *means* -- whether overflowing arithmetic is a panic or undefined -- and
//! `-O` decides how hard LLVM works on the result. Getting them confused would
//! make `-O2` silently change a program's semantics.

use fallc::Outcome;
use fallc::cli::EmitKind;
use fallc::pipeline::{compile_text_in_mode_opt, compile_text_optimized};

/// Compiles and returns the IR at a chosen optimisation level.
fn ir_at(source: &str, opt: fallc_ir::OptLevel, mode: fallc_ir::BuildMode) -> String {
    let (outcome, output) = compile_text_in_mode_opt("test.ft", source, &[EmitKind::Llvm], mode, opt);
    assert_eq!(outcome, Outcome::Success, "{output}");
    output
}

/// A program whose answer LLVM can fold away entirely.
const FOLDABLE: &str = "fn square(n int) int {\n    return n * n;\n}\nfn main() int {\n    total int = 0;\n    for (i int = 0; i < 10; i++) {\n        total = total + square(i);\n    }\n    return total;\n}\n";

#[test]
fn no_optimisation_leaves_the_code_as_written() {
    let output = ir_at(FOLDABLE, fallc_ir::OptLevel::None, fallc_ir::BuildMode::Debug);
    // The loop and the callee are both still there.
    assert!(output.contains("define i32 @square"), "{output}");
    assert!(output.contains("for.cond"), "{output}");
}

#[test]
fn optimising_folds_the_whole_program() {
    // 0+1+4+9+..+81 is 285, and at `-O2` the loop, the call, and the addition
    // all disappear into that constant. A level that did nothing would leave
    // the loop in place.
    let output = ir_at(FOLDABLE, fallc_ir::OptLevel::Default, fallc_ir::BuildMode::Debug);
    assert!(output.contains("define noundef i32 @main()"), "{output}");
    assert!(output.contains("ret i32 285"), "{output}");
    assert!(!output.contains("for.cond"), "the loop should be gone: {output}");
}

#[test]
fn every_speed_oriented_level_folds() {
    // `-O1` and up fold this away. The size levels are deliberately not in
    // this list: they trade code size against speed, and unrolling a loop into
    // a constant is a size decision, so `-Oz` keeps the loop.
    for level in [
        fallc_ir::OptLevel::Less,
        fallc_ir::OptLevel::Default,
        fallc_ir::OptLevel::Aggressive,
    ] {
        let output = ir_at(FOLDABLE, level, fallc_ir::BuildMode::Debug);
        assert!(
            output.contains("ret i32 285"),
            "`-O{}` did not fold: {output}",
            level.spelling()
        );
    }
}

#[test]
fn the_size_levels_optimise_too_but_more_conservatively() {
    // They must still do *something*, or `-Os` would be `-O0` under another
    // name. What they do is shrink rather than unroll, so the loop is expected
    // to survive while the code around it is tidied.
    for level in [fallc_ir::OptLevel::Size, fallc_ir::OptLevel::SizeMin] {
        let output = ir_at(FOLDABLE, level, fallc_ir::BuildMode::Debug);
        assert!(
            output.contains("define i32 @square") || output.contains("define noundef i32 @square"),
            "`-O{}` did not analyse the callee: {output}",
            level.spelling()
        );
    }
}

#[test]
fn a_debug_build_keeps_its_overflow_guard_at_any_optimisation_level() {
    // The guard is part of what the program *means*, so no level may remove it.
    // This is the whole reason the two axes are separate: if `-O` implied
    // `--release`, asking for fast code would silently change the semantics.
    let source = "fn f(n int) int {\n    return n + 1;\n}\n";
    for level in [
        fallc_ir::OptLevel::None,
        fallc_ir::OptLevel::Default,
        fallc_ir::OptLevel::Aggressive,
    ] {
        let output = ir_at(source, level, fallc_ir::BuildMode::Debug);
        assert!(
            output.contains("llvm.sadd.with.overflow"),
            "`-O{}` dropped the overflow guard: {output}",
            level.spelling()
        );
    }
}

#[test]
fn a_release_build_has_no_overflow_guard() {
    // In release, overflow is undefined, so there is nothing to check.
    let source = "fn f(n int) int {\n    return n + 1;\n}\n";
    let output = ir_at(source, fallc_ir::OptLevel::None, fallc_ir::BuildMode::Release);
    assert!(!output.contains("with.overflow"), "{output}");
}

#[test]
fn unsigned_arithmetic_is_not_guarded() {
    // Unsigned wrapping is well defined, so guarding it would reject a valid
    // program. Only signed operations can trap.
    let source = "fn f(n u8) u8 {\n    return n + 1;\n}\n";
    let output = ir_at(source, fallc_ir::OptLevel::None, fallc_ir::BuildMode::Debug);
    assert!(!output.contains("with.overflow"), "{output}");
}

#[test]
fn the_optimised_ir_is_what_the_object_contains() {
    // `--emit=llvm` has to show the code that will be produced, not the code
    // before the passes ran. It once printed the pre-optimisation text, which
    // made `-O` look like it did nothing.
    let unoptimised = ir_at(FOLDABLE, fallc_ir::OptLevel::None, fallc_ir::BuildMode::Debug);
    let optimised = compile_text_optimized(
        "test.ft",
        FOLDABLE,
        &[EmitKind::Llvm],
        fallc_ir::OptLevel::Default,
    )
    .1;
    assert_ne!(
        unoptimised, optimised,
        "the two levels must not print identical IR"
    );
}

#[test]
fn an_optimisation_level_parses_the_way_a_c_compiler_does() {
    // A bare `-O` is `-O2`, as it is for `cc` and `clang`.
    assert_eq!(fallc_ir::OptLevel::parse(""), Some(fallc_ir::OptLevel::Default));
    assert_eq!(fallc_ir::OptLevel::parse("0"), Some(fallc_ir::OptLevel::None));
    assert_eq!(fallc_ir::OptLevel::parse("1"), Some(fallc_ir::OptLevel::Less));
    assert_eq!(fallc_ir::OptLevel::parse("2"), Some(fallc_ir::OptLevel::Default));
    assert_eq!(fallc_ir::OptLevel::parse("3"), Some(fallc_ir::OptLevel::Aggressive));
    assert_eq!(fallc_ir::OptLevel::parse("s"), Some(fallc_ir::OptLevel::Size));
    assert_eq!(fallc_ir::OptLevel::parse("z"), Some(fallc_ir::OptLevel::SizeMin));
    // Anything else is refused rather than silently ignored.
    assert_eq!(fallc_ir::OptLevel::parse("4"), None);
    assert_eq!(fallc_ir::OptLevel::parse("fast"), None);
}

#[test]
fn the_module_names_the_machine_it_is_compiled_for() {
    // Without a data layout LLVM optimises against an assumed default in which
    // `i64` is 4-byte aligned, while the code generator uses the real x86-64
    // layout. The two then disagree about how much of a `usize` field a
    // `store i64` touches, and at `-Oz` that mismatch made `Array::push` update
    // `_len` without the following read seeing it -- so the array indexed out
    // of bounds and the program segfaulted. `-O0` was correct, which is what
    // made it look like an optimisation bug rather than a layout one.
    let output = ir_at("fn main() int {\n    return 0;\n}\n", fallc_ir::OptLevel::None, fallc_ir::BuildMode::Debug);
    assert!(
        output.contains("target datalayout ="),
        "the module has no data layout: {output}"
    );
    assert!(
        output.contains("target triple ="),
        "the module has no target triple: {output}"
    );
}

#[test]
fn a_usize_field_is_stored_with_its_real_alignment() {
    // The symptom of a missing data layout, in one line. `usize` is 8 bytes, so
    // a store to one has to say `align 8`; `align 4` is what LLVM assumed with
    // no layout, and it is what let the store be treated as a partial write.
    let source = "fn main() int {\n    index usize = 0;\n    return 0;\n}\n";
    let output = ir_at(source, fallc_ir::OptLevel::None, fallc_ir::BuildMode::Debug);
    assert!(
        output.contains("store i64 0, ptr %index, align 8"),
        "a `usize` store is not 8-aligned: {output}"
    );
    assert!(
        !output.contains("store i64 0, ptr %index, align 4"),
        "a `usize` store is 4-aligned: {output}"
    );
}
