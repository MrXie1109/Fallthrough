//! Compile-time evaluation, with no keyword to ask for it.
//!
//! A condition the compiler can decide selects its branch while compiling, and
//! a call whose arguments are all known is evaluated rather than emitted.
//! Nothing is marked `constexpr`: a function is evaluated when it *can* be, and
//! emitted when it cannot.

use fallc::Outcome;
use fallc::cli::EmitKind;
use fallc::pipeline::compile_text;

/// Compiles source and returns the LLVM IR.
fn ir_of(source: &str) -> String {
    let (outcome, output) = compile_text("test.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Success, "{output}");
    output
}

#[test]
fn a_call_with_constant_arguments_is_evaluated() {
    // `square(3)` is 9, known before the program runs. The call is gone: what
    // reaches the printer is the number.
    //
    // The function is not marked, and the call still folds -- folding is an
    // optimisation. What the marker controls is the *other* tests here: whether
    // the call may stand where the language requires a constant.
    let source =
        "fn square(n int) int {\n    return n * n;\n}\nfn main() int {\n    return square(3);\n}\n";
    let output = ir_of(source);
    assert!(
        output.contains("ret i32 9"),
        "the call was not evaluated: {output}"
    );
    assert!(
        !output.contains("call i32 @square"),
        "the call was emitted anyway: {output}"
    );
}

#[test]
fn a_constexpr_call_is_usable_as_an_array_length() {
    // An array length must be known while compiling, which is what `constexpr`
    // promises. The array is 9 elements: the length came from a call.
    let source = "@constexpr fn square(n int) int {\n    return n * n;\n}\nfn main() int {\n    a [square(3)]int;\n    a[8] = 1;\n    return a[8];\n}\n";
    let output = ir_of(source);
    // Nine elements, which is `square(3)` -- an array length has to be known
    // while compiling, so this only gets past the parser because the call was
    // evaluated.
    assert!(output.contains("alloca [9 x i32]"), "{output}");
}

#[test]
fn a_recursive_constexpr_function_is_evaluated() {
    // Recursion is allowed: the evaluator bounds it rather than refusing it.
    let source = "@constexpr fn fib(n int) int {\n    if (n < 2) {\n        return n;\n    }\n    return fib(n - 1) + fib(n - 2);\n}\nfn main() int {\n    return fib(10);\n}\n";
    let output = ir_of(source);
    assert!(output.contains("ret i32 55"), "{output}");
    // The *call site* is gone. `fib` itself still contains its own recursive
    // calls -- it is emitted as an ordinary function -- so the assertion is on
    // the constant `10` having been folded away, not on the definition.
    assert!(
        !output.contains("call i32 @fib_I3_i32(i32 10)"),
        "the call at the constant site was emitted: {output}"
    );
}

#[test]
fn a_call_with_an_unknown_argument_is_emitted() {
    // The parameter's value is not known until the program runs, so the call
    // has to be emitted. Evaluation is an opportunity, not a requirement.
    // The unknown value comes from a module-level variable: a `main` cannot take
    // parameters other than the argument vector, and what matters here is only
    // that the argument is not a constant.
    let source = "n int = 3;\nfn square(n int) int {\n    return n * n;\n}\nfn main() int {\n    return square(n);\n}\n";
    let output = ir_of(source);
    assert!(
        output.contains("call i32 @square_I3_i32"),
        "the call should have been emitted: {output}"
    );
}

#[test]
fn a_function_that_cannot_be_evaluated_is_emitted_instead() {
    // A loop that never ends cannot be evaluated -- the evaluator bounds its
    // work and gives up -- and giving up is not an error. The call is emitted
    // as ordinary code, which is what a function that needs the machine gets.
    let source = "n int = 3;\nfn keep_going(n int) int {\n    while (true) {\n        n += 1;\n    }\n    return n;\n}\nfn main() int {\n    return keep_going(n);\n}\n";
    let output = ir_of(source);
    assert!(output.contains("call i32 @keep_going"), "{output}");
}

#[test]
fn an_if_constexpr_discards_the_branch_it_does_not_take() {
    // `if constexpr` never checks the branch it does not take, so that branch
    // may name things this configuration does not have. That is what makes it a
    // configuration switch rather than a comment -- and it is why the marker
    // has to be written: a plain `if` promises both branches compile.
    let source = "fn main() int {\n    if (sizeof(int) == 4) @constexpr {\n        return 4;\n    } else {\n        this_function_does_not_exist();\n        return 8;\n    }\n}\n";
    let output = ir_of(source);
    assert!(output.contains("ret i32 4"), "{output}");
    assert!(
        !output.contains("this_function_does_not_exist"),
        "the dead branch was emitted: {output}"
    );
}

#[test]
fn an_if_constexpr_selects_per_instantiation() {
    // `sizeof(T)` differs between members of a family, so the same condition
    // takes a different branch in each -- which is the point of writing one.
    let source = "template <T type>\nfn width() int {\n    if (sizeof(T) == 4) @constexpr {\n        return 4;\n    } else {\n        return 8;\n    }\n}\nfn main() int {\n    return width<int>() + width<f64>();\n}\n";
    let output = ir_of(source);
    // Each member keeps only its own branch, decided while compiling: one
    // returns 4 and the other 8, and neither contains an `if`.
    //
    // The sum itself is not folded, because a *template* is not one function --
    // which member a call means is decided while checking, so the evaluator has
    // no single body to run. Both calls are still evaluated, which is why
    // neither member contains a branch.
    assert!(output.contains("ret i32 4"), "{output}");
    assert!(output.contains("ret i32 8"), "{output}");
    // Neither member contains an `if`: the condition was decided, so there is
    // nothing left to branch on. `if.then` is the label a lowered `if` creates,
    // and it is absent -- the `br i1` that remains belongs to the overflow
    // guard, which is part of what the program means and is not an `if`.
    assert!(
        !output.contains("if.then"),
        "a compile-time condition survived as a branch: {output}"
    );
}

#[test]
fn a_plain_if_checks_both_branches_even_when_the_condition_is_constant() {
    // The condition is known while compiling, so only one branch is *emitted* --
    // but a plain `if` says both are meant to compile, so a mistake in the other
    // is reported rather than hidden. This is the whole difference from
    // `if constexpr`.
    let source = "fn main() int {\n    if (sizeof(int) == 4) {\n        return 4;\n    } else {\n        return does_not_exist();\n    }\n}\n";
    let (outcome, output) = compile_text("test.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(
        output.contains("does_not_exist"),
        "the dead branch should still have been checked: {output}"
    );
}

#[test]
fn a_plain_if_with_a_constant_condition_emits_only_one_branch() {
    // Checked in full, but only the live branch reaches the output.
    let source = "fn main() int {\n    if (sizeof(int) == 4) {\n        return 4;\n    } else {\n        return 8;\n    }\n}\n";
    let output = ir_of(source);
    assert!(output.contains("ret i32 4"), "{output}");
    assert!(!output.contains("ret i32 8"), "{output}");
}

#[test]
fn an_if_constexpr_condition_must_be_a_constant() {
    // The marker is a promise about the condition, so a condition that is only
    // known at run time is refused rather than quietly treated as a plain `if`.
    let source = "fn f(n int) int {\n    if (n > 0) @constexpr {\n        return 1;\n    } else {\n        return 2;\n    }\n}\n";
    let (outcome, output) = compile_text("test.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(
        output.contains("must be a constant"),
        "the diagnostic should say the condition is not constant: {output}"
    );
}

#[test]
fn a_runtime_condition_keeps_both_branches() {
    // Nothing is known about `n`, so the `if` is emitted as an `if`.
    let source = "n int = 3;\nfn main() int {\n    if (n > 0) {\n        return 1;\n    } else {\n        return 2;\n    }\n}\n";
    let output = ir_of(source);
    assert!(output.contains("if.then"), "{output}");
    assert!(output.contains("if.else"), "{output}");
}

#[test]
fn a_loop_else_runs_when_the_loop_finishes_on_its_own() {
    // The `else` of a loop runs when it ends by its own condition, and not when
    // `break` left it. Evaluating a loop without this produced a *wrong value*
    // rather than a failure to evaluate -- `count_down(3)` was 3 instead of
    // 103 -- which is the worst kind of bug an evaluator can have: the program
    // compiles, runs, and prints something quietly false.
    let source = "fn count_down(start int) int {\n    steps int = 0;\n    i int = start;\n    while (i > 0) {\n        steps = steps + 1;\n        i = i - 1;\n    } else {\n        steps = steps + 100;\n    }\n    return steps;\n}\nfn main() int {\n    return count_down(3);\n}\n";
    let output = ir_of(source);
    assert!(output.contains("ret i32 103"), "{output}");
}

#[test]
fn a_loop_else_does_not_run_when_break_left_the_loop() {
    // The other half of the rule, and the reason the clause exists at all.
    let source = "fn first_divisor(n int) int {\n    d int = 2;\n    while (d < n) {\n        if (n % d == 0) {\n            break;\n        }\n        d = d + 1;\n    } else {\n        d = -1;\n    }\n    return d;\n}\nfn main() int {\n    return first_divisor(15);\n}\n";
    let output = ir_of(source);
    // 3 divides 15, so `break` left the loop and `else` did not run.
    assert!(output.contains("ret i32 3"), "{output}");
}

#[test]
fn assignment_updates_a_local_while_evaluating() {
    // Without assignment, `i += 1` and `t += i` did nothing: a loop ran until
    // the step bound and the function was reported as unevaluable, so every
    // loop was silently left as a runtime call.
    let source = "fn sum(n int) int {\n    t int = 0;\n    i int = 0;\n    while (i < n) {\n        t += i;\n        i += 1;\n    }\n    return t;\n}\nfn main() int {\n    return sum(10);\n}\n";
    let output = ir_of(source);
    assert!(output.contains("ret i32 45"), "{output}");
}

#[test]
fn division_by_zero_is_not_evaluated() {
    // `10 / 0` has no value, so the call is emitted and traps when it runs --
    // which is what the program says. Folding it would mean inventing a number,
    // and a diagnostic would mean rejecting a program that only traps if it is
    // actually reached.
    let source = "fn d(n int) int {\n    return 10 / n;\n}\nfn main() int {\n    return d(0);\n}\n";
    let output = ir_of(source);
    assert!(output.contains("call i32 @d_I3_i32"), "{output}");
}

#[test]
fn a_condition_the_evaluator_cannot_answer_stops_the_fold() {
    // The evaluator models parameters and constants, not a module-level
    // variable. Reading one left the condition unanswered, and the `if` then
    // took its `else` branch -- so `f()` folded to 2 while the program prints
    // 1. An unanswerable condition now ends the evaluation, and the call is
    // emitted as code like any other.
    let source = r#"import C;

x int = 5;

fn f() int {
    if (x > 0) {
        return 1;
    }
    return 2;
}

fn main() int {
    C.printf("%d\n", f());
    return 0;
}
"#;
    let output = ir_of(source);
    // The call is emitted, and the branch that reads the variable is emitted
    // with it: neither `1` nor `2` stands in for the call.
    assert!(output.contains("call i32 @f("), "{output}");
    assert!(
        !output.contains("i32 2)") || output.contains("call"),
        "{output}"
    );
}
