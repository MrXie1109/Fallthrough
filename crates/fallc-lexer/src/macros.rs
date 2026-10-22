//! The macro preprocessor: C-style macros over a token stream.
//!
//! # Where it sits
//!
//! This runs after [`crate::Lexer`] and before the parser, on the token stream
//! alone. Two consequences shape the whole module:
//!
//! * A definition's body ends at the end of its logical line, and the only
//!   stage that saw the whitespace is the scanner. [`Token::newline_before`]
//!   carries that fact, and a backslash-newline has already been folded away as
//!   trivia by [`crate::Lexer`], so the preprocessor never sees it.
//! * A macro name is a name, and a name's spelling is not part of its token
//!   kind. [`Token::text`] carries it, and [`Token::spelling`] is the single
//!   accessor this module uses to ask "what does this token stand for".
//!
//! # Shape
//!
//! ```text
//! define NAME body
//! export define NAME(a, b, ...) body
//! $NAME
//! $NAME(a, b)
//! ```
//!
//! Definitions are removed from the stream; invocations are replaced by the
//! macro's body with arguments substituted. Expansion is recursive, with a
//! depth limit, and a macro is never expanded inside its own expansion.
//!
//! # Rules
//!
//! Each of these is pinned by a test in `tests/macros.rs`.
//!
//! 1. **A definition must begin a logical line.** `define` is not a reserved
//!    word -- the language says it is recognised at the start of a module-level
//!    definition and is an ordinary name elsewhere -- and a token stream has no
//!    grammar to tell item position from expression position. "Begins the line
//!    (or the file)" is therefore the approximation, and every other `define`
//!    stays an ordinary name. A consequence worth stating: a definition written
//!    after another item on the same line is not recognised, and `export` must
//!    begin the line for `export define` to be one. A malformed definition (a
//!    non-identifier name) is reported and its line is dropped, so it does not
//!    also reach the parser.
//! 2. **A macro must be defined before it is used.** The stream is processed in
//!    order, as in C; a second pass would let a use precede its definition and
//!    would silently accept code a C programmer would call wrong.
//! 3. **Definitions are only recognised in the file's own stream.** A body that
//!    contains the word `define` copies it through; expansion output is never
//!    re-scanned for definitions.
//! 4. **`export` publishes a definition.** The macros a stream marks `export`
//!    come back from [`expand_macros_with`], and an importing file installs them
//!    as its prelude before expanding its own stream -- which is what makes
//!    `export define` visible to importers. A definition in the importing file
//!    replaces an installed one of the same name, silently.
//! 5. **Self-recursion is an error, and the invocation is left in place.** A
//!    macro (or cycle of macros) that reaches itself while being expanded is
//!    reported once; the unexpanded `$NAME(..)` tokens stay in the output so
//!    the rest of the line is still available to whoever is looking, though a
//!    non-empty diagnostic vector means the caller gets no tokens at all.
//! 6. **Whitespace is invisible in invocations.** `$MAX (a, b)` and `$MAX(a,b)`
//!    are the same invocation (even with a line break before the `(`), and
//!    `a ## b` equals `a##b`. The one place spacing matters is a function-like
//!    *definition*: its `(` must immediately follow the name, as in C, and
//!    "immediately" is measured by the tokens' spans touching -- so a space or
//!    a line continuation between the two makes the macro object-like.
//! 7. **Empty arguments are allowed**: `$F()` gives a zero-parameter macro no
//!    arguments and a one-parameter macro one empty argument, and `$F(,x)` is
//!    two arguments, the first empty. An empty argument next to `##` is a
//!    placemarker, as in C: `a ##` an empty `b` is `a`.
//! 8. **A variadic invocation with no variadic arguments** leaves
//!    `__VA_LIST__` empty, including after the fixed arguments: `$LOG(t)`
//!    expands as if the list were absent.
//! 9. **`__VA_LIST__` in a non-variadic body is an ordinary name** and is
//!    copied through. `#__VA_LIST__` is an error: `#` stringifies a *parameter*
//!    only.
//! 10. **Only parentheses balance an argument list.** A comma inside `{}` or
//!     `[]` at parenthesis depth zero still separates arguments, because only
//!     parentheses are counted; a caller that needs a comma grouped must add
//!     parentheses.
//! 11. **`##` pastes any two tokens that re-lex as exactly one token.** That
//!     covers identifier+identifier, identifier+number and number+number, and
//!     it also allows pasting punctuation into a longer mark (`+ ## +` is
//!     `++`). Anything else is an error.
//! 12. **A definition with an invalid `#`/`##` is reported and not
//!     registered**, so its body is never used; a redefinition is reported and
//!     the first definition is kept.
//! 13. **Diagnostics are collected, not fatal on the first one.** Every
//!     invocation that can be diagnosed in place is, and the whole vector is
//!     returned at the end.
//! 14. **The terminating `Eof` is preserved unchanged.** If the caller supplies
//!     a stream without one, none is invented; the parser's own buffer appends
//!     a synthetic one.

use std::collections::HashMap;

use fallc_span::{FileId, Span};

use crate::lexer::Lexer;
use crate::token::{Keyword, Punctuation, Token, TokenKind};

/// How many nested expansions are allowed before the preprocessor gives up.
///
/// A bound is what keeps a pathological definition from exhausting the stack;
/// 128 is deep enough for real macro layers and shallow enough to fail loudly.
const MAX_DEPTH: usize = 128;

/// The name that stands for a variadic macro's trailing arguments.
const VARIADIC_LIST: &str = "__VA_LIST__";

// Diagnostic codes. Macro expansion is a lexer-stage pass, so it uses the
// lexer's `L` range: `DiagnosticCode::lexer(code)` renders these as `L0002`
// upwards. `L0001` is the scanner's "unrecognised character"; `L0002` through
// `L0013` were unused before this module took them.
/// `$` was not followed by a macro name.
const CODE_SIGIL_WITHOUT_NAME: u16 = 2;
/// The name after `$` is not a macro.
const CODE_UNKNOWN_MACRO: u16 = 3;
/// An object-like macro was invoked with an argument list.
const CODE_OBJECT_WITH_ARGUMENTS: u16 = 4;
/// A function-like macro was invoked without an argument list.
const CODE_FUNCTION_WITHOUT_ARGUMENTS: u16 = 5;
/// The argument count does not match the parameter list.
const CODE_WRONG_ARGUMENT_COUNT: u16 = 6;
/// A `define` that cannot be understood: bad name, bad parameter list, bad
/// `#`/`##` use in the body.
const CODE_MALFORMED_DEFINITION: u16 = 7;
/// The name is already defined.
const CODE_REDEFINITION: u16 = 8;
/// `#` appeared outside a macro body.
const CODE_HASH_OUTSIDE_BODY: u16 = 9;
/// Expansion nested deeper than [`MAX_DEPTH`].
const CODE_TOO_DEEP: u16 = 10;
/// A macro reached itself while being expanded.
const CODE_SELF_RECURSION: u16 = 11;
/// A `##` result is not a single token.
const CODE_INVALID_PASTE: u16 = 12;
/// An argument list is not closed before the end of input.
const CODE_UNTERMINATED_ARGUMENTS: u16 = 13;

/// One problem found while expanding macros.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MacroDiagnostic {
    /// Where the programmer should look: the `$NAME` of the invocation for a
    /// problem found while expanding, or the offending token in a definition.
    pub span: Span,
    /// The diagnostic's code, such as `3`.
    ///
    /// This is a lexer-stage number: the driver builds the printable code with
    /// `DiagnosticCode::lexer(code)`, which renders `3` as `L0003`.
    pub code: u16,
    /// What went wrong, phrased for the programmer.
    pub message: String,
}

/// The facts a macro expansion may need about the file it is expanding.
///
/// A few names are provided by the compiler rather than defined by a program,
/// because they are facts about this compilation: the file's own name, the line,
/// the platform the binary is being built for. They are invoked like macros and
/// expand to literals: `$__builtin_FILE` is a string, `$__builtin_LINE` an integer.
///
/// The names are spelled with a leading double underscore so that a program's
/// macro cannot collide with one by accident, and `standard/meta.ftm` wraps them
/// under names a program reads (`$meta.FILE`).
#[derive(Clone, Debug, Default)]
pub struct Builtins {
    /// The path of the file being expanded, as the compiler was given it.
    pub file: String,
    /// Its text, so a line number can be counted from a byte offset.
    pub source: String,
    /// The target platform: `linux`, `windows`, `macos`, and so on.
    pub platform: String,
    /// The target architecture: `x86_64`, `aarch64`, and so on.
    pub arch: String,
    /// Whether the target is Windows.
    pub windows: bool,
    /// The build's date, as `YYYY-MM-DD`.
    pub date: String,
    /// The build's time, as `HH:MM:SS`.
    pub time: String,
}

impl Builtins {
    /// Returns the replacement text of a builtin, and whether it is a string.
    ///
    /// `None` for a name that is not a builtin. The second field says which
    /// token to make: a string literal needs quotes, an integer does not.
    fn value(&self, name: &str, offset: u32) -> Option<(String, bool)> {
        let number = |value: i64| Some((value.to_string(), false));
        match name {
            "__builtin_FILE" => Some((self.file.clone(), true)),
            "__builtin_LINE" => {
                let line = self.line_and_column(offset).0;
                number(i64::from(line))
            }
            "__builtin_COLUMN" => {
                let column = self.line_and_column(offset).1;
                number(i64::from(column))
            }
            "__builtin_PLATFORM" => Some((self.platform.clone(), true)),
            "__builtin_ARCH" => Some((self.arch.clone(), true)),
            "__builtin_OS_WINDOWS" => number(i64::from(self.windows)),
            "__builtin_OS_POSIX" => number(i64::from(!self.windows)),
            "__builtin_DATE" => Some((self.date.clone(), true)),
            "__builtin_TIME" => Some((self.time.clone(), true)),
            _ => None,
        }
    }

    /// The one-based line and column a byte offset falls on.
    ///
    /// A column counts bytes, which is what an editor's text does for the ASCII
    /// that starts a line; a line is what the reader calls a line.
    fn line_and_column(&self, offset: u32) -> (u32, u32) {
        let offset = (offset as usize).min(self.source.len());
        let mut line = 1u32;
        let mut column = 1u32;
        for byte in self.source.as_bytes()[..offset].iter() {
            if *byte == b'\n' {
                line += 1;
                column = 1;
            } else {
                column += 1;
            }
        }
        (line, column)
    }
}

/// Expands every macro in `tokens`, removing the definitions.
///
/// Every returned token that comes out of an expansion carries the span of the
/// `$NAME` that produced it, so a later diagnostic points at what the
/// programmer wrote rather than at the macro's definition.
///
/// # Errors
///
/// Returns every diagnostic found, not just the first. A macro that cannot be
/// expanded is left in the stream unchanged so that the rest of the file can
/// still be examined, but a caller that receives `Err` should not use the
/// (absent) output.
pub fn expand_macros(tokens: Vec<Token>) -> Result<Vec<Token>, Vec<MacroDiagnostic>> {
    expand_macros_with(tokens, &[], &Builtins::default()).map(|(tokens, _)| tokens)
}

/// Expands a stream with definitions installed from other modules.
///
/// The `prelude` is installed first, so a file may use a macro another module
/// exported. What comes back beside the tokens is what the stream's
/// preprocessor ended up knowing -- every definition it could see, and the
/// names it marked `export`, which are what its own importers will install.
pub fn expand_macros_with(
    tokens: Vec<Token>,
    prelude: &[Macro],
    builtins: &Builtins,
) -> Result<(Vec<Token>, Macros), Vec<MacroDiagnostic>> {
    let mut preprocessor = Preprocessor {
        builtins: builtins.clone(),
        macros: HashMap::new(),
        diagnostics: Vec::new(),
        stream_end: 0,
        exported: Vec::new(),
        defined: Vec::new(),
        imported: std::collections::HashSet::new(),
    };
    for definition in prelude {
        preprocessor.macros.insert(
            definition.name.clone(),
            MacroDef {
                params: definition.params.clone(),
                function_like: definition.function_like,
                variadic: definition.variadic,
                body: definition.body.clone(),
            },
        );
        preprocessor.imported.insert(definition.name.clone());
    }
    // The furthest offset any token reaches. A span conjured for a token that
    // came out of a definition must stay inside the file, or rendering a
    // diagnostic that points at it would ask for text that is not there.
    preprocessor.stream_end = tokens.iter().map(|token| token.span.hi).max().unwrap_or(0);
    let mut expanded = Vec::new();
    preprocessor.expand_into(&tokens, true, &[], 0, &mut expanded);
    // The input ends with exactly one `Eof`; it is not part of any body and is
    // passed through so the caller's stream still ends the way it did. A stream
    // without one gets none, rather than an invented one.
    match tokens.last() {
        Some(eof) if eof.is_eof() => expanded.push(eof.to_owned()),
        _ => {}
    }
    if preprocessor.diagnostics.is_empty() {
        // The prelude comes first, then the stream's own definitions: a name
        // defined twice is the one this stream wrote.
        let mut available: Vec<Macro> = prelude.to_vec();
        for definition in &preprocessor.defined {
            available.retain(|existing| existing.name != definition.name);
            available.push(definition.clone());
        }
        Ok((
            expanded,
            Macros {
                available,
                exported: preprocessor.exported,
            },
        ))
    } else {
        Err(preprocessor.diagnostics)
    }
}

/// What a stream's preprocessor ended up knowing.
#[derive(Clone, Debug, Default)]
pub struct Macros {
    /// Every definition the stream could see, in the order they were made:
    /// the prelude first, then the stream's own.
    pub available: Vec<Macro>,
    /// The names this stream marked `export`.
    pub exported: Vec<String>,
}

/// One definition, in the form another file's preprocessor can install.
///
/// A module's macros are expanded before the importing file is parsed, so what
/// crosses is the *definition*: the name, the parameter list, and the
/// replacement tokens exactly as written. An importing file installs these
/// before it expands its own stream, which is what makes `export define` mean
/// what it says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Macro {
    /// The name it is invoked by.
    pub name: String,
    /// The parameter names, empty for an object-like macro.
    pub params: Vec<String>,
    /// Whether it was written with a parameter list at all.
    pub function_like: bool,
    /// Whether the last parameter was `...`.
    pub variadic: bool,
    /// The replacement tokens, exactly as written.
    pub body: Vec<Token>,
}

/// One definition, as parsed.
#[derive(Clone, Debug)]
struct MacroDef {
    /// Parameter names, empty for an object-like macro and for a function-like
    /// one written `()`.
    params: Vec<String>,
    /// Whether the macro was written with a parameter list at all.
    function_like: bool,
    /// Whether the last parameter was `...`.
    variadic: bool,
    /// The replacement tokens, exactly as written.
    body: Vec<Token>,
}

/// A piece of a substituted replacement list, before `##` has been applied.
#[derive(Clone, Debug)]
enum Piece {
    /// A finished token.
    Token(Token),
    /// A `##` whose two operands are the tokens around it.
    Paste,
}

/// The preprocessor state for one file.
#[derive(Debug)]
struct Preprocessor {
    /// The compiler-provided names this file may expand.
    builtins: Builtins,
    macros: HashMap<String, MacroDef>,
    diagnostics: Vec<MacroDiagnostic>,
    /// The furthest source offset in the stream being expanded, so a span
    /// conjured for a token from a definition stays inside the file.
    stream_end: u32,
    /// The definitions this stream marked `export`, in the order written.
    exported: Vec<String>,
    /// Every definition made here, in order.
    defined: Vec<Macro>,
    /// Names installed from another module.
    ///
    /// A definition in this stream replaces one of them silently: a module's own
    /// `define` is its own, and shadowing an imported name is what every other
    /// declaration in the language does.
    imported: std::collections::HashSet<String>,
}

impl Preprocessor {
    /// Records a diagnostic.
    fn report(&mut self, span: Span, code: u16, message: impl Into<String>) {
        self.diagnostics.push(MacroDiagnostic {
            span,
            code,
            message: message.into(),
        });
    }

    /// Expands `input` into `out`.
    ///
    /// `top_level` additionally recognises definitions and removes them.
    /// Definitions nested inside a replacement list are not recognised: the
    /// argument is `false` for every recursive call.
    fn expand_into(
        &mut self,
        input: &[Token],
        top_level: bool,
        active: &[String],
        depth: usize,
        out: &mut Vec<Token>,
    ) {
        let mut index = 0;
        while index < input.len() {
            if input[index].is_eof() {
                break;
            }
            if top_level && self.starts_definition(input, index) {
                index = self.parse_definition(input, index);
                continue;
            }
            match input[index].kind {
                TokenKind::Punct(Punctuation::Dollar) => {
                    index = self.expand_invocation(input, index, active, depth, out);
                }
                TokenKind::Punct(Punctuation::Hash) => {
                    // `#` belongs to a macro body. A raw string is one token, so
                    // the `#` of `r#tag".."tag#` is never seen here, and
                    // attributes use `@`, so neither is disturbed.
                    self.report(
                        input[index].span,
                        CODE_HASH_OUTSIDE_BODY,
                        "`#` is only meaningful inside a macro body, as `#param`",
                    );
                    out.push(input[index].clone());
                    index += 1;
                }
                _ => {
                    out.push(input[index].clone());
                    index += 1;
                }
            }
        }
    }

    // ------------------------------------------------------------ definitions

    /// Returns `true` if a definition begins at `index`.
    ///
    /// A definition is an item, so it begins a logical line; that is the closest
    /// a token preprocessor can come to "item position". `define` anywhere else
    /// is an ordinary name, as the language requires. `export define` is a
    /// definition too; `export` followed by anything else is an ordinary
    /// keyword and is passed through.
    fn starts_definition(&self, input: &[Token], index: usize) -> bool {
        if index != 0 && !input[index].newline_before {
            return false;
        }
        // `export define NAME ...` publishes a definition; `export` is a
        // keyword, so it arrives as one.
        match input[index].kind {
            TokenKind::Keyword(Keyword::Export) => input
                .get(index + 1)
                .is_some_and(|token| token.kind == TokenKind::Keyword(Keyword::Define)),
            TokenKind::Keyword(Keyword::Define) => true,
            _ => false,
        }
    }

    /// Parses and registers a definition beginning at `start`, returning the
    /// index of the first token after it.
    fn parse_definition(&mut self, input: &[Token], start: usize) -> usize {
        let mut index = start;
        let mut exported = false;
        if input[index].kind == TokenKind::Keyword(Keyword::Export) {
            exported = true;
            index += 1;
        }
        let define_span = input[index].span;
        index += 1;

        let Some(name) = input.get(index).and_then(ident_name).map(ToOwned::to_owned) else {
            let span = input.get(index).map_or(define_span, |token| token.span);
            self.report(
                span,
                CODE_MALFORMED_DEFINITION,
                "a macro name must be an identifier",
            );
            return skip_to_line_end(input, index);
        };
        let name_span = input[index].span;
        index += 1;

        // The `(` of a parameter list must immediately follow the name, as in
        // C: `define F (x)` is an object-like macro whose body starts with `(`.
        let mut params = Vec::new();
        let mut function_like = false;
        let mut variadic = false;
        if let Some(open) = input.get(index) {
            if open.kind == TokenKind::Punct(Punctuation::OpenParen)
                && open.span.file == name_span.file
                && open.span.lo == name_span.hi
            {
                function_like = true;
                let Some(parsed) = self.parse_parameters(input, index) else {
                    return skip_to_line_end(input, index);
                };
                (params, variadic, index) = parsed;
            }
        }

        // The body runs to the end of the logical line. A backslash-newline is
        // not a line break, so the scanner has already folded it away and the
        // body simply continues.
        let body_start = index;
        let mut body_end = body_start;
        while body_end < input.len() {
            let token = &input[body_end];
            if token.is_eof() || token.newline_before {
                break;
            }
            body_end += 1;
        }
        let body = &input[body_start..body_end];

        if name == VARIADIC_LIST {
            self.report(
                name_span,
                CODE_MALFORMED_DEFINITION,
                format!("`{VARIADIC_LIST}` is reserved and cannot name a macro"),
            );
            return body_end;
        }
        if !self.validate_body(body, &params, function_like) {
            return body_end;
        }
        // A definition here replaces one that came from another module: the
        // file's own `define` is its own. Any other second definition is the
        // mistake it always was.
        if self.imported.remove(&name) {
            self.macros.remove(&name);
        } else if self.macros.contains_key(&name) {
            self.report(
                name_span,
                CODE_REDEFINITION,
                format!("macro `{name}` is already defined"),
            );
            return body_end;
        }
        let definition = MacroDef {
            params,
            function_like,
            variadic,
            body: body.to_vec(),
        };
        if exported {
            self.exported.push(name.clone());
        }
        self.defined.push(Macro {
            name: name.clone(),
            params: definition.params.clone(),
            function_like: definition.function_like,
            variadic: definition.variadic,
            body: definition.body.clone(),
        });
        self.macros.insert(name, definition);
        body_end
    }

    /// Parses a parameter list starting at `open`, returning the parameters,
    /// whether the list is variadic, and the index after `)`.
    ///
    /// Reports and returns `None` on a malformed list; the caller then drops the
    /// rest of the definition's line.
    fn parse_parameters(
        &mut self,
        input: &[Token],
        open: usize,
    ) -> Option<(Vec<String>, bool, usize)> {
        let mut params: Vec<String> = Vec::new();
        let mut variadic = false;
        let mut index = open + 1;
        // `()` is a function-like macro with no parameters.
        if input
            .get(index)
            .is_some_and(|token| token.kind == TokenKind::Punct(Punctuation::CloseParen))
        {
            return Some((params, variadic, index + 1));
        }
        loop {
            let Some(token) = input.get(index) else {
                self.report(
                    input[open].span,
                    CODE_MALFORMED_DEFINITION,
                    "unterminated macro parameter list",
                );
                return None;
            };
            if token.is_eof() {
                self.report(
                    input[open].span,
                    CODE_MALFORMED_DEFINITION,
                    "unterminated macro parameter list",
                );
                return None;
            }
            // `...` is a parameter of its own and must be the last one.
            if token.kind == TokenKind::Punct(Punctuation::DotDotDot) {
                variadic = true;
                index += 1;
                match input.get(index).map(|token| token.kind) {
                    Some(TokenKind::Punct(Punctuation::CloseParen)) => {
                        return Some((params, variadic, index + 1));
                    }
                    _ => {
                        self.report(
                            token.span,
                            CODE_MALFORMED_DEFINITION,
                            "`...` must be the last parameter of a macro",
                        );
                        return None;
                    }
                }
            }
            let Some(name) = ident_name(token).map(ToOwned::to_owned) else {
                self.report(
                    token.span,
                    CODE_MALFORMED_DEFINITION,
                    "a macro parameter must be an identifier",
                );
                return None;
            };
            if parameter_index(&params, &name).is_some() {
                self.report(
                    token.span,
                    CODE_MALFORMED_DEFINITION,
                    format!("macro parameter `{name}` is declared twice"),
                );
                return None;
            }
            params.push(name);
            index += 1;
            match input.get(index).map(|token| token.kind) {
                Some(TokenKind::Punct(Punctuation::Comma)) => index += 1,
                Some(TokenKind::Punct(Punctuation::CloseParen)) => {
                    return Some((params, variadic, index + 1));
                }
                _ => {
                    let span = input.get(index).map_or(token.span, |token| token.span);
                    self.report(
                        span,
                        CODE_MALFORMED_DEFINITION,
                        "expected `,` or `)` in a macro parameter list",
                    );
                    return None;
                }
            }
        }
    }

    /// Checks that a body uses `#` and `##` correctly.
    ///
    /// A `#` that is not stringifying a parameter and a `##` with no token on
    /// one side are rejected here, at the definition, rather than left for every
    /// invocation to trip over.
    fn validate_body(&mut self, body: &[Token], params: &[String], function_like: bool) -> bool {
        let mut valid = true;
        let mut index = 0;
        let mut previous_was_paste = false;
        while index < body.len() {
            if body[index].kind != TokenKind::Punct(Punctuation::Hash) {
                previous_was_paste = false;
                index += 1;
                continue;
            }
            if is_hash(body.get(index + 1)) {
                // `##`: a paste marker.
                if index == 0 {
                    self.report(
                        body[index].span,
                        CODE_MALFORMED_DEFINITION,
                        "`##` cannot begin a macro body",
                    );
                    valid = false;
                } else if previous_was_paste {
                    self.report(
                        body[index].span,
                        CODE_MALFORMED_DEFINITION,
                        "`##` must have a token on both sides",
                    );
                    valid = false;
                }
                if body.get(index + 2).is_none() {
                    self.report(
                        body[index].span,
                        CODE_MALFORMED_DEFINITION,
                        "`##` cannot end a macro body",
                    );
                    valid = false;
                }
                previous_was_paste = true;
                index += 2;
                continue;
            }
            // A single `#`: stringification, which needs a parameter.
            previous_was_paste = false;
            let stringifies = function_like
                && body
                    .get(index + 1)
                    .and_then(ident_name)
                    .is_some_and(|name| parameter_index(params, name).is_some());
            if !stringifies {
                self.report(
                    body[index].span,
                    CODE_MALFORMED_DEFINITION,
                    if function_like {
                        "`#` must be followed by one of the macro's parameters"
                    } else {
                        "`#` cannot stringify anything in an object-like macro"
                    },
                );
                valid = false;
                index += 1;
                continue;
            }
            index += 2;
        }
        valid
    }

    // ------------------------------------------------------------ invocations

    /// Expands the invocation beginning at the `$` at `index`, appending to
    /// `out` and returning the index of the first token after it.
    fn expand_invocation(
        &mut self,
        input: &[Token],
        index: usize,
        active: &[String],
        depth: usize,
        out: &mut Vec<Token>,
    ) -> usize {
        let dollar = &input[index];
        let name_index = index + 1;
        let Some(name) = input.get(name_index).and_then(ident_name) else {
            // `$` not followed by a name: leave the `$` for the parser to
            // choke on and keep going with the next token.
            self.report(
                dollar.span,
                CODE_SIGIL_WITHOUT_NAME,
                "`$` must be followed by a macro name",
            );
            out.push(dollar.clone());
            return name_index;
        };
        let name = name.to_owned();
        // A macro may be named through the module that publishes it:
        // `$meta.FILE`. The dotted name is one name, and it is taken only when
        // that whole name is a macro; otherwise the dot belongs to the code
        // after a `$NAME` invocation, exactly as before.
        let mut name = name;
        let mut last = name_index;
        while let (Some(dot), Some(next)) = (input.get(last + 1), input.get(last + 2)) {
            if dot.kind != TokenKind::Punct(Punctuation::Dot) {
                break;
            }
            let Some(part) = ident_name(next) else {
                break;
            };
            let candidate = format!("{name}.{part}");
            if !self.macros.contains_key(&candidate) {
                break;
            }
            name = candidate;
            last += 2;
        }
        let invocation_span = dollar.span.to(input[last].span);
        let Some(definition) = self.macros.get(&name).cloned() else {
            // A builtin is a value of the compiler's: this file's name, the
            // line, the platform the program is being built for. The names use
            // the `builtin_` prefix so a library macro and a compiler fact are
            // told apart by their spelling.
            if let Some((text, is_string)) = self.builtins.value(&name, dollar.span.lo) {
                let token = if is_string {
                    Token::new(TokenKind::String, invocation_span)
                        .with_text(format!("\"{text}\""))
                } else {
                    Token::new(TokenKind::Int, invocation_span).with_text(text)
                };
                out.push(token);
                return last + 1;
            }
            self.report(
                invocation_span,
                CODE_UNKNOWN_MACRO,
                format!("unknown macro `{name}`"),
            );
            out.push(dollar.clone());
            out.push(input[name_index].clone());
            return name_index + 1;
        };

        let open = last + 1;
        let has_argument_list = input
            .get(open)
            .is_some_and(|token| token.kind == TokenKind::Punct(Punctuation::OpenParen));

        if !definition.function_like {
            if has_argument_list {
                self.report(
                    invocation_span,
                    CODE_OBJECT_WITH_ARGUMENTS,
                    format!("macro `{name}` takes no arguments"),
                );
                // Consume the argument list too, so its tokens are not
                // reported a second time as ordinary code.
                return match matching_paren(input, open) {
                    Some(close) => close + 1,
                    None => {
                        self.report(
                            invocation_span,
                            CODE_UNTERMINATED_ARGUMENTS,
                            format!("the argument list of macro `{name}` is not closed"),
                        );
                        input.len()
                    }
                };
            }
            // An object-like macro is invoked by its name alone.
            let after = last + 1;
            if depth >= MAX_DEPTH {
                self.report(
                    invocation_span,
                    CODE_TOO_DEEP,
                    format!("macro expansion is nested more than {MAX_DEPTH} levels deep"),
                );
                out.extend(input[index..after].iter().cloned());
                return after;
            }
            if active.iter().any(|active_name| active_name == &name) {
                self.report(
                    invocation_span,
                    CODE_SELF_RECURSION,
                    format!("macro `{name}` expands into itself"),
                );
                out.extend(input[index..after].iter().cloned());
                return after;
            }
            let mut substituted = self.substitute(&definition, &[], &[], &[], invocation_span);
            self.spread_definition_spans(&mut substituted, invocation_span);
            let mut nested = active.to_vec();
            nested.push(name);
            self.expand_into(&substituted, false, &nested, depth + 1, out);
            return after;
        }

        if !has_argument_list {
            self.report(
                invocation_span,
                CODE_FUNCTION_WITHOUT_ARGUMENTS,
                format!("function-like macro `{name}` needs an argument list"),
            );
            out.push(dollar.clone());
            out.push(input[last].clone());
            return last + 1;
        }
        let Some(close) = matching_paren(input, open) else {
            self.report(
                invocation_span,
                CODE_UNTERMINATED_ARGUMENTS,
                format!("the argument list of macro `{name}` is not closed"),
            );
            return input.len();
        };
        let after = close + 1;
        let raw_arguments = split_arguments(
            input,
            open,
            close,
            // `()` is zero arguments only when the macro declares no
            // parameters; otherwise it is the single empty argument C sees.
            definition.params.is_empty(),
        );
        let fixed = definition.params.len();
        let arity_ok = if definition.variadic {
            raw_arguments.len() >= fixed
        } else {
            raw_arguments.len() == fixed
        };
        if !arity_ok {
            self.report(
                invocation_span,
                CODE_WRONG_ARGUMENT_COUNT,
                arity_message(&name, fixed, definition.variadic, raw_arguments.len()),
            );
            out.extend(input[index..after].iter().cloned());
            return after;
        }
        if depth >= MAX_DEPTH {
            self.report(
                invocation_span,
                CODE_TOO_DEEP,
                format!("macro expansion is nested more than {MAX_DEPTH} levels deep"),
            );
            out.extend(input[index..after].iter().cloned());
            return after;
        }
        if active.iter().any(|active_name| active_name == &name) {
            self.report(
                invocation_span,
                CODE_SELF_RECURSION,
                format!("macro `{name}` expands into itself"),
            );
            out.extend(input[index..after].iter().cloned());
            return after;
        }

        // Argument prescan: every argument is expanded before it is
        // substituted, except that `#` takes it as written. The macro itself is
        // not yet active, so `$F($F(1))` expands the inner call, as in C.
        let mut expanded_arguments: Vec<Vec<Token>> = Vec::with_capacity(fixed);
        for argument in raw_arguments.iter().take(fixed) {
            let mut expanded = Vec::new();
            self.expand_into(argument, false, active, depth + 1, &mut expanded);
            expanded_arguments.push(expanded);
        }
        let mut expanded_variadic: Vec<Vec<Token>> = Vec::new();
        for argument in raw_arguments.iter().skip(fixed) {
            let mut expanded = Vec::new();
            self.expand_into(argument, false, active, depth + 1, &mut expanded);
            expanded_variadic.push(expanded);
        }

        let mut substituted = self.substitute(
            &definition,
            &raw_arguments[..fixed],
            &expanded_arguments,
            &expanded_variadic,
            invocation_span,
        );
        self.spread_definition_spans(&mut substituted, invocation_span);
        let mut nested = active.to_vec();
        nested.push(name);
        self.expand_into(&substituted, false, &nested, depth + 1, out);
        after
    }

    /// Builds the replacement list for one invocation, with arguments
    /// substituted and `#` applied; `##` is left as [`Piece::Paste`] for
    /// [`Preprocessor::fold_pastes`].
    /// Gives the tokens that came from a definition a span of their own.
    ///
    /// Every token of a body is reported at the invocation, which is what makes
    /// a diagnostic point at the `$NAME` the programmer wrote. They cannot all
    /// carry the *same* span, though: the checker records facts by span -- which
    /// definition a name occurrence denotes, what type an expression has, what a
    /// constant folded to -- and two occurrences that share one span cannot be
    /// told apart. Argument text keeps its own span (it is real source), so this
    /// only touches the tokens the definition supplied.
    fn spread_definition_spans(&self, tokens: &mut [Token], invocation: Span) {
        let width = invocation.hi.saturating_sub(invocation.lo).max(1);
        let limit = self.stream_end.max(invocation.hi);
        let mut placed = 0u32;
        for token in tokens.iter_mut() {
            if token.span != invocation {
                continue;
            }
            let offset = placed % width;
            let round = placed / width;
            let lo = invocation.lo + offset;
            let room = limit.saturating_sub(lo).max(1);
            let hi = (lo + 1 + round % room).min(limit).max(lo + 1);
            token.span = Span::new(invocation.file, lo, hi);
            placed += 1;
        }
    }

    fn substitute(
        &mut self,
        definition: &MacroDef,
        raw_arguments: &[Vec<Token>],
        expanded_arguments: &[Vec<Token>],
        expanded_variadic: &[Vec<Token>],
        span: Span,
    ) -> Vec<Token> {
        let mut pieces = Vec::new();
        let mut index = 0;
        while index < definition.body.len() {
            let token = &definition.body[index];
            if token.kind == TokenKind::Punct(Punctuation::Hash) {
                // Validated at definition time: either `##` or `#param`.
                if is_hash(definition.body.get(index + 1)) {
                    pieces.push(Piece::Paste);
                    index += 2;
                    continue;
                }
                if let Some(parameter) = definition
                    .body
                    .get(index + 1)
                    .and_then(ident_name)
                    .and_then(|name| parameter_index(&definition.params, name))
                {
                    pieces.push(Piece::Token(
                        self.stringify(&raw_arguments[parameter], span),
                    ));
                }
                index += 2;
                continue;
            }
            // A parameter name is replaced by its prescanned argument.
            //
            // The argument's tokens keep the spans they were written at: they
            // come from the invocation's own argument list, so their spans are
            // already true, and they are distinct from each other. Retargeting
            // them all at the invocation would give every token in the argument
            // one span, and the checker's records -- which resolve a name by the
            // span of its occurrence -- cannot tell two occurrences apart when
            // they share one.
            if let Some(parameter) =
                ident_name(token).and_then(|name| parameter_index(&definition.params, name))
            {
                for argument_token in &expanded_arguments[parameter] {
                    let mut argument_token = argument_token.clone();
                    argument_token.newline_before = false;
                    pieces.push(Piece::Token(argument_token));
                }
                index += 1;
                continue;
            }
            // `__VA_LIST__` is the variadic arguments, comma-separated, so the
            // body can use it as one argument list.
            if definition.variadic && ident_name(token) == Some(VARIADIC_LIST) {
                for (position, argument) in expanded_variadic.iter().enumerate() {
                    if position > 0 {
                        pieces.push(Piece::Token(comma(span)));
                    }
                    for argument_token in argument {
                        // Argument text keeps its own span, for the reason the
                        // parameter case gives above.
                        let mut argument_token = argument_token.clone();
                        argument_token.newline_before = false;
                        pieces.push(Piece::Token(argument_token));
                    }
                }
                index += 1;
                continue;
            }
            pieces.push(Piece::Token(retarget(token, span)));
            index += 1;
        }
        self.fold_pastes(pieces, span)
    }

    /// Applies every `##` in `pieces`, returning the finished replacement list.
    ///
    /// An empty argument next to `##` acts as a placemarker (C's rule): pasting
    /// it with a token leaves that token alone.
    fn fold_pastes(&mut self, pieces: Vec<Piece>, span: Span) -> Vec<Token> {
        let mut out = Vec::new();
        let mut index = 0;
        while index < pieces.len() {
            match pieces.get(index) {
                Some(Piece::Token(token)) => {
                    out.push(token.clone());
                    index += 1;
                }
                Some(Piece::Paste) => {
                    let left = out.pop();
                    index += 1;
                    // Consecutive paste markers have no operand between them;
                    // they collapse into the same paste.
                    while matches!(pieces.get(index), Some(Piece::Paste)) {
                        index += 1;
                    }
                    let right = match pieces.get(index) {
                        Some(Piece::Token(token)) => {
                            index += 1;
                            Some(token.clone())
                        }
                        _ => None,
                    };
                    if let Some(token) = self.paste(left, right, span) {
                        out.push(token);
                    }
                }
                // Unreachable: the index only moves past pieces that exist.
                None => break,
            }
        }
        out
    }

    /// Pastes two tokens into one, as `##` requires.
    ///
    /// `None` for an operand means an empty argument. Returns `None` and
    /// reports when the two spellings do not re-lex as exactly one token.
    fn paste(&mut self, left: Option<Token>, right: Option<Token>, span: Span) -> Option<Token> {
        match (left, right) {
            (None, None) => None,
            (Some(token), None) | (None, Some(token)) => Some(token),
            (Some(left), Some(right)) => {
                let left_text = left.spelling().unwrap_or_default();
                let right_text = right.spelling().unwrap_or_default();
                let text = format!("{left_text}{right_text}");
                match lex_one(&text) {
                    Some(kind) => Some(Token::new(kind, span).with_text(text)),
                    None => {
                        self.report(
                            span,
                            CODE_INVALID_PASTE,
                            format!(
                                "pasting `{left_text}` and `{right_text}` does not form one token"
                            ),
                        );
                        None
                    }
                }
            }
        }
    }

    /// Stringifies an argument: its tokens joined by single spaces, as a string
    /// literal with `"` and `\` escaped.
    fn stringify(&self, argument: &[Token], span: Span) -> Token {
        let mut text = String::new();
        for (position, token) in argument.iter().enumerate() {
            if position > 0 {
                text.push(' ');
            }
            for character in token.spelling().unwrap_or_default().chars() {
                if character == '"' || character == '\\' {
                    text.push('\\');
                }
                text.push(character);
            }
        }
        Token::new(TokenKind::String, span).with_text(format!("\"{text}\""))
    }
}

/// Returns the identifier a token spells, if it is one.
fn ident_name(token: &Token) -> Option<&str> {
    match token.kind {
        TokenKind::Ident => token.spelling(),
        _ => None,
    }
}

/// Returns the position of `name` in `params`.
fn parameter_index(params: &[String], name: &str) -> Option<usize> {
    params.iter().position(|parameter| parameter == name)
}

/// Returns `true` if `token` is a `#`.
fn is_hash(token: Option<&Token>) -> bool {
    token.is_some_and(|token| token.kind == TokenKind::Punct(Punctuation::Hash))
}

/// Returns a copy of `token` placed at `span`.
///
/// Everything produced by an expansion points at the invocation, so a
/// diagnostic about the result points at the `$NAME` the programmer wrote.
fn retarget(token: &Token, span: Span) -> Token {
    let mut token = token.clone();
    token.span = span;
    token.newline_before = false;
    token
}

/// A comma token at `span`, used to join variadic arguments.
fn comma(span: Span) -> Token {
    Token::new(TokenKind::Punct(Punctuation::Comma), span)
}

/// Skips the rest of a malformed definition's logical line.
///
/// The definition's tokens are removed rather than passed on, so a broken
/// `define` produces one useful diagnostic instead of a cascade from the
/// parser. At least one token is consumed so the caller always progresses.
fn skip_to_line_end(input: &[Token], mut index: usize) -> usize {
    if index < input.len() && !input[index].is_eof() {
        index += 1;
    }
    while index < input.len() && !input[index].is_eof() && !input[index].newline_before {
        index += 1;
    }
    index
}

/// Returns the index of the `)` that closes the `(` at `open`.
///
/// Only parentheses are counted: brackets and braces are ordinary tokens inside
/// an argument list. `None` means the input ends first.
fn matching_paren(input: &[Token], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, token) in input.iter().enumerate().skip(open) {
        match token.kind {
            TokenKind::Punct(Punctuation::OpenParen) => depth += 1,
            TokenKind::Punct(Punctuation::CloseParen) => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            TokenKind::Eof => return None,
            _ => {}
        }
    }
    None
}

/// Splits the tokens between `open` and `close` into arguments.
///
/// `empty_list_is_no_arguments` decides what `()` means: no arguments at all
/// when the macro declares no parameters, and one empty argument otherwise.
fn split_arguments(
    input: &[Token],
    open: usize,
    close: usize,
    empty_list_is_no_arguments: bool,
) -> Vec<Vec<Token>> {
    if open + 1 == close {
        return if empty_list_is_no_arguments {
            Vec::new()
        } else {
            vec![Vec::new()]
        };
    }
    let mut arguments = Vec::new();
    let mut current = Vec::new();
    let mut depth = 0usize;
    for token in input.iter().take(close).skip(open + 1) {
        match token.kind {
            TokenKind::Punct(Punctuation::OpenParen) => {
                depth += 1;
                current.push(token.to_owned());
            }
            TokenKind::Punct(Punctuation::CloseParen) => {
                depth = depth.saturating_sub(1);
                current.push(token.to_owned());
            }
            TokenKind::Punct(Punctuation::Comma) if depth == 0 => {
                arguments.push(std::mem::take(&mut current));
            }
            _ => current.push(token.to_owned()),
        }
    }
    arguments.push(current);
    arguments
}

/// Lexes `text` as exactly one token, returning its kind.
///
/// `None` when the text is empty, lexes as several tokens, needs trivia, or
/// cannot be lexed at all. That is the test `##` must pass: a paste that does
/// not make one token is an error.
fn lex_one(text: &str) -> Option<TokenKind> {
    let mut lexer = Lexer::default();
    let result = lexer.tokenize(text, FileId::SYNTHETIC);
    if !result.diagnostics.is_empty() {
        return None;
    }
    let tokens = result.tokens_without_eof();
    if tokens.len() != 1 {
        return None;
    }
    let token = &tokens[0];
    if token.kind == TokenKind::Unknown {
        return None;
    }
    // The token must account for the whole text: a string literal that stopped
    // at a quote would leave the rest uncovered.
    if token.span.lo != 0 || token.span.hi as usize != text.len() {
        return None;
    }
    Some(token.kind)
}

/// Builds the message for a wrong argument count.
fn arity_message(name: &str, expected: usize, variadic: bool, found: usize) -> String {
    let expected_text = match (expected, variadic) {
        (0, false) => "no arguments".to_owned(),
        (1, false) => "1 argument".to_owned(),
        (count, false) => format!("{count} arguments"),
        (0, true) => "any number of arguments".to_owned(),
        (1, true) => "at least 1 argument".to_owned(),
        (count, true) => format!("at least {count} arguments"),
    };
    let found_text = if found == 1 {
        "1 argument".to_owned()
    } else {
        format!("{found} arguments")
    };
    let verb = if found == 1 { "was" } else { "were" };
    format!("macro `{name}` expects {expected_text}, but {found_text} {verb} given")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arity_messages_read_as_english() {
        assert_eq!(
            arity_message("F", 2, false, 3),
            "macro `F` expects 2 arguments, but 3 arguments were given"
        );
        assert_eq!(
            arity_message("F", 0, false, 1),
            "macro `F` expects no arguments, but 1 argument was given"
        );
        assert_eq!(
            arity_message("F", 1, true, 0),
            "macro `F` expects at least 1 argument, but 0 arguments were given"
        );
    }

    #[test]
    fn a_paste_that_is_one_token_lexes() {
        assert_eq!(lex_one("foobar"), Some(TokenKind::Ident));
        assert_eq!(lex_one("12"), Some(TokenKind::Int));
        assert_eq!(lex_one("++"), Some(TokenKind::Punct(Punctuation::PlusPlus)));
        assert_eq!(lex_one("1x"), None);
        assert_eq!(lex_one("+-"), None);
        assert_eq!(lex_one(""), None);
    }
}
