//! `type`, `union`, and `enum`: three ways to name a type.

use fallc::Outcome;
use fallc::cli::EmitKind;
use fallc::pipeline::compile_text;

/// Compiles source and returns the LLVM IR.
fn ir_of(source: &str) -> String {
    let (outcome, output) = compile_text("test.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Success, "{output}");
    output
}

/// Returns everything from the first function definition on.
///
/// Diagnostics quote the source line they are about, so a test that looks for a
/// word in the whole output can find it in a warning about the very line that
/// was supposed to prove the word had gone.
fn body(output: &str) -> &str {
    output.split("define").nth(1).unwrap_or("")
}

#[test]
fn an_alias_is_the_type_it_names() {
    // `Id` and `usize` are one type under two spellings, so a value of one is
    // accepted where the other is wanted with no conversion -- and the size is
    // `usize`'s, not a new type's.
    let source = "type Id = usize;\nfn main() int {\n    x Id = 5;\n    return 0;\n}\n";
    let output = ir_of(source);
    // `Id` mangles as `usize` does, which is what "the same type" means here.
    assert!(output.contains("alloca i64"), "{output}");
    assert!(!body(&output).contains("Id"), "{output}");
}

#[test]
fn an_alias_may_name_a_pointer() {
    let source = "type Name = *const char;\nfn f(n Name) int {\n    return 0;\n}\nfn main() int {\n    return f(\"hi\");\n}\n";
    let (outcome, output) = compile_text("test.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Success, "{output}");
}

#[test]
fn an_enum_counts_from_zero_and_continues_from_a_written_value() {
    // `RED` is 0, `BLUE` is 1, `GREEN = 5` says so, and `YELLOW` follows it at
    // 6 -- the rule C uses, which is what makes an enum's values predictable.
    //
    // Each value reaches the output as a constant, which is what the numbers
    // being folded means. A `switch` over the members is the test above; this
    // one pins the arithmetic.
    let source = "enum Color int {\n    RED,\n    BLUE,\n    GREEN = 5,\n    YELLOW,\n}\nfn main() int {\n    return Color.RED as int + Color.BLUE as int + Color.GREEN as int + Color.YELLOW as int;\n}\n";
    let output = ir_of(source);
    // Each member is a constant in the output rather than a load from
    // somewhere: `GREEN` is 5 and `YELLOW` is 6, which is the rule.
    assert!(output.contains("i32 5)"), "{output}");
    assert!(output.contains("i32 6)"), "{output}");
}

#[test]
fn an_enum_may_omit_its_base_type() {
    // The base defaults to `int`, so the members are `int`s.
    let source = "enum Dir {\n    UP,\n    DOWN,\n}\nfn main() int {\n    return Dir.DOWN as int;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("ret i32 1"), "{output}");
}

#[test]
fn an_enum_is_usable_as_a_switch_scrutinee() {
    // A member is a constant of the base type, so a `case` label takes one and
    // `switch` needs no conversion.
    let source = "enum Color int {\n    RED,\n    GREEN = 5,\n}\nfn f(c Color) int {\n    switch (c) {\n        case Color.RED:\n            return 1;\n        case Color.GREEN:\n            return 2;\n        default:\n            return 3;\n    }\n    return 0;\n}\nfn main() int {\n    return f(Color.GREEN);\n}\n";
    let output = ir_of(source);
    assert!(output.contains("switch i32"), "{output}");
}

#[test]
fn an_enum_value_must_be_a_constant() {
    let source = "enum E int {\n    A = f(),\n}\nfn f() int {\n    return 1;\n}\nfn main() int {\n    return 0;\n}\n";
    let (outcome, output) = compile_text("test.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{output}");
}

#[test]
fn an_enum_needs_an_integer_base_type() {
    let source = "enum E f64 {\n    A,\n}\nfn main() int {\n    return 0;\n}\n";
    let (outcome, output) = compile_text("test.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(
        output.contains("must be an integer"),
        "the diagnostic should say what is wrong: {output}"
    );
}

#[test]
fn a_union_lays_its_members_at_one_address() {
    // The point of a union: writing one member and reading another reinterprets
    // the same bytes. The size is the largest member, not the sum.
    let source = "union Value {\n    i int;\n    b byte;\n}\nfn main() int {\n    v Value;\n    v.i = 65;\n    return v.b as int;\n}\n";
    let output = ir_of(source);
    // The union occupies four bytes -- `int`, its largest member -- and *both*
    // members address offset zero, which is what makes the read see the write.
    assert!(output.contains("alloca { i32 }"), "{output}");
    assert!(
        !output.contains("alloca { i32, i8 }"),
        "the members were laid out side by side: {output}"
    );
    // Both members address index 0 of that one slot, which is what makes the
    // read see the write.
    // Two members were read or written, and both reach the same slot.
    assert!(
        output.matches(", i32 0, i32 0").count() >= 2,
        "the members are not at the same offset: {output}"
    );
}

#[test]
fn an_empty_union_occupies_nothing() {
    let source = "union Nothing {\n}\nfn main() int {\n    return sizeof(Nothing) as int;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("ret i32 0"), "{output}");
}

#[test]
fn a_union_member_may_be_written_through_a_field_access() {
    // Field access reaches the one slot every member shares, so assigning to a
    // member that is not the first still works.
    let source = "union Value {\n    i int;\n    f float;\n    b byte;\n}\nfn main() int {\n    v Value;\n    v.b = 7;\n    return v.i;\n}\n";
    let output = ir_of(source);
    // The byte is written into the shared storage.
    assert!(output.contains("store i8 7"), "{output}");
}

#[test]
fn each_for_loop_has_its_own_scope() {
    // A `for (i int = 0; ...)` introduces `i` for the loop, not for the
    // function. Resolving the loop in the enclosing scope made a second loop
    // with the same name a redefinition, which is the common case: `i` again
    // for the next loop.
    let source = "fn main() int {\n    t int = 0;\n    for (i int = 0; i < 3; i += 1) {\n        t += i;\n    }\n    for (i int = 0; i < 2; i += 1) {\n        t += i;\n    }\n    return t;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("ret i32"), "{output}");
}

#[test]
fn a_loop_variable_does_not_survive_its_loop() {
    // The other half of scoping it to the loop: using it afterwards is a
    // mistake, not a reference to the last value.
    let source = "fn main() int {\n    for (i int = 0; i < 3; i += 1) {\n    }\n    return i;\n}\n";
    let (outcome, output) = compile_text("test.fft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("`i`"), "{output}");
}

#[test]
fn a_block_is_a_statement() {
    // `{ .. }` where a statement is expected: its own scope, which is how a
    // scope is written where no `if` or loop supplies one.
    let source = "fn main() int {\n    x int = 0;\n    {\n        y int = 5;\n        x = y;\n    }\n    return x;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("ret i32"), "{output}");
}

#[test]
fn a_block_destroys_what_it_declared() {
    // The scope is what makes the destructor run at the closing brace, not at
    // the end of the function.
    let source = "type G { id int; }\nfn __drop__(this &G) void {\n    this.id = 0;\n}\nfn f() int {\n    {\n        a G = .{1};\n    }\n    b G = .{2};\n    return b.id;\n}\n";
    let output = ir_of(source);
    let first = output.find("call void @__drop__").expect("a destructor runs");
    let second = output.rfind("call void @__drop__").expect("a destructor runs");
    assert_ne!(first, second, "both blocks destroy their own value:\n{output}");
}

#[test]
fn a_switch_arm_may_be_written_as_a_block() {
    // An arm's body is a list of statements, so a block is what gives one arm
    // a scope of its own.
    let source = "enum Color int { RED, GREEN }\nfn name(c Color) int {\n    switch (c) {\n        case Color.RED: {\n            t int = 40;\n            return t + 1;\n        }\n        case Color.GREEN: {\n            return 2;\n        }\n    }\n    return 0;\n}\nfn main() int {\n    return name(Color.RED);\n}\n";
    let output = ir_of(source);
    // `t + 1` is checked for overflow at `-O0`, so the arm's answer is the
    // checked value rather than the literal.
    assert!(output.contains("ret i32 %ovf.result"), "{output}");
    // The second arm is a separate return, so the block did not swallow it.
    assert!(output.contains("ret i32 2"), "{output}");
}

#[test]
fn an_inner_scope_shadows_an_outer_name() {
    // Two variables with one spelling: the inner block's `x` is its own, and
    // the outer one is untouched when the block ends.
    let source = "fn main() int {\n    x int = 1;\n    {\n        x int = 2;\n        return x;\n    }\n    return x;\n}\n";
    let output = ir_of(source);
    // Two `alloca`s, one for each binding.
    assert_eq!(output.matches("alloca i32").count(), 2, "{output}");
}

#[test]
fn a_parameter_may_not_be_shadowed_by_a_top_level_local() {
    // A parameter lives in the function's outermost scope, so a local there is
    // a second declaration of the same name rather than a shadow.
    let source = "fn f(x int) int {\n    x int = 1;\n    return x;\n}\n";
    let (outcome, output) = compile_text("test.fft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("defined more than once"), "{output}");
}

#[test]
fn a_local_may_shadow_a_module_constant() {
    // The narrower name wins where it is written, and the constant is still the
    // constant elsewhere.
    let source = "LIMIT const int = 10;\nfn main() int {\n    LIMIT int = 3;\n    {\n        LIMIT int = 4;\n    }\n    return LIMIT;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("ret i32"), "{output}");
}

#[test]
fn the_anonymous_variable_may_be_declared_more_than_once() {
    // `_` declares storage and gives it no name, so nothing can be read back
    // and nothing can collide: a scope may hold as many as it likes.
    let source = "fn main() int {\n    _ int = 1;\n    _ int = 2;\n    _ int = 3;\n    return 0;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("ret i32 0"), "{output}");
}

#[test]
fn an_anonymous_variable_is_still_destroyed() {
    // It is a variable, not a discard: it lives until the scope ends, which is
    // what makes `_ Guard = lock();` mean something.
    let source = "type G { id int; }\nfn __drop__(this &G) void {\n    this.id = 0;\n}\nfn f() int {\n    _ G = .{1};\n    return 0;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("call void @__drop__"), "{output}");
}

#[test]
fn the_anonymous_variable_can_be_read() {
    // `_` is readable; what makes it special is that it may be declared again
    // in the same scope.
    let source = "fn main() int {\n    _ int = 7;\n    return _;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("store i32 7, ptr %_"), "{output}");
    assert!(output.contains("load i32, ptr %_"), "{output}");
}

#[test]
fn a_second_anonymous_declaration_shadows_the_first() {
    // The scope of one `_` runs to the end of the block or to the next `_`,
    // whichever comes first -- so reading it gives the most recent one. The
    // second declaration is the second `alloca`, and the return reads that.
    let source = "fn main() int {\n    _ int = 1;\n    _ int = 2;\n    return _;\n}\n";
    let output = ir_of(source);
    assert_eq!(output.matches("alloca i32").count(), 2, "{output}");
    assert!(output.contains("load i32, ptr %_1"), "{output}");
}

#[test]
fn an_anonymous_declaration_in_an_inner_block_ends_with_it() {
    // The outer `_` is visible again once the block closes, so the return reads
    // the first declaration and nothing reads the block's own.
    let source = "fn main() int {\n    _ int = 1;\n    {\n        _ int = 2;\n    }\n    return _;\n}\n";
    let output = ir_of(source);
    assert!(!output.contains("load i32, ptr %_1"), "{output}");
    assert!(output.contains("load i32, ptr %_"), "{output}");
}

#[test]
fn a_parameter_may_be_anonymous() {
    // A parameter the body does not use needs no name, and two of them in one
    // signature do not collide.
    let source = "fn f(_ int, _ int, used int) int {\n    return used;\n}\nfn main() int {\n    return f(1, 2, 3);\n}\n";
    let output = ir_of(source);
    assert!(output.contains("ret i32 3"), "{output}");
}

#[test]
fn one_mistake_is_reported_once() {
    // A call to a name nothing declares: the resolver reports the unknown
    // name, and the checker used to add "`nope` is not a function" about the
    // same place -- two sentences for one typo.
    let source = "fn f() int {\n    return nope(1);\n}\n";
    let (outcome, output) = compile_text("test.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("cannot find `nope`"), "{output}");
    assert!(
        !output.contains("is not a function"),
        "the same mistake was described twice:\n{output}"
    );
    assert_eq!(
        output.matches("cannot find").count(),
        1,
        "the same mistake was reported more than once:\n{output}"
    );
}

#[test]
fn a_repeated_check_does_not_repeat_the_diagnostic() {
    // A binding's initialiser is looked at while its constructor is chosen and
    // again when it is checked, and a `?!` outside a throwing function is
    // refused by both -- which printed the same error twice.
    let source = "type R {\n    v int;\n}\nfn __init__(this &R, n int) void except {\n    raise \"X\";\n}\nfn f() int {\n    a R = 1?!;\n    return 0;\n}\n";
    let (outcome, output) = compile_text("test.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert_eq!(
        output.matches("`?!` is only allowed").count(),
        1,
        "{output}"
    );
}
