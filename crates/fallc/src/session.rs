//! Per-compilation state.
//!
//! A [`Session`] is created once per `fallc` invocation and threaded through
//! every stage. It owns:
//!
//! * the [`SourceMap`], so spans from any stage resolve to text;
//! * one [`DiagnosticBag`] for the whole run, so diagnostics from different
//!   stages end up in one ordered list rather than five separate ones.
//!
//! Stages take `&mut Session` and push into it. This is the only piece of
//! shared mutable state in the compiler, and it is deliberately dumb: no
//! caching, no queries, just sources and diagnostics.

use fallc_diag::{Diagnostic, DiagnosticBag, Severity, Style, WarningPolicy};
use fallc_span::{FileId, SourceMap, Span};

/// Global state for one compilation.
#[derive(Debug)]
pub struct Session {
    sources: SourceMap,
    diagnostics: DiagnosticBag,
    /// Set when a stage gives up because the spec does not define the
    /// construct it met. Kept separate from ordinary errors so the driver can
    /// report it differently.
    hit_unspecified: Vec<(Span, String)>,
    /// Which warnings are reported, and whether they are errors.
    warnings: WarningPolicy,
    /// Whether diagnostics are printed in colour.
    ///
    /// Off unless the driver says otherwise: a diagnostic redirected into a file
    /// or compared by a test is text, and escapes in it are noise.
    color: bool,
    /// The files the user asked to compile, as paths that have been
    /// canonicalised where the file system allowed it.
    ///
    /// A warning about a library the program imports is not a warning about the
    /// program: the user cannot act on it here, and a library checked once per
    /// importing file would print the same remark once per file.
    input_files: Vec<std::path::PathBuf>,
}

impl Default for Session {
    fn default() -> Self {
        Session::new()
    }
}

impl Session {
    /// Creates an empty session.
    #[must_use]
    pub fn new() -> Self {
        Session {
            sources: SourceMap::new(),
            diagnostics: DiagnosticBag::new(),
            hit_unspecified: Vec::new(),
            warnings: WarningPolicy::new(),
            color: false,
            input_files: Vec::new(),
        }
    }

    /// Records the files the user asked to compile.
    pub fn set_input_files(&mut self, paths: impl IntoIterator<Item = std::path::PathBuf>) {
        self.input_files = paths
            .into_iter()
            .map(|path| std::fs::canonicalize(&path).unwrap_or(path))
            .collect();
    }

    /// Returns `true` if the source asks for this warning to be silent.
    ///
    /// `// no warning` on the line the warning starts on, on any line it
    /// covers, or on the line directly above, silences every warning there;
    /// `// no warning: S0140` silences that code, and `// no warning: unused`
    /// the group of that name. A longer remark ends its sentence with `--`
    /// before the marker: `// borrowed on purpose -- no warning`.
    ///
    /// The marker is the *tail* of the line's comment, so a string literal
    /// which happens to contain the words silences nothing: the text after it
    /// is not a comment, and a comment which does not end in the marker is
    /// prose.
    fn suppressed_by_comment(&self, diagnostic: &Diagnostic) -> bool {
        let Some(span) = diagnostic.primary else {
            return false;
        };
        if span.is_synthetic() {
            return false;
        }
        let Some(file) = self.sources.file(span.file) else {
            return false;
        };
        let first = file.line_col(span.lo).line;
        let last = file.line_col(span.hi).line;
        // The line above counts only when it holds nothing but a comment. A
        // trailing comment belongs to the statement it follows, and letting it
        // reach the next statement would silence the wrong line.
        let above = first
            .checked_sub(1)
            .filter(|line| *line > 0)
            .filter(|line| {
                file.line_text(*line)
                    .is_some_and(|text| text.trim_start().starts_with("//"))
            });
        above
            .into_iter()
            .chain(first..=last)
            .filter_map(|line| file.line_text(line))
            .filter_map(silence_request)
            .any(|request| request.covers(diagnostic))
    }

    /// Returns `true` if a warning about `span` is about code the user wrote.
    ///
    /// A span without a file -- a synthetic one, which the compiler builds for
    /// code it generates -- belongs to no file and is kept.
    fn is_user_code(&self, span: Option<Span>) -> bool {
        let Some(span) = span else {
            return true;
        };
        if span.is_synthetic() {
            return true;
        }
        let Some(file) = self.sources.file(span.file) else {
            return true;
        };
        let path = file.path().to_path_buf();
        let path = std::fs::canonicalize(&path).unwrap_or(path);
        self.input_files.iter().any(|input| input == &path)
    }

    /// Replaces the warning policy, which the driver builds from its flags.
    pub fn set_warnings(&mut self, warnings: WarningPolicy) {
        self.warnings = warnings;
    }

    /// Returns whether diagnostics are printed in colour.
    #[must_use]
    pub fn color(&self) -> bool {
        self.color
    }

    /// Chooses whether diagnostics are printed in colour.
    pub fn set_color(&mut self, color: bool) {
        self.color = color;
    }

    /// Returns the warning policy in force.
    #[must_use]
    pub fn warnings(&self) -> &WarningPolicy {
        &self.warnings
    }

    /// Returns the source map.
    #[must_use]
    pub fn sources(&self) -> &SourceMap {
        &self.sources
    }

    /// Registers a source file and returns its id.
    pub fn add_file(
        &mut self,
        path: impl Into<std::path::PathBuf>,
        text: impl Into<String>,
    ) -> FileId {
        self.sources.add_file(path, text)
    }

    /// Records a diagnostic.
    ///
    /// The warning policy is applied here, at the one door every diagnostic
    /// comes through: a warning whose group is turned off is dropped, and a
    /// warning when warnings are fatal becomes an error, so that every later
    /// question -- does this run have errors? -- has one answer.
    pub fn report(&mut self, mut diagnostic: Diagnostic) {
        if diagnostic.severity == Severity::Warning {
            if !self.warnings.admits(&diagnostic) {
                return;
            }
            if !self.is_user_code(diagnostic.primary) {
                return;
            }
            // A comment asking for silence is honoured before `-Werror` gets a
            // chance to make it loud: the reader said they know, and refusing
            // the request would defeat the point of the request.
            if self.suppressed_by_comment(&diagnostic) {
                return;
            }
            if self.warnings.denies() {
                diagnostic.severity = Severity::Error;
            }
        }
        self.diagnostics.push(diagnostic);
    }

    /// Records several diagnostics.
    pub fn report_all(&mut self, diagnostics: impl IntoIterator<Item = Diagnostic>) {
        for diagnostic in diagnostics {
            self.report(diagnostic);
        }
    }

    /// Marks the diagnostics recorded so far, so a speculative pass can be
    /// undone.
    ///
    /// Loading a program's imports twice -- once declaration-only to decide
    /// whether a cached module object exists, then fully if it does not -- must
    /// not report every diagnostic twice. The first pass is rolled back when it
    /// turns out not to be the one whose result is used.
    #[must_use]
    pub fn diagnostics_checkpoint(&self) -> (usize, usize) {
        (self.diagnostics.len(), self.hit_unspecified.len())
    }

    /// Drops every diagnostic recorded after `checkpoint`.
    pub fn rollback_diagnostics(&mut self, checkpoint: (usize, usize)) {
        self.diagnostics.truncate(checkpoint.0);
        self.hit_unspecified.truncate(checkpoint.1);
    }

    /// Returns `true` if any fatal diagnostic has been recorded.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.diagnostics.has_errors()
    }

    /// Returns the number of recorded diagnostics.
    #[must_use]
    pub fn diagnostic_count(&self) -> usize {
        self.diagnostics.len()
    }

    /// Returns the recorded diagnostics.
    #[must_use]
    pub fn diagnostics(&self) -> &DiagnosticBag {
        &self.diagnostics
    }

    /// Records that a stage reached a construct the spec does not define.
    ///
    /// The driver surfaces these as a distinct "the compiler is not finished"
    /// message rather than as a program error, because the user's program is
    /// not at fault.
    pub fn note_unspecified(&mut self, span: Span, what: impl Into<String>) {
        self.hit_unspecified.push((span, what.into()));
    }

    /// Returns the constructs this run could not process for lack of a spec.
    #[must_use]
    pub fn unspecified(&self) -> &[(Span, String)] {
        &self.hit_unspecified
    }

    /// Returns `true` if the run reached at least one unspecified construct.
    #[must_use]
    pub fn has_unspecified(&self) -> bool {
        !self.hit_unspecified.is_empty()
    }

    /// Renders every diagnostic against the source map.
    ///
    /// The explanations are included only when `-Wverbose` asked for them.
    #[must_use]
    pub fn render_diagnostics(&self) -> String {
        let style = Style {
            verbose: self.warnings.is_verbose(),
            color: self.color,
        };
        self.diagnostics
            .iter()
            .map(|diagnostic| diagnostic.render_styled(&self.sources, style))
            .collect()
    }

    /// Counts diagnostics of a given severity.
    #[must_use]
    pub fn count(&self, severity: Severity) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == severity)
            .count()
    }
}

/// What a `// no warning` comment asks to silence.
#[derive(Clone, PartialEq, Eq, Debug)]
enum SilenceRequest {
    /// `no warning` on its own: every warning on the line.
    Everything,
    /// The codes or group names after `no warning:`.
    Only(Vec<String>),
}

impl SilenceRequest {
    /// Returns `true` if this request covers `diagnostic`.
    fn covers(&self, diagnostic: &Diagnostic) -> bool {
        match self {
            SilenceRequest::Everything => true,
            SilenceRequest::Only(names) => {
                let code = diagnostic.code.map(|code| code.to_string().to_lowercase());
                let group = diagnostic.warning.map(str::to_lowercase);
                names.iter().any(|name| {
                    let name = name.to_lowercase();
                    code.as_deref() == Some(name.as_str())
                        || group.as_deref() == Some(name.as_str())
                })
            }
        }
    }
}

/// Reads the line's comment as a request for silence, if it is one.
///
/// Two shapes are accepted, and only two. A comment that *is* the request --
/// `// no warning`, `// no warning: S0140` -- and a longer remark whose last
/// sentence ends with `--` before the request: `// why this is fine -- no
/// warning`. A comment that merely mentions warnings is prose, and prose does
/// not silence anything.
fn silence_request(line: &str) -> Option<SilenceRequest> {
    let comment = line.rsplit_once("//")?.1.trim();
    if let Some(request) = directive(comment) {
        return Some(request);
    }
    let (_, tail) = comment.rsplit_once("--")?;
    directive(tail.trim())
}

/// Reads `no warning` or `no warning: <names>` from the start of `text`.
fn directive(text: &str) -> Option<SilenceRequest> {
    let rest = text.strip_prefix("no warning")?;
    let rest = rest.trim();
    if rest.is_empty() {
        return Some(SilenceRequest::Everything);
    }
    let names = rest.strip_prefix(':')?;
    let names: Vec<String> = names
        .split([',', ' '])
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    if names.is_empty() {
        return None;
    }
    Some(SilenceRequest::Only(names))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_starts_empty() {
        let session = Session::new();
        assert!(!session.has_errors());
        assert_eq!(session.diagnostic_count(), 0);
        assert!(!session.has_unspecified());
    }

    #[test]
    fn reported_errors_are_visible() {
        let mut session = Session::new();
        session.report(Diagnostic::error("boom"));
        assert!(session.has_errors());
        assert_eq!(session.count(Severity::Error), 1);
    }

    #[test]
    fn unspecified_notes_are_tracked_separately() {
        let mut session = Session::new();
        session.note_unspecified(Span::SYNTHETIC, "the grammar");
        assert!(session.has_unspecified());
        // Not an error: the user's program is not at fault.
        assert!(!session.has_errors());
        assert_eq!(session.unspecified().len(), 1);
    }
}
