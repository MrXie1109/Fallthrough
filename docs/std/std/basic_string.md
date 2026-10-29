# `std.basic_string`

A string over a character type.

`BasicString<Char>` is the shape every string has: a run of `Char` values,
its length, and the operations that read, append and compare. The character
type is a parameter because a string is not always bytes: `String` is the
`char` form, and `String16`/`String32` are the same operations over wider code
units.

    text BasicString<char> = BasicString<char>("hello");
    text.push('!');

# Storage

The storage is `Array<Char>`'s: a contiguous run that grows by doubling. The
wrapper names the character type, forwards the operations, and offers the
conversions that only a character string has (to `String` for `char`, from a
C string for `char`).

This is the simple form: one buffer, no small-string optimisation. `String`
is the `char` specialisation the rest of the library uses, and it keeps small
strings inside the object; the two have the same surface, and a program that
wants a wider character type reaches for this one.

## Declarations

### `type BasicString {`

A string of `Char` values.

```
export
template <Char type>
type BasicString {
    // The characters, in order.
    _data Array<Char>;
}
```

### `fn __init__(this &BasicString<Char>) void`

Creates an empty string.

```
export
template <Char type>
fn __init__(this &BasicString<Char>) void
```

### `fn __init__(this &BasicString<Char>, other &const BasicString<Char>) void`

Copies another string.

- `other`: the string to copy; its characters are copied one at a time, so
  the two strings share nothing afterwards

```
export
template <Char type>
fn __init__(this &BasicString<Char>, other &const BasicString<Char>) void
```

### `fn __init__(this &BasicString<Char>, other &&BasicString<Char>) void`

Takes another string's storage.

The member is initialised before the move assignment: assigning to an
uninitialised `Array` was an assignment whose destination storage is
whatever the object's bytes happened to be, and releasing that "storage"
aborted on an invalid pointer.

- `other`: the string to take the storage from, left empty

```
export
template <Char type>
fn __init__(this &BasicString<Char>, other &&BasicString<Char>) void
```

### `fn __im_init__(this &BasicString<Char>, text *const char) void`

Copies a C string, up to its terminator.

A `char` string literal reaches this constructor; a wider character type has
no literal syntax of its own, so it is built from an array.

- `text`: a null-terminated C string; the characters before the terminator
  are copied, each converted to `Char`, and the terminator itself is not part
  of the string

```
export
template <Char type>
fn __im_init__(this &BasicString<Char>, text *const char) void
```

### `fn __init__(this &BasicString<Char>, values &const Array<Char>) void`

Copies a run of characters.

- `values`: the characters to copy, in order; the whole array is taken, and
  does not need a terminator

```
export
template <Char type>
fn __init__(this &BasicString<Char>, values &const Array<Char>) void
```

### `fn __assign__(this &BasicString<Char>, other &const BasicString<Char>) void`

Copies another string into this one.

- `other`: the string to copy; this string's characters are dropped first,
  so the result holds exactly `other`'s

```
export
template <Char type>
fn __assign__(this &BasicString<Char>, other &const BasicString<Char>) void
```

### `fn __assign__(this &BasicString<Char>, other &&BasicString<Char>) void`

Moves another string's storage into this one.

- `other`: the string to take the storage from, left empty

```
export
template <Char type>
fn __assign__(this &BasicString<Char>, other &&BasicString<Char>) void
```

### `fn __drop__(this &BasicString<Char>) void`

Releases the storage.

```
export
template <Char type>
fn __drop__(this &BasicString<Char>) void
```

### `fn length(this &const BasicString<Char>) usize`

The number of characters.

```
export
template <Char type>
fn length(this &const BasicString<Char>) usize
```

### `fn empty(this &const BasicString<Char>) bool`

Whether there are no characters.

```
export
template <Char type>
fn empty(this &const BasicString<Char>) bool
```

### `fn capacity(this &const BasicString<Char>) usize`

How much room the string has before it grows.

```
export
template <Char type>
fn capacity(this &const BasicString<Char>) usize
```

### `fn data(this &BasicString<Char>) *Char`

The characters as a pointer.

```
export
template <Char type>
fn data(this &BasicString<Char>) *Char
```

### `fn const_data(this &const BasicString<Char>) *const Char`

The characters as a read-only pointer.

```
export
template <Char type>
fn const_data(this &const BasicString<Char>) *const Char
```

### `fn __getitem__(this &BasicString<Char>, index usize) &Char`

The character at `index`; writing through it writes the string.

- `index`: the character's position, counted from zero; it is not checked,
  so it must be less than `length()`

```
export
template <Char type>
fn __getitem__(this &BasicString<Char>, index usize) &Char
```

### `fn __getitem__(this &const BasicString<Char>, index usize) &const Char`

The character at `index`, read only.

- `index`: the character's position, counted from zero; it is not checked,
  so it must be less than `length()`

```
export
template <Char type>
fn __getitem__(this &const BasicString<Char>, index usize) &const Char
```

### `fn push(this &BasicString<Char>, value Char) void`

Appends one character.

- `value`: the character to copy onto the end

```
export
template <Char type>
fn push(this &BasicString<Char>, value Char) void
```

### `fn push_back(this &BasicString<Char>, value Char) void`

Adds one character at the end.

The same operation as `push`, under the name a generic sequence expects.

- `value`: the character to copy onto the end

```
export
template <Char type>
fn push_back(this &BasicString<Char>, value Char) void
```

### `fn push_string(this &BasicString<Char>, other &const BasicString<Char>) void`

Adds the characters of `other` at the end.

- `other`: the string whose characters are copied, in order, after this
  string's own; appending a string to itself is safe

```
export
template <Char type>
fn push_string(this &BasicString<Char>, other &const BasicString<Char>) void
```

### `fn __iadd__(this &BasicString<Char>, value Char) void`

Appends one character, which is what `+=` on a string does.

- `value`: the character to copy onto the end

```
export
template <Char type>
fn __iadd__(this &BasicString<Char>, value Char) void
```

### `fn clear(this &BasicString<Char>) void`

Removes every character.

```
export
template <Char type>
fn clear(this &BasicString<Char>) void
```

### `fn reserve(this &BasicString<Char>, wanted usize) void`

Makes room for `wanted` characters.

- `wanted`: how many characters must fit without another allocation; the
  string's length is unchanged, so a value at or below `capacity()` does
  nothing

```
export
template <Char type>
fn reserve(this &BasicString<Char>, wanted usize) void
```

### `fn resize(this &BasicString<Char>, wanted usize, filler Char) void`

Changes the number of characters, filling new ones with `filler`.

- `wanted`: the new length, in characters; a value below the current length
  truncates, and zero empties the string
- `filler`: the character each new position is copied from, used only when
  `wanted` is greater than the current length

```
export
template <Char type>
fn resize(this &BasicString<Char>, wanted usize, filler Char) void
```

### `fn begin(this &BasicString<Char>) *Char`

The first position.

```
export
template <Char type>
fn begin(this &BasicString<Char>) *Char
```

### `fn end(this &BasicString<Char>) *Char`

The one-past-the-end position.

```
export
template <Char type>
fn end(this &BasicString<Char>) *Char
```

### `fn begin(this &const BasicString<Char>) *const Char`

The first position, read only.

```
export
template <Char type>
fn begin(this &const BasicString<Char>) *const Char
```

### `fn end(this &const BasicString<Char>) *const Char`

The one-past-the-end position, read only.

```
export
template <Char type>
fn end(this &const BasicString<Char>) *const Char
```

### `fn erase(this &BasicString<Char>, at usize) void`

Removes one character.

- `at`: the position to remove, counted from zero; it must be less than
  `length()`, and every character after it moves down by one

```
export
template <Char type>
fn erase(this &BasicString<Char>, at usize) void
```

### `fn insert(this &BasicString<Char>, at usize, value Char) void`

Inserts one character before `at`.

- `at`: the position the new character takes, counted from zero; the
  character at it and every one after move up by one, and `at` equal to
  `length()` appends
- `value`: the character to copy into the new position

```
export
template <Char type>
fn insert(this &BasicString<Char>, at usize, value Char) void
```

### `fn __add__(this &const BasicString<Char>, other &const BasicString<Char>) BasicString<Char>`

Returns `other` after this string: concatenation.

- `other`: the string appended to a copy of this one; neither operand is
  changed, and the result is a new string

```
export
template <Char type>
fn __add__(this &const BasicString<Char>, other &const BasicString<Char>) BasicString<Char>
```

### `fn __eq__(this &const BasicString<Char>, other &const BasicString<Char>) bool`

Whether two strings hold the same characters.

- `other`: the string to compare with; lengths must match and every
  character must be equal, so case and exact code units count

```
export
template <Char type>
fn __eq__(this &const BasicString<Char>, other &const BasicString<Char>) bool
```

### `fn __ne__(this &const BasicString<Char>, other &const BasicString<Char>) bool`

Whether two strings differ.

- `other`: the string to compare with; this is the negation of `==`

```
export
template <Char type>
fn __ne__(this &const BasicString<Char>, other &const BasicString<Char>) bool
```

### `fn __lt__(this &const BasicString<Char>, other &const BasicString<Char>) bool`

Orders two strings by their characters: a shorter prefix comes first.

- `other`: the string to order this one against; the two are compared
  character by character up to the shorter length, and if those are equal
  the shorter string comes first

```
export
template <Char type>
fn __lt__(this &const BasicString<Char>, other &const BasicString<Char>) bool
```

### `fn find(this &const BasicString<Char>, value Char) isize`

The first position of `value`, or `-1`.

- `value`: the character to search for; the earliest position holding it is
  returned, and no position at all gives `-1`

```
export
template <Char type>
fn find(this &const BasicString<Char>, value Char) isize
```

### `fn contains(this &const BasicString<Char>, value Char) bool`

Whether `value` occurs.

- `value`: the character to search for; it occurs when `find` finds it at
  some position

```
export
template <Char type>
fn contains(this &const BasicString<Char>, value Char) bool
```

### `fn __str__(this &const BasicString<Char>) String`

The characters of a `char` string as a `String`.

A wider character type has no one-byte rendering; its `str` is an empty
string unless the characters happen to fit. Converting a `String16` for
display is a decoding question, which is the unicode module's job rather
than this one's.

```
export
template <Char type>
fn __str__(this &const BasicString<Char>) String
```
