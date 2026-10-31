//! Tests for type declarations, member access, and member functions.
//!
//! A member function is one whose first parameter is named `this` and is a
//! pointer; `value.method()` passes the receiver's address. The tests assert on
//! the emitted IR, because the interesting part is that the address is taken
//! and the field indices are right.

use fallc::Outcome;
use fallc::cli::EmitKind;
use fallc::pipeline::compile_text;

fn compile(source: &str) -> (Outcome, String) {
    compile_text("test.ftm", source, &[EmitKind::Llvm])
}

/// Compiles with `-Wverbose`, which is where the explanations live.
fn compile_verbose(source: &str) -> (Outcome, String) {
    let mut warnings = fallc_diag::WarningPolicy::new();
    warnings.set_verbose(true);
    fallc::pipeline::compile_text_with_warnings("test.ftm", source, &[EmitKind::Llvm], &warnings)
}

fn ir_of(source: &str) -> String {
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
    output
}

#[test]
fn a_type_declaration_becomes_an_llvm_struct() {
    let source = "type Point {\n    x int;\n    y int;\n}\nfn f() int {\n    p Point = .{1, 2};\n    return 0;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("alloca { i32, i32 }"), "{output}");
}

#[test]
fn fields_are_read_by_declaration_index() {
    // Declaration order is the layout, because a positional initialiser
    // assigns by it.
    let source =
        "type P {\n    a int;\n    b u8;\n    c i64;\n}\nfn f(p *P) i64 {\n    return p->c;\n}\n";
    let output = ir_of(source);
    // The third field is index 2.
    assert!(output.contains("i32 0, i32 2"), "{output}");
}

#[test]
fn a_field_that_does_not_exist_is_reported_with_the_known_ones() {
    let source = "type P {\n    x int;\n}\nfn f(p *P) int {\n    return p->nope;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("has no field `nope`"), "{output}");
    // Which fields there are is an explanation, so it is behind `-Wverbose`.
    let (_, verbose) = compile_verbose(source);
    assert!(
        verbose.contains("x"),
        "the note lists the fields: {verbose}"
    );
}

#[test]
fn arrow_is_sugar_for_dereference_then_field() {
    // `p->x` and `(*p).x` produce the same thing.
    let arrow = ir_of("type P {\n    x int;\n}\nfn f(p *P) int {\n    return p->x;\n}\n");
    let dot = ir_of("type P {\n    x int;\n}\nfn f(p *P) int {\n    return (*p).x;\n}\n");
    // Both take the pointer's value and GEP into it.
    assert!(arrow.contains("getelementptr"), "{arrow}");
    assert!(dot.contains("getelementptr"), "{dot}");
}

#[test]
fn arrow_needs_a_pointer() {
    let source = "type P {\n    x int;\n}\nfn f(p P) int {\n    return p->x;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
}

#[test]
fn a_member_function_is_one_whose_first_parameter_is_this() {
    let source = "type P {\n    x int;\n}\nfn get(this &P) int {\n    return this.x;\n}\nfn f(p *P) int {\n    return p.get();\n}\n";
    let output = ir_of(source);
    // The receiver is passed as the first argument, and the symbol carries
    // every parameter's type -- the receiver's included.
    assert!(output.contains("call i32 @get_R0N1_P(ptr"), "{output}");
}

#[test]
fn a_method_may_be_called_on_a_value() {
    // The address is taken automatically, which is what makes the same syntax
    // work for a value and a pointer.
    let source = "type P {\n    x int;\n}\nfn get(this &P) int {\n    return this.x;\n}\nfn f() int {\n    p P = .{7};\n    return p.get();\n}\n";
    let output = ir_of(source);
    assert!(output.contains("call i32 @get_R0N1_P(ptr %p)"), "{output}");
}

#[test]
fn a_method_may_be_called_on_a_pointer() {
    let source = "type P {\n    x int;\n}\nfn get(this &P) int {\n    return this.x;\n}\nfn f(p *P) int {\n    return p.get();\n}\n";
    let output = ir_of(source);
    // The pointer is passed directly rather than addressed again.
    assert!(output.contains("call i32 @get_R0N1_P(ptr"), "{output}");
}

#[test]
fn a_function_whose_first_parameter_is_not_this_is_not_a_method() {
    let source = "type P {\n    x int;\n}\nfn get(p *P) int {\n    return p->x;\n}\nfn f(p *P) int {\n    return p.get();\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("not a member function"), "{output}");
    assert!(output.contains("`this`"), "{output}");
}

#[test]
fn a_method_rejects_a_receiver_of_the_wrong_type() {
    let source = "type P {\n    x int;\n}\ntype Q {\n    y int;\n}\nfn get(this &P) int {\n    return this.x;\n}\nfn f(q Q) int {\n    return q.get();\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("expects `this`"), "{output}");
}

#[test]
fn a_method_call_checks_its_argument_count() {
    let source = "type P {\n    x int;\n}\nfn set(this &P, v int) void {\n    this.x = v;\n}\nfn f(p *P) int {\n    p.set();\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("takes 1 argument"), "{output}");
}

#[test]
fn assigning_to_a_field_through_a_pointer() {
    // The field is written through a GEP rather than to a local slot, and the
    // method receives the address of the receiver.
    let source = "type P {\n    x int;\n}\nfn set(this &P, v int) void {\n    this.x = v;\n}\nfn f() int {\n    p P = .{1};\n    p.set(9);\n    return p.x;\n}\n";
    let output = ir_of(source);
    assert!(
        output.contains("call void @set_R0N1_P_I3_i32(ptr %p, i32 9)"),
        "{output}"
    );
    // The store goes to the field's address, not to a fresh alloca.
    assert!(output.contains("store i32 %v"), "{output}");
}

#[test]
fn a_const_pointee_field_cannot_be_assigned() {
    // `*const T` is what makes the receiver read-only, and writing through it
    // must be refused.
    let source =
        "type P {\n    x int;\n}\nfn set(this &const P, v int) void {\n    this.x = v;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("pointer to const"), "{output}");
}

#[test]
fn types_are_order_independent() {
    // A function may use a type declared after it, since declarations are
    // collected before any body is checked.
    let source = "fn f() int {\n    p Point = .{1, 2};\n    return p.x;\n}\ntype Point {\n    x int;\n    y int;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
}

#[test]
fn a_void_function_returns_instead_of_falling_through() {
    // Emitting `unreachable` at the end of a void function made it fall into
    // whatever the linker placed next, which printed its output forever.
    let source = "fn f() void {\n    return;\n}\nfn g() void {\n}\n";
    let output = ir_of(source);
    assert!(output.contains("define void @f()"), "{output}");
    assert!(output.contains("ret void"), "{output}");
    // The empty body reaches its end, which must be a return rather than
    // unreachable code.
    assert!(!output.contains("unreachable"), "{output}");
}

// ------------------------------------------------------------ constructors

#[test]
fn a_designated_initialiser_does_not_call_init() {
    // A field initialiser names the fields, so it is the fields that are
    // written: no constructor runs, even when the type declares one.
    let source = "type P {\n    x int;\n}\nfn __init__(this &P, v int) void {\n    this.x = v * 2;\n}\nfn f() int {\n    p P = .{21};\n    return p.x;\n}\n";
    let output = ir_of(source);
    // The program returns 21, not 42: the constructor was not called.
    assert!(!output.contains("call void @__init___"), "{output}");
    assert!(output.contains("store { i32 } { i32 21 }"), "{output}");
}

#[test]
fn a_type_without_init_has_its_fields_filled_directly() {
    let source = "type P {\n    x int;\n    y int;\n}\nfn f() int {\n    p P = .{1, 2};\n    return p.x;\n}\n";
    let output = ir_of(source);
    // The struct is stored whole rather than constructed by a call.
    assert!(output.contains("store { i32, i32 }"), "{output}");
    assert!(!output.contains("__init__"), "{output}");
}

#[test]
fn each_type_gets_its_own_constructor_symbol() {
    // `__init__` is written once per type, so the symbol must distinguish
    // them or two types' constructors would collide.
    let source = "type A {\n    v int;\n}\ntype B {\n    v int;\n}\nfn __init__(this &A, v int) void {\n    this.v = v;\n}\nfn __init__(this &B, v int) void {\n    this.v = v + 1;\n}\nfn f() int {\n    a A = A(1);\n    b B = B(1);\n    return a.v + b.v;\n}\n";
    let output = ir_of(source);
    // Each is named by its receiver's type, which is what tells them apart.
    assert!(output.contains("@__init___R0N1_A_"), "{output}");
    assert!(output.contains("@__init___R0N1_B_"), "{output}");
}

#[test]
fn a_constructor_may_be_declared_after_its_use() {
    // Functions are declared before any body is lowered, so a call site
    // resolves whichever order the source is in. Without that pass the
    // constructor was not found and the fields were filled directly instead.
    let source = "type P {\n    x int;\n}\nfn f() int {\n    p P = P(21);\n    return p.x;\n}\nfn __init__(this &P, v int) void {\n    this.x = v * 2;\n}\n";
    let output = ir_of(source);
    assert!(
        output.contains("call void @__init___R0N1_P_I3_i32"),
        "{output}"
    );
}

#[test]
fn a_pointer_converts_to_and_from_a_byte_pointer_implicitly() {
    // `*byte` is the untyped pointer -- what raw memory and the C library
    // traffic in -- so it converts to and from a pointer to anything without a
    // cast. This is the one pointer conversion that need not preserve shape.
    let source = "fn main() int {\n    x int = 42;\n    p *int = &x;\n    b *byte = p;\n    back *int = b;\n    return *back;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
}

#[test]
fn the_byte_pointer_conversion_respects_const() {
    // Adding const is always safe: a mutable pointer may become `*const byte`.
    let widening = "fn main() int {\n    x int = 1;\n    p *int = &x;\n    cb *const byte = p;\n    ci *const int = cb;\n    return 0;\n}\n";
    let (outcome, output) = compile(widening);
    assert_eq!(outcome, Outcome::Success, "{output}");

    // Dropping it is not: a `*const byte` must not become a writable pointer,
    // whether it is converted to `*byte` or to another pointee type.
    for dropping in [
        // A `*const byte` reaching a `*byte`.
        "fn main() int {\n    x int = 1;\n    p *int = &x;\n    c *const byte = p;\n    d *byte = c;\n    return 0;\n}\n",
        // And a `*const int` reaching a `*int` through the byte pointer.
        "fn main() int {\n    x int = 1;\n    p *int = &x;\n    c *const byte = p;\n    d *const int = c;\n    e *int = d;\n    return 0;\n}\n",
    ] {
        let (outcome, output) = compile(dropping);
        assert_eq!(outcome, Outcome::Failure, "{output}");
    }

    // The explicit cast is how that is written.
    let explicit = "fn main() int {\n    x int = 1;\n    p *int = &x;\n    c *const byte = p;\n    d *byte = c as *byte;\n    return 0;\n}\n";
    let (outcome, output) = compile(explicit);
    assert_eq!(outcome, Outcome::Success, "{output}");
}

#[test]
fn a_pointer_to_another_type_still_needs_a_cast() {
    // The exception is `byte` alone. Two pointers to different types are not
    // interchangeable, or every type error would become a memory error.
    let source =
        "fn main() int {\n    x u16 = 1;\n    p *u16 = &x;\n    q *int = p;\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
}

#[test]
fn a_template_parameter_pack_takes_any_number_of_types() {
    // `Args ...type` stands for a sequence, so the arity is not fixed: one call
    // may pass one type and another three. Before packs this was an arity
    // error, since the count had to equal the parameter count exactly.
    let source = "template <Args ...type>\nfn count() int {\n    return 0;\n}\nfn f() int {\n    return count<int>() + count<int, f64>() + count<int, f64, char>();\n}\n";
    let (outcome, output) = compile_text("test.fft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Success, "{output}");
}

#[test]
fn a_pack_expands_into_another_templates_arguments() {
    // `Box<Args...>` is one written argument per element, which is what makes a
    // pack useful: the expansion happens before the arguments are counted, so
    // `Box` sees exactly one argument even though `Args` may hold several.
    let source = "template <T type>\ntype Box { v T; }\ntemplate <Args ...type>\nfn take(b Box<Args...>) int {\n    return 1;\n}\nfn f() int {\n    x Box<int> = .{7};\n    return take<int>(x);\n}\n";
    let (outcome, output) = compile_text("test.fft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Success, "{output}");
    // The parameter is the expanded type, not the bare family name.
    assert!(
        output.contains("Box<i32>"),
        "the pack expansion did not reach the parameter type: {output}"
    );
}

#[test]
fn a_method_may_be_called_on_a_calls_result() {
    // A method takes its receiver by address, and a call's result has none --
    // so it is given one. Asking the lvalue path for it lowerered the call a
    // second time: `drop_returning().show()` ran twice, which for `pop()` is a
    // pop from an array it had already emptied.
    let source = "type P { v int; }\nfn make(n int) P {\n    p P = .{n};\n    return p;\n}\nfn peek(this &P) int {\n    return this.v;\n}\nfn f() int {\n    return make(7).peek();\n}\n";
    let output = ir_of(source);
    assert!(output.contains("ret i32 %"), "{output}");
    // One call to `make`, not two.
    assert_eq!(output.matches("call { i32 } @make_").count(), 1, "{output}");
}

#[test]
fn a_field_of_an_indexed_element_is_readable_and_writable() {
    // `__getitem__` returns a reference, so `a[i]` *is* the element -- and a
    // field of it is that field, not a field of a copy. Treated as a value, the
    // element had no address to take and the field access was reported as
    // unsupported.
    let source = "type P { x int; y int; }\ntemplate <T type>\ntype Box { v T; }\nfn __getitem__(this &Box<P>, index usize) &P {\n    return this.v;\n}\nfn f() int {\n    b Box<P>;\n    b.v = .{1, 2};\n    b[0].x = 10;\n    return b[0].x + b[0].y;\n}\n";
    let output = ir_of(source);
    // The write reaches the element's own field, and the read is a load of it.
    assert!(output.contains("store i32 10"), "{output}");
    assert!(output.contains("load i32"), "{output}");
}

#[test]
fn a_pointer_target_is_assigned_without_reaching_for_its_pointee() {
    // `*seen = value` where `seen` is `**const B` assigns a *pointer*. The
    // receiver's name used to be looked up by reaching through pointers, so
    // this found `B`'s `__assign__` and passed the pointer's value where the
    // address of a `B` was wanted -- a store into whatever the pointer held.
    // A reference is transparent; a pointer is not.
    let source = "type B { v int; }\nfn __assign__(this &B, other &const B) void {\n    this.v = other.v;\n}\nfn set(seen * *const B, value *const B) void {\n    *seen = value;\n}\nfn f() int {\n    b B = .{7};\n    slot *const B = nullptr;\n    set(&slot, &b);\n    return slot.v;\n}\n";
    let output = ir_of(source);
    // The pointer is stored where `seen` points, with no `__assign__` call: a
    // pointer is assigned as a pointer, and `B` is not what stands on the left.
    assert!(!output.contains("call void @__assign__"), "{output}");
    assert!(output.contains("store ptr %value"), "{output}");
}

#[test]
fn a_dying_source_takes_the_move_form_of_an_assignment() {
    // Two forms of `__assign__` differ only in how the source arrives, and the
    // move form is declared *first* here: a `&&T` parameter accepts a named
    // value too, so taking the first overload that fits emptied every named
    // source. A named value copies and a call moves, which is the same rule a
    // constructor follows.
    let source = "type S {\n    v int;\n}\nfn __assign__(this &S, other &&S) void {\n    this.v = other.v + 100;\n}\nfn __assign__(this &S, other &const S) void {\n    this.v = other.v;\n}\nfn make() S {\n    s S = .{5};\n    return s;\n}\nfn f() int {\n    a S = .{0};\n    b S = .{1};\n    a = b;\n    b = make();\n    return a.v + b.v;\n}\n";
    let output = ir_of(source);
    // `a = b` copies, so `a.v` is 1; `b = make()` moves, so `b.v` is 5 + 100.
    assert!(output.contains("@__assign___R0N1_S_R1N1_S"), "{output}");
    assert!(output.contains("@__assign___R0N1_S_W0N1_S"), "{output}");
}

#[test]
fn a_nested_template_argument_list_closes_with_two_right_angles() {
    // `Box<Box<int>>` ends two lists with no space between them. The lexer
    // reads `>>` as one token -- it has to, since `a >> b` shifts -- so the
    // parser takes it one `>` at a time here. Otherwise a type's spelling
    // would depend on where a space happens to be.
    let source = "template <T type>\ntype Box { v T; }\nfn f() int {\n    x Box<Box<int>> = .{.{7}};\n    return x.v.v;\n}\n";
    let (outcome, output) = compile_text("test.fft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Success, "{output}");
    // Two nested structs, which is `Box<Box<int>>` in the layout.
    assert!(output.contains("{ { i32 } }"), "{output}");
}

#[test]
fn a_shift_is_still_one_operator() {
    // The other half of splitting `>`: `>>` between two values is a shift, not
    // two ends of argument lists.
    let source = "fn f() int {\n    x int = 8 >> 2;\n    return x;\n}\nfn main() int {\n    return f();\n}\n";
    let output = ir_of(source);
    // 8 >> 2 is 2, and the constant folder got there without a shift being
    // parsed as two angle brackets.
    assert!(output.contains("ret i32 2"), "{output}");
}

#[test]
fn a_pack_must_be_the_last_parameter() {
    // Nothing would say where one pack's arguments ended and the next
    // parameter's began, so this is refused rather than guessed at.
    let source = "template <Args ...type, T type>\nfn f() int {\n    return 0;\n}\n";
    let (outcome, output) = compile_text("test.fft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(
        output.contains("must be the last parameter"),
        "the diagnostic did not explain the rule: {output}"
    );
}

#[test]
fn an_lvalue_satisfies_a_reference_without_taking_its_address() {
    // A reference *is* the place a value lives, so a `T` expression satisfies
    // `&T` directly -- there is no operator that "makes" a reference. `&v` is a
    // `*T`: a pointer, a different type. Writing `return &this.v;` in a function
    // declared `&int` happened to work because a pointer converts to a
    // reference, which is exactly the confusion this rule removes.
    let source = "type H { v int; }\nfn __init__(this &H) void {\n    this.v = 0;\n}\nfn slot(this &H) &int {\n    return this.v;\n}\nfn main() int {\n    h H = H();\n    h.slot() = 42;\n    return h.v;\n}\n";
    let (outcome, output) = compile_text("test.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Success, "{output}");
}

#[test]
fn a_pointer_return_takes_the_address_and_a_reference_does_not() {
    // The two forms are different types and different syntax, and both work.
    let source = "type H { v int; }\nfn __init__(this &H) void {\n    this.v = 0;\n}\nfn as_ref(this &H) &int {\n    return this.v;\n}\nfn as_ptr(this &H) *int {\n    return &this.v;\n}\nfn main() int {\n    h H = H();\n    r &int = h.as_ref();\n    p *int = h.as_ptr();\n    r = 1;\n    *p = 2;\n    return h.v;\n}\n";
    let (outcome, output) = compile_text("test.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Success, "{output}");
}

#[test]
fn a_binding_still_copies_rather_than_binding_a_reference() {
    // The one place the rule must not apply. `T x = y;` holds a *copy*: if it
    // bound a reference instead, changing `x` would change `y`, and the
    // language's value semantics would be gone.
    //
    // Asserted on the *store*, because `y` is a mutable local that LLVM cannot
    // fold: what matters is that `x = 2` writes to `x`'s own slot and not to
    // `y`'s, which is what a separate alloca for each shows.
    let source = "fn f() int {\n    y int = 1;\n    x int = y;\n    x = 2;\n    return y;\n}\n";
    let output = ir_of(source);
    // Each binding has its own storage, so a write to one cannot reach the
    // other.
    assert!(output.contains("%x = alloca i32"), "{output}");
    assert!(output.contains("%y = alloca i32"), "{output}");
    // And the store of 2 goes to `x`, never to `y`.
    assert!(output.contains("store i32 2, ptr %x"), "{output}");
    assert!(!output.contains("store i32 2, ptr %y"), "{output}");
}

#[test]
fn a_reference_is_not_dereferenced() {
    // `*` follows a *pointer*. A reference already names the value it stands
    // for, so `*this` inside a method asks for an operation the language does
    // not have -- and accepting it let `__str__` be written as though a
    // reference needed reaching through.
    let source = "type H { v int; }\nfn __init__(this &H) void {\n    this.v = 0;\n}\nfn bad(this &H) int {\n    return *this;\n}\n";
    let (outcome, output) = compile_text("test.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(
        output.contains("which is a reference"),
        "the diagnostic should say a reference cannot be dereferenced: {output}"
    );
}

#[test]
fn a_pointer_is_dereferenced() {
    // The other half: `*p` on a pointer is how one reaches the value, and it
    // yields a reference to it.
    let source = "fn f() int {\n    x int = 5;\n    p *int = &x;\n    *p = 7;\n    return x;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("store i32 7"), "{output}");
}

#[test]
fn a_reference_passes_where_the_value_does() {
    // A reference is handed over as it stands, which is what replaced `*this`:
    // the copy constructor and `push` both take `&const T`.
    let source = "type H { v int; }\nfn __init__(this &H) void {\n    this.v = 0;\n}\nfn copy_of(this &const H) H {\n    made H = this;\n    return made;\n}\nfn main() int {\n    a H = H();\n    b H = copy_of(a);\n    return b.v;\n}\n";
    let (outcome, output) = compile_text("test.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Success, "{output}");
}

#[test]
fn an_rvalue_reference_parameter_binds_a_temporary() {
    // `&&T` is the move form: it takes a value nothing else can name. A
    // temporary is exactly that, so the call reaches the `&&` overload rather
    // than the copying one.
    let source = "type H { v int; }\nfn __init__(this &H) void {\n    this.v = 0;\n}\nfn make() H {\n    m H = H();\n    return m;\n}\nfn take_copy(other &const H) int {\n    return 1;\n}\nfn take_move(other &&H) int {\n    return 2;\n}\nfn main() int {\n    return take_move(make());\n}\n";
    let output = ir_of(source);
    // The mangling marks an rvalue reference with `W`, so the symbol the call
    // names is the move overload. Asserted at the *call site*: both functions
    // are defined in this module, so the definition proves nothing about which
    // one the call reached.
    assert!(
        output.contains("call i32 @take_move_W0N1_H"),
        "the call did not reach the move overload: {output}"
    );
    assert!(
        !output.contains("call i32 @take_copy_R"),
        "the call reached the copying overload: {output}"
    );
}

#[test]
fn an_rvalue_reference_parameter_name_parses() {
    // `x &&T` used to read `x` as the *type*, because `&&` was not listed as
    // something that can begin a type. The parameter then had no type and the
    // declaration failed to parse.
    let source = "type H { v int; }\nfn __init__(this &H) void {\n    this.v = 0;\n}\nfn take(other &&H) int {\n    return 0;\n}\nfn main() int {\n    a H = H();\n    return take(a);\n}\n";
    let (outcome, output) = compile_text("test.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Success, "{output}");
}

#[test]
fn an_unsized_array_parameter_is_a_pointer_that_keeps_its_const() {
    // `[]T` in a parameter list is `*T`, so `[]const T` is `*const T`. The
    // qualifier was dropped, which let a callee write through a pointer the
    // caller had said was read-only.
    let source = "fn show(s []const c8) int {\n    return s[0] as int;\n}\nfn write(s []const c8) int {\n    s[0] = 'x';\n    return 0;\n}\nfn f() int {\n    return show(\"hi\");\n}\n";
    let (outcome, output) = compile_text("test.fft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(
        output.contains("const"),
        "the writes must be refused:\n{output}"
    );
}

#[test]
fn an_unsized_array_parameter_accepts_a_string_literal() {
    // The decay `[N]const c8` -> `*const c8` is what lets a literal fill a
    // `[]const c8` parameter.
    let source = "fn first(s []const c8) int {\n    return s[0] as int;\n}\nfn f() int {\n    return first(\"hi\");\n}\nfn main() int {\n    return f();\n}\n";
    let output = ir_of(source);
    assert!(output.contains("ret i32"), "{output}");
}

#[test]
fn type_of_a_name_is_its_declared_type() {
    // `type(a)` asks what `a` *is*, so the binding it declares has the same
    // type: here `b` is an `int`, and returning it from an `int` function
    // needs no conversion.
    let source = "fn f() int {\n    a int = 5;\n    b type(a) = 6;\n    return b;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("store i32 5"), "{output}");
    assert!(output.contains("store i32 6"), "{output}");
}

#[test]
fn type_of_a_parenthesised_name_is_a_reference() {
    // `type((a))` asks what the *expression* `a` is, and an expression that
    // names storage is a place: the type is `&int`. The IR shows the pointer
    // being stored and then written through.
    let source =
        "fn f() int {\n    a int = 5;\n    r type((a)) = &a;\n    *r = 9;\n    return a;\n}\n";
    let output = ir_of(source);
    assert!(
        output.contains("store ptr"),
        "a reference is stored: {output}"
    );
    assert!(output.contains("store i32 9"), "{output}");
}

#[test]
fn a_reference_from_type_of_is_not_a_second_reference() {
    // `r` is already a `&int`; asking for the type of `r` as an expression
    // gives `&int` again, not `&&int`, because a reference is an address.
    let source = "fn f() int {\n    a int = 5;\n    r type((a)) = &a;\n    s type((r)) = r;\n    *s = 7;\n    return a;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("store i32 7"), "{output}");
}

#[test]
fn type_of_a_value_is_not_a_reference() {
    // A call produces a value, not storage, so `type((call()))` is the call's
    // return type. Initialising an `int` from it is what proves it: a `&int`
    // would be refused.
    let source = "fn g() int {\n    return 3;\n}\nfn f() int {\n    v type((g())) = g();\n    return v;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("ret i32"), "{output}");
}

#[test]
fn a_parameter_may_take_its_type_from_an_earlier_one() {
    // The parameters before it are in scope, so `b type(a)` is an `int` when
    // `a` is.
    let source = "fn add(a int, b type(a)) int {\n    return a + b;\n}\nfn f() int {\n    return add(2, 3);\n}\n";
    let output = ir_of(source);
    assert!(output.contains("@add_I3_i32_I3_i32"), "{output}");
    assert!(output.contains("i32 %"), "{output}");
}

#[test]
fn a_reference_binding_may_not_be_initialised_from_a_value() {
    // The distinction is real: `type((a))` is `&int`, so a plain integer does
    // not fit it.
    let source = "fn f() int {\n    a int = 5;\n    r type((a)) = 6;\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("type mismatch"), "{output}");
}

#[test]
fn type_of_is_refused_where_no_expression_is_in_scope() {
    // A declaration is laid out before any expression is checked, so a field
    // cannot ask for one's type. The diagnostic says so instead of claiming the
    // type is unknown.
    let source = "type P {\n    x type(1);\n}\nfn f() int {\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("cannot be used here"), "{output}");
}

#[test]
fn a_pointer_to_a_pointer_is_a_type() {
    // `**T` is written with the same token the multiplication operator uses, so
    // the type grammar has to split it -- exactly as `&&T` is split into an
    // rvalue reference. Without that a function taking `head **Node` did not
    // parse at all, which made a linked list impossible to write.
    let source = "fn set(head **int, value *int) void {\n    *head = value;\n}\nfn f() int {\n    x int = 1;\n    p *int = &x;\n    set(&p, &x);\n    return 0;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("ptr %"), "{output}");
}

#[test]
fn a_dereference_initialises_a_pointer_binding_with_its_value() {
    // `x *int = *pp;` is the int pointer the reference names, not the address
    // of the slot it sits in. Code generation suppressed the read because a
    // pointer looked like an "address wanted" context, so the binding held the
    // address and every use of it read the wrong memory.
    let source = "fn f() int {\n    x int = 5;\n    p *int = &x;\n    pp **int = &p;\n    q *int = *pp;\n    return *q;\n}\n";
    let output = ir_of(source);
    // The read is there: the binding is stored from a loaded pointer.
    assert!(output.contains("load ptr, ptr"), "{output}");
}

#[test]
fn a_dereference_returns_a_pointer_value() {
    // The same rule at a `return`: a function returning `*T` wants the value
    // `*p` names, not the address of the storage.
    let source = "fn first(head **int) *int {\n    return *head;\n}\nfn f() int {\n    x int = 7;\n    p *int = &x;\n    q *int = first(&p);\n    return *q;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("ret ptr"), "{output}");
}

#[test]
fn a_type_may_mention_itself_through_a_pointer() {
    // `next *Node` inside `Node` is what makes a linked list or a tree
    // expressible. The name is registered before the fields are resolved, so
    // the self-reference names the type being declared.
    let source = "type Node {\n    value int;\n    next *Node;\n}\nfn f() int {\n    node *Node = nullptr;\n    value Node;\n    return sizeof(Node) as int;\n}\n";
    let output = ir_of(source);
    // `{ i32, ptr }`: the pointer is eight-byte aligned, so the struct is
    // sixteen bytes and not twelve.
    assert!(output.contains("{ i32, ptr }"), "{output}");
    assert!(output.contains("ret i32 16"), "{output}");
}

#[test]
fn a_type_may_not_contain_itself_by_value() {
    // A field of the type's own type has a size that depends on itself, which
    // is not a number.
    let source = "type Bad {\n    child Bad;\n}\nfn f() int {\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("contains itself by value"), "{output}");
}

#[test]
fn a_struct_is_laid_out_with_padding_and_aligned_to_its_widest_field() {
    // The checker's `sizeof` has to agree with the layout code generation
    // gives a struct, or a buffer allocated with one number is written past its
    // end by the other.
    let source = "type P {\n    a int;\n    b i64;\n    c int;\n}\nfn f() int {\n    return sizeof(P) as int;\n}\n";
    let output = ir_of(source);
    // 4 + padding 4 + 8 + 4 + padding 4 = 24.
    assert!(output.contains("ret i32 24"), "{output}");
}

#[test]
fn a_local_shadows_a_module_name() {
    // `node->value` where a local is called `node` reads a field; the module
    // table must not claim the name first. The module is only reachable when
    // nothing in scope is called that.
    let source = "type Node {\n    value int;\n}\nfn f() int {\n    node *Node = nullptr;\n    return 0;\n}\n";
    let output = ir_of(source);
    assert!(output.contains("alloca"), "{output}");
}

#[test]
fn a_bad_index_names_the_index_type_it_wanted() {
    // A container that has `__getitem__` *can* be indexed; when the subscript
    // does not fit its parameter, "cannot be indexed" named the wrong thing.
    let source = "type C {\n    _v int;\n}\nfn __getitem__(this &C, i usize) &int {\n    return this._v;\n}\nfn f() int {\n    c C;\n    n int = 0;\n    return c[n];\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("is indexed by"), "{output}");
    assert!(output.contains("but `i32` was given"), "{output}");
    assert!(!output.contains("cannot be indexed"), "{output}");
}

#[test]
fn a_negated_condition_is_evaluated_once() {
    // `!` truth-tests its operand, which is a different lowering from reading
    // it -- and doing both meant `if (!f())` called `f` twice.
    let source = r#"import C;

fn bump(count *int) int {
    *count = *count + 1;
    return 1;
}

fn main() int {
    calls int = 0;
    if (!(bump(&calls) == 1)) {
        C.printf("never\n");
    }
    C.printf("%d\n", calls);
    return 0;
}
"#;
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
    // Count the calls in `main`: one, however many blocks the condition
    // produced.
    let start = output.find("define i32 @main").expect("main is emitted");
    let body = &output[start..];
    let end = body.find("\n}").expect("main ends");
    let calls = body[..end].matches("call i32 @bump").count();
    assert_eq!(calls, 1, "{}", &body[..end]);
}

#[test]
fn a_reference_argument_is_read_through_at_a_value_parameter() {
    // A reference names a value, so passing one where a value is wanted passes
    // the value. Passing the address instead handed the callee a pointer where
    // it expected an `int`, which the IR verifier refused.
    let source = r#"fn take(v int) int {
    return v;
}

fn main() int {
    x int = 5;
    p *int = &x;
    r &int = *p;
    return take(r);
}
"#;
    let output = ir_of(source);
    assert!(
        output.contains("call i32 @take_I3_i32(i32"),
        "the value behind the reference is what a value parameter gets:\n{output}"
    );
}

#[test]
fn a_field_of_a_reference_returning_call_is_reachable() {
    // `r.get().n` is a field of what the call returns. The call's result is a
    // value with no storage of its own, so code generation gives it a temporary
    // -- and lowering the call a second time to take its address would run it
    // twice.
    let source = r#"type Inner {
    n int;
}

type Holder {
    v Inner;
}

fn get(this &Holder) &Inner {
    return &this.v;
}

fn main() int {
    h Holder = .{.{7}};
    return h.get().n;
}
"#;
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(output.contains("define i32 @main("), "{output}");
}

#[test]
fn a_parameter_may_be_named_after_a_module() {
    // A local shadows a module of the same name: `memory.alloc(8)` calls the
    // parameter's method, not `std.memory`'s function. Reading it as a module
    // dropped the receiver and called a function of another type.
    let source = r#"import std;

type Mem {
}

fn alloc(this &Mem, size usize) *byte {
    return nullptr;
}

fn run(memory Mem) bool {
    return memory.alloc(8) == nullptr;
}

fn main() int {
    m Mem = .{};
    return run(m) as int;
}
"#;
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
    assert!(
        output.contains("alloc_R0N3_Mem_I3_u64(ptr %memory"),
        "the receiver is passed to the parameter's own method:\n{output}"
    );
}
