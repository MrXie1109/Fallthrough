# `std.compress`

Compression: adler32 and the zlib "stored" format.

    packed Bytes = compress.deflate_stored(&plain);
    plain2 Bytes = compress.inflate_stored(&packed)??;

# What this is

A complete zlib stream is a header, a series of deflate *blocks*, and an
adler32 of the data. Blocks come in three kinds: stored (the bytes as they
are), fixed-Huffman and dynamic-Huffman. This module writes and reads the
stored kind only: it is the format a stream is allowed to be, it needs no
Huffman table, and it makes the container -- header, framing, checksum --
exact and testable.

The two other block kinds are refused when read, rather than mis-decoded. A
decoder that says "this is not a stored-block stream" is honest; a general
inflater is a large piece of work that belongs in its own module, not hidden
behind a name that suggests it already exists.

# Errors

`inflate_stored` raises `ValueError` for a bad header, a bad block length, a
compressed block, truncated data, or a checksum that does not match.

## Declarations

### `MAX_STORED const usize = 65535usize;`

The largest length a stored block may declare.

```
MAX_STORED const usize = 65535usize;
```

### `export fn adler32(data &const Bytes) uint {`

The adler32 checksum of `data`.

```
export fn adler32(data &const Bytes) uint {
    first uint = 1u32;
    second uint = 0u32;
    index usize = 0;
    while (index < data.length()) {
        first = (first + data[index] as uint) % 65521u32;
        second = (second + first) % 65521u32;
        index += 1;
    }
    return (second << 16) | first;
}
```

### `fn push_u16(out &Bytes, value uint) void`

Pushes `value` as a little-endian 16-bit number.

- `out`: the stream the two bytes are appended to
- `value`: the number, of which only the low sixteen bits are written

```
fn push_u16(out &Bytes, value uint) void
```

### `fn push_u32_be(out &Bytes, value uint) void`

Pushes `value` as a big-endian 32-bit number.

- `out`: the stream the four bytes are appended to
- `value`: the number, of which only the low thirty-two bits are written

```
fn push_u32_be(out &Bytes, value uint) void
```

### `export fn deflate_stored(data &const Bytes) Bytes {`

Packs `data` as a zlib stream of stored blocks.

```
export fn deflate_stored(data &const Bytes) Bytes {
    packed Bytes = Bytes();
    // CMF: deflate, 32K window. FLG: no dictionary, fastest level, and the
    // check bits that make the header a multiple of 31.
    packed.push(0x78 as byte);
    packed.push(0x01 as byte);
    offset usize = 0;
    while (true) {
        remaining usize = data.length() - offset;
        length usize = remaining;
        if (length > MAX_STORED) {
            length = MAX_STORED;
        }
        last bool = offset + length >= data.length();
        if (last) {
            packed.push(0x01 as byte);
        } else {
            packed.push(0x00 as byte);
        }
        push_u16(&packed, length as uint);
        push_u16(&packed, (length as uint ^ 0xffffu32) & 0xffffu32);
        index usize = 0;
        while (index < length) {
            value byte = data[offset + index] as byte;
            packed.push(value);
            index += 1;
        }
        offset += length;
        if (last) {
            break;
        }
    }
    push_u32_be(&packed, adler32(data));
    return packed;
}
```

### `fn read_u16(data &const Bytes, offset usize) uint except`

Reads a little-endian 16-bit number at `offset`.

- `data`: the stream to read from
- `offset`: the byte the number starts at; it must leave two bytes

```
fn read_u16(data &const Bytes, offset usize) uint except
```

### `fn read_u32_be(data &const Bytes, offset usize) uint except`

Reads a big-endian 32-bit number at `offset`.

- `data`: the stream to read from
- `offset`: the byte the number starts at; it must leave four bytes

```
fn read_u32_be(data &const Bytes, offset usize) uint except
```

### `export fn inflate_stored(data &const Bytes) Bytes except {`

Unpacks a zlib stream of stored blocks.

```
export fn inflate_stored(data &const Bytes) Bytes except {
    if (data.length() < 2usize) {
        raise "ValueError": "the stream is too short to be zlib";
    }
    header uint = ((data[0] as uint) << 8) | (data[1] as uint);
    if ((header % 31u32) != 0u32) {
        raise "ValueError": "the zlib header check failed";
    }
    method uint = (data[0] as uint) & 0x0fu32;
    if (method != 8u32) {
        raise "ValueError": "the stream is not deflate";
    }
    plain Bytes = Bytes();
    offset usize = 2usize;
    while (true) {
        if (offset >= data.length()) {
            raise "ValueError": "the stream ended before its last block";
        }
        header_byte uint = data[offset] as uint;
        offset += 1;
        last bool = (header_byte & 0x01u32) == 1u32;
        block_kind uint = (header_byte >> 1) & 0x03u32;
        if (block_kind != 0u32) {
            raise "ValueError": "the stream uses a compressed block, which this module does not read";
        }
        length uint = read_u16(data, offset)??;
        check uint = read_u16(data, offset + 2usize)??;
        offset += 4usize;
        if ((length ^ check) != 0xffffu32) {
            raise "ValueError": "a stored block's length and its complement disagree";
        }
        if (offset + length as usize > data.length()) {
            raise "ValueError": "the stream ended inside a stored block";
        }
        index uint = 0u32;
        while (index < length) {
            value byte = data[offset + index as usize] as byte;
            plain.push(value);
            index += 1u32;
        }
        offset += length as usize;
        if (last) {
            break;
        }
    }
    if ((offset + 4usize) != data.length()) {
        raise "ValueError": "trailing bytes after the last block";
    }
    expected uint = read_u32_be(data, offset)??;
    if (expected != adler32(&plain)) {
        raise "ValueError": "the adler32 checksum does not match";
    }
    return plain;
}
```
