# `std.xml`

XML: elements, attributes and text, as a document of nodes.

    document XmlDocument = xml.parse(&text)??;
    root XmlValue = document.root();
    name String = root.name();
    child XmlValue = root.child(0)??;
    written String = xml.serialize(&root)??;

# Shapes

The *document* owns the nodes, one array with child indices, exactly as
`std.json` does; an `XmlValue` is a document-and-index handle. A node has a
name, its attributes, its child element indices and its own text. Mixed
content keeps its text in one run: `<p>a<b/>c</p>` has the text `ac` on the
element, which is enough for configuration and documents, and is stated here
rather than pretended to be a full DOM.

# The subset

* elements, with attributes (`<a x="1">`);
* text, with the five named entities and numeric ones (`&#65;`, `&#x41;`);
* comments and the XML declaration are skipped;
* CDATA is not supported, and neither is a DTD: a parser that guesses is
  worse than one that refuses.

# Errors

Malformed XML raises `ValueError`: a tag that does not close, a mismatched
end tag, an attribute without a value, an unknown entity.

## Declarations

### `export type XmlAttribute {`

One attribute: a name and a value.

```
export type XmlAttribute {
    // The attribute's name.
    _name String;
    // The attribute's value, with entities decoded.
    _value String;
}
```

### `export fn __init__(this &XmlAttribute) void {`

Creates an empty attribute.

```
export fn __init__(this &XmlAttribute) void {
    (&this._name)->__init__();
    (&this._value)->__init__();
}
```

### `export fn __init__(this &XmlAttribute, other &const XmlAttribute) void {`

Copies an attribute.

```
export fn __init__(this &XmlAttribute, other &const XmlAttribute) void {
    (&this._name)->__init__(other._name);
    (&this._value)->__init__(other._value);
}
```

### `export fn __init__(this &XmlAttribute, other &&XmlAttribute) void {`

Takes an attribute's storage.

```
export fn __init__(this &XmlAttribute, other &&XmlAttribute) void {
    (&this._name)->__init__();
    (&this._value)->__init__();
    this._name = util.move(other._name);
    this._value = util.move(other._value);
}
```

### `export fn __drop__(this &XmlAttribute) void {`

Releases an attribute's storage.

```
export fn __drop__(this &XmlAttribute) void {
    (&this._name)->__drop__();
    (&this._value)->__drop__();
}
```

### `export type XmlNode {`

One element of the document.

```
export type XmlNode {
    // The element's name.
    _name String;
    // The attributes, in document order.
    _attributes Array<XmlAttribute>;
    // The child elements, in document order.
    _children Array<uint>;
    // The element's own text.
    _text String;
}
```

### `export fn __init__(this &XmlNode) void {`

Creates an empty node.

```
export fn __init__(this &XmlNode) void {
    (&this._name)->__init__();
    (&this._attributes)->__init__();
    (&this._children)->__init__();
    (&this._text)->__init__();
}
```

### `export fn __init__(this &XmlNode, other &const XmlNode) void {`

Copies a node.

```
export fn __init__(this &XmlNode, other &const XmlNode) void {
    (&this._name)->__init__(other._name);
    (&this._attributes)->__init__(other._attributes);
    (&this._children)->__init__(other._children);
    (&this._text)->__init__(other._text);
}
```

### `export fn __init__(this &XmlNode, other &&XmlNode) void {`

Takes a node's storage.

```
export fn __init__(this &XmlNode, other &&XmlNode) void {
    (&this._name)->__init__();
    (&this._attributes)->__init__();
    (&this._children)->__init__();
    (&this._text)->__init__();
    this._name = util.move(other._name);
    this._attributes = util.move(other._attributes);
    this._children = util.move(other._children);
    this._text = util.move(other._text);
}
```

### `export fn __drop__(this &XmlNode) void {`

Releases a node's storage.

```
export fn __drop__(this &XmlNode) void {
    (&this._name)->__drop__();
    (&this._attributes)->__drop__();
    (&this._children)->__drop__();
    (&this._text)->__drop__();
}
```

### `export type XmlDocument {`

A parsed XML document.

```
export type XmlDocument {
    // The nodes.
    _nodes Array<XmlNode>;
    // The root element's index.
    _root uint;
}
```

### `export fn __init__(this &XmlDocument) void {`

Creates an empty document.

```
export fn __init__(this &XmlDocument) void {
    (&this._nodes)->__init__();
    this._root = 0u32;
}
```

### `export fn __init__(this &XmlDocument, other &&XmlDocument) void {`

Moves a document's storage.

```
export fn __init__(this &XmlDocument, other &&XmlDocument) void {
    (&this._nodes)->__init__();
    this._nodes = util.move(other._nodes);
    this._root = other._root;
}
```

### `export fn __drop__(this &XmlDocument) void {`

Releases the nodes.

```
export fn __drop__(this &XmlDocument) void {
    (&this._nodes)->__drop__();
}
```

### `export fn root(this &XmlDocument) XmlValue {`

The root element.

```
export fn root(this &XmlDocument) XmlValue {
    return XmlValue(this, this._root);
}
```

### `fn add_node(this &XmlDocument) uint`

Makes room for one node and returns its index.

```
fn add_node(this &XmlDocument) uint
```

### `export type XmlValue {`

A handle to one element in a document.

```
export type XmlValue {
    // The document.
    _document *XmlDocument;
    // The node's index.
    _index uint;
}
```

### `export fn __init__(this &XmlValue, document *XmlDocument, index uint) void {`

Builds a handle.

```
export fn __init__(this &XmlValue, document *XmlDocument, index uint) void {
    this._document = document;
    this._index = index;
}
```

### `fn node_of(this &const XmlValue) *XmlNode`

The node a handle names.

```
fn node_of(this &const XmlValue) *XmlNode
```

### `export fn name(this &const XmlValue) String {`

The element's name.

```
export fn name(this &const XmlValue) String {
    return this.node_of()._name;
}
```

### `export fn text(this &const XmlValue) String {`

The element's own text.

```
export fn text(this &const XmlValue) String {
    return this.node_of()._text;
}
```

### `export fn child_count(this &const XmlValue) usize {`

The number of child elements.

```
export fn child_count(this &const XmlValue) usize {
    return this.node_of()._children.length();
}
```

### `export fn child(this &const XmlValue, index usize) XmlValue except {`

The child element at `index`.

```
export fn child(this &const XmlValue, index usize) XmlValue except {
    if (index >= this.node_of()._children.length()) {
        raise "ValueError": "the element has no such child";
    }
    return XmlValue(this._document, this.node_of()._children[index]);
}
```

### `export fn attribute_count(this &const XmlValue) usize {`

The number of attributes.

```
export fn attribute_count(this &const XmlValue) usize {
    return this.node_of()._attributes.length();
}
```

### `export fn attribute_name(this &const XmlValue, index usize) String except {`

The attribute's name at `index`.

```
export fn attribute_name(this &const XmlValue, index usize) String except {
    if (index >= this.node_of()._attributes.length()) {
        raise "ValueError": "the element has no such attribute";
    }
    return this.node_of()._attributes[index]._name;
}
```

### `export fn attribute_value(this &const XmlValue, index usize) String except {`

The attribute's value at `index`.

```
export fn attribute_value(this &const XmlValue, index usize) String except {
    if (index >= this.node_of()._attributes.length()) {
        raise "ValueError": "the element has no such attribute";
    }
    return this.node_of()._attributes[index]._value;
}
```

### `export fn attribute(this &const XmlValue, key &const String) ?String {`

The value of the attribute named `key`, or an absent optional.

```
export fn attribute(this &const XmlValue, key &const String) ?String {
    index usize = 0;
    while (index < this.node_of()._attributes.length()) {
        if (this.node_of()._attributes[index]._name == key) {
            return optional.some<String>(this.node_of()._attributes[index]._value);
        }
        index += 1;
    }
    return optional.none<String>();
}
```

### `type Parser {`

The parser's state.

```
type Parser {
    _text String;
    _index usize;
    _document *XmlDocument;
}
```

### `fn __init__(this &Parser, text &const String, document *XmlDocument) void`

Creates a parser over `text`.
- `text`: the whole XML text the parser reads from, copied into the
  parser
- `document`: the document nodes are added to, pointed at rather than
  copied, so it must outlive the parser

```
fn __init__(this &Parser, text &const String, document *XmlDocument) void
```

### `export fn parse_xml(text &const String) XmlDocument except {`

Parses XML text into a document.

```
export fn parse_xml(text &const String) XmlDocument except {
    document XmlDocument = XmlDocument();
    parser Parser = Parser(text, &document);
    parser.skip_miscellaneous()??;
    if (!parser.at('<')) {
        raise "ValueError": "XML must start with an element";
    }
    root_index uint = parser.parse_element()??;
    parser.skip_miscellaneous()??;
    if (parser._index != parser._text.length()) {
        raise "ValueError": "trailing text after the root element";
    }
    document._root = root_index;
    return document;
}
```

### `fn skip_miscellaneous(this &Parser) void except`

Skips comments, the XML declaration and whitespace between elements.

```
fn skip_miscellaneous(this &Parser) void except
```

### `fn skip_until(this &Parser, closing *const char) void except`

Advances past `closing`, raising when the text ends first.
- `closing`: a null-terminated C string to scan for, such as `"-->"`; it
  and every byte before it are skipped, so the position ends up just
  past it

```
fn skip_until(this &Parser, closing *const char) void except
```

### `fn matches(this &Parser, word *const char) bool`

Whether the text at the parser matches `word`.
- `word`: a null-terminated C string compared byte for byte with the text
  at the current position

```
fn matches(this &Parser, word *const char) bool
```

### `fn starts_with(this &Parser, word *const char) bool`

Whether the text at the parser starts with `word`.
- `word`: a null-terminated C string compared byte for byte with the text
  at the current position

```
fn starts_with(this &Parser, word *const char) bool
```

### `fn parse_element(this &Parser) uint except`

Reads one element, returning its node index.

```
fn parse_element(this &Parser) uint except
```

### `fn parse_attributes(this &Parser, node *XmlNode) void except`

Reads attributes into `node` until the tag's end.
- `node`: the element the attributes are appended to, in document order

```
fn parse_attributes(this &Parser, node *XmlNode) void except
```

### `fn parse_name(this &Parser) String except`

Reads an element or attribute name.

```
fn parse_name(this &Parser) String except
```

### `fn parse_text_run(this &Parser) String except`

Reads text up to the next `<`, decoding entities.

```
fn parse_text_run(this &Parser) String except
```

### `fn decode_entities(raw &const String) String except`

Decodes the entities a text or attribute value may contain.
- `raw`: text as the document has it, entities still written out, with
  each `&` running to its `;`

```
fn decode_entities(raw &const String) String except
```

### `fn decode_entity(entity &const String) String except`

Decodes one entity name or number.
- `entity`: the bytes between the `&` and the `;`, so `lt` or `#x41` with
  neither the ampersand nor the terminator

```
fn decode_entity(entity &const String) String except
```

### `fn hex_digit(character char) uint except`

The value of one hex digit.
- `character`: one hex digit, `0`-`9`, `a`-`f` or `A`-`F`; anything else
  raises `ValueError`

```
fn hex_digit(character char) uint except
```

### `fn skip_space(this &Parser) void`

Skips whitespace.

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

### `fn expect(this &Parser, character char) void except`

Consumes `character` or raises.
- `character`: the one character the parser must be looking at; it is
  consumed, and anything else raises `ValueError`

```
fn expect(this &Parser, character char) void except
```

### `fn trim_spaces(text &String) String`

The string without its surrounding whitespace.
- `text`: the element's text, trimmed on both sides; the result is a
  fresh string, and the argument is not changed

```
fn trim_spaces(text &String) String
```

### `fn is_space(character char) bool`

Whether a character is whitespace.
- `character`: the byte to test; space, tab, newline and carriage return
  count, and nothing else does

```
fn is_space(character char) bool
```

### `fn c_length(word *const char) usize`

The length of a C string.
- `word`: a null-terminated C string; the bytes before the terminator
  are counted, and the terminator is not

```
fn c_length(word *const char) usize
```

### `export fn serialize(value &const XmlValue) String except {`

Writes an element and its children as XML text.

```
export fn serialize(value &const XmlValue) String except {
    result String = String();
    write_element(&result, value)??;
    return result;
}
```

### `fn write_element(out &String, value &const XmlValue) void except`

Appends one element.
- `out`: the buffer the element is appended to; its existing text is
  kept
- `value`: a handle to the element to write, with its whole subtree

```
fn write_element(out &String, value &const XmlValue) void except
```

### `fn write_escaped(out &String, text_value &const String) void`

Appends text with the five entities escaped.
- `out`: the buffer the text is appended to
- `text_value`: the raw text to escape; `<`, `>`, `&` and `"` become entities
  and every other byte is written as it is, `'` among them

```
fn write_escaped(out &String, text_value &const String) void
```
