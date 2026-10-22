//! The Fallthrough code generator: checked AST in, LLVM IR out.
//!
//! # There is no bespoke IR
//!
//! This crate deliberately does **not** define its own instruction set. LLVM IR
//! *is* the compiler's intermediate representation, and lowering targets it
//! directly through [`inkwell`]. That decision removes an entire layer from the
//! compiler: there is no translation from a private IR to LLVM, no private
//! instruction selection, and no private verifier to keep in sync with a
//! private opcode enum.
//!
//! What would have been `Opcode`, `Instruction`, `BasicBlock`, and `ValueId`
//! are now [`inkwell::values::FunctionValue`],
//! [`inkwell::basic_block::BasicBlock`], and the LLVM builder API. The
//! properties those types used to enforce by hand — blocks ending in exactly
//! one terminator, operands dominating their uses, well-formed phi nodes — are
//! checked by LLVM itself via [`Backend::verify`], which is strictly better
//! than a hand-written verifier because LLVM's is the one that actually
//! governs code generation.
//!
//! # Module map
//!
//! * [`backend`] — owns the [`inkwell::context::Context`] and the LLVM module.
//! * [`codegen`] — the [`Codegen`] trait and the AST walk that drives it.
//! * [`types`] — maps [`fallc_sema::Type`] to [`inkwell::types::BasicTypeEnum`].
//!
//! # An inkwell lifetime note
//!
//! LLVM objects borrow from the [`inkwell::context::Context`] that created
//! them, and inkwell encodes that in lifetimes. Several LLVM objects cannot
//! coexist in one struct when they borrow the same context (a known limitation
//! of the safe wrapper). [`Backend`] therefore owns the context and the module
//! and hands out borrowed views, rather than storing builder objects alongside
//! them.
//!
//! # Lowering rules
//!
//! Each construct lowers to the LLVM IR the language specifies. `&&` and `||`
//! short-circuit through blocks joined by a `phi`; `if`, the loops, and
//! `switch` become blocks joined by `br`, and `fallthrough` branches to the
//! next arm; aggregate values are materialised and copied. A signed overflow or
//! a division by zero traps in a debug build and is undefined in a release
//! build. `main` is emitted under the C ABI, with a wrapper when it may raise.

// `unsafe_code` is denied rather than forbidden here, for one documented
// exception in `backend.rs`: inkwell has no safe way to hold an LLVM context
// and the module derived from it in the same struct. The exception is a single
// function with a written safety argument, and no other module may add one
// without the same justification.
#![deny(unsafe_code)]

pub mod backend;
pub mod codegen;
pub mod types;

pub use backend::{Backend, CodegenError, ModuleText};
pub use codegen::{
    BuildMode, Codegen, ExternalSignature, LowerResult, Lowerer, ModuleBodyRef, ModuleResolutions,
    OptLevel,
};
pub use types::TypeMapper;
