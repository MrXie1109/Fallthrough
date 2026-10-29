# `std.iterator`

What a range loop asks of a type, and the hierarchy of positions.

A trait is a named set of requirements: operations that must compile for a
type, written as expressions against a parameter. A type models the trait
when every requirement compiles with the parameter standing for it.

Each trait adds one ability to the one above it, written as a question about
the same parameter: `InputIterator(it) -> true`. There is no inheritance
clause, and none is needed.

Nothing here is emitted: a trait describes what a type can already do.
Something bounded by `begin()` and `end()`.

Both must yield something, because the range loop compares them. What they
yield must model `InputIterator`.

## Declarations

### `export traits InputIterator(it Iter) {`

A position in a sequence that can be walked once.

`it` is written as a place, which is why `++it` is legal: `__inc__` takes
`&this` and modifies the receiver.

```
export traits InputIterator(it Iter) {
    ++it,
    *it -> !void,
    it != it -> bool,
}
```

### `export traits ForwardIterator(it Iter) {`

A position that can be walked more than once: two positions compare for
equality, and a copy made by `it++` stays usable.

```
export traits ForwardIterator(it Iter) {
    it == it -> bool,
    it++ -> !void,
    InputIterator(it) -> true,
}
```

### `export traits BidirectionalIterator(it Iter) {`

A position that can also go backwards.

```
export traits BidirectionalIterator(it Iter) {
    --it,
    ForwardIterator(it) -> true,
}
```

### `export traits RandomAccessIterator(it Iter) {`

A position that can be moved by an arbitrary count in one step.

The offset is a `usize`, the type indexing takes, so `it + 1` adopts it at
the call site. `+=` and `-=` are required as well, because a walk uses them.

```
export traits RandomAccessIterator(it Iter) {
    it + 1,
    it - 1,
    it += 1,
    it -= 1,
    it[1],
    it < it -> bool,
    BidirectionalIterator(it) -> true,
}
```
