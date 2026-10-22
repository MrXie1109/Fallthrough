//! End-to-end proof that this crate drives real LLVM.
//!
//! These tests build LLVM IR through the public API, run LLVM's own verifier,
//! and check the emitted assembly. They exist because the hand-rolled IR crate
//! this replaced could not have caught a genuine memory-safety bug: a field
//! ordering mistake here segfaulted inside LLVM. Anything that talks to LLVM
//! needs a test that actually executes it.

use fallc_ir::Backend;
use inkwell::values::BasicValue;

#[test]
fn emits_verifiable_llvm_assembly() {
    let backend = Backend::new("smoke");
    let context = backend.context();
    let i32_type = context.i32_type();

    let main = backend
        .module()
        .add_function("main", i32_type.fn_type(&[], false), None);
    let entry = context.append_basic_block(main, "entry");
    let builder = context.create_builder();
    builder.position_at_end(entry);
    builder
        .build_return(Some(&i32_type.const_int(0, false).as_basic_value_enum()))
        .expect("building a return cannot fail");

    backend.verify().expect("module must verify");
    let ir = backend.to_llvm_ir().0;
    assert!(ir.contains("define i32 @main()"), "{ir}");
    assert!(ir.contains("ret i32 0"), "{ir}");
}

#[test]
fn control_flow_lowers_to_real_basic_blocks() {
    // The shape `fallthrough` will eventually need: two blocks joined by an
    // unconditional branch, which is what `br label %x` compiles to.
    let backend = Backend::new("cfg");
    let context = backend.context();
    let void = context.void_type();

    let function = backend
        .module()
        .add_function("cfg", void.fn_type(&[], false), None);
    let entry = context.append_basic_block(function, "entry");
    let target = context.append_basic_block(function, "target");

    let builder = context.create_builder();
    builder.position_at_end(entry);
    builder
        .build_unconditional_branch(target)
        .expect("branch cannot fail");
    builder.position_at_end(target);
    builder.build_return(None).expect("return cannot fail");

    backend.verify().expect("module must verify");
    let ir = backend.to_llvm_ir().0;
    assert!(ir.contains("br label %target"), "{ir}");
    assert!(ir.contains("target:"), "{ir}");
}

#[test]
fn llvm_verifier_rejects_invalid_ir_from_this_crate() {
    // The verifier must be load-bearing: if it silently passed everything,
    // the tests above would prove nothing.
    let backend = Backend::new("invalid");
    let context = backend.context();
    let function =
        backend
            .module()
            .add_function("f", context.void_type().fn_type(&[], false), None);
    // A block with no terminator.
    let _entry = context.append_basic_block(function, "entry");

    assert!(backend.verify().is_err());
}

#[test]
fn dropping_a_backend_does_not_crash() {
    // Regression guard for a real bug: `Backend` holds an LLVM context and a
    // module created from it. Rust drops fields in declaration order, and
    // LLVM requires the module to be disposed before its context. The wrong
    // order segfaults here.
    for _ in 0..8 {
        let backend = Backend::new("drop");
        let context = backend.context();
        let function =
            backend
                .module()
                .add_function("f", context.void_type().fn_type(&[], false), None);
        let entry = context.append_basic_block(function, "entry");
        let builder = context.create_builder();
        builder.position_at_end(entry);
        builder.build_return(None).expect("return cannot fail");
        drop(backend);
    }
}
