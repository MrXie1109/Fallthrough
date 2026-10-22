//! Source positions, spans, and the source map.
//!
//! This crate is the foundation of the compiler's dependency graph: nothing in
//! the toolchain may be built on top of it in reverse. It deliberately knows
//! nothing about the Fallthrough language — only about *where* things are.
//!
//! # Design notes
//!
//! * Offsets are **byte** offsets into a single [`SourceFile`], and every span
//!   points into exactly one file. Cross-file locations are expressed by
//!   [`Span::file`] rather than by concatenating buffers.
//! * [`Span`] is `Copy` and 12 bytes wide, so it is cheap to attach to every
//!   token, AST node, and IR instruction.
//! * A span may be *empty* (`lo == hi`) to represent a point, and *synthetic*
//!   (see [`Span::SYNTHETIC`]) when a construct has no textual origin — for
//!   example a desugared node or an implicitly inserted instruction.
//! * Source text is UTF-8: a file that is not valid UTF-8 is rejected when it
//!   is read. A line ends at a `\n`, and the `\r` of a `\r\n` pair is trimmed
//!   when the line is rendered.

#![forbid(unsafe_code)]

use std::fmt;
use std::path::{Path, PathBuf};

/// Identifies a [`SourceFile`] registered in a [`SourceMap`].
///
/// IDs are dense and stable for the lifetime of the owning source map.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct FileId(u32);

impl FileId {
    /// The identifier reserved for the synthetic file that owns all
    /// compiler-generated, source-less nodes.
    pub const SYNTHETIC: FileId = FileId(u32::MAX);

    /// Creates a file id from its raw representation.
    ///
    /// Prefer [`SourceMap::add_file`]; this exists for tests and for
    /// serialisation boundaries.
    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        FileId(raw)
    }

    /// Returns the raw representation of this id.
    #[must_use]
    pub const fn as_raw(self) -> u32 {
        self.0
    }

    /// Returns `true` if this id refers to the synthetic file.
    #[must_use]
    pub const fn is_synthetic(self) -> bool {
        self.0 == u32::MAX
    }
}

impl fmt::Display for FileId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_synthetic() {
            f.write_str("<synthetic>")
        } else {
            write!(f, "file#{}", self.0)
        }
    }
}

/// A half-open byte range `[lo, hi)` within one source file.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Span {
    /// The file this span points into.
    pub file: FileId,
    /// Inclusive start byte offset.
    pub lo: u32,
    /// Exclusive end byte offset.
    pub hi: u32,
}

impl Span {
    /// A span that carries no source location.
    ///
    /// Used for compiler-generated constructs. Diagnostics that point at a
    /// synthetic span should render as "in compiler-generated code" rather
    /// than as a bogus file position.
    pub const SYNTHETIC: Span = Span {
        file: FileId::SYNTHETIC,
        lo: 0,
        hi: 0,
    };

    /// Creates a span covering `[lo, hi)` in `file`.
    ///
    /// # Panics
    ///
    /// Panics if `lo > hi`. A malformed span is a compiler bug, never user
    /// error, so this is deliberately a hard failure.
    #[must_use]
    pub const fn new(file: FileId, lo: u32, hi: u32) -> Self {
        assert!(lo <= hi, "span start must not exceed span end");
        Span { file, lo, hi }
    }

    /// Creates an empty span (a point) at `offset` in `file`.
    #[must_use]
    pub const fn point(file: FileId, offset: u32) -> Self {
        Span {
            file,
            lo: offset,
            hi: offset,
        }
    }

    /// Returns `true` if this span has no source location.
    #[must_use]
    pub const fn is_synthetic(self) -> bool {
        self.file.is_synthetic()
    }

    /// Returns `true` if this span covers no bytes.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.lo == self.hi
    }

    /// Returns the number of bytes covered by this span.
    #[must_use]
    pub const fn len(self) -> u32 {
        self.hi - self.lo
    }

    /// Returns the smallest span covering both `self` and `other`.
    ///
    /// If the two spans live in different files the result is
    /// [`Span::SYNTHETIC`]: there is no meaningful way to point at both.
    #[must_use]
    pub const fn to(self, other: Span) -> Span {
        if self.file.is_synthetic() || other.file.is_synthetic() {
            return Span::SYNTHETIC;
        }
        if self.file.as_raw() != other.file.as_raw() {
            return Span::SYNTHETIC;
        }
        Span {
            file: self.file,
            lo: if self.lo < other.lo {
                self.lo
            } else {
                other.lo
            },
            hi: if self.hi > other.hi {
                self.hi
            } else {
                other.hi
            },
        }
    }

    /// Returns a copy of this span shifted forward by `delta` bytes.
    #[must_use]
    pub const fn shifted(self, delta: u32) -> Span {
        Span {
            file: self.file,
            lo: self.lo + delta,
            hi: self.hi + delta,
        }
    }
}

impl fmt::Debug for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_synthetic() {
            f.write_str("Span::SYNTHETIC")
        } else {
            write!(f, "{}:{}..{}", self.file, self.lo, self.hi)
        }
    }
}

/// Convenience: build a span from an inclusive start and exclusive end.
impl From<(FileId, u32, u32)> for Span {
    fn from((file, lo, hi): (FileId, u32, u32)) -> Self {
        Span::new(file, lo, hi)
    }
}

/// A resolved, human-readable source position (1-based line and column).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LineCol {
    /// 1-based line number.
    pub line: u32,
    /// 1-based column number, counted in characters rather than bytes.
    pub column: u32,
}

impl fmt::Display for LineCol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

/// A single source file registered in a [`SourceMap`].
#[derive(Debug)]
pub struct SourceFile {
    id: FileId,
    path: PathBuf,
    text: String,
    /// Byte offset of the first character of each line.
    line_starts: Vec<u32>,
}

impl SourceFile {
    fn new(id: FileId, path: PathBuf, text: String) -> Self {
        let line_starts = compute_line_starts(&text);
        SourceFile {
            id,
            path,
            text,
            line_starts,
        }
    }

    /// Returns this file's id.
    #[must_use]
    pub fn id(&self) -> FileId {
        self.id
    }

    /// Returns this file's path as it was registered.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns this file's path rendered for diagnostics.
    #[must_use]
    pub fn display_path(&self) -> String {
        self.path.display().to_string()
    }

    /// Returns the full text of this file.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns the text covered by `span`, or `None` if it is synthetic or
    /// does not lie on UTF-8 character boundaries.
    #[must_use]
    pub fn slice(&self, span: Span) -> Option<&str> {
        if span.file != self.id {
            return None;
        }
        self.text.get(span.lo as usize..span.hi as usize)
    }

    /// Resolves a byte offset to a 1-based line/column pair.
    ///
    /// Offsets past the end of the file resolve to the end position. Columns
    /// are counted in characters so that diagnostics align with what an editor
    /// shows.
    #[must_use]
    pub fn line_col(&self, offset: u32) -> LineCol {
        let offset = offset.min(self.text.len() as u32);
        // `line_starts` is sorted; the line we want is the last entry <= offset.
        let line_idx = match self.line_starts.binary_search(&offset) {
            Ok(exact) => exact,
            Err(next) => next.saturating_sub(1),
        };
        let line_start = self.line_starts[line_idx];
        let prefix = &self.text[line_start as usize..offset as usize];
        LineCol {
            line: line_idx as u32 + 1,
            column: prefix.chars().count() as u32 + 1,
        }
    }

    /// Returns the text of a 1-based line, without its terminator.
    #[must_use]
    pub fn line_text(&self, line: u32) -> Option<&str> {
        let idx = (line.checked_sub(1)?) as usize;
        let start = *self.line_starts.get(idx)? as usize;
        let end = self
            .line_starts
            .get(idx + 1)
            .map_or(self.text.len(), |&e| e as usize);
        Some(self.text[start..end].trim_end_matches(['\n', '\r']))
    }

    /// Returns the number of lines in this file.
    #[must_use]
    pub fn line_count(&self) -> u32 {
        self.line_starts.len() as u32
    }
}

/// Owns every source file the compiler has been asked to process.
///
/// The source map is created once by the driver and shared immutably with
/// every later stage, which is why files can only be added before compilation
/// begins.
#[derive(Debug, Default)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    /// Creates an empty source map.
    #[must_use]
    pub fn new() -> Self {
        SourceMap { files: Vec::new() }
    }

    /// Registers `text` under `path` and returns its fresh id.
    pub fn add_file(&mut self, path: impl Into<PathBuf>, text: impl Into<String>) -> FileId {
        let id = FileId(
            u32::try_from(self.files.len()).expect("source map file count exceeds u32::MAX"),
        );
        self.files
            .push(SourceFile::new(id, path.into(), text.into()));
        id
    }

    /// Returns the file registered under `id`, if any.
    #[must_use]
    pub fn file(&self, id: FileId) -> Option<&SourceFile> {
        self.files.get(id.0 as usize)
    }

    /// Returns the file registered under `id`.
    ///
    /// # Panics
    ///
    /// Panics if `id` was not produced by this map. Diagnostics routinely
    /// resolve ids they were handed, and a dangling id means a compiler bug.
    #[must_use]
    pub fn file_or_panic(&self, id: FileId) -> &SourceFile {
        self.file(id)
            .unwrap_or_else(|| panic!("source map has no file with id {id}"))
    }

    /// Returns every registered file, in registration order.
    #[must_use]
    pub fn files(&self) -> &[SourceFile] {
        &self.files
    }

    /// Renders `span` as `path:line:column`.
    #[must_use]
    pub fn describe(&self, span: Span) -> String {
        if span.is_synthetic() {
            return "<synthetic>".to_owned();
        }
        match self.file(span.file) {
            Some(file) => {
                let lc = file.line_col(span.lo);
                format!("{}:{}:{}", file.display_path(), lc.line, lc.column)
            }
            None => format!("{}:{}..{}", span.file, span.lo, span.hi),
        }
    }
}

fn compute_line_starts(text: &str) -> Vec<u32> {
    let mut starts = vec![0u32];
    for (idx, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            starts.push(u32::try_from(idx + 1).expect("source file exceeds 4 GiB"));
        }
    }
    starts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_union_of_disjoint_ranges() {
        let a = Span::new(FileId::from_raw(0), 4, 8);
        let b = Span::new(FileId::from_raw(0), 1, 2);
        assert_eq!(a.to(b), Span::new(FileId::from_raw(0), 1, 8));
    }

    #[test]
    fn span_union_across_files_is_synthetic() {
        let a = Span::new(FileId::from_raw(0), 4, 8);
        let b = Span::new(FileId::from_raw(1), 4, 8);
        assert!(a.to(b).is_synthetic());
    }

    #[test]
    fn line_col_counts_characters_not_bytes() {
        let mut map = SourceMap::new();
        // "é" is two bytes but one column.
        let id = map.add_file("t.ft", "let é = 1;\nlet b = 2;\n");
        let file = map.file(id).expect("file was just added");
        assert_eq!(file.line_col(0), LineCol { line: 1, column: 1 });
        assert_eq!(file.line_col(12), LineCol { line: 2, column: 1 });
        assert_eq!(file.line_count(), 3);
    }

    #[test]
    fn slice_rejects_foreign_file() {
        let mut map = SourceMap::new();
        let a = map.add_file("a.ft", "hello");
        let b = map.add_file("b.ft", "world");
        let span = Span::new(b, 0, 5);
        assert_eq!(map.file(a).and_then(|f| f.slice(span)), None);
    }
}
