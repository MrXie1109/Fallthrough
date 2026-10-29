# `std.uuid`

UUIDs: generating version 4, parsing and printing.

    identifier Uuid = Uuid()??;
    text String = identifier as String;
    same bool = identifier == uuid.parse(&text)??;

# Storage

A UUID is sixteen bytes, kept as two 64-bit words so that a value is a pair of
integers: copying, comparing and returning one needs no ownership. The textual
form is the usual `8-4-4-4-12` lowercase hex.

# Versions

Only version 4 (random) is generated. `parse` accepts any version and reports
it with `version`; a program that must reject an older name-space UUID is the
one that knows why.

## Declarations

### `export type Uuid {`

A universally unique identifier.

```
export type Uuid {
    // The first eight bytes.
    _high u64;
    // The last eight bytes.
    _low u64;
}
```

### `export fn __init__(this &Uuid) void except {`

Creates a version 4 (random) UUID from the system's entropy.

```
export fn __init__(this &Uuid) void except {
    generator Random = Random();
    if (!generator.seed_from_entropy()??) {
        raise "UuidError": "the entropy source could not be read";
    }
    first_word uint = generator();
    second_word uint = generator();
    third_word uint = generator();
    fourth_word uint = generator();
    // Version 4 is the high nibble of byte six, which is the second byte of the
    // second word.
    second_word = (second_word & 0xffff0fffu32) | 0x00004000u32;
    // The RFC 4122 variant is the two high bits of byte eight, the first byte of
    // the third word.
    third_word = (third_word & 0x3fffffffu32) | 0x80000000u32;
    this._high = ((first_word as u64) << 32) | (second_word as u64);
    this._low = ((third_word as u64) << 32) | (fourth_word as u64);
}
```

### `export fn __as__(this &const Uuid, target *String) String {`

The text of a UUID: eight, four, four, four and twelve hex digits.

```
export fn __as__(this &const Uuid, target *String) String {
    text String = String();
    push_group(&text, (this._high >> 32) & 0xffffffffu64, 8usize);
    text.push('-');
    push_group(&text, (this._high >> 16) & 0xffffu64, 4usize);
    text.push('-');
    push_group(&text, this._high & 0xffffu64, 4usize);
    text.push('-');
    push_group(&text, (this._low >> 48) & 0xffffu64, 4usize);
    text.push('-');
    push_group(&text, this._low & 0xffffffffffffu64, 12usize);
    return text;
}
```

### `fn hex_char(digit uint) char`

One hexadecimal digit.

- `digit`: the value, 0 through 15

```
fn hex_char(digit uint) char
```

### `fn push_group(text &String, value u64, digits usize) void`

Appends the lowest `digits` hex digits of `value`.

- `text`: the string the digits are appended to
- `value`: the number to write
- `digits`: how many hex digits to write, counting from the lowest

```
fn push_group(text &String, value u64, digits usize) void
```

### `export fn parse_uuid(text &const String) Uuid except {`

Parses the textual form of a UUID.

```
export fn parse_uuid(text &const String) Uuid except {
    if (text.length() != 36usize) {
        raise "UuidError": "a UUID is thirty-six characters";
    }
    high u64 = 0u64;
    low u64 = 0u64;
    index usize = 0;
    count usize = 0;
    while (index < text.length()) {
        character char = text[index] as char;
        if (character == '-') {
            if (index != 8usize && index != 13usize && index != 18usize && index != 23usize) {
                raise "UuidError": "a UUID has dashes at fixed places";
            }
            index += 1;
            continue;
        }
        digits u64 = hex_digit(character)?? as u64;
        if (count < 16usize) {
            high = (high << 4) | digits;
        } else {
            low = (low << 4) | digits;
        }
        count += 1;
        index += 1;
    }
    if (count != 32usize) {
        raise "UuidError": "a UUID has thirty-two hex digits";
    }
    // The dashes are the last four digits of `high` region boundaries, so the
    // text's first sixteen digits are `high` and the rest are `low`.
    identifier Uuid = .{ high, low };
    return identifier;
}
```

### `fn hex_digit(character char) uint except`

The value of one hex digit.

- `character`: the digit, in either case

```
fn hex_digit(character char) uint except
```

### `export fn version(this &const Uuid) int {`

The version nibble: four for a generated UUID.

```
export fn version(this &const Uuid) int {
    return ((this._high >> 12) & 0x0fu64) as int;
}
```

### `export fn variant(this &const Uuid) int {`

The variant: two for the RFC 4122 form.

```
export fn variant(this &const Uuid) int {
    return ((this._low >> 62) & 0x03u64) as int;
}
```

### `export fn __eq__(this &const Uuid, other &const Uuid) bool {`

Whether two UUIDs name the same value.

```
export fn __eq__(this &const Uuid, other &const Uuid) bool {
    return this._high == other._high && this._low == other._low;
}
```

### `export fn __ne__(this &const Uuid, other &const Uuid) bool {`

Whether two UUIDs differ.

```
export fn __ne__(this &const Uuid, other &const Uuid) bool {
    return !(this == other);
}
```

### `export fn __str__(this &const Uuid) String {`

The text of a UUID, so `{}` prints it in the usual form.

```
export fn __str__(this &const Uuid) String {
    return this as String;
}
```
