//! Progress and recovery.
//!
//! Parsing a wrong program must still terminate and still leave one useful
//! diagnostic, which is what these helpers are for: recovery skips to an item
//! or a statement, and speculation rolls the tokens and the diagnostics back.

use super::*;

impl Parser {
    /// Skips tokens until something that can start an item appears.
    ///
    /// Recovery is token-at-a-time: one token is consumed per attempt, which
    /// always terminates but does not resume at the next declaration, because
    /// there is no item-level synchronisation set.
    pub fn recover_to_item(&mut self, stream: &mut TokenStream) {
        if !stream.at_eof() {
            stream.advance();
        }
    }

    /// Runs `f` speculatively: on `None`, restores the token position and
    /// discards any diagnostics `f` produced.
    pub fn speculate<T>(
        &mut self,
        stream: &mut TokenStream,
        f: impl FnOnce(&mut Self, &mut TokenStream) -> Option<T>,
    ) -> Option<T> {
        let checkpoint: Checkpoint = stream.checkpoint();
        let diagnostics_before = self.diagnostics.len();
        let result = f(self, stream);
        if result.is_none() {
            stream.rewind(checkpoint);
            self.diagnostics.truncate(diagnostics_before);
        }
        result
    }

    /// Consumes the current token if it has kind `kind`.
    pub fn eat(&mut self, stream: &mut TokenStream, kind: TokenKind) -> Option<Token> {
        stream.eat(kind)
    }

    /// Consumes `kind` or reports an error.
    pub fn expect(&mut self, stream: &mut TokenStream, kind: TokenKind) -> Option<Token> {
        match stream.eat(kind) {
            Some(token) => Some(token),
            None => {
                let found = stream.current_kind().describe();
                let wanted = kind.describe();
                self.push_diagnostic(
                    Diagnostic::error(format!("expected {wanted}, found {found}"))
                        .with_code(DiagnosticCode::parser(1))
                        .with_span(stream.current_span()),
                );
                None
            }
        }
    }

    /// Reports an unexpected token where `wanted` was required.
    pub fn report_unexpected(&mut self, stream: &TokenStream, wanted: &str) {
        let found = stream.current_kind().describe();
        self.push_diagnostic(
            Diagnostic::error(format!("expected {wanted}, found {found}"))
                .with_code(DiagnosticCode::parser(2))
                .with_span(stream.current_span()),
        );
    }

    /// Reports a construct that the parser recognises but cannot yet build,
    /// because the language spec has not fixed it.
    ///
    /// Distinct from a syntax error: this is the parser saying "this is a
    /// compiler-side gap", which makes an unimplemented path obvious during
    /// development instead of silently mis-parsing.
    /// Records a diagnostic produced by the parser.
    pub fn push_diagnostic(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    /// Returns the diagnostics accumulated so far.
    #[must_use]
    pub fn diagnostics(&self) -> &DiagnosticBag {
        &self.diagnostics
    }

    /// Enters a nesting level, reporting a diagnostic if the limit is
    /// exceeded.
    ///
    /// Returns `false` if the caller must bail out immediately; callers must
    /// then call [`Parser::leave`] exactly once, as if `enter` had returned
    /// `true`.
    pub(super) fn enter(&mut self, stream: &TokenStream) -> bool {
        if self.depth >= self.config.max_recursion_depth {
            if !self.poisoned {
                self.poisoned = true;
                self.push_diagnostic(
                    Diagnostic::error("expression nests too deeply")
                        .with_code(DiagnosticCode::parser(3))
                        .with_span(stream.current_span())
                        .with_note(format!(
                            "the parser gives up past {} levels of nesting",
                            self.config.max_recursion_depth
                        )),
                );
            }
            self.depth += 1;
            return false;
        }
        self.depth += 1;
        true
    }

    /// Leaves a nesting level. Must be called once per [`Parser::enter`].
    pub(super) fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
        if self.depth == 0 {
            self.poisoned = false;
        }
    }
}
