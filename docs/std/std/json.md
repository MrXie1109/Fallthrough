# `std.json`

JSON: a document, values into it, a parser and a writer.

    document JsonDocument = json.parse(&text)??;
    root JsonValue = document.root();
    key String = String("name");
    name String = root.member(&key).value()?? as String??;
    written String = json.serialize(&root)??;

# Ownership

The *document* owns the tree: every node lives in one `Array<JsonNodeData>`,
and the children fields are indices into it. A `JsonValue` is a small handle
-- the document and an index -- so copying one is copying two numbers, and
nothing is freed when it goes. The document must outlive its values.

This shape was chosen because a handle that owns a tree has to survive being
passed by value, and a two-word aggregate is passed as bits in this language:
the callee never runs the copy or move constructor, so an owning handle
would be copied without being copied and freed while still in use.

# Errors

Parsing raises `ValueError` on text that is not JSON, and an accessor raises
on a value of another kind. A missing member is `?JsonValue`, because "no
such key" is a question rather than a failure.

## Declarations

### `export enum JsonKind int {`

The kind of a JSON value.

```
export enum JsonKind int {
    // `null`.
    NULL_VALUE,
    // `true` or `false`.
    BOOLEAN,
    // A number, as a real.
    NUMBER,
    // A string.
    TEXT,
    // An array.
    ARRAY,
    // An object.
    OBJECT,
}
```

### `export type JsonNodeData {`

One node of the document.

The children of an array are indices in `_children`; an object's members are
the same indices in `_values`, paired with names in `_keys`.

```
export type JsonNodeData {
    // Which kind of value this is.
    _kind int;
    // The boolean, for `BOOLEAN`.
    _boolean bool;
    // The number, for `NUMBER`.
    _number f64;
    // The text, for `TEXT`.
    _text String;
    // The child indices, for `ARRAY`.
    _children Array<uint>;
    // The member names, for `OBJECT`.
    _keys Array<String>;
    // The member indices, for `OBJECT`.
    _values Array<uint>;
}
```

### `export fn __init__(this &JsonNodeData) void {`

Creates an empty node.

```
export fn __init__(this &JsonNodeData) void {
    this._kind = JsonKind.NULL_VALUE as int;
    this._boolean = false;
    this._number = 0.0;
    (&this._text)->__init__();
    (&this._children)->__init__();
    (&this._keys)->__init__();
    (&this._values)->__init__();
}
```

### `export fn __init__(this &JsonNodeData, other &const JsonNodeData) void {`

Copies a node.

```
export fn __init__(this &JsonNodeData, other &const JsonNodeData) void {
    this._kind = other._kind;
    this._boolean = other._boolean;
    this._number = other._number;
    (&this._text)->__init__(other._text);
    (&this._children)->__init__(other._children);
    (&this._keys)->__init__(other._keys);
    (&this._values)->__init__(other._values);
}
```

### `export fn __init__(this &JsonNodeData, other &&JsonNodeData) void {`

Takes a node's storage.

```
export fn __init__(this &JsonNodeData, other &&JsonNodeData) void {
    this._kind = other._kind;
    this._boolean = other._boolean;
    this._number = other._number;
    (&this._text)->__init__();
    (&this._children)->__init__();
    (&this._keys)->__init__();
    (&this._values)->__init__();
    this._text = util.move(other._text);
    this._children = util.move(other._children);
    this._keys = util.move(other._keys);
    this._values = util.move(other._values);
}
```

### `export fn __assign__(this &JsonNodeData, other &const JsonNodeData) void {`

Copies a node into this one.

```
export fn __assign__(this &JsonNodeData, other &const JsonNodeData) void {
    this._kind = other._kind;
    this._boolean = other._boolean;
    this._number = other._number;
    this._text = other._text;
    this._children = other._children;
    this._keys = other._keys;
    this._values = other._values;
}
```

### `export fn __drop__(this &JsonNodeData) void {`

Releases a node's storage.

```
export fn __drop__(this &JsonNodeData) void {
    (&this._text)->__drop__();
    (&this._children)->__drop__();
    (&this._keys)->__drop__();
    (&this._values)->__drop__();
}
```

### `export type JsonDocument {`

A parsed JSON document: every node, addressed by index.

```
export type JsonDocument {
    // The nodes; index zero is made by `parse` a dummy so that handles are
    // never zero-by-accident, and the root is `root_index`.
    _nodes Array<JsonNodeData>;
    // The root's index.
    _root uint;
}
```

### `export fn __init__(this &JsonDocument) void {`

Creates an empty document.

```
export fn __init__(this &JsonDocument) void {
    (&this._nodes)->__init__();
    this._root = 0u32;
}
```

### `export fn __init__(this &JsonDocument, other &&JsonDocument) void {`

Moves a document's storage.

```
export fn __init__(this &JsonDocument, other &&JsonDocument) void {
    (&this._nodes)->__init__();
    this._nodes = util.move(other._nodes);
    this._root = other._root;
}
```

### `export fn __drop__(this &JsonDocument) void {`

Releases every node.

```
export fn __drop__(this &JsonDocument) void {
    (&this._nodes)->__drop__();
}
```

### `export fn root(this &JsonDocument) JsonValue {`

The root value of the document.

```
export fn root(this &JsonDocument) JsonValue {
    return JsonValue(this, this._root);
}
```

### `fn add_node(this &JsonDocument) uint`

Makes room for one more node and returns its index.

```
fn add_node(this &JsonDocument) uint
```

### `export type JsonValue {`

A value inside a document.

The document must outlive every handle to it; the handles are deliberately
small and copyable so that they can be returned and passed without running a
destructor.

```
export type JsonValue {
    // The document the value lives in.
    _document *JsonDocument;
    // The node's index in the document.
    _index uint;
}
```

### `export fn __init__(this &JsonValue, document *JsonDocument, index uint) void {`

Builds a handle.

```
export fn __init__(this &JsonValue, document *JsonDocument, index uint) void {
    this._document = document;
    this._index = index;
}
```

### `fn node_of(this &const JsonValue) *JsonNodeData`

The node a handle names.

```
fn node_of(this &const JsonValue) *JsonNodeData
```

### `export fn kind(this &const JsonValue) JsonKind {`

The kind of a value.

```
export fn kind(this &const JsonValue) JsonKind {
    return this.node_of()._kind as JsonKind;
}
```

### `export fn length(this &const JsonValue) usize {`

The number of elements of an array, or members of an object.

```
export fn length(this &const JsonValue) usize {
    kind_value JsonKind = this.kind();
    if (kind_value == JsonKind.ARRAY) {
        return this.node_of()._children.length();
    }
    if (kind_value == JsonKind.OBJECT) {
        return this.node_of()._keys.length();
    }
    return 0;
}
```

### `export fn at(this &const JsonValue, index usize) JsonValue except {`

The element at `index` of an array.

```
export fn at(this &const JsonValue, index usize) JsonValue except {
    if (this.kind() != JsonKind.ARRAY || index >= this.node_of()._children.length()) {
        raise "ValueError": "the value has no such element";
    }
    return JsonValue(this._document, this.node_of()._children[index]);
}
```

### `export fn member(this &const JsonValue, key &const String) ?JsonValue {`

The value of a member, or an absent optional.

```
export fn member(this &const JsonValue, key &const String) ?JsonValue {
    if (this.kind() != JsonKind.OBJECT) {
        return optional.none<JsonValue>();
    }
    index usize = 0;
    while (index < this.node_of()._keys.length()) {
        if (this.node_of()._keys[index] == key) {
            return optional.some<JsonValue>(JsonValue(this._document, this.node_of()._values[index]));
        }
        index += 1;
    }
    return optional.none<JsonValue>();
}
```

### `export fn member_key(this &const JsonValue, index usize) String except {`

The member's key at `index` of an object.

```
export fn member_key(this &const JsonValue, index usize) String except {
    if (this.kind() != JsonKind.OBJECT || index >= this.node_of()._keys.length()) {
        raise "ValueError": "the value has no such member";
    }
    return this.node_of()._keys[index];
}
```

### `export fn member_value(this &const JsonValue, index usize) JsonValue except {`

The member's value at `index` of an object.

```
export fn member_value(this &const JsonValue, index usize) JsonValue except {
    if (this.kind() != JsonKind.OBJECT || index >= this.node_of()._values.length()) {
        raise "ValueError": "the value has no such member";
    }
    return JsonValue(this._document, this.node_of()._values[index]);
}
```

### `export fn __as__(this &const JsonValue, target *bool) bool except {`

The boolean, or a raise when the value is another kind.

```
export fn __as__(this &const JsonValue, target *bool) bool except {
    if (this.kind() != JsonKind.BOOLEAN) {
        raise "ValueError": "the value is not a boolean";
    }
    return this.node_of()._boolean;
}
```

### `export fn __as__(this &const JsonValue, target *f64) f64 except {`

The number, or a raise when the value is another kind.

```
export fn __as__(this &const JsonValue, target *f64) f64 except {
    if (this.kind() != JsonKind.NUMBER) {
        raise "ValueError": "the value is not a number";
    }
    return this.node_of()._number;
}
```

### `export fn __as__(this &const JsonValue, target *String) String except {`

The text, or a raise when the value is another kind.

```
export fn __as__(this &const JsonValue, target *String) String except {
    if (this.kind() != JsonKind.TEXT) {
        raise "ValueError": "the value is not text";
    }
    return this.node_of()._text;
}
```

### `export fn is_null(this &const JsonValue) bool {`

Whether the value is `null`.

```
export fn is_null(this &const JsonValue) bool {
    return this.kind() == JsonKind.NULL_VALUE;
}
```

### `type Parser {`

The parser's state: the text, how far it has been read, and the document
being built.

The document is a pointer: a `Parser` holding it by value would be built by
an aggregate literal, which copies the document's arrays as bits and then
frees the temporaries -- the shape that produced a double free.

```
type Parser {
    _text String;
    _index usize;
    _document *JsonDocument;
}
```

### `fn __init__(this &Parser, text &const String, document *JsonDocument) void`

Creates a parser over `text`, building into `document`.
- `text`: the whole JSON text the parser reads from, copied into the
  parser
- `document`: the document nodes are added to, pointed at rather than
  copied, so it must outlive the parser

```
fn __init__(this &Parser, text &const String, document *JsonDocument) void
```

### `export fn parse(text &const String) JsonDocument except {`

Parses JSON text into a document.

```
export fn parse(text &const String) JsonDocument except {
    document JsonDocument = JsonDocument();
    dummy uint = document.add_node();
    parser Parser = Parser(text, &document);
    root_index uint = parser.parse_into()??;
    parser.skip_space();
    if (parser._index != parser._text.length()) {
        raise "ValueError": "trailing text after the JSON value";
    }
    document._root = root_index;
    return document;
}
```

### `fn add_node(this &Parser, kind JsonKind) uint`

Adds one node holding `kind` and returns its index.
- `kind`: the kind the new node is given; its value field is left as it
  is, so the caller fills it in

```
fn add_node(this &Parser, kind JsonKind) uint
```

### `fn add_text(this &Parser, value &const String) uint`

Adds a text node.
- `value`: the text the new node holds, copied, so the caller's string may
  go away afterwards

```
fn add_text(this &Parser, value &const String) uint
```

### `fn add_number(this &Parser, value f64) uint`

Adds a number node.
- `value`: the real the new node holds; JSON's integers are reals here
  too

```
fn add_number(this &Parser, value f64) uint
```

### `fn add_boolean(this &Parser, value bool) uint`

Adds a boolean node.
- `value`: the boolean the new node holds

```
fn add_boolean(this &Parser, value bool) uint
```

### `fn parse_into(this &Parser) uint except`

Reads one value at the parser's position, returning its node index.

```
fn parse_into(this &Parser) uint except
```

### `fn parse_array(this &Parser) uint except`

Reads an array, returning its node index.

```
fn parse_array(this &Parser) uint except
```

### `fn parse_text(this &Parser) String except`

Reads a string, decoding its escapes.

```
fn parse_text(this &Parser) String except
```

### `fn parse_hex4(this &Parser) uint except`

Reads the four hex digits of a `\u` escape.

```
fn parse_hex4(this &Parser) uint except
```

### `fn json_hex_digit(character char) uint except`

The value of one hex digit.
- `character`: one hex digit, `0`-`9`, `a`-`f` or `A`-`F`; anything else
  raises `ValueError`

```
fn json_hex_digit(character char) uint except
```

### `fn parse_number(this &Parser) f64 except`

Reads a number.

```
fn parse_number(this &Parser) f64 except
```

### `fn skip_space(this &Parser) void`

Skips spaces, tabs and newlines.

```
fn skip_space(this &Parser) void
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

### `export fn serialize(value &const JsonValue) String except {`

Writes a value as compact JSON text.

```
export fn serialize(value &const JsonValue) String except {
    result String = String();
    write_into(&result, value)??;
    return result;
}
```

### `fn write_into(out &String, value &const JsonValue) void except`

Appends the JSON text of one value.
- `out`: the buffer the JSON text is appended to; its existing text is
  kept, and nothing is flushed
- `value`: the value to write, with its whole subtree; the kind decides
  how

```
fn write_into(out &String, value &const JsonValue) void except
```

### `fn write_text(out &String, text_value &const String) void`

Appends a quoted JSON string.
- `out`: the buffer the quoted string is appended to
- `text_value`: the raw text to quote; a `"`, a `\`, a newline, a carriage
  return and a tab are escaped, and every other byte is written as it is

```
fn write_text(out &String, text_value &const String) void
```
