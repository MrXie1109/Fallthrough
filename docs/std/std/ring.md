# `std.ring`

A ring buffer: a fixed-size queue that reuses its slots.

    ring Ring<int> = Ring<int>(4usize);
    ring.push(1);
    maybe int? = ring.pop();

The capacity is chosen at construction and never grows. A push into a full
ring fails and leaves the ring alone -- it does not overwrite the oldest value
-- so a producer that must not drop data sees the failure instead of losing
an element silently.

# Storage

The slots are raw memory with a head and a length, so a type with no default
constructor is constructed exactly when its value is pushed and dropped when
it is popped. `(head + index) % capacity` is the slot of logical element
`index`.

## Declarations

### `type Ring {`

A fixed-size first-in, first-out buffer.

```
export
template <T type>
type Ring {
    // The slots, of which `_cap` exist and `_len` are constructed.
    _storage *T;
    // How many slots there are.
    _cap usize;
    // Where the oldest element is.
    _head usize;
    // How many elements are stored.
    _len usize;
}
```

### `fn __init__(this &Ring<T>, capacity usize) void`

Creates a ring with room for `capacity` elements.

- `capacity`: how many elements the ring may hold at once; it is fixed for
  the ring's whole life, and zero makes a ring that every push fails on

```
export
template <T type>
fn __init__(this &Ring<T>, capacity usize) void
```

### `fn __drop__(this &Ring<T>) void`

Releases every stored element and the slots.

```
export
template <T type>
fn __drop__(this &Ring<T>) void
```

### `fn slot(this &Ring<T>, index usize) *T`

The element at logical position `index`.

- `index`: the element's position, counting from the oldest element as zero
  and never from the buffer's start

```
template <T type>
fn slot(this &Ring<T>, index usize) *T
```

### `fn slot(this &const Ring<T>, index usize) *const T`

The element at logical position `index`, read only.

- `index`: the element's position, counting from the oldest element as zero
  and never from the buffer's start

```
template <T type>
fn slot(this &const Ring<T>, index usize) *const T
```

### `fn length(this &const Ring<T>) usize`

How many elements are stored.

```
export
template <T type>
fn length(this &const Ring<T>) usize
```

### `fn capacity(this &const Ring<T>) usize`

How many elements the ring can hold.

```
export
template <T type>
fn capacity(this &const Ring<T>) usize
```

### `fn empty(this &const Ring<T>) bool`

Whether the ring is empty.

```
export
template <T type>
fn empty(this &const Ring<T>) bool
```

### `fn full(this &const Ring<T>) bool`

Whether the ring is full.

```
export
template <T type>
fn full(this &const Ring<T>) bool
```

### `fn push(this &Ring<T>, value T) bool`

Appends `value`, returning whether there was room.

- `value`: the element to copy into the next free slot; it is copied rather
  than consumed, and is not stored at all when the ring is full

```
export
template <T type>
fn push(this &Ring<T>, value T) bool
```

### `fn pop(this &Ring<T>) ?T`

Removes and returns the oldest element, or an absent optional when empty.

```
export
template <T type>
fn pop(this &Ring<T>) ?T
```

### `fn front(this &const Ring<T>) ?T`

The oldest element, or an absent optional when empty.

```
export
template <T type>
fn front(this &const Ring<T>) ?T
```

### `fn back(this &const Ring<T>) ?T`

The newest element, or an absent optional when empty.

```
export
template <T type>
fn back(this &const Ring<T>) ?T
```

### `fn get(this &const Ring<T>, index usize) ?T`

The element at logical position `index`, or an absent optional.

- `index`: the element's position, counting from the oldest element as zero;
  at or past the length there is no such element and the result is absent

```
export
template <T type>
fn get(this &const Ring<T>, index usize) ?T
```

### `fn clear(this &Ring<T>) void`

Removes every element.

```
export
template <T type>
fn clear(this &Ring<T>) void
```
