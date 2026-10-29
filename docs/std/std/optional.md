# `std.optional`

A value that may be absent.

`?T` is the short spelling of `Optional<T>`, and the reason the type exists:
a function that can fail to produce a `T` has two honest answers -- raise, or
return "there is no value". The exception covers the first; this type covers
the second, and `value()` bridges the two by raising when there is nothing.

    find_name(people, "ada");      // `?Person`
    name ?String = ...;
    if (name) {
        std.print("found {}\n", name.value())??;
    }

# Absence is a value

Unlike a pointer, `?T` holds its `T` inline: no allocation, and no null to
mistake for a valid value. A `?T` starts absent; constructing it from a `T`
makes it present, and `reset()` makes it absent again. Reading a present
`?T` is `value()`, which raises `NoneError` on an absent one, so the failure
travels the same path as every other failure in the program.

The comparison is structural: two optionals are equal when both are absent,
or when both are present and their values are equal.
A value that may be absent.

## Declarations

### `fn __init__(this &Optional<T>) void`

Creates an absent optional.

```
export
template <T type>
fn __init__(this &Optional<T>) void
```

### `fn __init__(this &Optional<T>, value T) void`

Creates a present optional holding `value`.

- `value`: the value to store; it starts out copied rather than taken, so a
  caller that is done with it writes `Optional<T>(util.move(value))`

```
export
template <T type>
fn __init__(this &Optional<T>, value T) void
```

### `fn __init__(this &Optional<T>, other &const Optional<T>) void`

Copies another optional: a value is copied, an absence stays absent.

- `other`: the optional to copy; it keeps what it holds

```
export
template <T type>
fn __init__(this &Optional<T>, other &const Optional<T>) void
```

### `fn __init__(this &Optional<T>, other &&Optional<T>) void`

Takes another optional's value.

- `other`: the optional to take from; a present one is copied and then left
  absent, so it releases nothing afterwards

```
export
template <T type>
fn __init__(this &Optional<T>, other &&Optional<T>) void
```

### `fn __assign__(this &Optional<T>, other &const Optional<T>) void`

Assigns another optional's value to this one.

- `other`: the optional to copy; this one releases what it held first, so an
  absent `other` makes this one absent too

```
export
template <T type>
fn __assign__(this &Optional<T>, other &const Optional<T>) void
```

### `fn __assign__(this &Optional<T>, other &&Optional<T>) void`

Moves another optional's value into this one.

- `other`: the optional to take from; this one releases what it held first,
  and a present `other` is left absent

```
export
template <T type>
fn __assign__(this &Optional<T>, other &&Optional<T>) void
```

### `fn __drop__(this &Optional<T>) void`

Releases the value, if there is one.

```
export
template <T type>
fn __drop__(this &Optional<T>) void
```

### `fn present(this &const Optional<T>) bool`

Whether there is a value.

```
export
template <T type>
fn present(this &const Optional<T>) bool
```

### `fn __bool__(this &const Optional<T>) bool`

The same answer as `present()`, so `if (opt)` reads as "if there is one".

```
export
template <T type>
fn __bool__(this &const Optional<T>) bool
```

### `fn value(this &const Optional<T>) T except`

The value, or a raise.

An absent optional has no value to hand out, so the question is answered the
way the language answers every other failure: an exception, here `NoneError`.

```
export
template <T type>
fn value(this &const Optional<T>) T except
```

### `fn reference(this &const Optional<T>) &const T except`

A reference to the value, or a raise.

```
export
template <T type>
fn reference(this &const Optional<T>) &const T except
```

### `fn value_or(this &const Optional<T>, fallback T) T`

The value, or `fallback` when there is none.

- `fallback`: the value to return when this optional is absent; it is copied
  either way, and is never used while there is a value

```
export
template <T type>
fn value_or(this &const Optional<T>, fallback T) T
```

### `fn reset(this &Optional<T>) void`

Makes the optional absent, releasing any value it held.

```
export
template <T type>
fn reset(this &Optional<T>) void
```

### `fn set(this &Optional<T>, value T) void`

Stores `value`, making the optional present.

- `value`: the value to store; it is copied rather than taken, and any value
  the optional held is released before it is stored

```
export
template <T type>
fn set(this &Optional<T>, value T) void
```

### `fn some(value T) Optional<T>`

The same value, as an optional.

- `value`: the value the optional will hold; wrapping copies it, so the
  caller keeps its own

```
export
template <T type>
fn some(value T) Optional<T>
```

### `fn none() Optional<T>`

No value, typed.

```
export
template <T type>
fn none() Optional<T>
```

### `fn __eq__(this &const Optional<T>, other &const Optional<T>) bool`

Whether two optionals hold equal values, or are both absent.

- `other`: the optional to compare against; a present one against an absent
  one is unequal whatever the values

```
export
template <T type>
fn __eq__(this &const Optional<T>, other &const Optional<T>) bool
```

### `fn __ne__(this &const Optional<T>, other &const Optional<T>) bool`

Whether two optionals differ.

- `other`: the optional to compare against; two absent optionals do not
  differ, so this is not simply "both hold values and those differ"

```
export
template <T type>
fn __ne__(this &const Optional<T>, other &const Optional<T>) bool
```
