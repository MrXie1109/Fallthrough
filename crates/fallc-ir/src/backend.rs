//! The LLVM backend: owns the context and the module, and renders the result.
//!
//! [`Backend`] is the only place in the compiler that holds LLVM state. It is
//! created once per compilation unit by the driver and passed down to lowering.
//!
//! # Why the context lives here
//!
//! Every LLVM object is owned by an [`inkwell::context::Context`], and the
//! context must outlive all of them. Making [`Backend`] own it, and handing out
//! borrowed views, keeps that relationship explicit in the type system rather
//! than in a comment.
//!
//! # Verification is LLVM's job
//!
//! [`Backend::verify`] runs LLVM's own module verifier. This replaces the
//! hand-written IR verifier the compiler used to carry: LLVM's verifier knows
//! far more (dominance, type agreement, phi correctness, terminator rules) and
//! is the thing that actually gates code generation, so agreeing with it is
//! what matters.

use std::fmt;
use std::rc::Rc;

use inkwell::OptimizationLevel;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::passes::PassBuilderOptions;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine, TargetTriple,
};

use crate::codegen::OptLevel;
use fallc_diag::{Diagnostic, DiagnosticBag, DiagnosticCode};
use fallc_span::Span;

/// Something that went wrong while generating LLVM IR.
///
/// Distinct from a diagnostic about the user's program: these are all
/// "the compiler could not do its job", including the spec-blocked case.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum CodegenError {
    /// Code generation reached a construct the language spec does not define.
    ///
    /// This is the expected outcome for most inputs today, and is not a bug.
    WaitingForSpec {
        /// What could not be lowered.
        what: String,
        /// Where in the source it was.
        span: Span,
    },
    /// LLVM rejected the module we built.
    Verification(String),
    /// LLVM could not emit the requested output.
    Emit(String),
}

impl CodegenError {
    /// Creates a spec-blocked error.
    #[must_use]
    pub fn waiting_for_spec(what: impl Into<String>, span: Span) -> Self {
        CodegenError::WaitingForSpec {
            what: what.into(),
            span,
        }
    }

    /// Returns the source span this error points at, if any.
    #[must_use]
    pub fn span(&self) -> Option<Span> {
        match self {
            CodegenError::WaitingForSpec { span, .. } => Some(*span),
            CodegenError::Verification(_) | CodegenError::Emit(_) => None,
        }
    }

    /// Converts this error into a diagnostic.
    ///
    /// A construct the compiler cannot lower becomes a
    /// [`fallc_diag::Severity::Bug`] diagnostic rather than an error, because
    /// the user's program is not at fault.
    #[must_use]
    pub fn to_diagnostic(&self) -> Diagnostic {
        match self {
            CodegenError::WaitingForSpec { what, span } => Diagnostic::bug(format!(
                "cannot generate code for {what}"
            ))
            .with_code(DiagnosticCode::ir(999))
            .with_span(*span)
            .with_note(
                "the program was understood -- the checker accepted it -- but this part of the \
                 code generator does not exist yet",
            ),
            CodegenError::Verification(message) => {
                Diagnostic::bug(format!("generated LLVM IR failed verification: {message}"))
                    .with_code(DiagnosticCode::ir(2))
            }
            CodegenError::Emit(message) => {
                Diagnostic::error(format!("cannot emit output: {message}"))
                    .with_code(DiagnosticCode::driver(2))
            }
        }
    }
}

impl fmt::Display for CodegenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CodegenError::WaitingForSpec { what, span } => {
                write!(f, "cannot generate code for {what} (at {span:?})")
            }
            CodegenError::Verification(message) => write!(f, "LLVM verification failed: {message}"),
            CodegenError::Emit(message) => write!(f, "cannot emit output: {message}"),
        }
    }
}

impl std::error::Error for CodegenError {}

/// The rendered textual form of a module.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ModuleText(pub String);

impl fmt::Display for ModuleText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Returns a reference to the `Context` owned by `context`, typed as
/// `'static`.
///
/// # Safety of the returned reference
///
/// The returned reference is tied to the allocation, not to the `Rc` value, so
/// it stays valid as long as that allocation is alive. The caller
/// ([`Backend`]) stores the `Rc` in the same struct as the reference, so the
/// allocation outlives every use of the reference.
///
/// The `'static` lifetime here is a requirement of LLVM's ownership model, not
/// a claim that the context lives forever: it is `Backend` that enforces the
/// real bound. This keeps the unsafety in one named place instead of spreading
/// a `transmute` across the crate.
#[allow(unsafe_code)] // The one place in the crate; see the safety argument above.
fn context_ref_of(context: &Rc<Context>) -> &'static Context {
    // SAFETY: `Rc::as_ptr` returns a pointer into the `Rc`'s allocation, which
    // is not moved when the `Rc` itself is moved. `Backend` holds the owning
    // `Rc` for as long as it holds the derived reference, so the allocation
    // cannot be freed while the reference is reachable.
    //
    // `Context` is `repr(transparent)` over a single pointer, so a shared
    // reference to it is valid for as long as the allocation lives, and the
    // context is never mutated through this path.
    unsafe { &*Rc::as_ptr(context) }
}

/// Owns the LLVM context and module for one compilation unit.
///
/// # Construction order
///
/// `Backend::new` must be called before any lowering, and the returned value
/// must outlive every LLVM object derived from it. The driver arranges this by
/// creating the backend and immediately running lowering within the same scope.
///
/// # Why the context is held in an `Rc`
///
/// LLVM objects borrow from the context that created them, so a struct holding
/// both is self-referential, and inkwell has no safe owned-pairing type. The
/// borrow checker cannot know that `Context` is `repr(transparent)` over a heap
/// pointer that is stable across moves, so a plain field would not compile even
/// though it would be correct.
///
/// Holding the context in an `Rc` fixes this without `unsafe`: the `Rc` keeps
/// the allocation alive, and `Rc::as_ptr` yields an address that does not move
/// when the `Rc` is moved. inkwell uses the same technique internally for its
/// execution engine, so this is the wrapper's own idiom rather than a trick.
///
/// The `Rc` is never cloned out of the backend, so the reference cannot
/// outlive the allocation it points into.
#[derive(Debug)]
pub struct Backend {
    /// The LLVM module.
    ///
    /// Declared **before** `context` on purpose: Rust drops struct fields in
    /// declaration order, and LLVM requires a module to be disposed before the
    /// context that owns it. Getting this backwards frees the context first and
    /// segfaults inside LLVM, so the order is load-bearing and must not be
    /// "tidied" into alphabetical or logical order.
    module: Module<'static>,
    /// Keeps the context allocation alive; see the type-level docs.
    context: Rc<Context>,
    diagnostics: DiagnosticBag,
}

impl Backend {
    /// Creates a backend with an LLVM module named `module_name`.
    ///
    /// The module name becomes LLVM's `ModuleID` and shows up in the emitted
    /// `.ll`, so it is derived from the input file name by the driver.
    #[must_use]
    pub fn new(module_name: &str) -> Self {
        Self::with_context(Rc::new(Context::create()), module_name)
    }

    /// Creates a backend whose module lives in `self`'s LLVM context.
    ///
    /// Two modules that will be linked together must come from one context:
    /// `Module::link_in_module` requires it. A cached module body is therefore
    /// built with this constructor, in the same context as the unit it is
    /// linked into, and both backends share the context allocation -- the last
    /// one to drop frees it, and each drops its own module first because the
    /// module field is declared before the `Rc`.
    #[must_use]
    pub fn new_in(&self, module_name: &str) -> Self {
        Self::with_context(Rc::clone(&self.context), module_name)
    }

    fn with_context(context: Rc<Context>, module_name: &str) -> Self {
        // Derive the reference from the same allocation the `Rc` owns. The
        // pointee does not move when `context` is moved into `Backend`, and
        // the `Rc` field keeps it alive for the backend's whole lifetime.
        let context_ref: &'static Context = context_ref_of(&context);
        let module = context_ref.create_module(module_name);
        let backend = Backend {
            context,
            module,
            diagnostics: DiagnosticBag::new(),
        };
        // Stamp the module with the machine it will actually be compiled for.
        //
        // Without this the module carries no data layout, and LLVM optimises
        // against an assumed default in which `i64` is 4-byte aligned. The
        // loads and stores the lowering emits then disagree with the layout
        // used to generate code, and at `-Oz` the alias analysis concluded a
        // `store i64 .., align 4` did not overwrite the whole of a `usize`
        // field -- so `Array::push` updated `_len` and the following read saw
        // the old value, indexing out of bounds.
        //
        // The triple and layout are taken from the *same* target machine the
        // object file is written with, so the two cannot drift apart.
        //
        // A machine that cannot be created is not fatal here: the module is
        // still well formed and the failure is reported where the object file
        // is written, which is the point at which it actually matters.
        if let Ok(machine) = backend.target_machine(OptimizationLevel::None) {
            let triple = TargetMachine::get_default_triple();
            backend.module.set_triple(&triple);
            backend
                .module
                .set_data_layout(&machine.get_target_data().get_data_layout());
        }
        backend
    }

    /// Returns a handle to the LLVM context, typed with the context's
    /// `'static`.
    ///
    /// This must be `'static` and not tied to `&self`. Every LLVM object
    /// created from the context (types, values, blocks) is itself typed with
    /// `'static`, and `Module<'ctx>` is invariant, so a `&self`-bound reference
    /// would make the API unusable: no LLVM object could be created and then
    /// used alongside the module.
    ///
    /// # Lifetime
    ///
    /// `'static` describes the *allocation*, which the `Rc` keeps alive for as
    /// long as the backend lives. As with [`Backend::module`], callers must not
    /// let context-derived objects outlive the backend. The driver satisfies
    /// this by doing all LLVM work inside one function scope.
    #[must_use]
    pub fn context(&self) -> &'static Context {
        context_ref_of(&self.context)
    }

    /// Returns the LLVM module under construction.
    ///
    /// # Lifetime
    ///
    /// The module is typed with the context's `'static`, because `Module<'ctx>`
    /// is invariant and cannot be shortened through `&self`. That means a
    /// caller *could* hold onto the returned reference past the backend's
    /// death, which would be a use-after-free.
    ///
    /// `Backend` therefore owns the context for its whole life and frees it
    /// only in `Drop`, and callers must not let a module reference outlive the
    /// backend. The driver already satisfies this by creating the backend and
    /// finishing all LLVM work inside one function scope. If this crate ever
    /// grows a long-lived backend, this is the API to revisit.
    #[must_use]
    pub fn module(&self) -> &Module<'static> {
        &self.module
    }

    /// Records a codegen error as a diagnostic.
    pub fn report(&mut self, error: &CodegenError) {
        self.diagnostics.push(error.to_diagnostic());
    }

    /// Records an arbitrary diagnostic.
    pub fn push_diagnostic(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    /// Returns `true` if any fatal diagnostic has been recorded.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.diagnostics.has_errors()
    }

    /// Takes the diagnostics recorded so far.
    #[must_use]
    pub fn take_diagnostics(&mut self) -> Vec<Diagnostic> {
        std::mem::take(&mut self.diagnostics).into_vec()
    }

    /// Runs LLVM's module verifier.
    ///
    /// # Errors
    ///
    /// Returns [`CodegenError::Verification`] carrying LLVM's own message,
    /// which names the offending function and instruction.
    pub fn verify(&self) -> Result<(), CodegenError> {
        self.module
            .verify()
            .map_err(|error| CodegenError::Verification(error.to_string()))
    }

    /// Renders the module as LLVM assembly text (`.ll`).
    #[must_use]
    pub fn to_llvm_ir(&self) -> ModuleText {
        ModuleText(self.module.print_to_string().to_string())
    }

    /// Renders the module as LLVM bitcode.
    #[must_use]
    pub fn write_bitcode_to_memory(&self) -> inkwell::memory_buffer::MemoryBuffer<'_> {
        self.module.write_bitcode_to_memory()
    }

    /// Runs LLVM's optimisation pipeline over the module.
    ///
    /// # What runs
    ///
    /// The pipeline is named as a string, in the same language `opt` accepts --
    /// `default<O2>`, `default<Os>` -- and LLVM expands it into its own pass
    /// list. Naming the pipeline rather than enumerating passes means the
    /// optimiser is exactly the one the installed LLVM ships, and there is no
    /// hand-written list here to fall out of date.
    ///
    /// `default<..>` is the right pipeline because it is the target-independent
    /// one; the target machine's own passes run later, when the object file is
    /// written, and those are what turn the optimised IR into machine code.
    ///
    /// At [`OptLevel::None`] nothing runs: the IR the front end emitted is what
    /// is wanted, and an empty pipeline would still cost a walk over it.
    pub fn optimize(&self, level: OptLevel) -> Result<(), CodegenError> {
        if level == OptLevel::None {
            return Ok(());
        }
        let machine = self.target_machine(level.as_llvm())?;
        // `spelling` is exactly the token the pipeline wants: a digit for the
        // numeric levels, `s` or `z` for the size ones.
        let pipeline = format!("default<O{}>", level.spelling());
        self.module
            .run_passes(&pipeline, &machine, PassBuilderOptions::create())
            .map_err(|error| {
                CodegenError::Emit(format!("the LLVM optimisation pipeline failed: {error}"))
            })
    }

    /// Returns the machine this compiler targets, for questions the target
    /// answers -- the width of a pointer, for instance.
    ///
    /// Targeting is host-only: there is no `--target` flag and no
    /// cross-compilation, so the triple, CPU, and features are the host's.
    ///
    /// Public because the lowering needs the pointer width to index, and a
    /// target machine is the only thing that knows it.
    pub fn target_machine(
        &self,
        optimization: OptimizationLevel,
    ) -> Result<TargetMachine, CodegenError> {
        self.target_machine_with(optimization, RelocMode::Default)
    }

    fn target_machine_with(
        &self,
        optimization: OptimizationLevel,
        relocation: RelocMode,
    ) -> Result<TargetMachine, CodegenError> {
        Target::initialize_native(&InitializationConfig::default()).map_err(|error| {
            CodegenError::Emit(format!("cannot initialise LLVM target: {error}"))
        })?;

        let triple = TargetMachine::get_default_triple();
        let target = Target::from_triple(&triple)
            .map_err(|error| CodegenError::Emit(format!("unknown target triple: {error}")))?;
        target
            .create_target_machine(
                &triple,
                &TargetMachine::get_host_cpu_name().to_string(),
                &TargetMachine::get_host_cpu_features().to_string(),
                optimization,
                relocation,
                CodeModel::Default,
            )
            .ok_or_else(|| CodegenError::Emit("cannot create a target machine".to_owned()))
    }

    /// Returns the target's pointer width, in bits.
    ///
    /// The layout a pointer imposes is the target's, not the language's: every
    /// type that contains one is as large as the machine makes it. The checker
    /// needs this to answer `sizeof`, and it comes from the same target machine
    /// the object file is written with, so the two cannot disagree.
    ///
    /// Associated rather than a method, because the checker is built before the
    /// module is and so has no backend to ask.
    pub fn pointer_bits() -> u32 {
        let Ok(machine) = Backend::new("pointer-width").target_machine(OptimizationLevel::None) else {
            // A machine that cannot be created leaves the width unknown, and
            // the checker reports `sizeof` of a pointer-containing type as not
            // being a constant -- which is what it is when the target is
            // unknown.
            return 0;
        };
        let data = machine.get_target_data();
        data.get_pointer_byte_size(None) * 8
    }

    /// Writes native object code for the host target to `path`.
    ///
    /// Targeting is host-only: there is no `--target` flag and no
    /// cross-compilation, so the object is for the machine the compiler runs
    /// on.
    ///
    /// # Errors
    ///
    /// Returns [`CodegenError::Emit`] if LLVM cannot initialise the target or
    /// write the file.
    /// Writes the module's bitcode to `path`.
    ///
    /// The module-object cache stores a module's lowered body as bitcode and
    /// links it into a later compilation, so the lowering is paid once.
    pub fn write_bitcode_to_file(&self, path: &std::path::Path) -> Result<(), CodegenError> {
        let buffer = self.write_bitcode_to_memory();
        std::fs::write(path, buffer.as_slice()).map_err(|error| {
            CodegenError::Emit(format!("cannot write `{}`: {error}", path.display()))
        })
    }

    /// Reads a module cache entry into this backend's context.
    ///
    /// The module must be linked into a module of the same context; that is
    /// why this is a method on the backend rather than a free function.
    pub fn read_module(&self, path: &std::path::Path) -> Result<Module<'static>, CodegenError> {
        let buffer = inkwell::memory_buffer::MemoryBuffer::create_from_file(path).map_err(
            |error| CodegenError::Emit(format!("cannot read `{}`: {error}", path.display())),
        )?;
        self.context().create_module_from_ir(buffer).map_err(|error| {
            CodegenError::Emit(format!("cannot load bitcode `{}`: {error}", path.display()))
        })
    }

    /// Links a module into this backend's module.
    ///
    /// The two must have been created in the same context; that is the contract
    /// `read_module` and [`Backend::new_in`] exist for.
    pub fn link_in_module(&self, other: Module<'static>) -> Result<(), CodegenError> {
        self.module
            .link_in_module(other)
            .map_err(|error| CodegenError::Emit(format!("cannot link a module body: {error}")))
    }

    pub fn write_object_file(
        &self,
        path: &std::path::Path,
        optimization: OptimizationLevel,
    ) -> Result<(), CodegenError> {
        self.write_object_file_with_relocation(path, optimization, RelocMode::Default)
    }

    /// Writes native object code that can go into a shared object.
    ///
    /// A shared object is loaded at an address the linker cannot know, so every
    /// access to a global or a function has to go through a relocation that
    /// works there. The target machine's position-independent mode is what
    /// emits those; a shared link of objects built for the default model fails
    /// with relocation errors.
    pub fn write_position_independent_object_file(
        &self,
        path: &std::path::Path,
        optimization: OptimizationLevel,
    ) -> Result<(), CodegenError> {
        self.write_object_file_with_relocation(path, optimization, RelocMode::PIC)
    }

    fn write_object_file_with_relocation(
        &self,
        path: &std::path::Path,
        optimization: OptimizationLevel,
        relocation: RelocMode,
    ) -> Result<(), CodegenError> {
        let machine = self.target_machine_with(optimization, relocation)?;

        machine
            .write_to_file(&self.module, FileType::Object, path)
            .map_err(|error| CodegenError::Emit(error.to_string()))
    }

    /// Returns the host target triple, for diagnostics.
    #[must_use]
    pub fn host_triple(&self) -> TargetTriple {
        TargetMachine::get_default_triple()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inkwell::values::BasicValue;

    #[test]
    fn a_fresh_module_verifies_and_prints_its_name() {
        let backend = Backend::new("hello");
        assert!(backend.verify().is_ok());
        let text = backend.to_llvm_ir();
        assert!(text.0.contains("ModuleID = 'hello'"), "{}", text.0);
    }

    /// Builds `i32 @answer() { ret i32 42 }` and returns the backend.
    ///
    /// Split out of the test body because the LLVM objects created here are
    /// typed with the context's `'static`, so they must not be interleaved
    /// with a borrow of the local `Rc` in the caller's scope.
    fn build_answer_module() -> Backend {
        let backend = Backend::new("probe");
        let context = backend.context();
        let i32_type = context.i32_type();
        let function = backend
            .module()
            .add_function("answer", i32_type.fn_type(&[], false), None);
        let entry = context.append_basic_block(function, "entry");
        let builder = context.create_builder();
        builder.position_at_end(entry);
        builder
            .build_return(Some(&i32_type.const_int(42, false).as_basic_value_enum()))
            .expect("building a return cannot fail");
        backend
    }

    #[test]
    fn a_hand_built_function_emits_real_llvm_ir() {
        let backend = build_answer_module();
        assert!(backend.verify().is_ok());
        let text = backend.to_llvm_ir().0;
        assert!(text.contains("define i32 @answer()"), "{text}");
        assert!(text.contains("ret i32 42"), "{text}");
    }

    /// Builds a function whose entry block has no terminator, which is invalid
    /// LLVM IR.
    fn build_unterminated_module() -> Backend {
        let backend = Backend::new("broken");
        let context = backend.context();
        let function =
            backend
                .module()
                .add_function("f", context.void_type().fn_type(&[], false), None);
        let _entry = context.append_basic_block(function, "entry");
        backend
    }

    #[test]
    fn verification_catches_a_missing_terminator() {
        let backend = build_unterminated_module();
        let result = backend.verify();
        assert!(
            matches!(result, Err(CodegenError::Verification(_))),
            "expected a verification failure, got {result:?}"
        );
    }

    #[test]
    fn a_construct_without_lowering_becomes_a_bug_diagnostic_not_an_error() {
        let error = CodegenError::waiting_for_spec("the `if` statement", Span::SYNTHETIC);
        let diagnostic = error.to_diagnostic();
        // The user's program is not at fault, so this must not be Severity::Error.
        assert_eq!(diagnostic.severity, fallc_diag::Severity::Bug);
        assert!(
            diagnostic.message.contains("`if` statement"),
            "the message names the construct: {}",
            diagnostic.message
        );
    }

    #[test]
    fn host_triple_is_available() {
        let backend = Backend::new("m");
        assert!(!backend.host_triple().as_str().is_empty());
    }
}
