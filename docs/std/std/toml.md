# `std.toml`

TOML: a small configuration format, parsed into entries.

    document TomlDocument = toml.parse(&text)??;
    port TomlValue = document.get(&String("server"), &String("port")).value()??;
    n i64 = port as i64??;

# The subset

This implements the part of TOML a configuration file actually uses:

* comments start with `#` and run to the end of the line;
* `[table]` starts a table, and keys before any table are at the root;
* `key = value`, with a bare or quoted key;
* values: quoted strings (with `\"`, `\\`, `\n`, `\t`), integers, reals,
  `true` and `false`.

Out of scope, and refused rather than misread: arrays, inline tables, dates,
multi-line strings, and dotted keys. A format's parser should say what it does
not understand, not guess.

# Shape

The document is a flat list of entries in file order, each with its table
name (empty at the root), key and value. That keeps the order a program wrote
and avoids a map type for what is usually a dozen settings; `get` is a linear
scan, which is the right cost for a configuration file.

## Declarations

### `export enum TomlKind int {`

What kind of value an entry holds.

```
export enum TomlKind int {
    // A quoted string.
    STRING,
    // A whole number.
    INTEGER,
    // A real number.
    REAL,
    // `true` or `false`.
    BOOLEAN,
}
```

### `export type TomlValue {`

One value.

```
export type TomlValue {
    // Which kind it is.
    _kind int;
    // The text of a string.
    _text String;
    // The number of an integer.
    _integer i64;
    // The number of a real.
    _real f64;
    // The value of a boolean.
    _boolean bool;
}
```

### `export fn __init__(this &TomlValue) void {`

Creates an empty value.

```
export fn __init__(this &TomlValue) void {
    this._kind = TomlKind.STRING as int;
    (&this._text)->__init__();
    this._integer = 0i64;
    this._real = 0.0;
    this._boolean = false;
}
```

### `export fn __init__(this &TomlValue, other &const TomlValue) void {`

Copies a value.

```
export fn __init__(this &TomlValue, other &const TomlValue) void {
    this._kind = other._kind;
    (&this._text)->__init__(other._text);
    this._integer = other._integer;
    this._real = other._real;
    this._boolean = other._boolean;
}
```

### `export fn __init__(this &TomlValue, other &&TomlValue) void {`

Takes a value's storage.

```
export fn __init__(this &TomlValue, other &&TomlValue) void {
    this._kind = other._kind;
    (&this._text)->__init__();
    this._text = util.move(other._text);
    this._integer = other._integer;
    this._real = other._real;
    this._boolean = other._boolean;
}
```

### `export fn __drop__(this &TomlValue) void {`

Releases a value's storage.

```
export fn __drop__(this &TomlValue) void {
    (&this._text)->__drop__();
}
```

### `export fn kind(this &const TomlValue) TomlKind {`

The kind of a value.

```
export fn kind(this &const TomlValue) TomlKind {
    return this._kind as TomlKind;
}
```

### `export fn __as__(this &const TomlValue, target *String) String except {`

The string, or a raise when the value is another kind.

```
export fn __as__(this &const TomlValue, target *String) String except {
    if (this.kind() != TomlKind.STRING) {
        raise "ValueError": "the value is not a string";
    }
    return this._text;
}
```

### `export fn __as__(this &const TomlValue, target *i64) i64 except {`

The integer, or a raise when the value is another kind.

```
export fn __as__(this &const TomlValue, target *i64) i64 except {
    if (this.kind() != TomlKind.INTEGER) {
        raise "ValueError": "the value is not an integer";
    }
    return this._integer;
}
```

### `export fn __as__(this &const TomlValue, target *f64) f64 except {`

The real, or a raise when the value is another kind.

```
export fn __as__(this &const TomlValue, target *f64) f64 except {
    if (this.kind() != TomlKind.REAL) {
        raise "ValueError": "the value is not a real";
    }
    return this._real;
}
```

### `export fn __as__(this &const TomlValue, target *bool) bool except {`

The boolean, or a raise when the value is another kind.

```
export fn __as__(this &const TomlValue, target *bool) bool except {
    if (this.kind() != TomlKind.BOOLEAN) {
        raise "ValueError": "the value is not a boolean";
    }
    return this._boolean;
}
```

### `export type TomlEntry {`

One `key = value` from the file.

```
export type TomlEntry {
    // The table it belongs to, empty at the root.
    _table String;
    // The key.
    _key String;
    // The value.
    _value TomlValue;
}
```

### `export fn __init__(this &TomlEntry) void {`

Creates an empty entry.

```
export fn __init__(this &TomlEntry) void {
    (&this._table)->__init__();
    (&this._key)->__init__();
    (&this._value)->__init__();
}
```

### `export fn __init__(this &TomlEntry, other &const TomlEntry) void {`

Copies an entry.

```
export fn __init__(this &TomlEntry, other &const TomlEntry) void {
    (&this._table)->__init__(other._table);
    (&this._key)->__init__(other._key);
    (&this._value)->__init__(other._value);
}
```

### `export fn __init__(this &TomlEntry, other &&TomlEntry) void {`

Takes an entry's storage.

```
export fn __init__(this &TomlEntry, other &&TomlEntry) void {
    (&this._table)->__init__();
    (&this._key)->__init__();
    (&this._value)->__init__();
    this._table = util.move(other._table);
    this._key = util.move(other._key);
    this._value = util.move(other._value);
}
```

### `export fn __drop__(this &TomlEntry) void {`

Releases an entry's storage.

```
export fn __drop__(this &TomlEntry) void {
    (&this._table)->__drop__();
    (&this._key)->__drop__();
    (&this._value)->__drop__();
}
```

### `export type TomlDocument {`

A parsed TOML file: its entries, in file order.

```
export type TomlDocument {
    // The entries.
    _entries Array<TomlEntry>;
}
```

### `export fn __init__(this &TomlDocument) void {`

Creates an empty document.

```
export fn __init__(this &TomlDocument) void {
    (&this._entries)->__init__();
}
```

### `export fn __init__(this &TomlDocument, other &&TomlDocument) void {`

Moves a document's storage.

```
export fn __init__(this &TomlDocument, other &&TomlDocument) void {
    (&this._entries)->__init__();
    this._entries = util.move(other._entries);
}
```

### `export fn __drop__(this &TomlDocument) void {`

Releases the entries.

```
export fn __drop__(this &TomlDocument) void {
    (&this._entries)->__drop__();
}
```

### `export fn length(this &const TomlDocument) usize {`

The number of entries.

```
export fn length(this &const TomlDocument) usize {
    return this._entries.length();
}
```

### `export fn table(this &const TomlDocument, index usize) String except {`

The table of entry `index`.

```
export fn table(this &const TomlDocument, index usize) String except {
    if (index >= this._entries.length()) {
        raise "ValueError": "the document has no such entry";
    }
    return this._entries[index]._table;
}
```

### `export fn key(this &const TomlDocument, index usize) String except {`

The key of entry `index`.

```
export fn key(this &const TomlDocument, index usize) String except {
    if (index >= this._entries.length()) {
        raise "ValueError": "the document has no such entry";
    }
    return this._entries[index]._key;
}
```

### `export fn value(this &const TomlDocument, index usize) TomlValue except {`

The value of entry `index`.

```
export fn value(this &const TomlDocument, index usize) TomlValue except {
    if (index >= this._entries.length()) {
        raise "ValueError": "the document has no such entry";
    }
    return this._entries[index]._value;
}
```

### `export fn get(`

The value of `key` in `table`, or an absent optional.

```
export fn get(
    this &const TomlDocument,
    table_name &const String,
    key_name &const String
) ?TomlValue {
    index usize = 0;
    while (index < this._entries.length()) {
        if (this._entries[index]._table == table_name && this._entries[index]._key == key_name) {
            return optional.some<TomlValue>(this._entries[index]._value);
        }
        index += 1;
    }
    return optional.none<TomlValue>();
}
```

### `fn trim_spaces(text &String) String`

The string with its surrounding spaces removed.

The substring helper returns a fresh string; the parser needs one without the
spaces a table line may carry inside its brackets.
- `text`: the string to trim; only spaces and tabs are removed, and a
  newline is not, so a table's name keeps a line break inside

```
fn trim_spaces(text &String) String
```

### `type Parser {`

The parser's state.

```
type Parser {
    _text String;
    _index usize;
    _table String;
    _document *TomlDocument;
}
```

### `fn __init__(this &Parser, text &const String, document *TomlDocument) void`

Creates a parser over `text`.
- `text`: the whole TOML text the parser reads from, copied into the
  parser
- `document`: the document entries are appended to, pointed at rather than
  copied, so it must outlive the parser

```
fn __init__(this &Parser, text &const String, document *TomlDocument) void
```

### `export fn parse_toml(text &const String) TomlDocument except {`

Parses TOML text into a document.

```
export fn parse_toml(text &const String) TomlDocument except {
    document TomlDocument = TomlDocument();
    parser Parser = Parser(text, &document);
    while (true) {
        parser.skip_space_and_comments();
        if (parser._index >= parser._text.length()) {
            break;
        }
        if (parser.at('[')) {
            parser.parse_table()??;
            continue;
        }
        parser.parse_entry()??;
    }
    return document;
}
```

### `fn parse_table(this &Parser) void except`

Reads a `[table]` line.

```
fn parse_table(this &Parser) void except
```

### `fn parse_entry(this &Parser) void except`

Reads a `key = value` line.

```
fn parse_entry(this &Parser) void except
```

### `fn parse_key(this &Parser) String except`

Reads a bare or quoted key.

```
fn parse_key(this &Parser) String except
```

### `fn parse_value(this &Parser) TomlValue except`

Reads a value.

```
fn parse_value(this &Parser) TomlValue except
```

### `fn parse_string(this &Parser) String except`

Reads a quoted string, decoding its escapes.

```
fn parse_string(this &Parser) String except
```

### `fn skip_space_and_comments(this &Parser) void`

Skips spaces, tabs and comments up to a line break, then the break itself.

```
fn skip_space_and_comments(this &Parser) void
```

### `fn skip_horizontal(this &Parser) void`

Skips spaces and tabs only.

```
fn skip_horizontal(this &Parser) void
```

### `fn at(this &Parser, character char) bool`

Whether the parser is looking at `character`.
- `character`: the character to look for; it is compared with the byte at
  the current position

```
fn at(this &Parser, character char) bool
```

### `fn expect_word(this &Parser, word *const char) void except`

Consumes `word` or raises.
- `word`: a null-terminated C string, compared byte for byte with the
  text at the current position and then consumed

```
fn expect_word(this &Parser, word *const char) void except
```

### `fn c_length(word *const char) usize`

The length of a C string.
- `word`: a null-terminated C string; the bytes before the terminator
  are counted, and the terminator is not

```
fn c_length(word *const char) usize
```
