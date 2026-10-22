//! Uses of a template parameter pack: folding it, counting it, indexing it.
//!
//! A pack is a list of compile-time values, so all three are answered while
//! compiling: `(values + ...)` combines the elements, `sizeof...(values)`
//! counts them, and `values[1]` picks one out. What each produces is a
//! constant, which is what lets it stand where a constant is required.

use fallc::Outcome;
use fallc::cli::EmitKind;
use fallc::pipeline::compile_text;

/// Compiles source and returns the LLVM IR.
fn ir_of(source: &str) -> String {
    let (outcome, output) = compile_text("test.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Success, "{output}");
    output
}

/// Compiles source that must be rejected, and returns the diagnostics.
fn errors_of(source: &str) -> String {
    let (outcome, output) = compile_text("test.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    output
}

#[test]
fn a_fold_combines_the_elements_of_a_pack() {
    // `1 + 2 + 3` is 6, known before the program runs, so the body is the
    // number rather than a loop over the pack.
    let source = "template <values ...int>\nconstexpr fn sum() int {\n    return (values + ...);\n}\nfn main() int {\n    return sum<1, 2, 3>();\n}\n";
    let output = ir_of(source);
    assert!(output.contains("ret i32 6"), "{output}");
}

#[test]
fn two_instantiations_of_a_fold_have_different_values() {
    // The body is shared; what differs is the pack. Each instantiation folds
    // its own elements, which is the point of making the arguments part of its
    // name: one function returns 3 and the other 10.
    let source = "template <values ...int>\nconstexpr fn sum() int {\n    return (values + ...);\n}\nfn main() int {\n    return sum<1, 2>() + sum<10>();\n}\n";
    let output = ir_of(source);
    assert!(output.contains("ret i32 3"), "{output}");
    assert!(output.contains("ret i32 10"), "{output}");
}

#[test]
fn a_fold_may_use_any_operator_the_language_has() {
    // The operator is recorded with the fold, so what it does is the ordinary
    // rule for that operator rather than a table of its own.
    let source = "template <values ...int>\nconstexpr fn product() int {\n    return (values * ...);\n}\nfn main() int {\n    return product<2, 3, 4>();\n}\n";
    let output = ir_of(source);
    assert!(output.contains("ret i32 24"), "{output}");
}

#[test]
fn folding_an_empty_pack_is_reported() {
    // The fold of no elements would need an identity for the operator, and
    // inventing one would make `sum<>()` mysteriously a number.
    let source = "template <values ...int>\nconstexpr fn sum() int {\n    return (values + ...);\n}\nfn main() int {\n    return sum<>();\n}\n";
    let output = errors_of(source);
    assert!(output.contains("empty pack"), "{output}");
}

#[test]
fn sizeof_counts_the_elements_of_a_pack() {
    // A count, not a size in bytes: a pack has no layout. Zero is a count like
    // any other, which is what makes `count<>()` answer rather than fail.
    let source = "template <values ...int>\nconstexpr fn count() usize {\n    return sizeof...(values);\n}\nfn main() int {\n    return count<7, 8, 9>() as int + count<>() as int;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("ret i64 3"), "{output}");
    assert!(output.contains("ret i64 0"), "{output}");
}

#[test]
fn sizeof_of_a_pack_is_usable_as_an_array_length() {
    // The count is a constant, so it stands where a constant is required --
    // which is the test of whether it really was folded.
    let source = "template <values ...int>\nconstexpr fn buffer() int {\n    a [sizeof...(values)]int;\n    a[2] = 5;\n    return a[2];\n}\nfn main() int {\n    return buffer<1, 2, 3>();\n}\n";
    let output = ir_of(source);
    assert!(output.contains("alloca [3 x i32]"), "{output}");
}

#[test]
fn a_pack_element_is_selected_by_a_constant_index() {
    // Every element is a compile-time value, so which one the program means
    // has to be fixed while compiling too.
    let source = "template <values ...int>\nconstexpr fn second() int {\n    return values[1];\n}\nfn main() int {\n    return second<10, 20, 30>();\n}\n";
    let output = ir_of(source);
    assert!(output.contains("ret i32 20"), "{output}");
}

#[test]
fn a_pack_index_may_be_computed_from_the_count() {
    // `sizeof...` is a constant expression like any other, so the index may be
    // arithmetic over it -- which is how the last element is written.
    let source = "template <values ...int>\nconstexpr fn last() int {\n    return values[sizeof...(values) - 1];\n}\nfn main() int {\n    return last<4, 5, 6>();\n}\n";
    let output = ir_of(source);
    assert!(output.contains("ret i32 6"), "{output}");
}

#[test]
fn a_pack_index_past_the_end_is_reported() {
    let source = "template <values ...int>\nconstexpr fn at() int {\n    return values[3];\n}\nfn main() int {\n    return at<1, 2, 3>();\n}\n";
    let output = errors_of(source);
    assert!(output.contains("out of range"), "{output}");
}

#[test]
fn a_pack_index_must_be_a_constant() {
    // A runtime value cannot select an element, because the elements do not
    // exist at run time.
    let source = "template <values ...int>\nconstexpr fn at(i usize) int {\n    return values[i];\n}\nfn main() int {\n    return at<1, 2, 3>(0);\n}\n";
    let output = errors_of(source);
    assert!(output.contains("must be a constant"), "{output}");
}

#[test]
fn a_template_call_may_stand_where_a_constant_is_required() {
    // `sum<1, 2, 3>()` is a `constexpr` template, so it belongs where the
    // language needs a constant -- an array length is the case that matters.
    // The length is resolved before the statement's expressions are checked,
    // so the call had not been instantiated yet and nothing could fold it.
    let source = "template <values ...int>\nconstexpr fn sum() int {\n    return (values + ...);\n}\nfn main() int {\n    a [sum<1, 2, 3>()]int;\n    a[5] = 1;\n    return a[5];\n}\n";
    let output = ir_of(source);
    assert!(output.contains("alloca [6 x i32]"), "{output}");
}

#[test]
fn a_template_call_may_size_a_second_array_in_the_same_function() {
    // Two lengths in one function, each its own instantiation: the value is
    // recorded per call, not per name.
    let source = "template <values ...int>\nconstexpr fn sum() int {\n    return (values + ...);\n}\nfn main() int {\n    a [sum<1, 2>()]int;\n    b [sum<2, 2>()]int;\n    a[2] = 1;\n    b[3] = 2;\n    return a[2] + b[3];\n}\n";
    let output = ir_of(source);
    assert!(output.contains("alloca [3 x i32]"), "{output}");
    assert!(output.contains("alloca [4 x i32]"), "{output}");
}

#[test]
fn a_template_call_may_be_the_condition_of_if_constexpr() {
    // The condition is decided before the branch that survives is checked, so
    // it is a constant position like an array length: the call has to be folded
    // and the dead branch dropped.
    let source = "template <values ...int>\nconstexpr fn count() usize {\n    return sizeof...(values);\n}\nfn main() int {\n    x int = 0;\n    if (count<1, 2>() == 2) @constexpr {\n        x = 7;\n    } else {\n        x = 9;\n    }\n    return x;\n}\n";
    let output = ir_of(source);
    // Only the live branch is emitted: the assignment in the dead one is not
    // there at all.
    assert!(output.contains("store i32 7"), "{output}");
    assert!(!output.contains("store i32 9"), "{output}");
}

#[test]
fn the_two_unary_fold_forms_differ_in_which_end_they_start_from() {
    // `(v - ...)` is `10 - (3 - 2)` = 9, and `(... - v)` is `(10 - 3) - 2` = 5:
    // the operator decides nothing about the order; the form does.
    let source = "template <values ...int>\nconstexpr fn right() int {\n    return (values - ...);\n}\ntemplate <values ...int>\nconstexpr fn left() int {\n    return (... - values);\n}\nfn main() int {\n    return right<10, 3, 2>() * 100 + left<10, 3, 2>();\n}\n";
    let output = ir_of(source);
    // Each body is the folded number, so the two ends are visible in the IR.
    assert!(output.contains("ret i32 9"), "{output}");
    assert!(output.contains("ret i32 5"), "{output}");
}

#[test]
fn a_binary_fold_takes_a_starting_value() {
    // `(v - ... - 0)` is `10 - (3 - (2 - 0))` = 9 and `(10 - ... - v)` is
    // `((10 - 10) - 3) - 2` = -5; with an empty pack each is its starting
    // value, 0 and 10.
    let source = "template <values ...int>\nconstexpr fn right() int {\n    return (values - ... - 0);\n}\ntemplate <values ...int>\nconstexpr fn left() int {\n    return (10 - ... - values);\n}\nfn main() int {\n    return right<10, 3, 2>() + left<10, 3, 2>() + right<>() + left<>();\n}\n";
    let output = ir_of(source);
    assert!(output.contains("ret i32 9"), "{output}");
    assert!(output.contains("ret i32 -5"), "{output}");
    // The empty instantiations fold to their starting values.
    assert!(output.contains("ret i32 10"), "{output}");
}

#[test]
fn an_empty_logical_fold_has_the_operator_identity() {
    // The conjunction of nothing is true and the disjunction of nothing is
    // false -- the two identities the language states.
    let source = "template <values ...bool>\nconstexpr fn all() bool {\n    return (values && ...);\n}\ntemplate <values ...bool>\nconstexpr fn any() bool {\n    return (values || ...);\n}\nfn main() int {\n    if (all<>()) {\n        a int = 1;\n        if (!any<>()) {\n            return 7;\n        }\n    }\n    return 0;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("ret i32 7"), "{output}");
}
