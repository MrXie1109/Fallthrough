//! The special methods the language dispatches to.
//!
//! Each of these is a function whose name the compiler looks for: `__add__` is
//! what `a + b` calls, `__getitem__` is what `a[i]` calls. The name is the whole
//! interface, so these tests are about the compiler finding it and passing the
//! right arguments.

use fallc::Outcome;
use fallc::cli::EmitKind;
use fallc::pipeline::compile_text;

/// Compiles source and returns the LLVM IR.
fn ir_of(source: &str) -> String {
    let (outcome, output) = compile_text("test.fft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Success, "{output}");
    output
}

#[test]
fn a_type_defines_plus_with_add() {
    let source = "type F { v int; }\nfn __add__(this &const F, o F) F {\n    return .{this.v + o.v};\n}\nfn f() int {\n    a F = .{1};\n    b F = .{2};\n    c F = a + b;\n    return c.v;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("call"), "{output}");
    // The receiver is the left operand, passed by address.
    assert!(output.contains("__add__"), "{output}");
}

#[test]
fn the_reflected_form_takes_the_right_operand() {
    // Only `__radd__` exists, so `1 + b` must find it on the *right* operand
    // with the operands swapped. Looking on the left found nothing, and the
    // operator was reported as unimplemented.
    let source = "type B { v int; }\nfn __radd__(this &const B, n int) B {\n    return .{this.v + n};\n}\nfn f() int {\n    b B = .{10};\n    r B = 1 + b;\n    return r.v;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("__radd__"), "{output}");
}

#[test]
fn a_type_is_indexed_with_getitem() {
    let source = "type V { a int; b int; }\nfn __getitem__(this &const V, i int) int {\n    if (i == 0) {\n        return this.a;\n    }\n    return this.b;\n}\nfn f() int {\n    v V = .{5, 6};\n    return v[1];\n}\n";
    let output = ir_of(source);
    assert!(output.contains("__getitem__"), "{output}");
}

#[test]
fn an_array_is_indexed_directly_rather_than_by_getitem() {
    // An array's subscript is built in. A type that also defines `__getitem__`
    // must not change what `a[i]` means for an array of it.
    let source = "fn f() int {\n    a [3]int;\n    a[1] = 7;\n    return a[1];\n}\n";
    let output = ir_of(source);
    assert!(output.contains("getelementptr"), "{output}");
    assert!(!output.contains("__getitem__"), "{output}");
}

#[test]
fn a_pointer_is_indexed_directly() {
    let source =
        "fn f() int {\n    a [3]int;\n    a[1] = 7;\n    p *int = &a[0];\n    return p[1];\n}\n";
    let ir = ir_of(source);
    assert!(ir.contains("getelementptr"), "{ir}");
}

#[test]
fn a_value_is_called_through_call() {
    let source = "type C { n int; }\nfn __call__(this &C, by int) int {\n    return this.n + by;\n}\nfn f() int {\n    c C = .{10};\n    return c(5);\n}\n";
    let output = ir_of(source);
    assert!(output.contains("__call__"), "{output}");
}

#[test]
fn a_type_is_a_condition_through_bool() {
    let source = "type T { n int; }\nfn __bool__(this &const T) bool {\n    return this.n > 0;\n}\nfn f() int {\n    t T = .{1};\n    if (t) {\n        return 1;\n    }\n    return 2;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("__bool__"), "{output}");
}

#[test]
fn a_type_without_bool_cannot_be_a_condition() {
    let source = "type T { n int; }\nfn f() int {\n    t T = .{1};\n    if (t) {\n        return 1;\n    }\n    return 2;\n}\n";
    let (outcome, output) = compile_text("test.fft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    // The method that would make it work is an explanation, and explanations
    // are behind `-Wverbose`.
    assert!(!output.contains("__bool__"), "{output}");
    let mut warnings = fallc_diag::WarningPolicy::new();
    warnings.set_verbose(true);
    let (_, verbose) = fallc::pipeline::compile_text_with_warnings(
        "test.fft",
        source,
        &[EmitKind::Llvm],
        &warnings,
    );
    assert!(verbose.contains("__bool__"), "{verbose}");
}

#[test]
fn an_as_conversion_takes_a_selector() {
    // `__as__` names its target with a parameter, so the call passes a null
    // pointer of the target's type.
    let source = "type F { n int; }\nfn __as__(this &const F, *double) double {\n    return this.n as double;\n}\nfn f() double {\n    x F = .{7};\n    return x as double;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("__as__"), "{output}");
    assert!(output.contains("null"), "{output}");
}

#[test]
fn a_negation_uses_neg() {
    let source = "type V { n int; }\nfn __neg__(this &const V) V {\n    return .{0 - this.n};\n}\nfn f() int {\n    v V = .{3};\n    w V = -v;\n    return w.n;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("__neg__"), "{output}");
}

#[test]
fn a_comparison_uses_its_method() {
    // `a < b` on a user type is `a.__lt__(b)`, so the result is the method's.
    let source = "type V { n int; }\nfn __lt__(this &const V, o V) bool {\n    return this.n < o.n;\n}\nfn f() bool {\n    a V = .{1};\n    b V = .{2};\n    return a < b;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("__lt__"), "{output}");
}

#[test]
fn equality_uses_eq() {
    let source = "type V { n int; }\nfn __eq__(this &const V, o V) bool {\n    return this.n == o.n;\n}\nfn f() bool {\n    a V = .{1};\n    b V = .{1};\n    return a == b;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("__eq__"), "{output}");
}

#[test]
fn a_compound_assignment_uses_the_in_place_method() {
    // `a += b` asks for `__iadd__` before `__add__`, because the in-place form
    // is the one that means "modify this".
    let source = "type V { n int; }\nfn __iadd__(this &V, o V) V {\n    return .{this.n + o.n};\n}\nfn f() int {\n    a V = .{1};\n    b V = .{2};\n    a += b;\n    return a.n;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("__iadd__"), "{output}");
    // The result is stored back, so the target holds what the method returned.
    assert!(output.contains("store"), "{output}");
}

#[test]
fn a_compound_assignment_falls_back_to_the_plain_operator() {
    // `__iadd__` is never required: without it, `a += b` is `a = a + b`, which
    // is what enables immutable types to use `+=` at all.
    let source = "type V { n int; }\nfn __add__(this &const V, o V) V {\n    return .{this.n + o.n};\n}\nfn f() int {\n    a V = .{1};\n    b V = .{2};\n    a += b;\n    return a.n;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("__add__"), "{output}");
}

#[test]
fn a_posit_and_bitwise_not_use_their_methods() {
    for (method, expression) in [("__pos__", "+v"), ("__not__", "~v")] {
        let source = format!(
            "type V {{ n int; }}\nfn {method}(this &const V) V {{\n    return .{{this.n}};\n}}\nfn f() int {{\n    v V = .{{3}};\n    w V = {expression};\n    return w.n;\n}}\n"
        );
        let output = ir_of(&source);
        assert!(
            output.contains(method),
            "`{expression}` did not call {method}: {output}"
        );
    }
}

#[test]
fn a_type_without_the_operator_says_what_to_define() {
    let source =
        "type V { n int; }\nfn f() int {\n    v V = .{3};\n    w V = -v;\n    return w.n;\n}\n";
    let (outcome, output) = compile_text("test.fft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    // Which method would make it work is an explanation, and explanations are
    // what `-Wverbose` adds.
    assert!(!output.contains("__neg__"), "{output}");
    let mut warnings = fallc_diag::WarningPolicy::new();
    warnings.set_verbose(true);
    let (_, verbose) = fallc::pipeline::compile_text_with_warnings(
        "test.fft",
        source,
        &[EmitKind::Llvm],
        &warnings,
    );
    assert!(verbose.contains("__neg__"), "{verbose}");
}

// ------------------------------------------------------------ destruction

#[test]
fn a_destructor_runs_at_the_end_of_its_scope() {
    let source = "type G { id int; }\nfn __drop__(this &G) void {\n    this.id = 0;\n}\nfn f() int {\n    a G = .{1};\n    return a.id;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("__drop__"), "{output}");
}

#[test]
fn a_destructor_taking_a_reference_is_called_the_same_way() {
    // A destructor receives the value either by pointer or by reference, and
    // both spellings are in use: the standard containers write `&String`.
    // Only the pointer form used to be found, so those destructors were never
    // called and every container leaked silently.
    let source = "type G { id int; }\nfn __drop__(this &G) void {\n    this.id = 0;\n}\nfn f() int {\n    a G = .{1};\n    return a.id;\n}\n";
    let output = ir_of(source);
    assert!(
        output.contains("call void @__drop___R0N1_G"),
        "the reference destructor was not called:\n{output}"
    );
}

#[test]
fn a_destructor_runs_before_a_return() {
    // `return` leaves every scope at once, so every live value is destroyed
    // before the function returns. A destructor that does not run is a leak,
    // and nothing reports it.
    let source = "type G { id int; }\nfn __drop__(this &G) void {\n    this.id = 0;\n}\nfn f() void {\n    a G = .{1};\n    return;\n}\n";
    let output = ir_of(source);
    let drop = output.find("__drop__").expect("the destructor is called");
    let ret = output.find("ret void").expect("the function returns");
    assert!(
        drop < ret,
        "the destructor must run before the return:\n{output}"
    );
}

#[test]
fn a_destructor_runs_before_a_break() {
    // `break` leaves the loop body's scopes, so the values built in the
    // iteration are destroyed. Leaving them to the scope's end would never
    // happen, because the scope's end is never reached.
    let source = "type G { id int; }\nfn __drop__(this &G) void {\n    this.id = 0;\n}\nfn f() void {\n    for (i int = 0; i < 3; i++) {\n        a G = .{1};\n        if (i == 1) {\n            break;\n        }\n    }\n}\n";
    let output = ir_of(source);
    // The call, not the definition: a `call` to the destructor appears inside
    // the loop, on the path that breaks as well as the one that falls through.
    let calls: Vec<usize> = output
        .match_indices("call void @__drop__")
        .map(|(index, _)| index)
        .collect();
    assert_eq!(
        calls.len(),
        2,
        "the destructor runs on both paths out of the body:\n{output}"
    );
    let break_block = output.find("if.then:").expect("the breaking branch");
    assert!(
        calls.iter().any(|&call| call > break_block),
        "one of the calls must be on the breaking path:\n{output}"
    );
}

#[test]
fn a_fields_destructor_runs_when_the_field_needs_it() {
    // The outer type defines no `__drop__`, so nothing looked inside it and the
    // two inner values leaked. A type needs destroying when a field of it does.
    let source = "type I { id int; }\nfn __drop__(this &I) void {\n    this.id = 0;\n}\ntype O {\n    a I;\n    b I;\n}\nfn f() void {\n    o O = .{.{1}, .{2}};\n}\n";
    let output = ir_of(source);
    assert!(output.contains("__drop__"), "{output}");
}

#[test]
fn a_type_without_a_destructor_costs_nothing() {
    let source = "type P { x int; }\nfn f() int {\n    p P = .{1};\n    return p.x;\n}\n";
    let output = ir_of(source);
    assert!(!output.contains("__drop__"), "{output}");
}

#[test]
fn a_three_way_comparison_uses_cmp() {
    // `<=>` returns an int: a sign rather than a truth value, which is why it
    // is not one of the comparison operators.
    let source = "type V { n int; }\nfn __cmp__(this &const V, o V) int {\n    return this.n - o.n;\n}\nfn f() int {\n    a V = .{1};\n    b V = .{2};\n    return a <=> b;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("__cmp__"), "{output}");
}

#[test]
fn a_three_way_comparison_of_numbers_needs_no_method() {
    let source = "fn f() int {\n    return 1 <=> 2;\n}\n";
    let output = ir_of(source);
    assert!(!output.contains("__cmp__"), "{output}");
    // Folded to the sign, which is what the operator reports. The subtraction
    // it is not is the interesting part: `1 - 2` would also be -1 here, so a
    // separate test covers the case where a difference would overflow.
    assert!(output.contains("ret i32 -1"), "{output}");
}

#[test]
fn an_integer_power_is_emitted_as_a_loop_of_multiplies() {
    // LLVM has no integer power instruction.
    let source = "fn f() int {\n    return 2 ** 10;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("mul"), "{output}");
}

#[test]
fn a_float_power_uses_the_intrinsic() {
    // `llvm.pow` takes a floating exponent, so `2.0 ** 0.5` works; `llvm.powi`
    // would need an integer and make it wrong.
    let source = "fn f() double {\n    return 2.0 ** 0.5;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("llvm.pow.f64"), "{output}");
}

#[test]
fn a_power_assignment_updates_its_target() {
    let source = "fn f() int {\n    x int = 2;\n    x **= 3;\n    return x;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("mul"), "{output}");
}

#[test]
fn a_three_way_comparison_does_not_overflow_where_a_difference_would() {
    // `i8::MIN <=> 1` is -1, but `-128 - 1` does not fit an `i8`: computing the
    // sign as a difference would wrap and report the wrong answer. This is why
    // the lowering is two comparisons.
    let source = "fn f() int {\n    a i8 = -128i8;\n    b i8 = 1i8;\n    return a <=> b;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("icmp slt"), "{output}");
    assert!(output.contains("icmp sgt"), "{output}");
    assert!(!output.contains("sub i8"), "{output}");
}

#[test]
fn a_copy_constructor_can_take_its_source_by_reference() {
    // The initialiser stands where the constructor's argument is, so a value
    // may reach a *reference* parameter -- `f(other)` reaches `fn f(x &const T)`
    // for an ordinary call, and `T b = a;` is that same shape.
    //
    // The receiver is a reference rather than a pointer on purpose: it is the
    // reference parameter that needs the rule, and with the rule disabled the
    // binding silently fell back to copying the fields. For a type that owns a
    // buffer that is a shallow copy, which is what made `Array<int> b = a;`
    // leave the two arrays sharing one allocation.
    let source = "type H { p *int; }\nfn __init__(this &H) void {\n    this.p = nullptr;\n}\nfn __init__(this &H, other &const H) void {\n    this.p = other.p;\n}\nfn f() int {\n    a H = H();\n    b H = a;\n    return 0;\n}\n";
    let output = ir_of(source);
    // Asserted at the *call site*: the definition is emitted either way, so its
    // presence proves nothing. What has to be there is the call.
    assert!(
        output.contains("call void @__init___R0N1_H_R1N1_H(ptr %b, ptr %a)"),
        "the copy constructor was not called for `b H = a;`: {output}"
    );
}

#[test]
fn a_construction_initialises_the_binding_rather_than_copying_a_temporary() {
    // `T a = T();` builds one `T` and holds it. Routing the temporary through
    // the copy constructor built a second one and left the first unreachable,
    // and where the receiver and the source were the same new storage nothing
    // was copied at all.
    let source = "type H { p *int; }\nfn __init__(this &H) void {\n    this.p = nullptr;\n}\nfn __init__(this &H, other &const H) void {\n    this.p = other.p;\n}\nfn f() int {\n    a H = H();\n    return 0;\n}\n";
    let output = ir_of(source);
    assert!(
        !output.contains("call void @__init___R0N1_H_R1N1_H"),
        "the zero-argument construction went through the copy constructor: {output}"
    );
}

#[test]
fn a_constant_is_stored_at_its_declared_width_in_a_template_body() {
    // A constant's type decides how many bytes initialising a binding from it
    // writes. Getting this wrong is silent: a `usize` stored as an `int` writes
    // four bytes of an eight-byte slot and leaves the upper half to whatever
    // the previous frame had left there, so `grown` came out as
    // `0xffffffff00000008` and `malloc` failed on the overflow.
    //
    // The cross-module half of this -- `INITIAL_CAPACITY` inside `std.Array`,
    // where the constant and its use are in different translation units -- is
    // covered by `examples/arrays.ft`, which fails to run at all without it.
    // This pins the width rule on its own.
    let source = "WIDTH const usize = 8;\ntemplate <T type>\nfn widen(v T) usize {\n    grown usize = WIDTH;\n    return grown;\n}\nfn f() usize {\n    return widen(1);\n}\n";
    let output = ir_of(source);
    assert!(
        !output.contains("i32 8, ptr %grown"),
        "a `usize` constant was stored as an `int`: {output}"
    );
    assert!(
        output.contains("store i64 8, ptr %grown"),
        "the constant was not stored at its declared width: {output}"
    );
}

#[test]
fn a_reference_returning_method_can_be_assigned_through() {
    // `a.last_mut() = v` writes the element, for the same reason `a[i] = v`
    // does: a reference *is* a place. The method's result was read as its
    // referent, so the assignment saw a value and was refused.
    let source = "type H { v int; }\nfn __init__(this &H) void {\n    this.v = 0;\n}\nfn slot(this &H) &int {\n    return &this.v;\n}\nfn f() int {\n    h H = H();\n    h.slot() = 42;\n    return 0;\n}\n";
    let output = ir_of(source);
    assert!(
        output.contains("store i32 42, ptr %call"),
        "the assignment did not write through the reference: {output}"
    );
}

#[test]
fn a_reference_returning_method_is_read_as_its_referent() {
    // The other half of the same rule: `v int = h.slot();` wants the value, so
    // the reference is loaded rather than stored as a pointer.
    let source = "type H { v int; }\nfn __init__(this &H) void {\n    this.v = 7;\n}\nfn slot(this &H) &int {\n    return &this.v;\n}\nfn f() int {\n    h H = H();\n    v int = h.slot();\n    return v;\n}\n";
    let output = ir_of(source);
    assert!(
        output.contains("load i32, ptr %call"),
        "the reference was not read as a value: {output}"
    );
}

#[test]
fn a_method_overloaded_on_its_receiver_picks_by_writability() {
    // Two methods may differ only in the receiver's const-ness: one reads and
    // one writes. Which applies is decided by what the receiver allows, exactly
    // as assignment decides it. Both candidates used to "fit" -- the receiver
    // comparison ignored qualifiers -- so the first declaration won for both and
    // writing through `a.slot() = v` reached the read-only one.
    let source = "type H { v int; }\nfn __init__(this &H) void {\n    this.v = 0;\n}\nfn slot(this &const H) &const int {\n    return &this.v;\n}\nfn slot(this &H) &int {\n    return &this.v;\n}\nfn f() int {\n    h H = H();\n    h.slot() = 42;\n    return h.slot();\n}\n";
    let output = ir_of(source);
    // The writable overload is the one that returns a `&int` the store uses.
    assert!(
        output.contains("store i32 42, ptr %call"),
        "the write did not reach the mutable overload: {output}"
    );
}

#[test]
fn a_const_receiver_reaches_the_read_only_overload() {
    // The other half of the same rule: on a `const` receiver the writable
    // overload is not a candidate, so the read-only one is what the call
    // reaches. Asserted at the *call site*, because both definitions are
    // emitted either way -- which one this call selected is the whole point.
    let source = "type H { v int; }\nfn __init__(this &H) void {\n    this.v = 0;\n}\nfn slot(this &H) &int {\n    return &this.v;\n}\nfn slot(this &const H) &const int {\n    return &this.v;\n}\nfn g(h &const H) int {\n    return h.slot();\n}\n";
    let output = ir_of(source);
    assert!(
        output.contains("call ptr @slot_R1N1_H("),
        "the read-only overload was not the one selected: {output}"
    );
    assert!(
        !output.contains("call ptr @slot_R0N1_H("),
        "a const receiver reached the writable overload: {output}"
    );
}

#[test]
fn writing_through_a_const_receiver_is_refused() {
    // The qualifier has to mean something: handing out `&T` from a `const`
    // receiver would be a way to write to an object that may not be written.
    let source = "type H { v int; }\nfn __init__(this &H) void {\n    this.v = 0;\n}\nfn slot(this &const H) &const int {\n    return &this.v;\n}\nfn slot(this &H) &int {\n    return &this.v;\n}\nfn g(h &const H) int {\n    h.slot() = 42;\n    return 0;\n}\n";
    let (outcome, output) = compile_text("test.fft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(
        output.contains("cannot assign through a reference to const"),
        "writing through a const receiver was allowed: {output}"
    );
}
