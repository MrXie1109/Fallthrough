# `std.heap`

A binary heap, and the priority queue built on it.

# The order is a type

`Heap<T, Less<T>>` keeps the smallest value on top, and `Heap<T, Greater<T>>`
the largest: the comparator is the same one a tree takes, so a program that
has an order for its own type has a heap for it too.

# The shape

The elements live in one buffer in level order, so the children of the element
at `i` are at `2i + 1` and `2i + 2` and no links are stored. A push appends and
walks the new value up while it belongs above its parent; a pop takes the top,
moves the last element into its place, and walks that one down. Both are
logarithmic, and neither reorders anything else.

## Declarations

### `INITIAL_CAPACITY const usize = 8;`

How much room an empty heap asks for, in elements.

```
INITIAL_CAPACITY const usize = 8;
```

### `type Heap {`

A heap: the value that comes first is on top.

```
export
template <T type, Cmp type = Less<T>, A type = DefaultAllocator>
type Heap {
    // The element storage: `_cap` slots, of which `_len` are alive.
    _data *T;
    // How many slots there are.
    _cap usize;
    // How many elements there are.
    _len usize;
    // The allocator the buffer comes from.
    mem A;
    // The order.
    cmp Cmp;
}
```

### `fn __init__(this &Heap<T, Cmp, A>) void`

Creates an empty heap.

```
export
template <T type, Cmp type = Less<T>, A type = DefaultAllocator>
fn __init__(this &Heap<T, Cmp, A>) void
```

### `fn __init__(this &Heap<T, Cmp, A>, order Cmp, memory A) void`

Creates a heap with the caller's order and allocator.
- `order`: the comparison that decides which element is on top
- `memory`: the allocator the buffer is grown from

```
export
template <T type, Cmp type = Less<T>, A type = DefaultAllocator>
fn __init__(this &Heap<T, Cmp, A>, order Cmp, memory A) void
```

### `fn __init__(this &Heap<T, Cmp, A>, other &const Heap<T, Cmp, A>) void`

Creates a heap with a copy of every element of `other`.
- `other`: the heap to copy

```
export
template <T type, Cmp type = Less<T>, A type = DefaultAllocator>
fn __init__(this &Heap<T, Cmp, A>, other &const Heap<T, Cmp, A>) void
```

### `fn __drop__(this &Heap<T, Cmp, A>) void`

Destroys the elements and the buffer.

```
export
template <T type, Cmp type = Less<T>, A type = DefaultAllocator>
fn __drop__(this &Heap<T, Cmp, A>) void
```

### `fn length(this &const Heap<T, Cmp, A>) usize`

Returns how many elements the heap holds.

```
export
template <T type, Cmp type = Less<T>, A type = DefaultAllocator>
fn length(this &const Heap<T, Cmp, A>) usize
```

### `fn capacity(this &const Heap<T, Cmp, A>) usize`

Returns how many elements the buffer has room for.

```
export
template <T type, Cmp type = Less<T>, A type = DefaultAllocator>
fn capacity(this &const Heap<T, Cmp, A>) usize
```

### `fn empty(this &const Heap<T, Cmp, A>) bool`

Returns `true` when the heap holds nothing.

```
export
template <T type, Cmp type = Less<T>, A type = DefaultAllocator>
fn empty(this &const Heap<T, Cmp, A>) bool
```

### `fn before(this &Heap<T, Cmp, A>, a &const T, b &const T) bool`

Whether `a` belongs above `b` under the heap's order.
- `a`: the element asking for priority
- `b`: the element it is compared with

```
fn before(this &Heap<T, Cmp, A>, a &const T, b &const T) bool
```

### `fn reserve(this &Heap<T, Cmp, A>, wanted usize) void`

Makes room for `wanted` elements.
- `wanted`: how many elements room must be made for

```
export
template <T type, Cmp type = Less<T>, A type = DefaultAllocator>
fn reserve(this &Heap<T, Cmp, A>, wanted usize) void
```

### `fn swap(this &Heap<T, Cmp, A>, a usize, b usize) void`

Exchanges two elements, carrying the first through a value of its own.

The elements are *copied* rather than assigned: an element read out of a
buffer is a named place, so the copying constructor is the one that applies,
and assigning over a value that is already alive would drop a buffer the other
side still points at. Each slot is destroyed first and constructed again, so
every buffer has exactly one owner.
- `a`: one slot
- `b`: the other slot

```
fn swap(this &Heap<T, Cmp, A>, a usize, b usize) void
```

### `fn sift_up(this &Heap<T, Cmp, A>, index usize) void`

Walks the element at `index` up until its parent belongs above it.
- `index`: the element to walk up

```
fn sift_up(this &Heap<T, Cmp, A>, index usize) void
```

### `fn sift_down(this &Heap<T, Cmp, A>, index usize) void`

Walks the element at `index` down until it belongs above both children.

```
fn sift_down(this &Heap<T, Cmp, A>, index usize) void
```

### `fn push(this &Heap<T, Cmp, A>, value T) void`

Adds `value`, which then finds its place in the order.

- `value`: the element to add, moved in

```
export
template <T type, Cmp type = Less<T>, A type = DefaultAllocator>
fn push(this &Heap<T, Cmp, A>, value T) void
```

### `fn top(this &const Heap<T, Cmp, A>) &const T`

Returns the value that comes first.

```
export
template <T type, Cmp type = Less<T>, A type = DefaultAllocator>
fn top(this &const Heap<T, Cmp, A>) &const T
```

### `fn top(this &Heap<T, Cmp, A>) &T`

Returns the value that comes first as a place that can be written.

Writing through it is the caller's promise that the order still holds; the
heap cannot check one field of a value it was handed.

```
export
template <T type, Cmp type = Less<T>, A type = DefaultAllocator>
fn top(this &Heap<T, Cmp, A>) &T
```

### `fn pop(this &Heap<T, Cmp, A>) T`

Removes the value that comes first and returns it.

```
export
template <T type, Cmp type = Less<T>, A type = DefaultAllocator>
fn pop(this &Heap<T, Cmp, A>) T
```

### `fn clear(this &Heap<T, Cmp, A>) void`

Removes every element.

```
export
template <T type, Cmp type = Less<T>, A type = DefaultAllocator>
fn clear(this &Heap<T, Cmp, A>) void
```

### `type PriorityQueue {`

A priority queue: a heap whose surface is `push`, `pop` and `top`.

```
export
template <T type, Cmp type = Less<T>, A type = DefaultAllocator>
type PriorityQueue {
    heap Heap<T, Cmp, A>;
}
```

### `fn __init__(this &PriorityQueue<T, Cmp, A>) void`

```
export
template <T type, Cmp type = Less<T>, A type = DefaultAllocator>
fn __init__(this &PriorityQueue<T, Cmp, A>) void
```

### `fn __init__(this &PriorityQueue<T, Cmp, A>, order Cmp, memory A) void`

Creates an empty queue with the caller's order and allocator.

- `order`: the comparison that decides which value comes first
- `memory`: the allocator the buffer is grown from

```
export
template <T type, Cmp type = Less<T>, A type = DefaultAllocator>
fn __init__(this &PriorityQueue<T, Cmp, A>, order Cmp, memory A) void
```

### `fn __drop__(this &PriorityQueue<T, Cmp, A>) void`

```
export
template <T type, Cmp type = Less<T>, A type = DefaultAllocator>
fn __drop__(this &PriorityQueue<T, Cmp, A>) void
```

### `fn push(this &PriorityQueue<T, Cmp, A>, value T) void`

Adds `value` with its priority.

- `value`: the element to add, moved in

```
export
template <T type, Cmp type = Less<T>, A type = DefaultAllocator>
fn push(this &PriorityQueue<T, Cmp, A>, value T) void
```

### `fn pop(this &PriorityQueue<T, Cmp, A>) T`

Removes the value whose priority comes first.

```
export
template <T type, Cmp type = Less<T>, A type = DefaultAllocator>
fn pop(this &PriorityQueue<T, Cmp, A>) T
```

### `fn top(this &const PriorityQueue<T, Cmp, A>) &const T`

Returns the value whose priority comes first.

```
export
template <T type, Cmp type = Less<T>, A type = DefaultAllocator>
fn top(this &const PriorityQueue<T, Cmp, A>) &const T
```

### `fn top(this &PriorityQueue<T, Cmp, A>) &T`

```
export
template <T type, Cmp type = Less<T>, A type = DefaultAllocator>
fn top(this &PriorityQueue<T, Cmp, A>) &T
```

### `fn length(this &const PriorityQueue<T, Cmp, A>) usize`

```
export
template <T type, Cmp type = Less<T>, A type = DefaultAllocator>
fn length(this &const PriorityQueue<T, Cmp, A>) usize
```

### `fn empty(this &const PriorityQueue<T, Cmp, A>) bool`

```
export
template <T type, Cmp type = Less<T>, A type = DefaultAllocator>
fn empty(this &const PriorityQueue<T, Cmp, A>) bool
```

### `fn clear(this &PriorityQueue<T, Cmp, A>) void`

```
export
template <T type, Cmp type = Less<T>, A type = DefaultAllocator>
fn clear(this &PriorityQueue<T, Cmp, A>) void
```
