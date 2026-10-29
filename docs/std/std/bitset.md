# `std.bitset`

A fixed number of bits, packed into words.

    bits Bitset = Bitset(64usize);
    bits.set(3);
    if (bits.test(3)) { ... }
    n usize = bits.count_ones();

The bits are stored in `u64` words, so 64 bits fit one word and a set or test
is one shift and one mask. The length is fixed at construction: a bitset is a
small fixed-size collection, and `resize` changes the size when a program
needs a different one.

# Bounds

`set`, `clear_bit`, `toggle` and `test` panic on an index past the length.
That is the same choice `Array` makes for `[]`: a bit that does not exist has
no value, and returning zero for it would hide an off-by-one.

## Declarations

### `export type Bitset {`

A fixed-size set of bits.

```
export type Bitset {
    // The bits, 64 per word.
    _words Array<u64>;
    // How many bits are meaningful.
    _bits usize;
}
```

### `export fn __init__(this &Bitset, bits usize) void {`

Creates a bitset of `bits` bits, all clear.

```
export fn __init__(this &Bitset, bits usize) void {
    (&this._words)->__init__();
    this._bits = bits;
    words usize = (bits + 63usize) / 64usize;
    index usize = 0;
    while (index < words) {
        this._words.push(0u64);
        index += 1;
    }
}
```

### `export fn __init__(this &Bitset, other &const Bitset) void {`

Copies another bitset.

```
export fn __init__(this &Bitset, other &const Bitset) void {
    (&this._words)->__init__();
    this._bits = other._bits;
    index usize = 0;
    while (index < other._words.length()) {
        this._words.push(other._words[index]);
        index += 1;
    }
}
```

### `export fn __drop__(this &Bitset) void {`

Releases the words.

```
export fn __drop__(this &Bitset) void {
    (&this._words)->__drop__();
}
```

### `export fn length(this &const Bitset) usize {`

The number of bits.

```
export fn length(this &const Bitset) usize {
    return this._bits;
}
```

### `fn word_of(index usize) usize`

Shifts `index` into its word.

- `index`: the bit's number, counting from zero

```
fn word_of(index usize) usize
```

### `fn bit_of(index usize) u64`

The bit's position inside its word, as a mask.

- `index`: the bit's number, counting from zero

```
fn bit_of(index usize) u64
```

### `export fn set(this &Bitset, index usize) bool {`

Sets the bit at `index` and returns its previous value.

```
export fn set(this &Bitset, index usize) bool {
    if (index >= this._bits) {
        util.panic("Bitset index out of range");
    }
    word usize = word_of(index);
    mask u64 = bit_of(index);
    previous bool = (this._words[word] & mask) != 0u64;
    this._words[word] = this._words[word] | mask;
    return previous;
}
```

### `export fn clear_bit(this &Bitset, index usize) bool {`

Clears the bit at `index` and returns its previous value.

```
export fn clear_bit(this &Bitset, index usize) bool {
    if (index >= this._bits) {
        util.panic("Bitset index out of range");
    }
    word usize = word_of(index);
    mask u64 = bit_of(index);
    previous bool = (this._words[word] & mask) != 0u64;
    this._words[word] = this._words[word] & (0u64 - mask - 1u64);
    return previous;
}
```

### `export fn toggle(this &Bitset, index usize) bool {`

Flips the bit at `index` and returns its previous value.

```
export fn toggle(this &Bitset, index usize) bool {
    if (index >= this._bits) {
        util.panic("Bitset index out of range");
    }
    word usize = word_of(index);
    mask u64 = bit_of(index);
    previous bool = (this._words[word] & mask) != 0u64;
    this._words[word] = this._words[word] ^ mask;
    return previous;
}
```

### `export fn test(this &const Bitset, index usize) bool {`

Whether the bit at `index` is set.

```
export fn test(this &const Bitset, index usize) bool {
    if (index >= this._bits) {
        util.panic("Bitset index out of range");
    }
    return (this._words[word_of(index)] & bit_of(index)) != 0u64;
}
```

### `export fn count_ones(this &const Bitset) usize {`

How many bits are set.

```
export fn count_ones(this &const Bitset) usize {
    total usize = 0;
    index usize = 0;
    while (index < this._words.length()) {
        total += popcount(this._words[index]);
        index += 1;
    }
    return total;
}
```

### `export fn count_zeros(this &const Bitset) usize {`

How many bits are clear.

```
export fn count_zeros(this &const Bitset) usize {
    return this._bits - this.count_ones();
}
```

### `export fn none(this &const Bitset) bool {`

Whether no bit is set.

```
export fn none(this &const Bitset) bool {
    return this.count_ones() == 0;
}
```

### `export fn any(this &const Bitset) bool {`

Whether any bit is set.

```
export fn any(this &const Bitset) bool {
    return this.count_ones() > 0;
}
```

### `export fn clear(this &Bitset) void {`

Clears every bit.

```
export fn clear(this &Bitset) void {
    index usize = 0;
    while (index < this._words.length()) {
        this._words[index] = 0u64;
        index += 1;
    }
}
```

### `export fn set_all(this &Bitset) void {`

Sets every bit.

```
export fn set_all(this &Bitset) void {
    index usize = 0;
    while (index < this._words.length()) {
        this._words[index] = 0xffffffffffffffffu64;
        index += 1;
    }
}
```

### `export fn merge(this &Bitset, other &const Bitset) void {`

Sets the bits of `other` in this one, a union in place.

```
export fn merge(this &Bitset, other &const Bitset) void {
    index usize = 0;
    limit usize = this._words.length();
    if (other._words.length() < limit) {
        limit = other._words.length();
    }
    while (index < limit) {
        this._words[index] = this._words[index] | other._words[index];
        index += 1;
    }
}
```

### `export fn intersect(this &Bitset, other &const Bitset) void {`

Keeps only the bits `other` has, an intersection in place.

```
export fn intersect(this &Bitset, other &const Bitset) void {
    index usize = 0;
    while (index < this._words.length()) {
        if (index < other._words.length()) {
            this._words[index] = this._words[index] & other._words[index];
        } else {
            this._words[index] = 0u64;
        }
        index += 1;
    }
}
```

### `export fn __eq__(this &const Bitset, other &const Bitset) bool {`

Whether the same bits are set.

```
export fn __eq__(this &const Bitset, other &const Bitset) bool {
    if (this._bits != other._bits) {
        return false;
    }
    index usize = 0;
    while (index < this._words.length()) {
        if (this._words[index] != other._words[index]) {
            return false;
        }
        index += 1;
    }
    return true;
}
```

### `export fn __ne__(this &const Bitset, other &const Bitset) bool {`

Whether different bits are set.

```
export fn __ne__(this &const Bitset, other &const Bitset) bool {
    return !(this == other);
}
```

### `fn popcount(value u64) usize`

The number of set bits in one word, by parallel count.

- `value`: the word to count

```
fn popcount(value u64) usize
```
