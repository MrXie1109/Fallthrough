# `std.crypto`

SHA-256 and HMAC-SHA-256.

    digest Bytes = crypto.sha256(&message);
    hex String = crypto.sha256_hex(&message);
    mac String = crypto.hmac_sha256_hex(&key, &message);

# What this is, and is not

SHA-256 and its HMAC are implemented from the specification and tested against
the standard vectors ("abc", the empty message, and the RFC 4231 MAC cases).
This is *not* a cryptography library: there is no constant-time discipline for
comparisons, no key management, and no opinion about where a digest is
acceptable. A program that needs a security guarantee needs more than a hash.

# Scope

SHA-256 only. SHA-1 and MD5 are deliberately absent rather than provided with a
warning: a caller that wants a digest reaches for the one this module has, and
a module that says which hash it offers does not invite the older ones.

## Declarations

### `type Sha256 {`

The working state.

```
type Sha256 {
    first uint;
    second uint;
    third uint;
    fourth uint;
    fifth uint;
    sixth uint;
    seventh uint;
    eighth uint;
}
```

### `fn initial_state() Sha256`

The SHA-256 initial state: the fractional parts of the square roots of the
first eight primes.

```
fn initial_state() Sha256
```

### `fn round_constants() Array<uint>`

The sixty-four round constants: the fractional parts of the cube roots of the
first sixty-four primes.

```
fn round_constants() Array<uint>
```

### `fn rotate_right(value uint, count int) uint`

Rotates `value` right by `count` bits.

- `value`: the word to rotate
- `count`: how many places, between 1 and 31

```
fn rotate_right(value uint, count int) uint
```

### `fn sigma0(value uint) uint`

The first sigma of the message schedule.
- `value`: the word to mix

```
fn sigma0(value uint) uint
```

### `fn sigma1(value uint) uint`

The second sigma of the message schedule.
- `value`: the word to mix

```
fn sigma1(value uint) uint
```

### `fn epsilon0(value uint) uint`

The first sigma of the round function.
- `value`: the word to mix

```
fn epsilon0(value uint) uint
```

### `fn epsilon1(value uint) uint`

The second sigma of the round function.
- `value`: the word to mix

```
fn epsilon1(value uint) uint
```

### `fn big_sigma0(value uint) uint`

The big sigma the round adds.
- `value`: the word to mix

```
fn big_sigma0(value uint) uint
```

### `fn big_sigma1(value uint) uint`

The big sigma the round adds.
- `value`: the word to mix

```
fn big_sigma1(value uint) uint
```

### `fn compress(message &const Bytes) Sha256`

Runs the compression function over the padded message, returning the state.

- `message`: the bytes to hash, not yet padded

```
fn compress(message &const Bytes) Sha256
```

### `export fn sha256(message &const Bytes) Bytes {`

Hashes `message` with SHA-256, returning the 32 digest bytes.

```
export fn sha256(message &const Bytes) Bytes {
    state Sha256 = compress(message);
    return digest_bytes(state);
}
```

### `export fn sha256_state(message &const Bytes) Array<uint> {`

The eight state words, for tests that compare against a reference.

```
export fn sha256_state(message &const Bytes) Array<uint> {
    state Sha256 = compress(message);
    words Array<uint> = Array<uint>();
    words.push(state.first);
    words.push(state.second);
    words.push(state.third);
    words.push(state.fourth);
    words.push(state.fifth);
    words.push(state.sixth);
    words.push(state.seventh);
    words.push(state.eighth);
    return words;
}
```

### `fn pad(message &const Bytes) Bytes`

Pads a message: a one bit, zeros, and the length in bits, big-endian.

- `message`: the bytes to pad

```
fn pad(message &const Bytes) Bytes
```

### `fn digest_bytes(state Sha256) Bytes`

Writes the state as thirty-two big-endian bytes.

- `state`: the eight words the compression function left

```
fn digest_bytes(state Sha256) Bytes
```

### `fn push_word(out &Bytes, value uint) void`

Appends one word, most significant byte first.

- `out`: the stream the four bytes are appended to
- `value`: the word to write

```
fn push_word(out &Bytes, value uint) void
```

### `export fn hex(data &const Bytes) String {`

The lowercase hexadecimal text of `data`.

```
export fn hex(data &const Bytes) String {
    digits String = String("0123456789abcdef");
    text String = String();
    index usize = 0;
    while (index < data.length()) {
        value uint = data[index] as uint;
        text.push(digits[(value >> 4) as usize]);
        text.push(digits[(value & 0x0fu32) as usize]);
        index += 1;
    }
    return text;
}
```

### `export fn sha256_hex(message &const Bytes) String {`

The SHA-256 digest of `message` as lowercase hex.

```
export fn sha256_hex(message &const Bytes) String {
    digest Bytes = sha256(message);
    return hex(&digest);
}
```

### `export fn hmac_sha256(key &const Bytes, message &const Bytes) Bytes {`

HMAC-SHA-256 over `message` with `key`.

RFC 2104's construction: hash the key into a block, exclusive-or it with the
inner and outer pads, and hash `H((key ^ opad) || H((key ^ ipad) || message))`.

```
export fn hmac_sha256(key &const Bytes, message &const Bytes) Bytes {
    block Bytes = Bytes();
    index usize = 0usize;
    if (key.length() > 64usize) {
        digest Bytes = sha256(key);
        index usize = 0;
        while (index < digest.length()) {
            value byte = digest[index] as byte;
            block.push(value);
            index += 1;
        }
    } else {
        index = 0;
        while (index < key.length()) {
            value byte = key[index] as byte;
            block.push(value);
            index += 1;
        }
    }
    while (block.length() < 64usize) {
        block.push(0 as byte);
    }
    inner Bytes = Bytes();
    outer Bytes = Bytes();
    index = 0;
    while (index < 64usize) {
        inner_value byte = block[index] as byte;
        inner.push((inner_value as uint ^ 0x36u32) as byte);
        outer.push((inner_value as uint ^ 0x5cu32) as byte);
        index += 1;
    }
    index = 0;
    while (index < message.length()) {
        value byte = message[index] as byte;
        inner.push(value);
        index += 1;
    }
    inner_digest Bytes = sha256(&inner);
    index = 0;
    while (index < inner_digest.length()) {
        value byte = inner_digest[index] as byte;
        outer.push(value);
        index += 1;
    }
    return sha256(&outer);
}
```

### `export fn hmac_sha256_hex(key &const Bytes, message &const Bytes) String {`

The HMAC-SHA-256 of `message` with `key` as lowercase hex.

```
export fn hmac_sha256_hex(key &const Bytes, message &const Bytes) String {
    mac Bytes = hmac_sha256(key, message);
    return hex(&mac);
}
```
