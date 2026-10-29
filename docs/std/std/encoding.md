# `std.encoding`

Encodings: hex, base64, and URL percent-encoding.

The three are the representations a byte sequence takes when it has to travel
as text: hex for a dump, base64 for a header or a JSON string, percent for a
URL. Each has an encode that always succeeds and a decode that raises
`ValueError` on text that is not that encoding.

    hex String = hex_encode(&bytes);        // "4142"
    back Bytes = hex_decode(&hex)??;         // the same bytes

# Not a codec

This is not text encoding: the input of `encode` is `Bytes` and the output of
`decode` is `Bytes`, because the bytes need not be text at all. Converting
text to and from UTF-8 is `std.unicode`'s job.

## Declarations

### `export fn hex_encode(bytes &const Bytes) String {`

Encodes bytes as lowercase hex.

```
export fn hex_encode(bytes &const Bytes) String {
    text String = String();
    index usize = 0;
    while (index < bytes.length()) {
        value u8 = bytes[index];
        text.push(hex_digit((value >> 4u8) as u8));
        text.push(hex_digit(value & 0x0fu8));
        index += 1;
    }
    return text;
}
```

### `fn hex_digit(value u8) char`

One lowercase hex digit.
- `value`: the four-bit value to write, zero to fifteen; a larger one
  keeps only its low four bits

```
fn hex_digit(value u8) char
```

### `export fn hex_decode(text &const String) Bytes except {`

Decodes lowercase or uppercase hex, raising on anything else.

```
export fn hex_decode(text &const String) Bytes except {
    if (text.length() % 2 != 0) {
        raise "ValueError": "hex text has an odd number of characters";
    }
    result Bytes = Bytes();
    index usize = 0;
    while (index < text.length()) {
        high u8 = hex_value(text[index])??;
        low u8 = hex_value(text[index + 1])??;
        result.push((high * 16u8 + low) as byte);
        index += 2;
    }
    return result;
}
```

### `fn hex_value(character char) u8 except`

The value of one hex character.
- `character`: one hex digit, `0`-`9`, `a`-`f` or `A`-`F`; anything else
  raises `ValueError`

```
fn hex_value(character char) u8 except
```

### `export fn base64_encode(bytes &const Bytes) String {`

Encodes bytes as standard base64, with `=` padding.

```
export fn base64_encode(bytes &const Bytes) String {
    text String = String();
    index usize = 0;
    while (index + 3 <= bytes.length()) {
        block uint = ((bytes[index] as uint) << 16u32)
            | ((bytes[index + 1] as uint) << 8u32)
            | (bytes[index + 2] as uint);
        text.push(base64_char((block >> 18u32) as u8));
        text.push(base64_char((block >> 12u32) as u8));
        text.push(base64_char((block >> 6u32) as u8));
        text.push(base64_char(block as u8));
        index += 3;
    }
    remaining usize = bytes.length() - index;
    if (remaining == 1) {
        block uint = (bytes[index] as uint) << 16u32;
        text.push(base64_char((block >> 18u32) as u8));
        text.push(base64_char((block >> 12u32) as u8));
        text.push('=');
        text.push('=');
    } else if (remaining == 2) {
        block uint = ((bytes[index] as uint) << 16u32) | ((bytes[index + 1] as uint) << 8u32);
        text.push(base64_char((block >> 18u32) as u8));
        text.push(base64_char((block >> 12u32) as u8));
        text.push(base64_char((block >> 6u32) as u8));
        text.push('=');
    }
    return text;
}
```

### `fn base64_char(value u8) char`

One base64 alphabet character from its six-bit value.
- `value`: the six-bit value to write, zero to sixty-three; a larger one
  keeps only its low six bits

```
fn base64_char(value u8) char
```

### `export fn base64_decode(text &const String) Bytes except {`

Decodes standard base64, raising on anything else.

```
export fn base64_decode(text &const String) Bytes except {
    result Bytes = Bytes();
    accumulator uint = 0u32;
    bits uint = 0u32;
    index usize = 0;
    while (index < text.length()) {
        character char = text[index];
        if (character == '=') {
            break;
        }
        value uint = base64_value(character)??;
        accumulator = (accumulator << 6u32) | value;
        bits += 6u32;
        if (bits >= 8u32) {
            bits -= 8u32;
            result.push(((accumulator >> bits) & 0xffu32) as byte);
        }
        index += 1;
    }
    return result;
}
```

### `fn base64_value(character char) uint except`

The six-bit value of one base64 character.
- `character`: one character of the standard alphabet, `A`-`Z`, `a`-`z`,
  `0`-`9`, `+` or `/`; anything else raises `ValueError`

```
fn base64_value(character char) uint except
```

### `export fn percent_encode(text &const String) String {`

Encodes text for a URL: letters, digits and `- _ . ~` stay, everything else
becomes `%XX`.

```
export fn percent_encode(text &const String) String {
    result String = String();
    index usize = 0;
    while (index < text.length()) {
        character char = text[index];
        if (unreserved(character)) {
            result.push(character);
        } else {
            result.push('%');
            result.push(percent_digit(((character as int) >> 4) as u8));
            result.push(percent_digit(((character as int)) as u8 & 0x0fu8));
        }
        index += 1;
    }
    return result;
}
```

### `fn unreserved(character char) bool`

Whether a character may stand for itself in a URL.
- `character`: the character to test; a letter, a digit or one of `- _ . ~`
  stands for itself, and every other value needs a `%XX` escape

```
fn unreserved(character char) bool
```

### `fn percent_digit(value u8) char`

One uppercase digit of a percent escape.
- `value`: the four-bit value to write, zero to fifteen; a larger one
  keeps only its low four bits

```
fn percent_digit(value u8) char
```

### `export fn percent_decode(text &const String) Bytes except {`

Decodes `%XX` escapes, raising on anything else.

```
export fn percent_decode(text &const String) Bytes except {
    result Bytes = Bytes();
    index usize = 0;
    while (index < text.length()) {
        character char = text[index];
        if (character == '%') {
            if (index + 2 >= text.length()) {
                raise "ValueError": "a percent escape is incomplete";
            }
            high u8 = hex_value(text[index + 1])??;
            low u8 = hex_value(text[index + 2])??;
            result.push((high * 16u8 + low) as byte);
            index += 3;
        } else {
            result.push(character as byte);
            index += 1;
        }
    }
    return result;
}
```
