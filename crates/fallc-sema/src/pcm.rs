//! Reading and writing a module's export table as bytes.
//!
//! A module is parsed, resolved, and its declarations checked every time it is
//! imported. That work produces one thing an importer actually needs: the
//! [`ExternalModule`] table. A `.pcm` file is that table, written down, so a
//! later compilation can read it instead of repeating the three passes.
//!
//! # Why a format of our own
//!
//! Three properties matter, and a self-describing binary format buys all three
//! at once:
//!
//! - **No new dependency.** [`fallc_sema`] is the language's front end; it
//!   depends on the AST and nothing else today, and a serialisation crate would
//!   be its first dependency that is not part of the language.
//! - **The bytes are reproducible.** Every map is written in key order, so the
//!   same module in the same source produces the same file, byte for byte. Two
//!   builds can compare files instead of rebuilding to see whether a module
//!   changed, and the cache key is the content rather than a timestamp.
//! - **The format is inspectable.** A dumped `.pcm` names its fields and their
//!   values in order, so a disagreement between what a module exports and what
//!   an importer sees is read out of the file rather than guessed at.
//!
//! # What is *not* written
//!
//! Types are written as text, not as structures. `TypeSyntax` is an arena of
//! nodes whose ids are meaningful only inside one parse, and `Type` carries
//! spans that point into one file's tree. Writing either as-is would produce a
//! file that only the compilation that wrote it could read.
//!
//! The text is the type as the language spells it -- `*const std.String` -- and
//! reading it back runs the same resolution an importer runs for a type it
//! wrote itself. A `.pcm` therefore says what the declaring module *declared*,
//! which is exactly what an interface is, and the importer derives the same
//! answers from it as it would from the source.

use std::collections::{HashMap, HashSet};

use crate::resolve::{
    ExportEnum, ExportInterface, ExportInterfaceMethod, ExportType, ExportedField, ExportedGlobal,
    ExportedSpecialisation, ExportedVariable, ExternalModule, FunctionSignature, ResolvedSignature,
};
use crate::template::{Arguments, TemplateArgument};
use crate::types::{CharType, FloatType, IntType, NominalType, Type, UnspecifiedType};

/// Raised when a byte string is not a well-formed module table.
///
/// The caller's answer is always the same -- ignore the file and compile the
/// module from its source -- so the variants exist to make a corrupt cache
/// diagnosable rather than to be handled differently.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum DecodeError {
    /// The bytes ran out in the middle of a value.
    Truncated,
    /// A tag byte named no variant.
    UnknownTag {
        /// What the tag was read as.
        what: &'static str,
        /// The byte that was found.
        tag: u8,
    },
    /// A string was not UTF-8, or a list claimed a length the file cannot hold.
    Malformed(&'static str),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Truncated => write!(out, "the module table ends in the middle of a value"),
            Self::UnknownTag { what, tag } => {
                write!(out, "unknown {what} tag {tag}")
            }
            Self::Malformed(what) => write!(out, "malformed {what}"),
        }
    }
}

/// Writes values out in a fixed order.
///
/// A thin wrapper rather than a trait so that the order is read from the code
/// that calls it: `write_str(field)`, then `write_u32(count)`, and the reader
/// calls the same sequence.
#[derive(Default, Debug)]
pub struct Writer {
    bytes: Vec<u8>,
}

impl Writer {
    /// Starts an empty writer.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the bytes written so far.
    #[must_use]
    pub fn finish(self) -> Vec<u8> {
        self.bytes
    }

    /// Writes one byte.
    pub fn u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    /// Writes a `bool` as one byte.
    pub fn bool(&mut self, value: bool) {
        self.bytes.push(u8::from(value));
    }

    /// Writes an unsigned count.
    ///
    /// A length is at most a file's worth of items, so four bytes is enough and
    /// a variable-length encoding would only make the reader harder to follow.
    pub fn u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    /// Writes a `u64`.
    pub fn u64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    /// Writes a `usize`, as a `u32`: every list in a module table is far below
    /// four billion entries.
    pub fn count(&mut self, value: usize) {
        self.u32(u32::try_from(value).unwrap_or(u32::MAX));
    }

    /// Writes a signed number.
    pub fn i64(&mut self, value: i64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    /// Writes a UTF-8 string with its length.
    pub fn str(&mut self, value: &str) {
        self.count(value.len());
        self.bytes.extend_from_slice(value.as_bytes());
    }

    /// Writes an optional value as a tag and the value.
    pub fn opt<T>(&mut self, value: Option<&T>, write: impl FnOnce(&mut Self, &T)) {
        match value {
            Some(value) => {
                self.u8(1);
                write(self, value);
            }
            None => self.u8(0),
        }
    }

    /// Writes a list as its length and then its elements.
    pub fn list<T>(&mut self, values: &[T], write: impl Fn(&mut Self, &T)) {
        self.count(values.len());
        for value in values {
            write(self, value);
        }
    }
}

/// Reads values written by [`Writer`], in the same order.
#[derive(Debug)]
pub struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    /// Starts reading a byte string.
    #[must_use]
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    /// Whether every byte has been consumed.
    ///
    /// Checked after reading a table: a file with bytes left over was written
    /// by a different version of this code, and reading a prefix of it would
    /// silently drop whatever changed.
    #[must_use]
    pub fn at_end(&self) -> bool {
        self.at == self.bytes.len()
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], DecodeError> {
        let end = self.at.checked_add(count).ok_or(DecodeError::Truncated)?;
        if end > self.bytes.len() {
            return Err(DecodeError::Truncated);
        }
        let slice = &self.bytes[self.at..end];
        self.at = end;
        Ok(slice)
    }

    /// Reads one byte.
    pub fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take(1)?[0])
    }

    /// Reads a `bool`.
    pub fn bool(&mut self) -> Result<bool, DecodeError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            tag => Err(DecodeError::UnknownTag { what: "bool", tag }),
        }
    }

    /// Reads an unsigned count.
    pub fn u32(&mut self) -> Result<u32, DecodeError> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// Reads a `u64`.
    pub fn u64(&mut self) -> Result<u64, DecodeError> {
        let bytes = self.take(8)?;
        let mut array = [0u8; 8];
        array.copy_from_slice(bytes);
        Ok(u64::from_le_bytes(array))
    }

    /// Reads a count, refusing one larger than the bytes can possibly hold.
    ///
    /// The bound is what stops a corrupt length from becoming a request for
    /// gigabytes: every element takes at least one byte, so a list cannot be
    /// longer than the file.
    pub fn count(&mut self) -> Result<usize, DecodeError> {
        let count = self.u32()? as usize;
        if count > self.bytes.len() {
            return Err(DecodeError::Malformed("list length"));
        }
        Ok(count)
    }

    /// Reads a signed number.
    pub fn i64(&mut self) -> Result<i64, DecodeError> {
        let bytes = self.take(8)?;
        let mut array = [0u8; 8];
        array.copy_from_slice(bytes);
        Ok(i64::from_le_bytes(array))
    }

    /// Reads a UTF-8 string.
    pub fn str(&mut self) -> Result<String, DecodeError> {
        let len = self.count()?;
        let bytes = self.take(len)?;
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| DecodeError::Malformed("string"))
    }

    /// Reads a raw run of bytes, for a section this crate does not interpret.
    ///
    /// # Errors
    ///
    /// Truncated when fewer than `count` bytes remain.
    pub fn take_bytes(&mut self, count: usize) -> Result<&'a [u8], DecodeError> {
        self.take(count)
    }

    /// Reads an optional value.
    pub fn opt<T>(
        &mut self,
        read: impl FnOnce(&mut Self) -> Result<T, DecodeError>,
    ) -> Result<Option<T>, DecodeError> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(read(self)?)),
            tag => Err(DecodeError::UnknownTag {
                what: "option",
                tag,
            }),
        }
    }

    /// Reads a list.
    pub fn list<T>(
        &mut self,
        mut read: impl FnMut(&mut Self) -> Result<T, DecodeError>,
    ) -> Result<Vec<T>, DecodeError> {
        let count = self.count()?;
        let mut values = Vec::with_capacity(count.min(1024));
        for _ in 0..count {
            values.push(read(self)?);
        }
        Ok(values)
    }
}

// --- types ---------------------------------------------------------------

/// Writes a type as the language spells it.
///
/// Spans are dropped: where a type was written is a property of one file's tree,
/// and the reader has no tree. What crosses is the name and the shape, which is
/// what the importer resolves.
fn write_type(out: &mut Writer, ty: &Type) {
    match ty {
        Type::Int(int) => {
            out.u8(0);
            out.u8(int_tag(*int));
        }
        Type::Float(float) => {
            out.u8(1);
            out.u8(float_tag(*float));
        }
        Type::Character(character) => {
            out.u8(2);
            out.u8(char_tag(*character));
        }
        Type::Byte => out.u8(3),
        Type::Bool => out.u8(4),
        Type::String => out.u8(5),
        Type::NullPointer => out.u8(6),
        Type::Nominal(nominal) => {
            out.u8(7);
            out.str(&nominal.name);
            out.opt(nominal.declared_in.as_ref(), |out, module| out.str(module));
            out.list(&nominal.fields, |out, (name, ty)| {
                out.str(name);
                write_type(out, ty);
            });
            out.list(&nominal.mutable_fields, |out, name| out.str(name));
            out.list(&nominal.field_alignments, |out, (name, align)| {
                out.str(name);
                out.u64(*align);
            });
            out.bool(nominal.is_union);
            out.bool(nominal.is_incomplete);
            out.opt(nominal.union_storage.as_ref(), |out, index| {
                out.count(*index);
            });
            match &nominal.template_arguments {
                Some((name, arguments)) => {
                    out.u8(1);
                    out.str(name);
                    write_arguments(out, arguments);
                }
                None => out.u8(0),
            }
        }
        Type::Pointer {
            pointee,
            pointee_is_const,
            pointer_is_const,
        } => {
            out.u8(8);
            out.bool(*pointee_is_const);
            out.bool(*pointer_is_const);
            write_type(out, pointee);
        }
        Type::Reference {
            referent,
            referent_is_const,
            is_rvalue,
        } => {
            out.u8(9);
            out.bool(*referent_is_const);
            out.bool(*is_rvalue);
            write_type(out, referent);
        }
        Type::Array { element, len } => {
            out.u8(10);
            out.u64(*len);
            write_type(out, element);
        }
        Type::Const(inner) => {
            out.u8(11);
            write_type(out, inner);
        }
        Type::Volatile(inner) => {
            out.u8(12);
            write_type(out, inner);
        }
        Type::UnsizedArray(inner) => {
            out.u8(13);
            write_type(out, inner);
        }
        Type::Function {
            parameters,
            returns,
            abi,
            variadic,
            throws,
        } => {
            out.u8(14);
            out.list(parameters, write_type);
            write_type(out, returns);
            out.opt(abi.as_ref(), |out, abi| out.str(abi));
            out.bool(*variadic);
            out.bool(*throws);
        }
        Type::Unspecified(unspecified) => {
            out.u8(15);
            out.str(&unspecified.text);
        }
    }
}

fn read_type(input: &mut Reader<'_>, span: fallc_span::Span) -> Result<Type, DecodeError> {
    Ok(match input.u8()? {
        0 => Type::Int(int_from_tag(input.u8()?)?),
        1 => Type::Float(float_from_tag(input.u8()?)?),
        2 => Type::Character(char_from_tag(input.u8()?)?),
        3 => Type::Byte,
        4 => Type::Bool,
        5 => Type::String,
        6 => Type::NullPointer,
        7 => {
            let name = input.str()?;
            let declared_in = input.opt(Reader::str)?;
            let fields = input.list(|input| {
                let name = input.str()?;
                Ok((name, read_type(input, span)?))
            })?;
            let mutable_fields = input.list(Reader::str)?;
            let field_alignments = input.list(|input| {
                let name = input.str()?;
                Ok((name, input.u64()?))
            })?;
            let is_union = input.bool()?;
            let is_incomplete = input.bool()?;
            let union_storage = input.opt(Reader::count)?;
            let template_arguments = input
                .opt(|input| {
                    let name = input.str()?;
                    Ok((name, read_arguments(input, span)?))
                })?
                .map(|(name, arguments)| (name, arguments));
            Type::Nominal(NominalType {
                name,
                declared_in,
                fields,
                mutable_fields,
                field_alignments,
                is_union,
                is_incomplete,
                union_storage,
                template_arguments,
            })
        }
        8 => {
            let pointee_is_const = input.bool()?;
            let pointer_is_const = input.bool()?;
            Type::Pointer {
                pointee: Box::new(read_type(input, span)?),
                pointee_is_const,
                pointer_is_const,
            }
        }
        9 => {
            let referent_is_const = input.bool()?;
            let is_rvalue = input.bool()?;
            Type::Reference {
                referent: Box::new(read_type(input, span)?),
                referent_is_const,
                is_rvalue,
            }
        }
        10 => {
            let len = input.u64()?;
            Type::Array {
                element: Box::new(read_type(input, span)?),
                len,
            }
        }
        11 => Type::Const(Box::new(read_type(input, span)?)),
        12 => Type::Volatile(Box::new(read_type(input, span)?)),
        13 => Type::UnsizedArray(Box::new(read_type(input, span)?)),
        14 => {
            let parameters = input.list(|input| read_type(input, span))?;
            let returns = Box::new(read_type(input, span)?);
            let abi = input.opt(Reader::str)?;
            let variadic = input.bool()?;
            let throws = input.bool()?;
            Type::Function {
                parameters,
                returns,
                abi,
                variadic,
                throws,
            }
        }
        15 => Type::Unspecified(UnspecifiedType {
            text: input.str()?,
            span,
        }),
        tag => {
            return Err(DecodeError::UnknownTag { what: "type", tag });
        }
    })
}

// The integer, float and character types are enum variants with a fixed order,
// so each is written as its index. The `.len()` bound in `*_from_tag` is what
// keeps a corrupt byte from producing a variant that does not exist -- the
// enums have no catch-all, so the tag is checked against the list of spellings
// the language has.

const INTS: &[IntType] = &[
    IntType::I8,
    IntType::I16,
    IntType::I32,
    IntType::I64,
    IntType::I128,
    IntType::U8,
    IntType::U16,
    IntType::U32,
    IntType::U64,
    IntType::U128,
];

const FLOATS: &[FloatType] = &[FloatType::Single, FloatType::Double];

const CHARS: &[CharType] = &[CharType::C8, CharType::C16, CharType::C32];

fn int_tag(value: IntType) -> u8 {
    INTS.iter()
        .position(|candidate| *candidate == value)
        .map_or(0, |index| index as u8)
}

fn int_from_tag(tag: u8) -> Result<IntType, DecodeError> {
    INTS.get(tag as usize)
        .copied()
        .ok_or(DecodeError::UnknownTag {
            what: "int type",
            tag,
        })
}

fn float_tag(value: FloatType) -> u8 {
    FLOATS
        .iter()
        .position(|candidate| *candidate == value)
        .map_or(0, |index| index as u8)
}

fn float_from_tag(tag: u8) -> Result<FloatType, DecodeError> {
    FLOATS
        .get(tag as usize)
        .copied()
        .ok_or(DecodeError::UnknownTag {
            what: "float type",
            tag,
        })
}

fn char_tag(value: CharType) -> u8 {
    CHARS
        .iter()
        .position(|candidate| *candidate == value)
        .map_or(0, |index| index as u8)
}

fn char_from_tag(tag: u8) -> Result<CharType, DecodeError> {
    CHARS
        .get(tag as usize)
        .copied()
        .ok_or(DecodeError::UnknownTag {
            what: "char type",
            tag,
        })
}

// --- template arguments --------------------------------------------------

fn write_arguments(out: &mut Writer, arguments: &Arguments) {
    out.list(&arguments.0, write_argument);
}

fn read_arguments(
    input: &mut Reader<'_>,
    span: fallc_span::Span,
) -> Result<Arguments, DecodeError> {
    Ok(Arguments(input.list(|input| read_argument(input, span))?))
}

fn write_argument(out: &mut Writer, argument: &TemplateArgument) {
    match argument {
        TemplateArgument::Type(ty) => {
            out.u8(0);
            write_type(out, ty);
        }
        TemplateArgument::Int(value) => {
            out.u8(1);
            out.i64(*value);
        }
        TemplateArgument::Text(bytes) => {
            out.u8(2);
            out.count(bytes.len());
            out.list(bytes, |out, byte| out.u8(*byte));
        }
        TemplateArgument::Pack(elements) => {
            out.u8(3);
            out.list(elements, write_argument);
        }
    }
}

fn read_argument(
    input: &mut Reader<'_>,
    span: fallc_span::Span,
) -> Result<TemplateArgument, DecodeError> {
    Ok(match input.u8()? {
        0 => TemplateArgument::Type(read_type(input, span)?),
        1 => TemplateArgument::Int(input.i64()?),
        2 => {
            let len = input.count()?;
            TemplateArgument::Text(input.take(len)?.to_vec())
        }
        3 => TemplateArgument::Pack(input.list(|input| read_argument(input, span))?),
        tag => {
            return Err(DecodeError::UnknownTag {
                what: "template argument",
                tag,
            });
        }
    })
}

// --- written types, as text ----------------------------------------------

/// Writes a written type as text.
///
/// The alternative -- storing the syntax tree -- would write node ids that mean
/// nothing outside the parse that produced them. Text is the same information
/// in a form the reader can parse back, and it is what an interface has always
/// been (`crate::interface`).
fn write_syntax(out: &mut Writer, ty: &fallc_ast::TypeSyntax) {
    out.str(&syntax_text(ty));
}

fn read_syntax(
    input: &mut Reader<'_>,
    parse: SyntaxParser,
) -> Result<fallc_ast::TypeSyntax, DecodeError> {
    let text = input.str()?;
    Ok(parse(&text))
}

/// Renders a written type back to source text.
///
/// Every form the language has, with spans dropped: the result parses to a
/// `TypeSyntax` of the same shape, which is the property the reader relies on.
#[must_use]
pub fn syntax_text(ty: &fallc_ast::TypeSyntax) -> String {
    syntax_text_inner(ty)
}

fn syntax_text_inner(ty: &fallc_ast::TypeSyntax) -> String {
    use fallc_ast::TypeSyntax;
    match ty {
        TypeSyntax::Named(named) => {
            let mut out = named.path.clone();
            if !named.arguments.is_empty() {
                let rendered: Vec<String> = named
                    .arguments
                    .iter()
                    .map(|argument| match argument {
                        fallc_ast::TemplateArgumentSyntax::Type(ty) => syntax_text_inner(ty),
                        // A value argument is an expression in the declaring
                        // module's tree. `TypeSyntax` keeps its id, not its
                        // text, so a written `[N]T` where `N` is a name loses
                        // that name here -- which is why a field whose length is
                        // an expression travels *resolved* as well
                        // (`ExportedField::resolved`).
                        fallc_ast::TemplateArgumentSyntax::Value(expr) => expr.to_string(),
                        fallc_ast::TemplateArgumentSyntax::Named { path, .. } => path.clone(),
                    })
                    .collect();
                out.push('<');
                out.push_str(&rendered.join(", "));
                out.push('>');
            }
            if named.is_expansion {
                out.push_str("...");
            }
            out
        }
        TypeSyntax::Array(array) => {
            format!("[{}]{}", array.len, syntax_text_inner(&array.element))
        }
        TypeSyntax::UnsizedArray(inner) => format!("[]{}", syntax_text_inner(inner)),
        TypeSyntax::Pointer(inner) => format!("*{}", syntax_text_inner(inner)),
        TypeSyntax::PointerToConst(inner) => format!("*const {}", syntax_text_inner(inner)),
        TypeSyntax::ConstPointer(inner) => format!("const *{}", syntax_text_inner(inner)),
        TypeSyntax::Reference(inner) => format!("&{}", syntax_text_inner(inner)),
        TypeSyntax::ConstReference(inner) => format!("&const {}", syntax_text_inner(inner)),
        TypeSyntax::RvalueReference(inner) => format!("&&{}", syntax_text_inner(inner)),
        TypeSyntax::ConstRvalueReference(inner) => {
            format!("&&const {}", syntax_text_inner(inner))
        }
        TypeSyntax::Volatile(inner) => format!("volatile {}", syntax_text_inner(inner)),
        TypeSyntax::Const(inner) => format!("const {}", syntax_text_inner(inner)),
        TypeSyntax::Auto(_) => "auto".to_owned(),
        TypeSyntax::DecltypeAuto(_) => "type(auto)".to_owned(),
        TypeSyntax::TypeOf(_) => "type(auto)".to_owned(),
        TypeSyntax::Member(member) => {
            format!("{}.{}", syntax_text_inner(&member.outer), member.name)
        }
        TypeSyntax::Function(function) => {
            let parameters: Vec<String> =
                function.parameters.iter().map(syntax_text_inner).collect();
            let mut out = String::from("fn(");
            out.push_str(&parameters.join(", "));
            out.push_str(") ");
            out.push_str(&syntax_text_inner(&function.returns));
            out
        }
        TypeSyntax::Unresolved(unresolved) => unresolved.text.clone(),
    }
}

/// Builds a written type from its text, with the parser this crate cannot call.
///
/// Turning text into a type is the parser's job, and the parser depends on this
/// crate -- so the reader is handed the function instead of calling it. The
/// driver, which has both, passes `fallc_parser::parse_type_source`.
pub type SyntaxParser = fn(&str) -> fallc_ast::TypeSyntax;

/// The parser a reader uses when the caller has none.
///
/// A type of structure is kept as text naming itself, which the checker reports
/// where it resolves. Every caller in this workspace supplies the real parser,
/// so this exists for a test that has no parser at hand.
#[must_use]
pub fn unresolved_syntax(text: &str) -> fallc_ast::TypeSyntax {
    fallc_ast::TypeSyntax::Unresolved(fallc_ast::Unresolved {
        text: text.to_owned(),
        span: fallc_span::Span::SYNTHETIC,
    })
}

// --- export table --------------------------------------------------------

fn write_map<T>(out: &mut Writer, map: &HashMap<String, T>, write: impl Fn(&mut Writer, &T)) {
    let mut names: Vec<&String> = map.keys().collect();
    names.sort();
    out.count(names.len());
    for name in names {
        out.str(name);
        write(out, &map[name]);
    }
}

fn read_map<T>(
    input: &mut Reader<'_>,
    mut read: impl FnMut(&mut Reader<'_>) -> Result<T, DecodeError>,
) -> Result<HashMap<String, T>, DecodeError> {
    let count = input.count()?;
    let mut map = HashMap::with_capacity(count.min(1024));
    for _ in 0..count {
        let name = input.str()?;
        map.insert(name, read(input)?);
    }
    Ok(map)
}

fn write_signature(out: &mut Writer, signature: &FunctionSignature) {
    out.bool(signature.is_coroutine);
    out.list(&signature.param_names, |out, name| out.str(name));
    out.list(&signature.param_types, |out, ty| write_syntax(out, ty));
    write_syntax(out, &signature.return_type);
    out.bool(signature.variadic);
    out.count(signature.required_params);
    // A default is an expression in the declaring module's tree, so the id
    // means nothing to a reader. What crosses is `default_values`, which is the
    // folded answer; the *presence* of the expression is kept so that a reader
    // knows the parameter may be left out, and the id is written as a
    // placeholder that names no expression.
    out.list(&signature.defaults, |out, default| {
        out.bool(default.is_some());
    });
    out.opt(signature.declared_in.as_ref(), |out, name| out.str(name));
    out.list(&signature.default_values, |out, value| match value {
        Some(value) => {
            out.u8(1);
            out.i64(*value);
        }
        None => out.u8(0),
    });
    out.bool(signature.exported);
    out.bool(signature.is_external);
    out.opt(signature.abi.as_ref(), |out, abi| out.str(abi));
    out.opt(signature.pack.as_ref(), |out, index| out.count(*index));
    out.list(&signature.template_params, write_template_param);
    out.str(&signature.written_name);
    out.str(&signature.bound_as);
    // A `??(cond)` condition is the same problem as a default: an expression id
    // in one tree. A reader outside that tree treats the function as always
    // able to raise, which is what `throws_condition: None` says.
    out.bool(signature.throws_condition.is_some());
    out.bool(signature.throws);
    out.opt(signature.deprecated.as_ref(), |out, message| {
        out.str(message)
    });
    out.opt(signature.resolved.as_ref(), |out, resolved| {
        out.list(&resolved.params, write_type);
        write_type(out, &resolved.returns);
        out.str(&resolved.symbol);
    });
    out.bool(signature.instantiated);
}

fn read_signature(
    input: &mut Reader<'_>,
    span: fallc_span::Span,
    parse: SyntaxParser,
) -> Result<FunctionSignature, DecodeError> {
    let is_coroutine = input.bool()?;
    let param_names = input.list(Reader::str)?;
    let param_types = input.list(|input| read_syntax(input, parse))?;
    let return_type = read_syntax(input, parse)?;
    let variadic = input.bool()?;
    let required_params = input.count()?;
    let defaults: Vec<bool> = input.list(|input| input.bool())?;
    let declared_in = input.opt(Reader::str)?;
    let default_values = input.list(|input| match input.u8()? {
        0 => Ok(None),
        1 => Ok(Some(input.i64()?)),
        tag => Err(DecodeError::UnknownTag {
            what: "default value",
            tag,
        }),
    })?;
    let exported = input.bool()?;
    let is_external = input.bool()?;
    let abi = input.opt(Reader::str)?;
    let pack = input.opt(Reader::count)?;
    let template_params = input.list(|input| read_template_param(input, parse))?;
    let written_name = input.str()?;
    let bound_as = input.str()?;
    let has_condition = input.bool()?;
    let throws = input.bool()?;
    let deprecated = input.opt(Reader::str)?;
    let resolved = input.opt(|input| {
        let params = input.list(|input| read_type(input, span))?;
        let returns = read_type(input, span)?;
        let symbol = input.str()?;
        Ok(ResolvedSignature {
            params,
            returns,
            symbol,
        })
    })?;
    let instantiated = input.bool()?;
    Ok(FunctionSignature {
        is_coroutine,
        param_names,
        param_types,
        return_type,
        variadic,
        required_params,
        // The expression itself stayed in the declaring module. A parameter
        // that had one still reports one, so that a reader knows the argument
        // may be left out; the folded value is what it fills in.
        defaults: defaults
            .iter()
            .map(|present| present.then_some(fallc_ast::ExprId::DUMMY))
            .collect(),
        declared_in,
        default_values,
        exported,
        is_external,
        abi,
        pack,
        template_params,
        written_name,
        bound_as,
        throws_condition: has_condition.then_some(fallc_ast::ExprId::DUMMY),
        throws,
        deprecated,
        resolved,
        instantiated,
    })
}

fn write_template_param(out: &mut Writer, param: &fallc_ast::TemplateParam) {
    out.str(&param.name);
    match &param.kind {
        fallc_ast::TemplateParamKind::Type => out.u8(0),
        fallc_ast::TemplateParamKind::Value(ty) => {
            out.u8(1);
            write_syntax(out, ty);
        }
    }
    out.bool(param.is_pack);
    match &param.default {
        None => out.u8(0),
        Some(fallc_ast::TemplateDefault::Type(ty)) => {
            out.u8(1);
            write_syntax(out, ty);
        }
        Some(fallc_ast::TemplateDefault::Value(_)) => {
            // A value default is an expression in the declaring tree; a reader
            // treats the parameter as one that must be supplied.
            out.u8(2);
        }
    }
}

fn read_template_param(
    input: &mut Reader<'_>,
    parse: SyntaxParser,
) -> Result<fallc_ast::TemplateParam, DecodeError> {
    let name = input.str()?;
    let kind = match input.u8()? {
        0 => fallc_ast::TemplateParamKind::Type,
        1 => fallc_ast::TemplateParamKind::Value(read_syntax(input, parse)?),
        tag => {
            return Err(DecodeError::UnknownTag {
                what: "template parameter",
                tag,
            });
        }
    };
    let is_pack = input.bool()?;
    let default = match input.u8()? {
        0 => None,
        1 => Some(fallc_ast::TemplateDefault::Type(read_syntax(input, parse)?)),
        2 => None,
        tag => {
            return Err(DecodeError::UnknownTag {
                what: "template default",
                tag,
            });
        }
    };
    Ok(fallc_ast::TemplateParam {
        name,
        name_span: fallc_span::Span::SYNTHETIC,
        kind,
        is_pack,
        default,
        span: fallc_span::Span::SYNTHETIC,
    })
}

fn write_export_type(out: &mut Writer, ty: &ExportType) {
    out.str(&ty.name);
    out.list(&ty.fields, |out, field| {
        out.str(&field.name);
        write_syntax(out, &field.ty);
        out.opt(field.resolved.as_ref(), write_type);
        out.bool(field.mutable);
        out.opt(field.align.as_ref(), |out, align| out.u64(*align));
    });
    out.list(&ty.template_params, write_template_param);
    out.bool(ty.is_union);
    out.list(&ty.nested, |out, (name, syntax)| {
        out.str(name);
        write_syntax(out, syntax);
    });
    out.list(&ty.specialisations, |out, specialisation| {
        out.list(&specialisation.params, write_template_param);
        out.list(&specialisation.pattern, write_type);
        out.list(&specialisation.fields, |out, (name, ty)| {
            out.str(name);
            write_type(out, ty);
        });
        out.bool(specialisation.is_union);
        out.list(&specialisation.mutable_fields, |out, name| out.str(name));
        out.list(&specialisation.alignments, |out, (name, align)| {
            out.str(name);
            out.u64(*align);
        });
        out.list(&specialisation.constants, |out, (name, value)| {
            out.str(name);
            out.i64(*value);
        });
    });
    out.list(&ty.constants, |out, (name, value)| {
        out.str(name);
        out.i64(*value);
    });
    out.bool(ty.is_forward);
    out.opt(ty.alias.as_ref(), write_syntax);
}

fn read_export_type(
    input: &mut Reader<'_>,
    span: fallc_span::Span,
    parse: SyntaxParser,
) -> Result<ExportType, DecodeError> {
    let name = input.str()?;
    let fields = input.list(|input| {
        let name = input.str()?;
        let ty = read_syntax(input, parse)?;
        let resolved = input.opt(|input| read_type(input, span))?;
        let mutable = input.bool()?;
        let align = input.opt(Reader::u64)?;
        Ok(ExportedField {
            name,
            ty,
            resolved,
            mutable,
            align,
        })
    })?;
    let template_params = input.list(|input| read_template_param(input, parse))?;
    let is_union = input.bool()?;
    let nested = input.list(|input| {
        let name = input.str()?;
        Ok((name, read_syntax(input, parse)?))
    })?;
    let specialisations = input.list(|input| {
        let params = input.list(|input| read_template_param(input, parse))?;
        let pattern = input.list(|input| read_type(input, span))?;
        let fields = input.list(|input| {
            let name = input.str()?;
            Ok((name, read_type(input, span)?))
        })?;
        let is_union = input.bool()?;
        let mutable_fields = input.list(Reader::str)?;
        let alignments = input.list(|input| {
            let name = input.str()?;
            Ok((name, input.u64()?))
        })?;
        let constants = input.list(|input| {
            let name = input.str()?;
            Ok((name, input.i64()?))
        })?;
        Ok(ExportedSpecialisation {
            params,
            pattern,
            fields,
            is_union,
            mutable_fields,
            alignments,
            constants,
        })
    })?;
    let constants = input.list(|input| {
        let name = input.str()?;
        Ok((name, input.i64()?))
    })?;
    let is_forward = input.bool()?;
    let alias = input.opt(|input| read_syntax(input, parse))?;
    Ok(ExportType {
        name,
        fields,
        template_params,
        is_union,
        nested,
        specialisations,
        constants,
        is_forward,
        alias,
    })
}

/// Writes a module's export table.
///
/// The bytes are a function of the table alone: every map is written in name
/// order, so two writes of one table produce the same file.
#[must_use]
pub fn encode(module: &ExternalModule) -> Vec<u8> {
    let mut out = Writer::new();
    write_map(&mut out, &module.symbols, |out, signatures| {
        out.list(signatures, write_signature);
    });
    write_map(&mut out, &module.types, write_export_type);
    write_map(&mut out, &module.constants, |out, value| out.i64(*value));
    write_map(&mut out, &module.constant_types, write_syntax);
    write_map(&mut out, &module.variables, |out, variable| {
        out.str(&variable.symbol);
        write_syntax(out, &variable.ty);
    });
    write_map(&mut out, &module.globals, |out, global| {
        out.str(&global.symbol);
        write_type(out, &global.ty);
        out.opt(global.deprecated.as_ref(), |out, message| out.str(message));
        out.bool(global.thread_local);
    });
    {
        let mut traits: Vec<&String> = module.traits.iter().collect();
        traits.sort();
        out.list(&traits, |out, name| out.str(name));
    }
    write_map(&mut out, &module.enums, |out, enumeration| {
        out.str(&enumeration.name);
        out.opt(enumeration.base.as_ref(), write_syntax);
        out.list(&enumeration.members, |out, (name, value)| {
            out.str(name);
            out.i64(*value);
        });
    });
    write_map(&mut out, &module.interfaces, |out, interface| {
        out.str(&interface.name);
        out.list(&interface.template_params, write_template_param);
        out.list(&interface.methods, |out, method| {
            out.str(&method.name);
            out.list(&method.params, write_syntax);
            write_syntax(out, &method.returns);
            out.bool(method.throws);
        });
        out.bool(interface.is_forward);
    });
    out.finish()
}

/// Reads a module's export table.
///
/// # Errors
///
/// A byte string this module did not write -- truncated, written by an older
/// version, or corrupt -- is refused rather than half-read. The caller then
/// falls back to compiling the module from its source.
pub fn decode(bytes: &[u8], parse: SyntaxParser) -> Result<ExternalModule, DecodeError> {
    let span = fallc_span::Span::SYNTHETIC;
    let mut input = Reader::new(bytes);
    let symbols = read_map(&mut input, |input| {
        input.list(|input| read_signature(input, span, parse))
    })?;
    let types = read_map(&mut input, |input| read_export_type(input, span, parse))?;
    let constants = read_map(&mut input, |input| input.i64())?;
    let constant_types = read_map(&mut input, |input| read_syntax(input, parse))?;
    let variables = read_map(&mut input, |input| {
        let symbol = input.str()?;
        let ty = read_syntax(input, parse)?;
        Ok(ExportedVariable { symbol, ty })
    })?;
    let globals = read_map(&mut input, |input| {
        let symbol = input.str()?;
        let ty = read_type(input, span)?;
        let deprecated = input.opt(Reader::str)?;
        let thread_local = input.bool()?;
        Ok(ExportedGlobal {
            symbol,
            ty,
            deprecated,
            thread_local,
        })
    })?;
    let traits: HashSet<String> = input.list(Reader::str)?.into_iter().collect();
    let enums = read_map(&mut input, |input| {
        let name = input.str()?;
        let base = input.opt(|input| read_syntax(input, parse))?;
        let members = input.list(|input| {
            let name = input.str()?;
            Ok((name, input.i64()?))
        })?;
        Ok(ExportEnum {
            name,
            base,
            members,
        })
    })?;
    let interfaces = read_map(&mut input, |input| {
        let name = input.str()?;
        let template_params = input.list(|input| read_template_param(input, parse))?;
        let methods = input.list(|input| {
            let name = input.str()?;
            let params = input.list(|input| read_syntax(input, parse))?;
            let returns = read_syntax(input, parse)?;
            let throws = input.bool()?;
            Ok(ExportInterfaceMethod {
                name,
                params,
                returns,
                throws,
            })
        })?;
        let is_forward = input.bool()?;
        Ok(ExportInterface {
            name,
            template_params,
            methods,
            is_forward,
        })
    })?;
    if !input.at_end() {
        return Err(DecodeError::Malformed("trailing bytes"));
    }
    Ok(ExternalModule {
        symbols,
        types,
        constants,
        constant_types,
        variables,
        globals,
        traits,
        enums,
        interfaces,
    })
}
