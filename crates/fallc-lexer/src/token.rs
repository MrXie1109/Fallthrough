//! The token vocabulary of Fallthrough.
//!
//! [`TokenKind`] names what a token is; [`Keyword`] and [`Punctuation`] are
//! tables mapping spellings to variants, which is what lets the scanner
//! recognise a keyword or a mark without a hand-written match. Adding a
//! reserved word or an operator is a table entry plus a variant, and the
//! scanner and parser pick it up.
//!
//! Every keyword in [`KEYWORDS`] is reserved. (`dynamic` and `delete` were
//! only contextual keywords and are therefore absent from the table: the parser
//! recognises them by position.

use fallc_span::Span;

/// A lexical token: a kind, the span it covers, its spelling, and trivia links.
///
/// Trivia (whitespace and comments) is attached to the token *following* it,
/// which keeps the token stream dense while still letting later stages recover
/// formatting. If trivia is disabled in [`crate::LexerConfig`], `leading` is
/// always empty.
///
/// A token carries more than its kind because a kind is `Copy` and cannot name
/// the thing it is: `TokenKind::Ident` says "a name" without saying *which*
/// name. Two pieces of information are therefore stored alongside it:
///
/// * [`Token::text`], the spelling, present exactly when the kind does not
///   imply it. It is also where a macro expansion puts a spelling that has no
///   source text of its own -- a pasted identifier or a stringified argument.
/// * [`Token::newline_before`], whether a line break separates this token from
///   the previous one. The scanner is the only stage that ever sees the
///   whitespace, and a macro definition's body ends at the end of its line.
///
/// Trivia is attached but not classified: one `Trivia` value stands for any run
/// of whitespace and comments, so no later stage can tell a doc comment from a
/// space.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Token {
    /// What kind of token this is.
    pub kind: TokenKind,
    /// The exact source range this token covers.
    pub span: Span,
    /// The token's spelling, when the kind alone does not determine it.
    ///
    /// [`TokenKind::spelling`] names every kind whose spelling is fixed: a
    /// keyword's word and a punctuation mark's characters. Everything else --
    /// a name, a literal, an unrecognised character -- stores its text here.
    /// A macro expansion may also put a *synthesised* spelling here (the result
    /// of `##`, or the string literal produced by `#`), which is why a token's
    /// text is not necessarily the source text covered by its span. Readers
    /// that want "the text this token stands for" should use
    /// [`Token::spelling`].
    pub text: Option<Box<str>>,
    /// Trivia that preceded this token on the same logical line.
    pub leading: Vec<Trivia>,
    /// Whether a line break separates this token from the previous one.
    ///
    /// A backslash-newline is a line *continuation*, not a line break: it joins
    /// two physical lines into one logical line, so it does not set this. That
    /// is what lets a macro definition continue onto the next line while a
    /// definition still ends at the end of its logical line.
    pub newline_before: bool,
}

impl Token {
    /// Creates a token with no leading trivia, no stored spelling, and no line
    /// break before it.
    ///
    /// `text` is left empty because a constructor cannot know it; a token whose
    /// kind does not imply its spelling (an identifier, say) should be given
    /// one with [`Token::with_text`] before the preprocessor sees it.
    #[must_use]
    pub fn new(kind: TokenKind, span: Span) -> Self {
        Token {
            kind,
            span,
            text: None,
            leading: Vec::new(),
            newline_before: false,
        }
    }

    /// Creates a token with the given leading trivia.
    #[must_use]
    pub fn with_trivia(kind: TokenKind, span: Span, leading: Vec<Trivia>) -> Self {
        Token {
            kind,
            span,
            text: None,
            leading,
            newline_before: false,
        }
    }

    /// Returns this token with its spelling stored.
    ///
    /// Used by the scanner for every token whose spelling the kind does not
    /// imply, and by macro expansion for tokens it synthesises.
    #[must_use]
    pub fn with_text(mut self, text: impl Into<Box<str>>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// Returns this token marked as beginning a new logical line (or not).
    #[must_use]
    pub fn with_newline_before(mut self, newline_before: bool) -> Self {
        self.newline_before = newline_before;
        self
    }

    /// Returns the spelling this token stands for, if it is known.
    ///
    /// This prefers [`Token::text`] and falls back to the spelling the kind
    /// implies, so it answers "what would this be written as" for both scanned
    /// and macro-synthesised tokens. `None` only for tokens that have no
    /// spelling at all (end of input, or a hand-built token with neither text
    /// nor a fixed spelling).
    #[must_use]
    pub fn spelling(&self) -> Option<&str> {
        match &self.text {
            Some(text) => Some(text),
            None => self.kind.spelling(),
        }
    }

    /// Returns `true` if this token is [`TokenKind::Eof`].
    #[must_use]
    pub fn is_eof(&self) -> bool {
        matches!(self.kind, TokenKind::Eof)
    }
}

/// Non-semantic text between tokens.
///
/// One variant, because the scanner does not classify trivia: a run of
/// whitespace and comments arrives as a single [`Trivia::Unspecified`]. The
/// type exists so that giving trivia kinds later does not change a signature in
/// this crate.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Trivia {
    /// An unclassified run of trivia.
    Unspecified { span: Span },
}

impl Trivia {
    /// Returns the source range this trivia covers.
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Trivia::Unspecified { span } => *span,
        }
    }
}

/// The kind of a [`Token`].
///
/// A token kind is either an identifier, a string literal, a keyword, or a
/// punctuation mark. Keywords and punctuation are distinct types
/// ([`Keyword`], [`Punctuation`]) because the keyword and punctuation tables
/// map spellings to them, and keeping them separate makes an accidental
/// keyword/operator mix-up a type error.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[non_exhaustive]
pub enum TokenKind {
    /// A name: an identifier.
    Ident,
    /// A string literal.
    ///
    /// The token keeps the source spelling; decoding escapes is `fallc-ast`'s
    /// job. An ordinary literal carries a terminating NUL in its *type*, which
    /// is `[N]const c8` or the width its prefix names.
    String,
    /// A character literal, such as `'x'`.
    ///
    /// Its type is `c8`, or whatever the prefix names: `u16'x'` is a `c16`.
    /// The width prefix is part of the literal, not a separate token, so the
    /// token records only that a character literal was scanned.
    Char,
    /// A string literal that does not end with a zero, such as `b"Hello"`.
    ///
    /// An ordinary string gains a terminator so it can be handed to C; `b`
    /// suppresses that, for a string whose length is the point.
    ByteString,
    /// An interpolated string: `f"..."`.
    ///
    /// The text between the quotes holds placeholders, `{expr}`, whose values
    /// are the arguments of the format call the literal becomes. It is its own
    /// token because the difference is not in the bytes but in how they are
    /// read: every `{` opens an expression, and the text between them is
    /// format text.
    FString,
    /// A raw string literal: `r"..."` or `r#tag"..."tag#`.
    ///
    /// Nothing in it is an escape, so `r"C:\dir"` is the four characters
    /// `C:\dir`, and it may hold a newline. The delimiter form exists for the
    /// text that has to contain a `"`: `r#q"a "quoted" word"q#` is one string
    /// whose delimiter is `q`. The scanner records only that a raw string was
    /// seen; the bytes are the source text between the quotes.
    RawString,
    /// A floating-point literal, such as `1.5` or `2.0f`.
    ///
    /// The suffix selects the type: none means `double`, `f` means `float`.
    Float,
    /// An integer literal, such as `0`, `42`, `0xFF`, or `5u8`.
    ///
    /// The spelling carries the base, any `_` separators, and the type suffix;
    /// `fallc-ast` decodes the value.
    Int,
    /// A reserved word.
    Keyword(Keyword),
    /// A punctuation mark or delimiter.
    Punct(Punctuation),
    /// End of input. Always the last token of a [`crate::LexResult`].
    Eof,
    /// A character that cannot start any token.
    ///
    /// Always accompanied by a diagnostic, and always exactly one character
    /// wide, so the scanner can keep making progress. Distinct from `Eof` so
    /// that the parser can tell "input ended" from "input is malformed".
    Unknown,
}

impl TokenKind {
    /// A short human-readable name, used in diagnostics.
    #[must_use]
    pub const fn describe(self) -> &'static str {
        match self {
            TokenKind::Ident => "identifier",
            TokenKind::String => "string literal",
            TokenKind::Char => "character literal",
            TokenKind::ByteString => "byte-string literal",
            TokenKind::RawString => "raw string literal",
            TokenKind::FString => "interpolated string literal",
            TokenKind::Float => "floating-point literal",
            TokenKind::Int => "integer literal",
            TokenKind::Keyword(k) => k.spelling(),
            TokenKind::Punct(p) => p.spelling(),
            TokenKind::Eof => "end of input",
            TokenKind::Unknown => "unrecognised token",
        }
    }

    /// Returns the kind's fixed source spelling, if it has one.
    ///
    /// A keyword and a punctuation mark are written the same way every time, so
    /// their spelling is a property of the kind and does not have to be stored
    /// on the token. Every other kind -- a name, a literal, an unrecognised
    /// character -- needs its text carried by the token itself; end of input
    /// has no spelling at all.
    #[must_use]
    pub const fn spelling(self) -> Option<&'static str> {
        match self {
            TokenKind::Keyword(keyword) => Some(keyword.spelling()),
            TokenKind::Punct(punct) => Some(punct.spelling()),
            _ => None,
        }
    }

    /// Returns `true` if this kind is a keyword.
    #[must_use]
    pub const fn is_keyword(self) -> bool {
        matches!(self, TokenKind::Keyword(_))
    }

    /// Returns `true` if this kind is a literal.
    #[must_use]
    pub const fn is_literal(self) -> bool {
        matches!(
            self,
            TokenKind::String
                | TokenKind::FString
                | TokenKind::ByteString
                | TokenKind::RawString
                | TokenKind::Int
                | TokenKind::Float
        )
    }

    /// Returns the keyword, if this is one.
    #[must_use]
    pub const fn keyword(self) -> Option<Keyword> {
        match self {
            TokenKind::Keyword(k) => Some(k),
            _ => None,
        }
    }

    /// Returns the punctuation mark, if this is one.
    #[must_use]
    pub const fn punct(self) -> Option<Punctuation> {
        match self {
            TokenKind::Punct(p) => Some(p),
            _ => None,
        }
    }
}

/// A reserved word.
///
/// Every reserved word of the language. `dynamic` and `delete` are contextual
/// rather than reserved, so they are absent: the parser recognises them by
/// position.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[non_exhaustive]
pub enum Keyword {
    /// `as` — explicit type conversion.
    As,
    /// `auto` — deduce this type from the initialiser.
    Auto,
    /// `fn` — function declaration.
    Fn,
    /// `if`
    If,
    /// `else` — also the loop `else` clause.
    Else,
    /// `while`
    While,
    /// `yield` — hand a value to the resumer of a coroutine and continue with
    /// what it sends next.
    Yield,
    /// `for` — both the C-style and the range form.
    For,
    /// `switch`
    Switch,
    /// `this` — the receiver of a member function.
    ///
    /// A reserved word because it is not an ordinary name: a function whose
    /// first parameter is called `this` is a member function, and that
    /// determination must not depend on which names a program happens to use.
    This,
    /// `case`
    Case,
    /// `const`
    Const,
    /// `default`
    Default,
    /// `define` — begins a macro definition at the start of a logical line.
    ///
    /// A reserved word: a macro preprocessor that recognised it only by taking
    /// an identifier would let a program shadow it, and the definition would
    /// stop meaning what it says.
    Define,
    /// `delete` — reserved for a future deletion expression.
    Delete,
    /// `defer` — runs a block when its scope exits.
    Defer,
    /// `enum`
    ///
    /// Declares a named integer type whose values have names: `enum Color int
    /// { RED, GREEN }`. The members are named constants of the type, reached as
    /// `Color.RED`.
    Enum,
    /// `union`
    ///
    /// Declares a type whose fields share one address, so writing one member
    /// and reading another reinterprets the bits. The program decides which
    /// member is live.
    Union,
    /// `traits` — a named set of requirements a type may satisfy.
    ///
    /// Plural because it is read as one: `traits InputIterator(it Iter) { .. }`
    /// declares the traits a type must have, not a single promise it makes.
    Traits,
    /// `break`
    Break,
    /// `continue`
    Continue,
    /// `return`
    Return,
    /// `raise` — also used bare, to re-raise.
    Raise,
    /// `try`
    Try,
    /// `except`
    Except,
    /// `export` — makes a declaration visible to importing modules.
    ///
    /// A reserved word: it introduces a declaration at the top level, and a
    /// program that used it as a name would read like one.
    Export,
    /// `finally`
    Finally,
    /// `true`
    True,
    /// `type`
    Type,
    /// `false`
    False,
    /// `import`
    Import,
    /// `from` — the module a `from M import ..` declaration names.
    From,
    /// `link` — a link instruction.
    Link,
    /// `interface` — a named set of method signatures a type may satisfy.
    ///
    /// Like `traits` in purpose, but its requirements are signatures rather
    /// than expressions: a value of an interface type carries a copy of some
    /// concrete value and a table of the methods that act on it.
    Interface,
    /// `lambda` — an anonymous function.
    ///
    /// Reserved rather than contextual: `lambda(x int) -> x` has to be an
    /// expression wherever an expression may start, and a variable called
    /// `lambda` would make that ambiguous.
    Lambda,
    /// `static_assert` — a check the compiler makes.
    ///
    /// A keyword of its own rather than `static` plus a name: the assertion is
    /// not a declaration and has no relation to static storage.
    StaticAssert,
    /// `nullptr` — the null pointer.
    ///
    /// A keyword rather than a name because it has no type of its own: it
    /// converts to any pointer, so it cannot be a value of some particular
    /// pointer type.
    Nullptr,
    /// `fallthrough` — switch case fall-through.
    Fallthrough,
    /// `template` — the parameter list of a generic function or type.
    Template,
    /// `volatile` — memory that must be read and written exactly as written.
    ///
    /// Every access is a real access: the compiler may not cache the value in a
    /// register, reorder two accesses, or remove one it believes is dead. That
    /// is what a device register or a variable another thread writes needs.
    Volatile,
    /// `mutable` — a field that a `const` member function may still change.
    ///
    /// A property of the *field*, written after its type, because it is the
    /// field that is exceptional rather than its type: the same `int` is
    /// mutable in one type and not in another.
    Mutable,
    /// `sizeof` — how many bytes a type occupies.
    Sizeof,
    /// `alignof` — the alignment a type is required to have.
    Alignof,
}

impl Keyword {
    /// Returns the keyword's source spelling.
    #[must_use]
    pub const fn spelling(self) -> &'static str {
        match self {
            Keyword::As => "as",
            Keyword::Auto => "auto",
            Keyword::Fn => "fn",
            Keyword::If => "if",
            Keyword::Else => "else",
            Keyword::While => "while",
            Keyword::Yield => "yield",
            Keyword::For => "for",
            Keyword::Switch => "switch",
            Keyword::This => "this",
            Keyword::Case => "case",
            Keyword::Const => "const",
            Keyword::Default => "default",
            Keyword::Define => "define",
            Keyword::Delete => "delete",
            Keyword::Defer => "defer",
            Keyword::Enum => "enum",
            Keyword::Union => "union",
            Keyword::Break => "break",
            Keyword::Continue => "continue",
            Keyword::Return => "return",
            Keyword::Raise => "raise",
            Keyword::Try => "try",
            Keyword::Except => "except",
            Keyword::Export => "export",
            Keyword::Finally => "finally",
            Keyword::True => "true",
            Keyword::Type => "type",
            Keyword::False => "false",
            Keyword::Import => "import",
            Keyword::From => "from",
            Keyword::Link => "link",
            Keyword::Interface => "interface",
            Keyword::Lambda => "lambda",
            Keyword::StaticAssert => "static_assert",
            Keyword::Nullptr => "nullptr",
            Keyword::Fallthrough => "fallthrough",
            Keyword::Template => "template",
            Keyword::Traits => "traits",
            Keyword::Volatile => "volatile",
            Keyword::Mutable => "mutable",
            Keyword::Sizeof => "sizeof",
            Keyword::Alignof => "alignof",
        }
    }

    /// Returns `true` if this keyword begins a statement.
    ///
    /// Used by the parser's error recovery to decide where to resynchronise,
    /// which is why it is a property of the keyword rather than a list in the
    /// parser: adding a statement keyword here makes recovery aware of it.
    #[must_use]
    pub const fn starts_statement(self) -> bool {
        matches!(
            self,
            Keyword::If
                | Keyword::While
                | Keyword::For
                | Keyword::Switch
                | Keyword::Fn
                | Keyword::Break
                | Keyword::Continue
                | Keyword::Return
                | Keyword::Raise
                | Keyword::Try
                | Keyword::Fallthrough
        )
    }
}

/// The table of reserved words, sorted by spelling.
///
/// `KEYWORDS` must stay sorted: [`lookup_keyword`] binary-searches it, and a
/// test below checks the ordering so that adding a keyword out of order fails
/// loudly instead of silently failing to lex.
pub static KEYWORDS: &[(&str, Keyword)] = &[
    ("alignof", Keyword::Alignof),
    ("as", Keyword::As),
    ("auto", Keyword::Auto),
    ("break", Keyword::Break),
    ("case", Keyword::Case),
    ("const", Keyword::Const),
    ("continue", Keyword::Continue),
    ("default", Keyword::Default),
    ("defer", Keyword::Defer),
    ("define", Keyword::Define),
    ("delete", Keyword::Delete),
    ("else", Keyword::Else),
    ("enum", Keyword::Enum),
    ("except", Keyword::Except),
    ("export", Keyword::Export),
    ("fallthrough", Keyword::Fallthrough),
    ("false", Keyword::False),
    ("finally", Keyword::Finally),
    ("fn", Keyword::Fn),
    ("for", Keyword::For),
    ("from", Keyword::From),
    ("if", Keyword::If),
    ("import", Keyword::Import),
    ("interface", Keyword::Interface),
    ("lambda", Keyword::Lambda),
    ("link", Keyword::Link),
    ("mutable", Keyword::Mutable),
    ("nullptr", Keyword::Nullptr),
    ("raise", Keyword::Raise),
    ("return", Keyword::Return),
    ("sizeof", Keyword::Sizeof),
    ("static_assert", Keyword::StaticAssert),
    ("switch", Keyword::Switch),
    ("template", Keyword::Template),
    ("this", Keyword::This),
    ("traits", Keyword::Traits),
    ("true", Keyword::True),
    ("try", Keyword::Try),
    ("type", Keyword::Type),
    ("union", Keyword::Union),
    ("volatile", Keyword::Volatile),
    ("while", Keyword::While),
    ("yield", Keyword::Yield),
];

/// Looks up `text` in [`KEYWORDS`].
#[must_use]
pub fn lookup_keyword(text: &str) -> Option<Keyword> {
    KEYWORDS
        .binary_search_by(|(spelling, _)| (*spelling).cmp(text))
        .ok()
        .map(|idx| KEYWORDS[idx].1)
}

/// A punctuation mark, delimiter, or operator.
///
/// Every mark of the language. [`lookup_punctuation`] binary-searches
/// [`PUNCTUATION`], and the scanner takes the longest spelling found there, so
/// `<<=` beats `<<` beats `<`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[non_exhaustive]
pub enum Punctuation {
    /// `(`
    OpenParen,
    /// `)`
    CloseParen,
    /// `{`
    OpenBrace,
    /// `}`
    CloseBrace,
    /// `[`
    OpenBracket,
    /// `]`
    CloseBracket,
    /// `;`
    Semi,
    /// `:`
    Colon,
    /// `,`
    Comma,
    /// `...` — a variadic parameter list.
    DotDotDot,
    /// `..` — a range: `low .. high`, the high end excluded.
    DotDot,
    /// `..=` — a range with the high end included.
    DotDotEq,
    /// `.` — member access, as in `e.type()` and `std.exception()`.
    Dot,
    /// `.{` — the designated-initialiser opener.
    ///
    /// A single token, not `.` followed by `{`, so that `.` stays unambiguous
    /// as member access. Any whitespace between the two characters breaks it
    /// back into two tokens.
    DotBrace,
    /// `$` — the sigil before an invocation of a macro.
    Dollar,
    /// `@` — the sigil before an attribute.
    At,
    /// `#` — the sigil inside a macro body: `#param` stringifies, `a ## b`
    /// pastes. Outside a macro body it begins nothing.
    Hash,

    // ---- assignment ----
    /// `=`
    Eq,
    /// `+=`
    PlusEq,
    /// `-=`
    MinusEq,
    /// `*=`
    StarEq,
    /// `/=`
    SlashEq,
    /// `%=`
    PercentEq,
    /// `&=`
    AmpEq,
    /// `|=`
    PipeEq,
    /// `^=`
    CaretEq,
    /// `<<=`
    ShlEq,
    /// `>>=`
    ShrEq,

    // ---- arithmetic ----
    /// `+`
    Plus,
    /// `-`
    Minus,
    /// `->` — member access through a pointer.
    Arrow,
    /// `++`
    PlusPlus,
    /// `--`
    MinusMinus,
    /// `*`
    Star,
    /// `**` — exponentiation.
    StarStar,
    /// `**=` — exponentiation in place.
    StarStarEq,
    /// `/`
    Slash,
    /// `%`
    Percent,

    // ---- bitwise ----
    /// `&`
    Amp,
    /// `|`
    Pipe,
    /// `^`
    Caret,
    /// `~`
    Tilde,
    /// `<<`
    Shl,
    /// `>>`
    Shr,

    // ---- logical ----
    /// `&&`
    AmpAmp,
    /// `||`
    PipePipe,
    /// `!`
    Bang,

    // ---- comparison ----
    /// `==`
    EqEq,
    /// `!=`
    BangEq,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `<=>` — the three-way comparison, which `__cmp__` implements.
    Cmp,
    /// `>`
    Gt,
    /// `>=`
    Ge,

    // ---- conditional ----
    /// `?` — the condition of `a ? b : c`.
    Question,
}

impl Punctuation {
    /// Returns the mark's source spelling.
    #[must_use]
    pub const fn spelling(self) -> &'static str {
        match self {
            Punctuation::OpenParen => "(",
            Punctuation::CloseParen => ")",
            Punctuation::OpenBrace => "{",
            Punctuation::CloseBrace => "}",
            Punctuation::OpenBracket => "[",
            Punctuation::CloseBracket => "]",
            Punctuation::Semi => ";",
            Punctuation::Colon => ":",
            Punctuation::Comma => ",",
            Punctuation::Dot => ".",
            Punctuation::DotDotDot => "...",
            Punctuation::DotDot => "..",
            Punctuation::DotDotEq => "..=",
            Punctuation::DotBrace => ".{",
            Punctuation::Dollar => "$",
            Punctuation::At => "@",
            Punctuation::Hash => "#",
            Punctuation::Eq => "=",
            Punctuation::PlusEq => "+=",
            Punctuation::MinusEq => "-=",
            Punctuation::StarEq => "*=",
            Punctuation::SlashEq => "/=",
            Punctuation::PercentEq => "%=",
            Punctuation::AmpEq => "&=",
            Punctuation::PipeEq => "|=",
            Punctuation::CaretEq => "^=",
            Punctuation::ShlEq => "<<=",
            Punctuation::ShrEq => ">>=",
            Punctuation::Plus => "+",
            Punctuation::Minus => "-",
            Punctuation::Arrow => "->",
            Punctuation::PlusPlus => "++",
            Punctuation::MinusMinus => "--",
            Punctuation::Star => "*",
            Punctuation::StarStar => "**",
            Punctuation::StarStarEq => "**=",
            Punctuation::Slash => "/",
            Punctuation::Percent => "%",
            Punctuation::Amp => "&",
            Punctuation::Pipe => "|",
            Punctuation::Caret => "^",
            Punctuation::Tilde => "~",
            Punctuation::Shl => "<<",
            Punctuation::Shr => ">>",
            Punctuation::AmpAmp => "&&",
            Punctuation::PipePipe => "||",
            Punctuation::Bang => "!",
            Punctuation::EqEq => "==",
            Punctuation::BangEq => "!=",
            Punctuation::Lt => "<",
            Punctuation::Le => "<=",
            Punctuation::Cmp => "<=>",
            Punctuation::Gt => ">",
            Punctuation::Ge => ">=",
            Punctuation::Question => "?",
        }
    }

    /// Returns the matching close delimiter, if this is an opening one.
    #[must_use]
    pub const fn closing(self) -> Option<Punctuation> {
        match self {
            Punctuation::OpenParen => Some(Punctuation::CloseParen),
            Punctuation::OpenBrace => Some(Punctuation::CloseBrace),
            Punctuation::OpenBracket => Some(Punctuation::CloseBracket),
            _ => None,
        }
    }

    /// Returns `true` if this is an assignment operator.
    ///
    /// Compound assignments such as `+=` are assignments too: they need an
    /// lvalue, not just a value.
    #[must_use]
    pub const fn is_assignment(self) -> bool {
        matches!(
            self,
            Punctuation::Eq
                | Punctuation::PlusEq
                | Punctuation::MinusEq
                | Punctuation::StarEq
                | Punctuation::SlashEq
                | Punctuation::PercentEq
                | Punctuation::AmpEq
                | Punctuation::PipeEq
                | Punctuation::CaretEq
                | Punctuation::ShlEq
                | Punctuation::ShrEq
                | Punctuation::StarStarEq
        )
    }

    /// For a compound assignment, returns the operator it applies.
    ///
    /// `None` for plain `=`, which applies no operator. This is what lets one
    /// assignment node handle both `x = y` and `x += y`.
    #[must_use]
    pub const fn compound_base(self) -> Option<Punctuation> {
        Some(match self {
            Punctuation::PlusEq => Punctuation::Plus,
            Punctuation::MinusEq => Punctuation::Minus,
            Punctuation::StarEq => Punctuation::Star,
            Punctuation::SlashEq => Punctuation::Slash,
            Punctuation::PercentEq => Punctuation::Percent,
            Punctuation::AmpEq => Punctuation::Amp,
            Punctuation::PipeEq => Punctuation::Pipe,
            Punctuation::CaretEq => Punctuation::Caret,
            Punctuation::ShlEq => Punctuation::Shl,
            Punctuation::ShrEq => Punctuation::Shr,
            Punctuation::StarStarEq => Punctuation::StarStar,
            _ => return None,
        })
    }
}

/// The table of punctuation spellings, sorted by spelling.
///
/// Sorted for the same reason as [`KEYWORDS`], and checked by a test.
pub static PUNCTUATION: &[(&str, Punctuation)] = &[
    ("!", Punctuation::Bang),
    ("!=", Punctuation::BangEq),
    ("#", Punctuation::Hash),
    ("$", Punctuation::Dollar),
    ("%", Punctuation::Percent),
    ("%=", Punctuation::PercentEq),
    ("&", Punctuation::Amp),
    ("&&", Punctuation::AmpAmp),
    ("&=", Punctuation::AmpEq),
    ("(", Punctuation::OpenParen),
    (")", Punctuation::CloseParen),
    ("*", Punctuation::Star),
    ("**", Punctuation::StarStar),
    ("**=", Punctuation::StarStarEq),
    ("*=", Punctuation::StarEq),
    ("+", Punctuation::Plus),
    ("++", Punctuation::PlusPlus),
    ("+=", Punctuation::PlusEq),
    (",", Punctuation::Comma),
    ("-", Punctuation::Minus),
    ("--", Punctuation::MinusMinus),
    ("-=", Punctuation::MinusEq),
    ("->", Punctuation::Arrow),
    (".", Punctuation::Dot),
    ("..", Punctuation::DotDot),
    ("...", Punctuation::DotDotDot),
    ("..=", Punctuation::DotDotEq),
    (".{", Punctuation::DotBrace),
    ("/", Punctuation::Slash),
    ("/=", Punctuation::SlashEq),
    (":", Punctuation::Colon),
    (";", Punctuation::Semi),
    ("<", Punctuation::Lt),
    ("<<", Punctuation::Shl),
    ("<<=", Punctuation::ShlEq),
    ("<=", Punctuation::Le),
    ("<=>", Punctuation::Cmp),
    ("=", Punctuation::Eq),
    ("==", Punctuation::EqEq),
    (">", Punctuation::Gt),
    (">=", Punctuation::Ge),
    (">>", Punctuation::Shr),
    (">>=", Punctuation::ShrEq),
    ("?", Punctuation::Question),
    ("@", Punctuation::At),
    ("[", Punctuation::OpenBracket),
    ("]", Punctuation::CloseBracket),
    ("^", Punctuation::Caret),
    ("^=", Punctuation::CaretEq),
    ("{", Punctuation::OpenBrace),
    ("|", Punctuation::Pipe),
    ("|=", Punctuation::PipeEq),
    ("||", Punctuation::PipePipe),
    ("}", Punctuation::CloseBrace),
    ("~", Punctuation::Tilde),
];

/// Looks up `text` in [`PUNCTUATION`].
#[must_use]
pub fn lookup_punctuation(text: &str) -> Option<Punctuation> {
    PUNCTUATION
        .binary_search_by(|(spelling, _)| (*spelling).cmp(text))
        .ok()
        .map(|idx| PUNCTUATION[idx].1)
}

/// A bracket-like delimiter pair.
///
/// Uninhabited: the parser matches brackets on [`Punctuation`] directly, so
/// nothing constructs a `Delimiter`. It is retained so that introducing one
/// would not change a signature in this crate.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[non_exhaustive]
pub enum Delimiter {}

#[cfg(test)]
mod tests {
    use super::*;
    use fallc_span::FileId;

    #[test]
    fn unknown_spellings_are_not_found() {
        assert_eq!(lookup_keyword("let"), None);
        assert_eq!(lookup_punctuation("`"), None);
    }

    #[test]
    fn eof_is_recognised() {
        let tok = Token::new(TokenKind::Eof, Span::point(FileId::from_raw(0), 0));
        assert!(tok.is_eof());
        assert!(!TokenKind::Unknown.is_keyword());
    }

    #[test]
    fn a_spelling_comes_from_the_kind_or_from_the_token() {
        let span = Span::point(FileId::from_raw(0), 0);
        // Punctuation and keywords know their spelling...
        let semi = Token::new(TokenKind::Punct(Punctuation::Semi), span);
        assert_eq!(semi.text, None);
        assert_eq!(semi.spelling(), Some(";"));
        let conditional = Token::new(TokenKind::Keyword(Keyword::If), span);
        assert_eq!(conditional.spelling(), Some("if"));
        // ...while a name has to carry it.
        let name = Token::new(TokenKind::Ident, span);
        assert_eq!(name.spelling(), None);
        assert!(!name.newline_before);
        assert_eq!(name.with_text("x").spelling(), Some("x"));
        // End of input stands for nothing writable.
        assert_eq!(Token::new(TokenKind::Eof, span).spelling(), None);
    }

    #[test]
    fn every_keyword_is_in_the_table() {
        // A keyword the table omits lexes as an identifier, so `fallthrough;`
        // became a name reference and failed to resolve. The table is generated
        // from this list, so a missing entry is a real omission rather than a
        // sorting mistake.
        let every_keyword = [
            Keyword::As,
            Keyword::Auto,
            Keyword::Break,
            Keyword::Alignof,
            Keyword::Case,
            Keyword::Const,
            Keyword::Continue,
            Keyword::Default,
            Keyword::Else,
            Keyword::Except,
            Keyword::Fallthrough,
            Keyword::False,
            Keyword::Finally,
            Keyword::Fn,
            Keyword::For,
            Keyword::If,
            Keyword::Import,
            Keyword::Raise,
            Keyword::Return,
            Keyword::From,
            Keyword::Link,
            Keyword::Lambda,
            Keyword::Interface,
            Keyword::StaticAssert,
            Keyword::Nullptr,
            Keyword::Switch,
            Keyword::This,
            Keyword::True,
            Keyword::Type,
            Keyword::Try,
            Keyword::While,
            Keyword::Yield,
            Keyword::Template,
            Keyword::Volatile,
            Keyword::Mutable,
            Keyword::Sizeof,
            Keyword::Enum,
            Keyword::Union,
            Keyword::Traits,
        ];
        for keyword in every_keyword {
            assert_eq!(
                lookup_keyword(keyword.spelling()),
                Some(keyword),
                "`{}` is missing from KEYWORDS",
                keyword.spelling()
            );
        }
        assert_eq!(
            KEYWORDS.len(),
            every_keyword.len(),
            "the table covers every keyword"
        );
    }

    #[test]
    fn keyword_table_is_sorted_and_consistent() {
        // `lookup_keyword` binary-searches, so an out-of-order entry silently
        // stops lexing. Check the ordering and the round trip.
        for pair in KEYWORDS.windows(2) {
            assert!(
                pair[0].0 < pair[1].0,
                "KEYWORDS is not sorted: {:?} then {:?}",
                pair[0].0,
                pair[1].0
            );
        }
        for &(spelling, keyword) in KEYWORDS {
            assert_eq!(lookup_keyword(spelling), Some(keyword));
            assert_eq!(keyword.spelling(), spelling);
        }
    }

    #[test]
    fn punctuation_table_is_sorted_and_consistent() {
        // This test exists because the table was once written in visual rather
        // than ASCII order, which made `(` and `)` stop lexing.
        for pair in PUNCTUATION.windows(2) {
            assert!(
                pair[0].0 < pair[1].0,
                "PUNCTUATION is not sorted: {:?} then {:?}",
                pair[0].0,
                pair[1].0
            );
        }
        for &(spelling, punct) in PUNCTUATION {
            assert_eq!(lookup_punctuation(spelling), Some(punct));
            assert_eq!(punct.spelling(), spelling);
        }
    }

    #[test]
    fn delimiters_pair_correctly() {
        assert_eq!(
            Punctuation::OpenBrace.closing(),
            Some(Punctuation::CloseBrace)
        );
        assert_eq!(
            Punctuation::OpenParen.closing(),
            Some(Punctuation::CloseParen)
        );
        assert_eq!(Punctuation::Semi.closing(), None);
        for &(_, punct) in PUNCTUATION {
            if let Some(close) = punct.closing() {
                assert_eq!(close.closing(), None, "{close:?} must not open");
            }
        }
    }
}
