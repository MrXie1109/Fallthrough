# 01 Lexical

> [中文](01-lexical.zh.md)

## Scope

This volume specifies how source text is divided into tokens, and the lexical forms of
comments, identifiers, keywords, literals and operators. How tokens combine into
syntactic structures is specified in the following volumes.

## Source text

2.1 A source file **must** be encoded in UTF-8. An implementation must not accept an
invalid UTF-8 sequence.

2.2 Whitespace (space, tab, newline, carriage return) separates tokens during lexical
analysis and does not constitute a syntactic structure.

2.3 Comments and whitespace are collectively called trivia. An implementation
**should** retain trivia and its source position in the token stream, so that the source
text can be reconstructed completely; whether it retains them does not affect program
semantics.

## Comments

3.1 A line comment begins with `//` and ends at the end of the line.

3.2 A block comment begins with `/*` and ends at the first `*/` after it. Block comments
**must not** nest: the first `*/` terminates the comment, and any further `*/` is a
syntax error.

3.3 Comments are trivia and take no part in syntactic or semantic analysis.

## Identifiers

4.1 An identifier **must** begin with a character of class XID_Start or with `_`, and
continue with characters of class XID_Continue, digits or `_`.

4.2 Identifiers are case sensitive.

4.3 An implementation **must not** apply Unicode normalization to identifiers: two
identifiers with different code-point sequences are different names.

4.4 `_` is a legal identifier, but has special rules for repeated declaration, see
[09 Scopes](10-scopes.md#_).

## Keywords

5.1 The following words are reserved and **must not** be used as identifiers:

```text
alignof  as       auto     break    case     const    continue default
defer    define   delete   else     enum     except   export   fallthrough
false    finally  fn       for      from     if       import   interface
lambda   link     mutable  nullptr  raise    return   sizeof   static_assert
switch   template this     traits   true     try      type     union
volatile while
```

5.2 The list in 5.1 is the complete set: no other word is reserved, and a program may
declare an identifier with any word not on it. In particular `alignas`, `constexpr`,
`dynamic`, `extern`, `static` and `thread_local` are ordinary identifiers. The
declarations they name are written with attributes, in which the word after `@` is read
as a word rather than as a keyword; see [03 Declarations §9](03-declarations.md#attributes).

5.3 `export` **is** reserved: it introduces a declaration that importers may see, and it
is written before the attribute list rather than as an attribute; see
[03 Declarations §9](03-declarations.md#attributes).

5.4 `delete` is reserved for a future deletion expression and has no meaning yet. `define`
introduces a macro definition at the start of a logical line; it is reserved so that a
definition always means what it says, because a preprocessor that recognised it only by
reading an identifier would let a program shadow that identifier.

5.5 The name after `@` is not a keyword: it is read as an ordinary word, and the
attributes and positions that are allowed are given in
[03 Declarations §9](03-declarations.md#attributes).

## Literals

### General rules

6.1.1 A literal is a token that has a type. The type of an integer or floating-point
literal may be determined by context; when the context requires a type, the literal
takes that type (see [02 Types](02-types.md)).

6.1.2 `_` between digits is a separator and does not change the value.

6.1.3 An integer suffix writes the type itself; it is not a conversion: the type of
`42u8` is `u8`. Using that literal where another type is required is a type error, not
an implicit conversion.

### Integer literals

| Form | Type |
| ---- | ---- |
| `42`, `0x2A`, `0o52`, `0b101010` | determined by context; `int` when there is no context |
| `42u8`, `42usize`, `42i64`, etc. | the integer type written by the suffix |

6.2.1 The integer prefixes `0x`, `0o` and `0b` denote hexadecimal, octal and binary
respectively; without a prefix the literal is decimal.

### Floating-point literals

| Form | Type |
| ---- | ---- |
| `1.5` | `double` (that is, `f64`) |
| `1.5f` | `float` (that is, `f32`) |

### Character literals

6.4.1 The type of `'x'` is `c8`; the width prefixes `u16'x'` and `u32'x'` give wider
character types.

6.4.2 A character literal contains exactly one character (or one escape sequence).

### String literals

| Form | Type | Terminator | Notes |
| ---- | ---- | ------ | ---- |
| `"text"` | `[N]const c8` | yes | interprets escapes; stored as UTF-8 |
| `"""text"""` | `[N]const c8` | yes | may span lines; interprets escapes; does not trim indentation |
| `u16"text"`, `u32"text"` | `[N]const c16` / `[N]const c32` | yes | element width determined by the prefix |
| `b"bytes"` | `[N]const byte` | no | element type is `byte` |
| `r"text"` | `[N]const c8` | yes | raw string; does not interpret escapes |
| `r#tag"text"tag#` | `[N]const c8` | yes | delimited raw string |
| `f"text{expr}text"` | — | — | interpolated string; not a value of its own (6.5.5) |

6.5.1 `N` is the number of array elements: for the `c8`/`c16`/`c32` forms, `N` equals
the number of decoded elements plus one (including the terminator); for the `b` form,
`N` equals the number of bytes (no terminator).

6.5.2 A multi-line string opens and closes with three quotes and may span lines; a
single `"` is content, and `\"""` is an escaped quote that does not close the literal.
A multi-line string interprets escapes and does not trim leading whitespace.

6.5.3 A raw string does not interpret escapes; its content is preserved verbatim. The
delimited form of a raw string is `r#TAG"…"TAG#`: `TAG` must not contain whitespace or
`"`, and its length must not exceed 64 bytes; the closing delimiter must repeat the same
`TAG`. When what follows `r` is neither `"` nor `#…"`, `r` is an identifier.

6.5.4 The `u16`/`u32` prefixes select the element type; the `b` prefix selects the
`byte` element type. The `b` form denotes raw memory and contains no terminator.

6.5.5 An interpolated string is written `f"…"`: the `f` is a prefix, and what follows is
read to the closing `"` like an ordinary string, escapes included. It is **not** a literal
of an array type: it denotes a call, and
[07 Expressions](08-expressions.md#interpolated-strings) says what the call is. An
interpolated string may span lines the way `"…"` does, since a newline inside it is an
ordinary character. There is no `f"""…"""` form.

### Boolean literals and the null pointer

6.6.1 The type of `true` and `false` is `bool`.

6.6.2 `nullptr` has no fixed pointer type; it takes the pointer type required by the
position of use.

### Range literals

6.6.3 `low .. high` and `low ..= high` denote a range: the series of integers that starts
at `low` and steps by one. The step is one and the elements are `i64`.

6.6.4 The range excludes `high` and the range with `=` includes it: `1 .. 5` walks
1, 2, 3, 4, and `1 ..= 5` walks 1, 2, 3, 4, 5.

6.6.5 The two ends are expressions, not necessarily literals; each **must** have an
integer type or be inferable as one. `1 .. n + 1` is the range from 1 to `n`, because a
range binds looser than every binary operator.

6.6.6 A range literal is the standard library's `Range`: `1 .. 10` is
`std.range.Range(1, 10, 1)` and `1 ..= 10` is `std.range.Range(1, 11, 1)`. It therefore
**requires** `import std;` or `import std.range;`, and it may be stored in a `Range`, used
as the subject of a range loop (see [05 Statements](06-statements.md#range-loops)), or
passed wherever a `Range` is expected.

6.6.7 A range **must not** be chained: `1 .. 2 .. 3` does not conform to this standard.

The following program prints `1 2 3 4` and then `1 2 3 4 5`:

```ft
import C;
import std;

fn main() int except {
    for (i i64 : 1 .. 5) { C.printf("%ld ", i); }
    C.printf("| ");
    for (i i64 : 1 ..= 5) { C.printf("%ld ", i); }
    C.printf("\n");
    return 0;
}
```

### Escape sequences

6.7.1 Character and string literals support the following escapes: `\n`, `\t`, `\r`,
`\\`, `\'`, `\"`, `\0`, `\xNN`, `\u{…}`.

6.7.2 The content of `\u{…}` is one to six hexadecimal digits denoting a Unicode code
point. `\u` without braces is not an escape: `\u0041` is six characters.

6.7.3 Any other escape sequence is a syntax error, and an implementation **must**
report a diagnostic.

## Operators and precedence

7.1 The precedence of binary and unary operators, from tightest to loosest, is as
follows. Associativity is specified by the table; operators of the same level associate
as the table states.

| Level | Operators | Associativity |
| ---- | ------ | ------ |
| 13 | prefix `+ - ! ~ &`, `*` (dereference), `++`, `--` | right |
| 12 | `as` | left |
| 11 | `**` | right |
| 10 | `* / %` | left |
| 9 | `+ -` | left |
| 8 | `<< >>` | left |
| 7 | `< <= > >= <=>` | left |
| 6 | `== !=` | left |
| 5 | `&` | left |
| 4 | `^` | left |
| 3 | `\|` | left |
| 2 | `&&` | left |
| 1 | `\|\|` | left |
| 0 | `?:`, `=` and the compound assignments | right |
| — | `..`, `..=` (looser than every operator above) | — |

7.1.1 A range is not an operator over two values: its two ends are integers and its
whole is a `Range`. It therefore sits below `?:` and takes each end as a full expression,
so `1 .. n + 1` is the range from 1 to `n + 1`.

7.2 Postfix forms (call, subscript, field access, `++`, `--`, `??`, `!!`) have higher
precedence than any binary operator in the table.
precedence than any binary operator in the table.

7.3 `as` has higher precedence than any binary operator and lower precedence than the
prefix operators: `-x as u8` is equivalent to `(-x) as u8`, and `x as u8 == 7` is
equivalent to `(x as u8) == 7`.

7.4 `**` is right associative: `2 ** 3 ** 2` is equivalent to `2 ** (3 ** 2)`.

7.5 Bitwise operators have lower precedence than comparison operators; this agrees with
C.

## Macro and attribute tokens

8.1 `$` introduces a macro invocation: `$NAME` or `$NAME(args)`, see
[03 Declarations](03-declarations.md#macros).

8.2 `@` introduces an attribute: `@name` or `@name(args)`, see
[03 Declarations](03-declarations.md#attributes).

8.3 `#` has meaning as a token only inside a macro body: `#param` turns an argument
into a string literal, and `a ## b` pastes two tokens into one. A `#` inside a raw
string belongs to the literal.

## Special rules for token splitting

9.1 `.{` is **one** token, used for designated initializers. To obtain the two tokens
`.` and `{`, one must write `. {`.

9.2 `->` is the pointer member access token; in a `traits` or `interface` requirement it
also denotes the result type, see [03 Declarations](03-declarations.md#traits).

9.3 In a template argument list, `>` and `>>` are split as needed: the `>>` in
`Array<Array<int>>` is split into two `>`.

9.4 Tokens are separated by whitespace or trivia; adjacent operators are split by
longest match, and combinations not listed in Table 7.1 may constitute a single token
(for example `->`, `++`), whose meaning is specified by the grammar.
