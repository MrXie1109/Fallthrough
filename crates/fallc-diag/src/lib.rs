//! Diagnostics: severities, messages, labelled spans, and rendering.
//!
//! Every stage of the compiler reports problems by pushing [`Diagnostic`]s
//! into a [`DiagnosticBag`] rather than by printing or by returning `Err`.
//! This keeps stages independent of the output format and lets a single pass
//! report many errors instead of stopping at the first.
//!
//! Warnings are not errors by default: each belongs to a named group that the
//! driver can turn off with `-Wno-<group>`, and `-Werror` makes what is left
//! fatal. See [`WarningPolicy`].

#![forbid(unsafe_code)]

use std::fmt::Write as _;

use fallc_span::{SourceMap, Span};

/// How serious a [`Diagnostic`] is.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Severity {
    /// Something the user probably did not intend, but which is accepted.
    Warning,
    /// The program cannot continue to be compiled.
    Error,
    /// A compiler bug or an unsatisfiable internal invariant.
    Bug,
}

impl Severity {
    /// Returns the short lowercase name used when rendering.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Severity::Warning => "warning",
            Severity::Error => "error",
            Severity::Bug => "internal compiler error",
        }
    }

    /// Returns `true` if a diagnostic of this severity stops compilation.
    #[must_use]
    pub const fn is_fatal(self) -> bool {
        matches!(self, Severity::Error | Severity::Bug)
    }
}

/// How a rendered diagnostic is dressed.
///
/// Separate from the diagnostic itself, because the same diagnostic is printed
/// to a terminal and compared in a test: the text is the contract, the colour is
/// a courtesy.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Style {
    /// Whether the explanations are printed.
    pub verbose: bool,
    /// Whether the output is coloured.
    pub color: bool,
}

impl Style {
    /// The plain style: no explanations, no colour.
    #[must_use]
    pub fn plain() -> Style {
        Style::default()
    }

    /// Adds the explanations.
    #[must_use]
    pub fn verbose(mut self) -> Style {
        self.verbose = true;
        self
    }

    /// Adds colour.
    #[must_use]
    pub fn color(mut self) -> Style {
        self.color = true;
        self
    }
}

/// The warning groups the driver can turn on and off by name.
///
/// A warning belongs to at most one group, and the group is what `-W<name>` and
/// `-Wno-<name>` name. The list is here rather than in the driver so that the
/// driver can report an unknown name against it, and so that adding a warning
/// adds its group in one place.
pub const WARNING_GROUPS: &[(&str, &str)] = &[
    (
        "useless-raise-mark",
        "`??` on a call that cannot raise anything",
    ),
    ("unreachable", "a statement the program can never reach"),
    ("unused", "a binding that is never read"),
    ("unused-parameter", "a parameter that is never read"),
    ("shadow", "a local that hides a name of the enclosing function"),
    (
        "constant-condition",
        "an `if`/`while` condition written as a literal",
    ),
    (
        "assignment-in-condition",
        "an assignment where a condition is wanted",
    ),
    ("self-comparison", "a value compared with itself"),
    ("division-by-zero", "a divisor that is a constant zero"),
    ("use-after-move", "a value read after `std.move` emptied it"),
    (
        "deprecated",
        "a declaration marked `@deprecated` is used",
    ),
];

/// Which warnings are reported, and whether they are errors.
///
/// The default is every group on and no warning fatal: a warning is something
/// the program probably did not intend, and printing it is the whole point.
/// `-Wno-<name>` turns one group off, `-W<name>` back on, and `-Werror` makes
/// what is left fatal.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct WarningPolicy {
    /// The groups that are turned off, by name.
    disabled: Vec<String>,
    /// Whether a warning is reported as an error.
    deny: bool,
    /// Whether diagnostics carry their explanations and suggested fixes.
    ///
    /// Off by default: the line a diagnostic is about, and what is wrong with
    /// it, is what is read a hundred times a day. The reasoning behind the rule
    /// -- and the shape of the fix -- is what `-Wverbose` adds.
    verbose: bool,
}

impl WarningPolicy {
    /// Creates the default policy: every group on, no warning fatal.
    #[must_use]
    pub fn new() -> Self {
        WarningPolicy::default()
    }

    /// Turns a group off. The name is not checked here; `-Wno-` does that.
    pub fn disable(&mut self, group: impl Into<String>) {
        let group = group.into();
        if !self.disabled.contains(&group) {
            self.disabled.push(group);
        }
    }

    /// Turns a group back on.
    pub fn enable(&mut self, group: &str) {
        self.disabled.retain(|disabled| disabled != group);
    }

    /// Turns every group on.
    pub fn enable_all(&mut self) {
        self.disabled.clear();
    }

    /// Makes every warning that is still reported fatal.
    pub fn deny(&mut self) {
        self.deny = true;
    }

    /// Asks for diagnostics to carry their explanations.
    pub fn set_verbose(&mut self, verbose: bool) {
        self.verbose = verbose;
    }

    /// Returns `true` if diagnostics are to carry their explanations.
    #[must_use]
    pub fn is_verbose(&self) -> bool {
        self.verbose
    }

    /// Returns `true` if a diagnostic survives the policy.
    ///
    /// Only warnings can be turned off: an error is not a request, so no flag
    /// makes one go away.
    #[must_use]
    pub fn admits(&self, diagnostic: &Diagnostic) -> bool {
        if diagnostic.severity != Severity::Warning {
            return true;
        }
        match diagnostic.warning {
            Some(group) => !self.disabled.iter().any(|disabled| disabled == group),
            // A warning that named no group cannot be turned off, because there
            // is no name to write.
            None => true,
        }
    }

    /// Returns `true` if warnings are to be reported as errors.
    #[must_use]
    pub fn denies(&self) -> bool {
        self.deny
    }
}

/// Returns `true` if `name` is a warning group.
#[must_use]
pub fn is_warning_group(name: &str) -> bool {
    WARNING_GROUPS.iter().any(|(group, _)| *group == name)
}

/// A stable identifier for a diagnostic, e.g. `L0001`.
///
/// Codes are allocated per stage using the [`DiagnosticCode::lexer`],
/// [`DiagnosticCode::parser`], and friends constructors so that stages never
/// collide.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct DiagnosticCode {
    prefix: char,
    number: u16,
}

impl DiagnosticCode {
    /// Creates a code from a stage prefix and a number.
    #[must_use]
    pub const fn new(prefix: char, number: u16) -> Self {
        DiagnosticCode { prefix, number }
    }

    /// A lexer diagnostic code.
    #[must_use]
    pub const fn lexer(number: u16) -> Self {
        DiagnosticCode::new('L', number)
    }

    /// A parser diagnostic code.
    #[must_use]
    pub const fn parser(number: u16) -> Self {
        DiagnosticCode::new('P', number)
    }

    /// A name-resolution or type-checking diagnostic code.
    #[must_use]
    pub const fn sema(number: u16) -> Self {
        DiagnosticCode::new('S', number)
    }

    /// An IR-building or IR-validation diagnostic code.
    #[must_use]
    pub const fn ir(number: u16) -> Self {
        DiagnosticCode::new('I', number)
    }

    /// A driver-level diagnostic code (bad flags, missing files, ...).
    #[must_use]
    pub const fn driver(number: u16) -> Self {
        DiagnosticCode::new('D', number)
    }
}

impl std::fmt::Display for DiagnosticCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{:04}", self.prefix, self.number)
    }
}

/// An extra span attached to a diagnostic, with an explanation.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Label {
    /// Where the label points.
    pub span: Span,
    /// The text shown alongside the highlighted range.
    pub message: String,
}

impl Label {
    /// Creates a label.
    #[must_use]
    pub fn new(span: Span, message: impl Into<String>) -> Self {
        Label {
            span,
            message: message.into(),
        }
    }

    /// Creates a label with no message, for pure highlighting.
    #[must_use]
    pub fn bare(span: Span) -> Self {
        Label {
            span,
            message: String::new(),
        }
    }
}

/// A single reported problem.
///
/// The wording and the code belong to the stage that reports the problem; this
/// type carries them, so a diagnostic renders identically wherever it
/// originates.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Diagnostic {
    /// How serious the problem is.
    pub severity: Severity,
    /// Optional stable code.
    pub code: Option<DiagnosticCode>,
    /// The primary message.
    pub message: String,
    /// The location the diagnostic is primarily about.
    pub primary: Option<Span>,
    /// The warning group this diagnostic belongs to, if it is a warning.
    ///
    /// The name is what `-W<name>` and `-Wno-<name>` select. A warning without
    /// a group is always reported.
    pub warning: Option<&'static str>,
    /// Additional locations that help explain the problem.
    pub labels: Vec<Label>,
    /// Suggested fixes, rendered after the message.
    pub notes: Vec<String>,
}

impl Diagnostic {
    /// Starts building a diagnostic with the given severity and message.
    #[must_use]
    pub fn new(severity: Severity, message: impl Into<String>) -> Self {
        Diagnostic {
            severity,
            code: None,
            message: message.into(),
            primary: None,
            warning: None,
            labels: Vec::new(),
            notes: Vec::new(),
        }
    }

    /// Creates an [`Severity::Error`] diagnostic.
    #[must_use]
    pub fn error(message: impl Into<String>) -> Self {
        Diagnostic::new(Severity::Error, message)
    }

    /// Creates a [`Severity::Warning`] diagnostic.
    #[must_use]
    pub fn warning(message: impl Into<String>) -> Self {
        Diagnostic::new(Severity::Warning, message)
    }

    /// Creates a [`Severity::Bug`] diagnostic.
    ///
    /// Use this for broken invariants. It means "the compiler is wrong", not
    /// "the program is wrong".
    #[must_use]
    pub fn bug(message: impl Into<String>) -> Self {
        Diagnostic::new(Severity::Bug, message)
    }

    /// Attaches a stable code.
    #[must_use]
    pub fn with_code(mut self, code: DiagnosticCode) -> Self {
        self.code = Some(code);
        self
    }

    /// Names the warning group this diagnostic belongs to.
    ///
    /// A no-op on anything that is not a warning, so a constructor cannot end
    /// up with a severity that disagrees with its group.
    #[must_use]
    pub fn with_warning(mut self, group: &'static str) -> Self {
        if self.severity == Severity::Warning {
            self.warning = Some(group);
        }
        self
    }

    /// Sets the primary span.
    #[must_use]
    pub fn with_span(mut self, span: Span) -> Self {
        self.primary = Some(span);
        self
    }

    /// Appends a secondary label.
    #[must_use]
    pub fn with_label(mut self, span: Span, message: impl Into<String>) -> Self {
        self.labels.push(Label::new(span, message));
        self
    }

    /// Appends an explanatory note.
    #[must_use]
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    /// Returns `true` if this diagnostic stops compilation.
    #[must_use]
    pub fn is_fatal(&self) -> bool {
        self.severity.is_fatal()
    }

    /// Renders this diagnostic against `sources` in a human-readable form.
    ///
    /// The notes are included: this is the detailed form, and the driver picks
    /// between it and `Diagnostic::render_terse` with `-Wverbose`.
    #[must_use]
    pub fn render(&self, sources: &SourceMap) -> String {
        self.render_with(sources, true)
    }

    /// Renders this diagnostic, with or without its explanations.
    ///
    /// What is always shown: the severity, the code, the message, the location
    /// with the caret, and every label -- a label is *where else* to look, which
    /// is part of what is wrong. What `verbose` decides is the `= note:` lines,
    /// which say why the rule is the rule and what to write instead.
    #[must_use]
    pub fn render_with(&self, sources: &SourceMap, verbose: bool) -> String {
        self.render_body(sources, Style { verbose, color: false })
    }

    /// Renders this diagnostic with `style` applied.
    #[must_use]
    pub fn render_styled(&self, sources: &SourceMap, style: Style) -> String {
        self.render_body(sources, style)
    }

    fn render_body(&self, sources: &SourceMap, style: Style) -> String {
        let color = style.color;
        let severity_color = match self.severity {
            Severity::Warning => "1;33",
            Severity::Error => "1;31",
            Severity::Bug => "1;35",
        };
        let mut out = String::new();
        let severity = paint(self.severity.as_str(), severity_color, color);
        // The group is named in the detailed form, because the name is what
        // `-Wno-<group>` and `// no warning: <group>` take.
        let group = match (style.verbose, self.warning) {
            (true, Some(group)) => format!(" ({group})"),
            _ => String::new(),
        };
        match self.code {
            Some(code) => {
                let _ = writeln!(
                    out,
                    "{severity}{}{group}: {}",
                    paint(&format!("[{code}]"), "1", color),
                    self.message
                );
            }
            None => {
                let _ = writeln!(out, "{severity}: {}", self.message);
            }
        }
        if let Some(span) = self.primary {
            let _ = writeln!(
                out,
                "  {} {}",
                paint("-->", "1;34", color),
                sources.describe(span)
            );
            if !span.is_synthetic() {
                if let Some(file) = sources.file(span.file) {
                    let lc = file.line_col(span.lo);
                    if let Some(line) = file.line_text(lc.line) {
                        // The gutter is as wide as the number needs it to be,
                        // and the same width on every line: a four-digit line
                        // number must not push the source one column right of
                        // the carets under it.
                        let gutter = lc.line.to_string().len().max(3);
                        let bar = paint("|", "1;34", color);
                        let _ = writeln!(out, "{:>gutter$} {bar}", "");
                        // The padding is applied to the number, and the
                        // colour to the padded text: painting first would
                        // count the escapes as columns and pull the number
                        // left.
                        let number = paint(&format!("{:>gutter$}", lc.line), "1;34", color);
                        let _ = writeln!(out, "{number} {bar} {line}");
                        let underline = caret_line(span, line, lc.column);
                        let (padding, carets) = split_underline(&underline);
                        let _ = writeln!(
                            out,
                            "{:>gutter$} {bar} {padding}{}",
                            "",
                            paint(carets, severity_color, color)
                        );
                    }
                }
            }
        }
        for label in &self.labels {
            let _ = writeln!(
                out,
                "  {} {}: {}",
                paint("=", "1;34", color),
                sources.describe(label.span),
                label.message
            );
        }
        if style.verbose {
            for note in &self.notes {
                let _ = writeln!(out, "  {} {note}", paint("= note:", "1;36", color));
            }
        }
        out
    }
}

pub fn paint(text: &str, sgr: &str, color: bool) -> String {
    if color {
        format!("\u{1b}[{sgr}m{text}\u{1b}[0m")
    } else {
        text.to_owned()
    }
}

/// Wraps `text` in the ANSI colour `sgr`, when colour is wanted.
///
/// One attribute per call rather than a renderer: the output is read as text by
/// tools far more often than it is read as colour by a person, and a tool that
/// strips escapes sees exactly the text it saw before colour existed.
/// Splits an underline into its leading spaces and its carets.
///
/// The two are drawn in different colours: the padding is nothing at all, and
/// colouring it would paint a band across the line.
fn split_underline(underline: &str) -> (&str, &str) {
    let carets = underline.find('^').unwrap_or(0);
    underline.split_at(carets)
}

/// Builds the line that underlines `span`: padding, then carets.
///
/// The padding is measured in *terminal columns*, not in characters. A tab
/// takes as many columns as the next tab stop, and a character that a terminal
/// draws double-width -- the CJK ideographs this documentation is written in,
/// for one -- takes two. Counting characters instead put the carets under the
/// wrong part of the line for any source that contains either, which is the one
/// thing a caret must never do.
fn caret_line(span: Span, line: &str, column: u32) -> String {
    let start = (column as usize).saturating_sub(1);
    let mut characters = line.chars();
    let prefix: String = characters.by_ref().take(start).collect();
    let pad = display_width(&prefix);
    // The span's length is in bytes; the characters it covers are what the
    // underline covers. A span that ends on another line stops at this one.
    let mut covered = String::new();
    for character in characters {
        if covered.len() >= span.len() as usize {
            break;
        }
        covered.push(character);
    }
    let width = display_width(&covered).max(1);
    let mut out = String::with_capacity(pad + width);
    out.push_str(&" ".repeat(pad));
    out.push('^');
    out.extend(std::iter::repeat_n('~', width.saturating_sub(1)));
    out
}

/// Returns how many terminal columns `text` occupies, tabs included.
fn display_width(text: &str) -> usize {
    let mut column = 0;
    for character in text.chars() {
        column += match character {
            '\t' => 8 - (column % 8),
            _ => char_width(character),
        };
    }
    column
}

/// Returns how many columns a character is drawn in.
///
/// The wide ranges are the ones a terminal draws two cells wide: CJK
/// ideographs and their compatibility forms, Hangul, fullwidth forms, and the
/// emoji blocks. Combining marks draw in no cell of their own. Everything else
/// is one column, which is what the rest of a program is made of.
fn char_width(character: char) -> usize {
    let code = character as u32;
    let wide = (0x1100..=0x115F).contains(&code)
        || (0x2E80..=0x303E).contains(&code)
        || (0x3041..=0x33FF).contains(&code)
        || (0x3400..=0x4DBF).contains(&code)
        || (0x4E00..=0x9FFF).contains(&code)
        || (0xA000..=0xA4CF).contains(&code)
        || (0xAC00..=0xD7A3).contains(&code)
        || (0xF900..=0xFAFF).contains(&code)
        || (0xFE30..=0xFE6F).contains(&code)
        || (0xFF00..=0xFF60).contains(&code)
        || (0xFFE0..=0xFFE6).contains(&code)
        || (0x1F300..=0x1F64F).contains(&code)
        || (0x1F900..=0x1F9FF).contains(&code)
        || (0x20000..=0x3FFFD).contains(&code);
    if wide {
        return 2;
    }
    let combining = (0x0300..=0x036F).contains(&code)
        || (0x1AB0..=0x1AFF).contains(&code)
        || (0x20D0..=0x20FF).contains(&code)
        || (0xFE20..=0xFE2F).contains(&code);
    if combining { 0 } else { 1 }
}

/// Collects diagnostics produced by a stage.
///
/// A bag also records whether the compiler has already emitted a fatal
/// diagnostic, so stages can bail out early via [`DiagnosticBag::has_errors`]
/// without threading a separate error flag through every signature.
#[derive(Debug, Default)]
pub struct DiagnosticBag {
    diagnostics: Vec<Diagnostic>,
}

impl DiagnosticBag {
    /// Creates an empty bag.
    #[must_use]
    pub fn new() -> Self {
        DiagnosticBag::default()
    }

    /// Records a diagnostic.
    ///
    /// A diagnostic identical to one already recorded is dropped. Checking is
    /// not one pass over an expression: a call is resolved, probed, and then
    /// resolved again with the chosen overload, and a binding's initialiser is
    /// looked at once while its constructor is chosen and once when it is
    /// checked. Every one of those can reach the same mistake, and the same
    /// sentence twice reads like two separate faults.
    ///
    /// "Identical" means the same severity, code, primary span and message. A
    /// second diagnostic about the same place with *different* words is kept:
    /// it is a different fact, and the reader decides whether it is noise.
    pub fn push(&mut self, diagnostic: Diagnostic) {
        let duplicate = self.diagnostics.iter().any(|existing| {
            existing.severity == diagnostic.severity
                && existing.code == diagnostic.code
                && existing.primary == diagnostic.primary
                && existing.message == diagnostic.message
        });
        if !duplicate {
            self.diagnostics.push(diagnostic);
        }
    }

    /// Records an error and returns `()` for convenient early-return use.
    pub fn error(&mut self, message: impl Into<String>) {
        self.push(Diagnostic::error(message));
    }

    /// Records a warning.
    pub fn warn(&mut self, message: impl Into<String>) {
        self.push(Diagnostic::warning(message));
    }

    /// Returns `true` if any recorded diagnostic is fatal.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(Diagnostic::is_fatal)
    }

    /// Returns the number of recorded diagnostics.
    #[must_use]
    pub fn len(&self) -> usize {
        self.diagnostics.len()
    }

    /// Returns `true` if nothing has been recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }

    /// Iterates over the recorded diagnostics.
    pub fn iter(&self) -> std::slice::Iter<'_, Diagnostic> {
        self.diagnostics.iter()
    }

    /// Consumes the bag, yielding all diagnostics.
    #[must_use]
    pub fn into_vec(self) -> Vec<Diagnostic> {
        self.diagnostics
    }

    /// Discards every diagnostic past the first `len`.
    ///
    /// This is what makes speculative parsing silent: a speculative attempt
    /// records the length before it starts, and truncates back to it if the
    /// attempt fails. Truncating to a length longer than the bag is a no-op.
    pub fn truncate(&mut self, len: usize) {
        self.diagnostics.truncate(len);
    }

    /// Removes and returns every diagnostic, leaving the bag empty.
    pub fn drain(&mut self) -> std::vec::Drain<'_, Diagnostic> {
        self.diagnostics.drain(..)
    }

    /// Appends every diagnostic from `other`, leaving `other` empty.
    pub fn absorb(&mut self, other: &mut DiagnosticBag) {
        for diagnostic in other.diagnostics.drain(..) {
            self.push(diagnostic);
        }
    }

    /// Renders every diagnostic, in the order they were recorded.
    ///
    /// The detailed form; see [`DiagnosticBag::render_with`].
    #[must_use]
    pub fn render(&self, sources: &SourceMap) -> String {
        self.render_with(sources, true)
    }

    /// Renders every diagnostic, with or without the explanations.
    #[must_use]
    pub fn render_with(&self, sources: &SourceMap, verbose: bool) -> String {
        self.diagnostics
            .iter()
            .map(|d| d.render_with(sources, verbose))
            .collect::<Vec<_>>()
            .join("")
    }
}

impl Extend<Diagnostic> for DiagnosticBag {
    fn extend<T: IntoIterator<Item = Diagnostic>>(&mut self, iter: T) {
        for diagnostic in iter {
            self.push(diagnostic);
        }
    }
}

impl IntoIterator for DiagnosticBag {
    type Item = Diagnostic;
    type IntoIter = std::vec::IntoIter<Diagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.diagnostics.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallc_span::SourceMap;

    #[test]
    fn bag_tracks_fatality() {
        let mut bag = DiagnosticBag::new();
        bag.warn("just a warning");
        assert!(!bag.has_errors());
        bag.error("now it is broken");
        assert!(bag.has_errors());
        assert_eq!(bag.len(), 2);
    }

    #[test]
    fn color_is_added_only_when_it_is_asked_for() {
        let mut sources = SourceMap::new();
        let text = "x = 1;\n";
        let id = sources.add_file("t.ft", text);
        let diag = Diagnostic::warning("something to see")
            .with_code(DiagnosticCode::sema(140))
            .with_span(Span::new(id, 4, 5))
            .with_label(Span::new(id, 0, 1), "here it is")
            .with_note("and why");

        let plain = diag.render_styled(&sources, Style::plain());
        assert!(!plain.contains('\u{1b}'), "{plain}");
        // The words are untouched by the colour: a tool that strips the escapes
        // sees the same text it saw before there was colour.
        let colored = diag.render_styled(&sources, Style::plain().color().verbose());
        let stripped = strip_ansi(&colored);
        let expected = diag.render_styled(&sources, Style::plain().verbose());
        assert_eq!(stripped, expected);
        assert!(colored.contains("\u{1b}[1;33mwarning"), "{colored:?}");
        assert!(colored.contains("\u{1b}[1;36m= note:"), "{colored:?}");
    }

    /// Removes the escapes, so that coloured and plain output can be compared.
    fn strip_ansi(text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut characters = text.chars();
        while let Some(character) = characters.next() {
            if character != '\u{1b}' {
                out.push(character);
                continue;
            }
            // `\u{1b}[ ... m`
            let _ = characters.next();
            for character in characters.by_ref() {
                if character == 'm' {
                    break;
                }
            }
        }
        out
    }

    #[test]
    fn a_caret_is_placed_by_columns_not_by_characters() {
        // A wide character is drawn in two columns and a tab runs to the next
        // tab stop. Counting characters put the underline under the wrong part
        // of the line, which is the one thing an underline must never do.
        assert_eq!(display_width("abc"), 3);
        assert_eq!(display_width("一二三"), 6);
        assert_eq!(display_width("\t"), 8);
        assert_eq!(display_width("a\tb"), 9);
        assert_eq!(display_width("a\tb\tc"), 17);

        let mut sources = SourceMap::new();
        let text = "一二三 x;\n";
        let start = text.find('x').expect("x is there") as u32;
        let id = sources.add_file("t.ft", text);
        // `x` is the fourth character, so it is two columns right of where a
        // character count would put it.
        let diag = Diagnostic::error("boom").with_span(Span::new(id, start, start + 1));
        let rendered = diag.render(&sources);
        let underline = rendered
            .lines()
            .find(|line| line.contains('^'))
            .expect("an underline");
        let caret = underline.find('^').expect("a caret");
        let bar = underline.find('|').expect("a gutter bar");
        // Past the bar: the two columns of `| `, then the seven columns the
        // three ideographs and the space before `x` are drawn in.
        assert_eq!(caret - bar, 2 + 7, "{rendered}");
        // And that is where the source line draws `x`: six columns of
        // ideographs, one of the space, two of the `| ` before it.
        assert_eq!(caret - bar, 2 + 6 + 1, "{rendered}");
    }

    #[test]
    fn the_gutter_is_as_wide_as_the_line_number() {
        let mut sources = SourceMap::new();
        let text = "\n".repeat(999) + "boom\n";
        let start = text.find("boom").expect("the word is there") as u32;
        let id = sources.add_file("t.ft", text);
        let diag = Diagnostic::error("boom!!").with_span(Span::new(id, start, start + 4));
        let rendered = diag.render(&sources);
        // Four digits need four columns, and every line of the gutter uses the
        // same width as the number does.
        let bars: Vec<usize> = rendered
            .lines()
            .filter_map(|line| line.find('|'))
            .collect();
        assert_eq!(bars.len(), 3, "{rendered}");
        assert!(bars.windows(2).all(|pair| pair[0] == pair[1]), "{rendered}");
    }

    #[test]
    fn render_includes_location_and_label() {
        let mut sources = SourceMap::new();
        let id = sources.add_file("t.ft", "abc def\n");
        let bad = Span::new(id, 4, 7);
        let diag = Diagnostic::error("unknown thing")
            .with_code(DiagnosticCode::parser(1))
            .with_span(bad)
            .with_note("did you mean something else?");

        let text = diag.render(&sources);
        assert!(text.contains("t.ft:1:5"), "{text}");
        assert!(text.contains("P0001"), "{text}");
        assert!(text.contains("unknown thing"), "{text}");
    }
}
