# `std.tuple`

Tuples: a fixed, ordered group of values with names for the positions.

A tuple is a small type with one field per element: `Pair<int, String>` has
`first` and `second`, and `Triple<A, B, C>` adds `third`. The fields are
public, so a program reads and writes the positions directly; the
constructors and the copy/move control exist because the elements may own
storage -- a `Pair<String, int>` copies its text when it is copied, rather
than sharing the string's buffer and freeing it twice.

    point Pair<int, int> = Pair<int, int>(3, 4);
    std.print("{} {}\n", point.first, point.second)??;

# Why not a built-in

A tuple is a type family, which the language already has: the fields, the
constructors, the copy and the comparison are what `template <..> type` and
its methods express. Nothing about a tuple needs the compiler, so it lives in
the library where its element-wise rules can be read.

## Declarations

### `type Pair {`

Two values, named by position.

```
export
template <A type, B type>
type Pair {
    // The first value.
    first A;
    // The second value.
    second B;
}
```

### `fn __init__(this &Pair<A, B>, first A, second B) void`

Creates a pair from its values.

- `first`: the first element; an owning element is copied rather than taken
- `second`: the second element, copied the same way

```
export
template <A type, B type>
fn __init__(this &Pair<A, B>, first A, second B) void
```

### `fn __init__(this &Pair<A, B>, other &const Pair<A, B>) void`

Copies another pair, element by element.

- `other`: the pair to copy; it keeps what it holds

```
export
template <A type, B type>
fn __init__(this &Pair<A, B>, other &const Pair<A, B>) void
```

### `fn __init__(this &Pair<A, B>, other &&Pair<A, B>) void`

Takes another pair's elements.

- `other`: the pair to take from, left with moved-from elements

```
export
template <A type, B type>
fn __init__(this &Pair<A, B>, other &&Pair<A, B>) void
```

### `fn __assign__(this &Pair<A, B>, other &const Pair<A, B>) void`

Copies another pair into this one.

- `other`: the pair to copy; it keeps what it holds

```
export
template <A type, B type>
fn __assign__(this &Pair<A, B>, other &const Pair<A, B>) void
```

### `fn __assign__(this &Pair<A, B>, other &&Pair<A, B>) void`

Moves another pair's elements into this one.

- `other`: the pair to take from, left with moved-from elements

```
export
template <A type, B type>
fn __assign__(this &Pair<A, B>, other &&Pair<A, B>) void
```

### `fn __drop__(this &Pair<A, B>) void`

Releases both elements.

```
export
template <A type, B type>
fn __drop__(this &Pair<A, B>) void
```

### `fn __eq__(this &const Pair<A, B>, other &const Pair<A, B>) bool`

Whether two pairs hold equal elements.

- `other`: the pair to compare against, element by element

```
export
template <A type, B type>
fn __eq__(this &const Pair<A, B>, other &const Pair<A, B>) bool
```

### `fn __ne__(this &const Pair<A, B>, other &const Pair<A, B>) bool`

Whether two pairs differ.

- `other`: the pair to compare against; any differing element makes it true

```
export
template <A type, B type>
fn __ne__(this &const Pair<A, B>, other &const Pair<A, B>) bool
```

### `type Triple {`

Three values, named by position.

```
export
template <A type, B type, C type>
type Triple {
    // The first value.
    first A;
    // The second value.
    second B;
    // The third value.
    third C;
}
```

### `fn __init__(this &Triple<A, B, C>, first A, second B, third C) void`

Creates a triple from its values.

- `first`: the first element; an owning element is copied rather than taken
- `second`: the second element, copied the same way
- `third`: the third element, copied the same way

```
export
template <A type, B type, C type>
fn __init__(this &Triple<A, B, C>, first A, second B, third C) void
```

### `fn __init__(this &Triple<A, B, C>, other &const Triple<A, B, C>) void`

Copies another triple, element by element.

- `other`: the triple to copy; it keeps what it holds

```
export
template <A type, B type, C type>
fn __init__(this &Triple<A, B, C>, other &const Triple<A, B, C>) void
```

### `fn __init__(this &Triple<A, B, C>, other &&Triple<A, B, C>) void`

Takes another triple's elements.

- `other`: the triple to take from, left with moved-from elements

```
export
template <A type, B type, C type>
fn __init__(this &Triple<A, B, C>, other &&Triple<A, B, C>) void
```

### `fn __assign__(this &Triple<A, B, C>, other &const Triple<A, B, C>) void`

Copies another triple into this one.

- `other`: the triple to copy; it keeps what it holds

```
export
template <A type, B type, C type>
fn __assign__(this &Triple<A, B, C>, other &const Triple<A, B, C>) void
```

### `fn __assign__(this &Triple<A, B, C>, other &&Triple<A, B, C>) void`

Moves another triple's elements into this one.

- `other`: the triple to take from, left with moved-from elements

```
export
template <A type, B type, C type>
fn __assign__(this &Triple<A, B, C>, other &&Triple<A, B, C>) void
```

### `fn __drop__(this &Triple<A, B, C>) void`

Releases all three elements.

```
export
template <A type, B type, C type>
fn __drop__(this &Triple<A, B, C>) void
```

### `fn __eq__(this &const Triple<A, B, C>, other &const Triple<A, B, C>) bool`

Whether two triples hold equal elements.

- `other`: the triple to compare against, element by element

```
export
template <A type, B type, C type>
fn __eq__(this &const Triple<A, B, C>, other &const Triple<A, B, C>) bool
```
