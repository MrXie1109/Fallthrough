//! How `Lowerer` reports what it could not compile.

use super::*;

impl<'ctx> Lowerer<'ctx> {
    /// Lowers the `fallthrough` construct.
    ///
    /// The rule is settled: `fallthrough;` is a statement that branches to the
    /// next arm of the enclosing `switch`, or to the join block after the last
    /// arm. The walk applies it in `Lowerer::lower_switch`, which is the
    /// construct that knows the next arm; this entry point has no callers in the
    /// walk, so it reports rather than branching to a target it does not have.
    ///
    /// # Errors
    ///
    /// Always returns [`CodegenError::WaitingForSpec`], because there is no
    /// enclosing arm here to supply the target block.
    pub fn lower_fallthrough(&mut self, span: Span) -> Result<(), CodegenError> {
        // The target block comes from the enclosing `switch`, which only
        // `lower_switch` knows.
        Err(CodegenError::waiting_for_spec("`fallthrough`", span))
    }

    /// Reports that a construct cannot yet be lowered.
    ///
    /// Used for the paths no implemented construct reaches, so that a
    /// partially-implemented compiler fails at the exact place it gave up
    /// instead of emitting wrong IR.
    pub fn report_waiting_for_spec(&mut self, span: Span, what: &str) {
        self.diagnostics
            .push(CodegenError::waiting_for_spec(what, span).to_diagnostic());
    }

    /// Records an error message as a diagnostic.
    pub(super) fn report_error(&mut self, message: &str) {
        self.diagnostics.push(
            fallc_diag::Diagnostic::bug(message.to_owned())
                .with_code(fallc_diag::DiagnosticCode::ir(2)),
        );
    }

    /// Records an arbitrary diagnostic.
    pub fn push_diagnostic(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    /// Returns the diagnostics accumulated so far.
    #[must_use]
    pub fn diagnostics(&self) -> &DiagnosticBag {
        &self.diagnostics
    }
}
