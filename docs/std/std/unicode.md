# `std.unicode`

UTF-8: code points to bytes and back.

    bytes String = unicode.encode(0x00e9)??;   // "é" as two bytes
    points Array<uint> = unicode.decode(&text)??;
    n usize = unicode.count(&text);            // code points, not bytes

A `String` is a sequence of bytes. UTF-8 is the rule that says which bytes
are one character, and this module is that rule: encoding one code point,
decoding a whole text, counting code points, and saying whether the bytes are
valid UTF-8 at all.

# Errors

Encoding refuses a code point that UTF-8 cannot represent -- above
`0x10FFFF`, or a surrogate -- with `ValueError`. Decoding refuses text that is
not UTF-8: a continuation byte where a start byte belongs, a truncated
sequence, an over-long form, or a surrogate. `valid` answers the same
question without raising, for a caller that is asking rather than requiring.

## Declarations

### `MAX_CODE_POINT const uint = 0x10ffffu32;`

The largest code point Unicode has.

```
MAX_CODE_POINT const uint = 0x10ffffu32;
```

### `export fn encode(code_point uint) String except {`

Encodes one code point as UTF-8 bytes.

```
export fn encode(code_point uint) String except {
    if (code_point > MAX_CODE_POINT) {
        raise "ValueError": "the code point is above U+10FFFF";
    }
    if (code_point >= 0xd800u32 && code_point <= 0xdfffu32) {
        raise "ValueError": "a surrogate is not a code point";
    }
    text String = String();
    if (code_point < 0x80u32) {
        text.push(byte_char(code_point));
    } else if (code_point < 0x800u32) {
        text.push(byte_char(0xc0u32 | (code_point >> 6u32)));
        text.push(byte_char(0x80u32 | (code_point & 0x3fu32)));
    } else if (code_point < 0x10000u32) {
        text.push(byte_char(0xe0u32 | (code_point >> 12u32)));
        text.push(byte_char(0x80u32 | ((code_point >> 6u32) & 0x3fu32)));
        text.push(byte_char(0x80u32 | (code_point & 0x3fu32)));
    } else {
        text.push(byte_char(0xf0u32 | (code_point >> 18u32)));
        text.push(byte_char(0x80u32 | ((code_point >> 12u32) & 0x3fu32)));
        text.push(byte_char(0x80u32 | ((code_point >> 6u32) & 0x3fu32)));
        text.push(byte_char(0x80u32 | (code_point & 0x3fu32)));
    }
    return text;
}
```

### `fn byte_char(value uint) char`

One byte as a character of the string it is stored in.
- `value`: one UTF-8 byte, zero to `0xFF`, which is re-interpreted as the
  character of the same value

```
fn byte_char(value uint) char
```

### `export fn valid(text &const String) bool {`

Whether `text` is valid UTF-8.

```
export fn valid(text &const String) bool {
    index usize = 0;
    while (index < text.length()) {
        next usize = sequence_length(text, index);
        if (next == 0usize) {
            return false;
        }
        index += next;
    }
    return true;
}
```

### `export fn count(text &const String) usize {`

How many code points `text` has, not counting invalid bytes.

```
export fn count(text &const String) usize {
    total usize = 0;
    index usize = 0;
    while (index < text.length()) {
        length usize = sequence_length(text, index);
        if (length == 0usize) {
            index += 1;
        } else {
            index += length;
            total += 1;
        }
    }
    return total;
}
```

### `export fn decode(text &const String) Array<uint> except {`

Decodes every code point of `text`, raising on invalid UTF-8.

```
export fn decode(text &const String) Array<uint> except {
    points Array<uint> = Array<uint>();
    index usize = 0;
    while (index < text.length()) {
        length usize = sequence_length(text, index);
        if (length == 0usize) {
            raise "ValueError": "the text is not valid UTF-8";
        }
        points.push(code_point_at(text, index, length)??);
        index += length;
    }
    return points;
}
```

### `export fn next_code_point(text &const String, index usize) Pair<uint, usize> except {`

The code point at `index`, and how many bytes it takes.

```
export fn next_code_point(text &const String, index usize) Pair<uint, usize> except {
    length usize = sequence_length(text, index);
    if (length == 0usize) {
        raise "ValueError": "the text is not valid UTF-8";
    }
    return Pair<uint, usize>(code_point_at(text, index, length)??, length);
}
```

### `fn sequence_length(text &const String, index usize) usize`

How many bytes the sequence starting at `index` takes, or zero when the bytes
there are not a valid sequence.
- `text`: the bytes to read from
- `index`: the byte offset of a sequence's first byte, counted from zero;
  an offset past the end gives zero rather than reading

```
fn sequence_length(text &const String, index usize) usize
```

### `fn continuation(text &const String, index usize) bool`

Whether the byte at `index` is a continuation byte.
- `text`: the bytes to read from
- `index`: the byte offset to test, counted from zero; an offset past the
  end answers `false`

```
fn continuation(text &const String, index usize) bool
```

### `fn code_point_at(text &const String, index usize, length usize) uint except`

The code point a checked sequence encodes.
- `text`: the bytes of the sequence, already known to be valid UTF-8
- `index`: the byte offset the sequence starts at, counted from zero
- `length`: how many bytes the sequence takes, as `sequence_length` said
  it does; it decides how many continuation bytes are folded in

```
fn code_point_at(text &const String, index usize, length usize) uint except
```

### `fn mask_for(length usize) uint`

The bits of the first byte that belong to the code point.
- `length`: how many bytes the sequence takes, two to four; it selects the
  mask of the lead byte's payload bits

```
fn mask_for(length usize) uint
```
