//! A language server for the Fallthrough language.
//!
//! `fallc --lsp` speaks the Language Server Protocol on stdin and stdout. It
//! is what an editor extension attaches to: the compiler already knows how to
//! lex, parse and check a file, and a language server is that knowledge wearing
//! the protocol an editor understands.
//!
//! # What it does
//!
//! * `initialize` / `initialized` / `shutdown` / `exit` -- the lifecycle, as the
//!   protocol specifies it.
//! * `textDocument/didOpen`, `didChange` (full sync), `didSave`, `didClose` --
//!   documents are kept in memory, so an unsaved buffer is checked as it is.
//! * `textDocument/publishDiagnostics` -- after every change, the file is
//!   type-checked and its diagnostics are sent with the editor's own positions.
//!
//! # What it does not do yet
//!
//! Completion, hover, go-to-definition and formatting are not implemented: they
//! need the resolution and type tables to survive past the check, which is a
//! separate piece of work. The protocol requires the server to declare what it
//! supports, so it declares only diagnostics and accepts the requests it does
//! not answer with the protocol's "method not found", which editors ignore.
//!
//! # Why the messages are parsed by hand
//!
//! The driver has no dependencies, and the registry is not reachable from the
//! machine this is built on. `json.rs` is the part of JSON a language server
//! needs; it is small, and it keeps `fallc` a single static binary.

pub mod json;

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use crate::session::Session;
use json::Value;

/// Runs the server until the client closes the connection.
///
/// Returns the process exit code: 0 for an ordinary shutdown, 1 when the
/// protocol was violated badly enough to stop.
pub fn run() -> u8 {
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    let mut server = Server::new();
    loop {
        let message = match read_message(&mut input) {
            Ok(Some(message)) => message,
            // The client closed the connection: the protocol's own exit.
            Ok(None) => return 0,
            Err(error) => {
                let _ = write_message(&mut output, &error_response(Value::Null, -32700, &error));
                return 1;
            }
        };
        if let Some(response) = server.handle(&message, &mut output) {
            if write_message(&mut output, &response).is_err() {
                return 1;
            }
        }
        if server.exited {
            return 0;
        }
    }
}

/// One open document.
struct Document {
    /// The text as the editor has it, including unsaved changes.
    text: String,
}

/// The server's state across messages.
struct Server {
    /// The root the client opened, used to resolve relative paths.
    root: Option<PathBuf>,
    /// Every open document, by URI.
    documents: HashMap<String, Document>,
    /// Set once `shutdown` has been answered.
    shutdown: bool,
    /// Set when `exit` arrives.
    exited: bool,
}

impl Server {
    fn new() -> Server {
        Server {
            root: None,
            documents: HashMap::new(),
            shutdown: false,
            exited: false,
        }
    }

    /// Handles one message, returning the response when it needs one.
    fn handle(&mut self, message: &Value, output: &mut impl Write) -> Option<Value> {
        let method = message.get_str("method");
        let id = message.get("id").cloned();
        // A notification has no id and no response; a request has both.
        let Some(method) = method else {
            // A response to something this server never asks for.
            return None;
        };
        match method {
            "initialize" => {
                self.root = message
                    .get("params")
                    .and_then(|params| params.get_str("rootUri"))
                    .and_then(uri_to_path);
                id.map(|id| success(id, self.initialize_result()))
            }
            "initialized" => None,
            "shutdown" => {
                self.shutdown = true;
                id.map(|id| success(id, Value::Null))
            }
            "exit" => {
                self.exited = true;
                None
            }
            "textDocument/didOpen" => {
                if let Some((uri, text)) = document_from(message, "textDocument")
                    && let Some(text) = text
                {
                    self.documents.insert(uri.clone(), Document { text });
                    self.publish(&uri, output);
                }
                None
            }
            "textDocument/didChange" => {
                if let Some((uri, text)) = changed_document(message) {
                    if let Some(document) = self.documents.get_mut(&uri) {
                        document.text = text;
                    } else {
                        self.documents.insert(uri.clone(), Document { text });
                    }
                    self.publish(&uri, output);
                }
                None
            }
            "textDocument/didSave" => {
                // The text is already current, either from `didChange` or from
                // the disk; re-checking is what confirms a saved file.
                if let Some(uri) = path_of(message) {
                    self.publish(&uri, output);
                }
                None
            }
            "textDocument/didClose" => {
                if let Some(uri) = path_of(message) {
                    self.documents.remove(&uri);
                    // The diagnostics for a closed file are withdrawn by
                    // sending an empty list.
                    let notification = notification(
                        "textDocument/publishDiagnostics",
                        json::Value::Object(
                            [
                                ("uri".to_owned(), Value::string(uri)),
                                ("diagnostics".to_owned(), Value::Array(Vec::new())),
                            ]
                            .into_iter()
                            .collect(),
                        ),
                    );
                    let _ = write_message(output, &notification);
                }
                None
            }
            // A request whose method is not implemented must be answered, and
            // the protocol's answer is "method not found"; ignoring it leaves
            // the client waiting.
            _ => id.map(|id| error_response(id, -32601, "method not found")),
        }
    }

    /// What this server can do.
    fn initialize_result(&self) -> Value {
        let mut capabilities = std::collections::BTreeMap::new();
        // Full sync: every change sends the whole file. Incremental would need
        // the ranges applied in order, and a full document is one message.
        capabilities.insert("textDocumentSync".to_owned(), Value::number(1));
        Value::Object(
            [
                ("capabilities".to_owned(), Value::Object(capabilities)),
                (
                    "serverInfo".to_owned(),
                    json::Value::Object(
                        [
                            ("name".to_owned(), Value::string("fallc")),
                            (
                                "version".to_owned(),
                                Value::string(env!("CARGO_PKG_VERSION")),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    ),
                ),
            ]
            .into_iter()
            .collect(),
        )
    }

    /// Checks a document and sends its diagnostics.
    fn publish(&mut self, uri: &str, output: &mut impl Write) {
        let Some(path) = uri_to_path(uri) else {
            return;
        };
        let Some(document) = self.documents.get(uri) else {
            return;
        };
        let diagnostics = check(&path, &document.text, self.root.as_deref());
        let payload = json::Value::Object(
            [
                ("uri".to_owned(), Value::string(uri)),
                ("diagnostics".to_owned(), Value::Array(diagnostics)),
            ]
            .into_iter()
            .collect(),
        );
        let notification = notification("textDocument/publishDiagnostics", payload);
        let _ = write_message(output, &notification);
    }
}

/// Checks `text` as `path` and renders its diagnostics for the protocol.
///
/// The file is written to a temporary path first when the editor's text is not
/// what the compiler would read from disk -- an unsaved buffer -- because the
/// compiler reads files, and rewriting the whole pipeline to take a string
/// would be a larger change than the server needs. A saved file is checked in
/// place, so imports beside it resolve as they will at build time.
fn check(path: &Path, text: &str, root: Option<&Path>) -> Vec<Value> {
    let on_disk = std::fs::read_to_string(path).ok();
    let scratch = on_disk.as_deref() != Some(text);
    let temporary = if scratch {
        let name = format!(
            "{}.{}.lsp.ft",
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("buffer"),
            std::process::id()
        );
        let directory = root
            .filter(|root| root.is_dir())
            .or_else(|| path.parent())
            .map_or_else(std::env::temp_dir, Path::to_path_buf);
        let candidate = directory.join(name);
        if std::fs::write(&candidate, text).is_err() {
            return Vec::new();
        }
        Some(candidate)
    } else {
        None
    };
    let subject = temporary.as_deref().unwrap_or(path);
    let mut session = Session::new();
    session.set_color(false);
    session.set_input_files([subject.to_path_buf()]);
    crate::pipeline::check_file(&mut session, subject);
    let diagnostics = to_protocol(&session, subject, path);
    if let Some(temporary) = &temporary {
        let _ = std::fs::remove_file(temporary);
    }
    diagnostics
}

/// Renders the session's diagnostics as protocol diagnostics.
///
/// `reported_as` is the URI the editor knows the file by: a buffer checked
/// through a temporary file must still point into the buffer.
fn to_protocol(session: &Session, subject: &Path, reported_as: &Path) -> Vec<Value> {
    let sources = session.sources();
    let mut out = Vec::new();
    for diagnostic in session.diagnostics().iter() {
        let severity = match diagnostic.severity {
            fallc_diag::Severity::Error | fallc_diag::Severity::Bug => 1,
            fallc_diag::Severity::Warning => 2,
        };
        let code = diagnostic.code.map(|code| Value::string(code.to_string()));
        let message = match &code {
            Some(Value::String(code)) => format!("[{code}] {}", diagnostic.message),
            _ => diagnostic.message.clone(),
        };
        let mut entry: std::collections::BTreeMap<String, Value> = [
            (
                "range".to_owned(),
                range_of(sources, diagnostic.primary, reported_as),
            ),
            ("severity".to_owned(), Value::number(severity)),
            ("source".to_owned(), Value::string("fallc")),
            ("message".to_owned(), Value::string(message)),
        ]
        .into_iter()
        .collect();
        if let Some(code) = code {
            entry.insert("code".to_owned(), code);
        }
        if let Some(primary) = diagnostic.primary
            && !primary.is_synthetic()
        {
            // A diagnostic about another file is reported at that file's own
            // position, which is what the editor needs to open it. The buffer
            // itself is not "another file" even when it was checked through a
            // temporary: pointing the editor at a scratch path it never opened
            // would be noise.
            if let Some(file) = sources.file(primary.file)
                && !same_file(file.path(), subject, reported_as)
            {
                entry.insert(
                    "relatedInformation".to_owned(),
                    Value::Array(vec![related(sources, primary)]),
                );
            }
        }
        out.push(Value::Object(entry));
    }
    out
}

/// Whether two paths name the same file, as far as a diagnostic can tell.
///
/// The buffer is checked through a temporary file, so its diagnostic's path is
/// the temporary even though the editor knows the buffer by its own path; both
/// spellings have to count as the same file.
fn same_file(reported: &Path, subject: &Path, editor: &Path) -> bool {
    reported == editor || reported == subject
}

/// One piece of related information, for a diagnostic that points elsewhere.
fn related(sources: &fallc_span::SourceMap, span: fallc_span::Span) -> Value {
    let location = match sources.file(span.file) {
        Some(file) => json::Value::Object(
            [
                ("uri".to_owned(), Value::string(path_to_uri(file.path()))),
                (
                    "range".to_owned(),
                    range_of(sources, Some(span), file.path()),
                ),
            ]
            .into_iter()
            .collect(),
        ),
        None => Value::Null,
    };
    json::Value::Object(
        [
            ("location".to_owned(), location),
            ("message".to_owned(), Value::string("reported here")),
        ]
        .into_iter()
        .collect(),
    )
}

/// The protocol range of a span, in the editor's own coordinates.
///
/// Lines and characters are 0-based here, unlike the compiler's 1-based
/// positions. The end of the range is the byte after the span, which is what
/// the protocol means by an exclusive end.
fn range_of(
    sources: &fallc_span::SourceMap,
    span: Option<fallc_span::Span>,
    reported_as: &Path,
) -> Value {
    let Some(span) = span.filter(|span| !span.is_synthetic()) else {
        return zero_range();
    };
    let Some(file) = sources.file(span.file) else {
        return zero_range();
    };
    let _ = reported_as;
    let start = file.line_col(span.lo);
    let end = file.line_col(span.hi);
    position_range(
        start.line.saturating_sub(1),
        start.column.saturating_sub(1),
        end.line.saturating_sub(1),
        end.column.saturating_sub(1),
    )
}

fn position_range(
    start_line: u32,
    start_character: u32,
    end_line: u32,
    end_character: u32,
) -> Value {
    json::Value::Object(
        [
            (
                "start".to_owned(),
                json::Value::Object(
                    [
                        ("line".to_owned(), Value::number(start_line)),
                        ("character".to_owned(), Value::number(start_character)),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
            (
                "end".to_owned(),
                json::Value::Object(
                    [
                        ("line".to_owned(), Value::number(end_line)),
                        ("character".to_owned(), Value::number(end_character)),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn zero_range() -> Value {
    position_range(0, 0, 0, 0)
}

/// Reads the document's URI and text from a `didOpen` message.
fn document_from(message: &Value, key: &str) -> Option<(String, Option<String>)> {
    let document = message.get("params")?.get(key)?;
    let uri = document.get_str("uri")?.to_owned();
    Some((uri, document.get_str("text").map(str::to_owned)))
}

/// Reads the URI and full text from a `didChange` message.
///
/// Full sync means the last change carries the whole document; the protocol
/// allows several changes in one notification, so the last one wins.
fn changed_document(message: &Value) -> Option<(String, String)> {
    let params = message.get("params")?;
    let uri = params.get("textDocument")?.get_str("uri")?.to_owned();
    let changes = match params.get("contentChanges") {
        Some(Value::Array(changes)) => changes,
        _ => return None,
    };
    let text = changes
        .iter()
        .rev()
        .find_map(|change| change.get_str("text").map(str::to_owned))?;
    Some((uri, text))
}

/// Reads the document URI from a message that only names one.
fn path_of(message: &Value) -> Option<String> {
    message
        .get("params")?
        .get("textDocument")?
        .get_str("uri")
        .map(str::to_owned)
}

/// Converts a `file://` URI to a path.
fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    // A Windows URI spells the drive as `/C:/...`; the leading slash is the
    // URI's, not the path's.
    let rest = if rest.starts_with('/') && rest.get(2..3) == Some(":") {
        &rest[1..]
    } else {
        rest
    };
    Some(PathBuf::from(percent_decode(rest)))
}

/// Converts a path to a `file://` URI.
pub fn path_to_uri(path: &Path) -> String {
    let text = path.to_string_lossy();
    let mut out = String::from("file://");
    for character in text.chars() {
        match character {
            // A drive letter keeps its colon, which is legal in a path segment.
            ':' | '/' | '\\' | '-' | '_' | '.' | '~' => out.push(character),
            c if c.is_ascii_alphanumeric() => out.push(c),
            c => {
                let mut buffer = [0u8; 4];
                for byte in c.encode_utf8(&mut buffer).bytes() {
                    out.push_str(&format!("%{byte:02X}"));
                }
            }
        }
    }
    out
}

/// Decodes `%XX` escapes in a URI path.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%'
            && let (Some(high), Some(low)) = (hex(bytes.get(at + 1)), hex(bytes.get(at + 2)))
        {
            out.push(high * 16 + low);
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(byte: Option<&u8>) -> Option<u8> {
    match byte? {
        b'0'..=b'9' => Some(byte? - b'0'),
        b'a'..=b'f' => Some(byte? - b'a' + 10),
        b'A'..=b'F' => Some(byte? - b'A' + 10),
        _ => None,
    }
}

/// A successful response.
fn success(id: Value, result: Value) -> Value {
    json::Value::Object(
        [
            ("jsonrpc".to_owned(), Value::string("2.0")),
            ("id".to_owned(), id),
            ("result".to_owned(), result),
        ]
        .into_iter()
        .collect(),
    )
}

/// An error response.
fn error_response(id: Value, code: i64, message: &str) -> Value {
    json::Value::Object(
        [
            ("jsonrpc".to_owned(), Value::string("2.0")),
            ("id".to_owned(), id),
            (
                "error".to_owned(),
                json::Value::Object(
                    [
                        ("code".to_owned(), Value::number(code)),
                        ("message".to_owned(), Value::string(message)),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

/// A notification.
fn notification(method: &str, params: Value) -> Value {
    json::Value::Object(
        [
            ("jsonrpc".to_owned(), Value::string("2.0")),
            ("method".to_owned(), Value::string(method)),
            ("params".to_owned(), params),
        ]
        .into_iter()
        .collect(),
    )
}

/// Reads one `Content-Length` framed message.
///
/// Returns `None` at end of input, which is how a client closes the server.
fn read_message(input: &mut impl BufRead) -> Result<Option<Value>, String> {
    let mut length: Option<usize> = None;
    let mut line = String::new();
    loop {
        line.clear();
        let read = input
            .read_line(&mut line)
            .map_err(|error| format!("cannot read the message header: {error}"))?;
        if read == 0 {
            return Ok(None);
        }
        let header = line.trim_end_matches(['\r', '\n']);
        if header.is_empty() {
            // The empty line ends the headers.
            break;
        }
        if let Some(value) = header.strip_prefix("Content-Length:") {
            length = Some(
                value
                    .trim()
                    .parse()
                    .map_err(|_| "the Content-Length header is not a number".to_owned())?,
            );
        }
    }
    let length = length.ok_or_else(|| "a message has no Content-Length header".to_owned())?;
    let mut body = vec![0u8; length];
    input
        .read_exact(&mut body)
        .map_err(|error| format!("cannot read the message body: {error}"))?;
    let text = String::from_utf8(body).map_err(|_| "the message body is not UTF-8".to_owned())?;
    json::parse(&text).map(Some).map_err(|error| error.message)
}

/// Writes one `Content-Length` framed message.
fn write_message(output: &mut impl Write, message: &Value) -> std::io::Result<()> {
    let body = message.to_text();
    write!(output, "Content-Length: {}\r\n\r\n{}", body.len(), body)?;
    output.flush()
}
