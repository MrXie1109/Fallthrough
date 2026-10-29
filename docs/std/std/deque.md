# `std.deque`

A double-ended queue, and the two adapters that are views of it: a queue and a
stack.

# A ring, not a line

The elements live in one buffer, and the first one is not necessarily at its
start: a `push_front` walks the front index backwards, wrapping around, and a
`push_back` writes past the end's index. Nothing moves, which is what makes
both ends O(1) -- an `Array` can only afford one of them.

# Growth copies

A full deque allocates a buffer twice the size and copies the elements into it
in order, front first. As in `Array`, an element type whose move may raise is
copied rather than moved, so a raise leaves the old buffer whole.

## Declarations

### `INITIAL_CAPACITY const usize = 8;`

How much room an empty deque asks for, in elements.

```
INITIAL_CAPACITY const usize = 8;
```

### `type Deque {`

A sequence with a cheap push and pop at both ends.

```
export
template <T type, A type = DefaultAllocator>
type Deque {
    // The element storage: `_cap` slots, of which `_len` are alive.
    _data *T;
    // How many slots there are.
    _cap usize;
    // Which slot holds the first element.
    _front usize;
    // How many elements there are.
    _len usize;
    // The allocator the buffer comes from.
    mem A;
}
```

### `fn __init__(this &Deque<T, A>) void`

Creates an empty deque.

```
export
template <T type, A type = DefaultAllocator>
fn __init__(this &Deque<T, A>) void
```

### `fn __init__(this &Deque<T, A>, count usize, value T) void`

Creates a deque of `count` copies of `value`.

- `count`: how many copies to store, all at the back; zero makes an empty
  deque and allocates nothing
- `value`: the element each of the `count` slots is constructed from, so
  every element starts equal to it

```
export
template <T type, A type = DefaultAllocator>
fn __init__(this &Deque<T, A>, count usize, value T) void
```

### `fn __init__(this &Deque<T, A>, other &const Deque<T, A>) void`

Creates a deque with a copy of every element of `other`.

- `other`: the deque to copy; its elements are copied front first, so the
  two deques share nothing afterwards

```
export
template <T type, A type = DefaultAllocator>
fn __init__(this &Deque<T, A>, other &const Deque<T, A>) void
```

### `fn __init__(this &Deque<T, A>, other &&Deque<T, A>) void`

Creates a deque that takes the buffer of `other`.

- `other`: the deque to take the buffer and allocator from, left empty

```
export
template <T type, A type = DefaultAllocator>
fn __init__(this &Deque<T, A>, other &&Deque<T, A>) void
```

### `fn __assign__(this &Deque<T, A>, other &const Deque<T, A>) void`

Copies the elements of `other` over this one.

- `other`: the deque to copy; this deque's elements are destroyed first, so
  the result holds exactly `other`'s

```
export
template <T type, A type = DefaultAllocator>
fn __assign__(this &Deque<T, A>, other &const Deque<T, A>) void
```

### `fn __assign__(this &Deque<T, A>, other &&Deque<T, A>) void`

Takes the buffer of `other`, releasing what this deque held.

- `other`: the deque to take the buffer from, left empty

```
export
template <T type, A type = DefaultAllocator>
fn __assign__(this &Deque<T, A>, other &&Deque<T, A>) void
```

### `fn __drop__(this &Deque<T, A>) void`

Destroys the elements and the buffer.

```
export
template <T type, A type = DefaultAllocator>
fn __drop__(this &Deque<T, A>) void
```

### `fn slot(this &Deque<T, A>, index usize) *T`

The slot holding the element at `index`.

- `index`: the element's position, counted from the front as zero; it must
  be less than `length()`, and the slot is found by wrapping past the front
  index

```
fn slot(this &Deque<T, A>, index usize) *T
```

### `fn slot_of(this &const Deque<T, A>, index usize) *const T`

The slot holding the element at `index`, read through a constant deque.

- `index`: the element's position, counted from the front as zero; it must
  be less than `length()`, and the slot is found by wrapping past the front
  index

```
fn slot_of(this &const Deque<T, A>, index usize) *const T
```

### `fn length(this &const Deque<T, A>) usize`

Returns how many elements the deque holds.

```
export
template <T type, A type = DefaultAllocator>
fn length(this &const Deque<T, A>) usize
```

### `fn capacity(this &const Deque<T, A>) usize`

Returns how many elements the buffer has room for.

```
export
template <T type, A type = DefaultAllocator>
fn capacity(this &const Deque<T, A>) usize
```

### `fn empty(this &const Deque<T, A>) bool`

Returns `true` when the deque holds nothing.

```
export
template <T type, A type = DefaultAllocator>
fn empty(this &const Deque<T, A>) bool
```

### `fn reserve(this &Deque<T, A>, wanted usize) void`

Makes room for `wanted` elements, front first.

The elements are moved into the new buffer in order, so the front becomes
index zero again; a type whose move may raise is copied instead.

- `wanted`: how many elements must fit without another allocation; the
  length is unchanged, so a value at or below `capacity()` does nothing

```
export
template <T type, A type = DefaultAllocator>
fn reserve(this &Deque<T, A>, wanted usize) void
```

### `fn push(this &Deque<T, A>, value T) void`

Adds `value` at the back.

The same operation as `push_back`, under the shorter name a sequence uses.

- `value`: the element to put at the back; it is moved out of the
  parameter's own storage rather than copied

```
export
template <T type, A type = DefaultAllocator>
fn push(this &Deque<T, A>, value T) void
```

### `fn push_back(this &Deque<T, A>, value T) void`

Adds `value` at the back.

- `value`: the element to put at the back; it is moved out of the
  parameter's own storage rather than copied

```
export
template <T type, A type = DefaultAllocator>
fn push_back(this &Deque<T, A>, value T) void
```

### `fn push_front(this &Deque<T, A>, value T) void`

Adds `value` at the front.

- `value`: the element to put at the front; it is moved out of the
  parameter's own storage rather than copied

```
export
template <T type, A type = DefaultAllocator>
fn push_front(this &Deque<T, A>, value T) void
```

### `fn pop_back(this &Deque<T, A>) T`

Removes the last element and returns it.

```
export
template <T type, A type = DefaultAllocator>
fn pop_back(this &Deque<T, A>) T
```

### `fn pop_front(this &Deque<T, A>) T`

Removes the first element and returns it.

```
export
template <T type, A type = DefaultAllocator>
fn pop_front(this &Deque<T, A>) T
```

### `fn front(this &const Deque<T, A>) &const T`

Returns the first element.

```
export
template <T type, A type = DefaultAllocator>
fn front(this &const Deque<T, A>) &const T
```

### `fn front(this &Deque<T, A>) &T`

Returns the first element as a place that can be written.

```
export
template <T type, A type = DefaultAllocator>
fn front(this &Deque<T, A>) &T
```

### `fn back(this &const Deque<T, A>) &const T`

Returns the last element.

```
export
template <T type, A type = DefaultAllocator>
fn back(this &const Deque<T, A>) &const T
```

### `fn back(this &Deque<T, A>) &T`

Returns the last element as a place that can be written.

```
export
template <T type, A type = DefaultAllocator>
fn back(this &Deque<T, A>) &T
```

### `fn __getitem__(this &const Deque<T, A>, index usize) &const T`

Returns the element at `index`.

- `index`: the element's position, counted from the front as zero; it is
  not checked, so the caller must keep it below `length()`

```
export
template <T type, A type = DefaultAllocator>
fn __getitem__(this &const Deque<T, A>, index usize) &const T
```

### `fn __getitem__(this &Deque<T, A>, index usize) &T`

Returns the element at `index` as a place that can be written.

- `index`: the element's position, counted from the front as zero; it is
  not checked, so the caller must keep it below `length()`

```
export
template <T type, A type = DefaultAllocator>
fn __getitem__(this &Deque<T, A>, index usize) &T
```

### `fn clear(this &Deque<T, A>) void`

Removes every element.

```
export
template <T type, A type = DefaultAllocator>
fn clear(this &Deque<T, A>) void
```

### `type DequeIterator {`

----------------------------------------------------------------- iterators
A position in a deque, walking from the front to the back.

The buffer, the front and the capacity are taken when the iterator is made, so
a step is arithmetic rather than a walk back to the deque -- and a deque that
grows while it is being walked leaves the iterator on the old buffer, which is
the same promise `Array` gives its pointers.

```
export
template <T type, A type = DefaultAllocator>
type DequeIterator {
    // The buffer being walked.
    data *T;
    // How many slots it has.
    cap usize;
    // Which slot the first element is in.
    front usize;
    // How many elements in the walk has come.
    index usize;
}
```

### `fn __init__(this &DequeIterator<T, A>) void`

```
export
template <T type, A type = DefaultAllocator>
fn __init__(this &DequeIterator<T, A>) void
```

### `fn __init__(`

Creates a position from the deque's own state.

- `data`: the buffer to walk, which the iterator copies rather than holds a
  reference to
- `cap`: how many slots the buffer has, which the step arithmetic wraps at
- `front`: the slot holding the deque's first element, which is what makes
  index zero mean the front
- `index`: how far along the walk the position stands, counted from the
  front as zero; it is not wrapped, so `end()` passes it as the length

```
export
template <T type, A type = DefaultAllocator>
fn __init__(
    this &DequeIterator<T, A>,
    data *T,
    cap usize,
    front usize,
    index usize
) void
```

### `fn __inc__(this &DequeIterator<T, A>) void`

```
export
template <T type, A type = DefaultAllocator>
fn __inc__(this &DequeIterator<T, A>) void
```

### `fn __deref__(this &DequeIterator<T, A>) &T`

```
export
template <T type, A type = DefaultAllocator>
fn __deref__(this &DequeIterator<T, A>) &T
```

### `fn __add__(this &const DequeIterator<T, A>, offset usize) DequeIterator<T, A>`

A position `offset` slots further on, which is what makes a deque a random
access range: the algorithms move by an index rather than stepping one at a
time.

- `offset`: how many slots further along the walk to stand, added to the
  current index; it is not wrapped, so the caller stays within `end()`

```
export
template <T type, A type = DefaultAllocator>
fn __add__(this &const DequeIterator<T, A>, offset usize) DequeIterator<T, A>
```

### `fn __sub__(this &const DequeIterator<T, A>, offset usize) DequeIterator<T, A>`

A position `offset` slots before this one.

- `offset`: how many slots back along the walk to stand, subtracted from the
  current index; it is not wrapped, so the caller stops no earlier than
  `begin()`

```
export
template <T type, A type = DefaultAllocator>
fn __sub__(this &const DequeIterator<T, A>, offset usize) DequeIterator<T, A>
```

### `fn __dec__(this &DequeIterator<T, A>) void`

Steps back one slot.

```
export
template <T type, A type = DefaultAllocator>
fn __dec__(this &DequeIterator<T, A>) void
```

### `fn __iadd__(this &DequeIterator<T, A>, offset usize) void`

Moves forward by `offset` slots in one step.

- `offset`: how many slots further along the walk to stand, added to the
  current index

```
export
template <T type, A type = DefaultAllocator>
fn __iadd__(this &DequeIterator<T, A>, offset usize) void
```

### `fn __isub__(this &DequeIterator<T, A>, offset usize) void`

Moves back by `offset` slots in one step.

- `offset`: how many slots back along the walk to stand, subtracted from the
  current index

```
export
template <T type, A type = DefaultAllocator>
fn __isub__(this &DequeIterator<T, A>, offset usize) void
```

### `fn __getitem__(this &DequeIterator<T, A>, at usize) &T`

The element `at` slots on, which is what makes `it[n]` read a random access
position the way a pointer does.

- `at`: how many slots past this position to read, not including it, so
  zero is the element the position itself stands on

```
export
template <T type, A type = DefaultAllocator>
fn __getitem__(this &DequeIterator<T, A>, at usize) &T
```

### `fn __lt__(this &const DequeIterator<T, A>, other &const DequeIterator<T, A>) bool`

Whether this position comes before `other` in the walk.

- `other`: the position to compare with; only the distance walked is
  compared, so an `end()` of one deque can order against a position of
  another

```
export
template <T type, A type = DefaultAllocator>
fn __lt__(this &const DequeIterator<T, A>, other &const DequeIterator<T, A>) bool
```

### `fn __eq__(this &const DequeIterator<T, A>, other &const DequeIterator<T, A>) bool`

Whether two positions stand on the same slot of the same buffer.

- `other`: the position to compare with, whose buffer and index must both
  match; two positions are never equal while they remain on one buffer

```
export
template <T type, A type = DefaultAllocator>
fn __eq__(this &const DequeIterator<T, A>, other &const DequeIterator<T, A>) bool
```

### `fn __ne__(this &const DequeIterator<T, A>, other &const DequeIterator<T, A>) bool`

Whether two positions differ.

- `other`: the position to compare with; this is the negation of `==`, so a
  different buffer alone makes the positions unequal

```
export
template <T type, A type = DefaultAllocator>
fn __ne__(this &const DequeIterator<T, A>, other &const DequeIterator<T, A>) bool
```

### `type ConstDequeIterator {`

A position in a deque, read only.

```
export
template <T type, A type = DefaultAllocator>
type ConstDequeIterator {
    data *const T;
    cap usize;
    front usize;
    index usize;
}
```

### `fn __init__(this &ConstDequeIterator<T, A>) void`

```
export
template <T type, A type = DefaultAllocator>
fn __init__(this &ConstDequeIterator<T, A>) void
```

### `fn __init__(`

Creates a read-only position from the deque's own state.

- `data`: the buffer to walk, which the iterator copies rather than holds a
  reference to
- `cap`: how many slots the buffer has, which the step arithmetic wraps at
- `front`: the slot holding the deque's first element, which is what makes
  index zero mean the front
- `index`: how far along the walk the position stands, counted from the
  front as zero; it is not wrapped, so `end()` passes it as the length

```
export
template <T type, A type = DefaultAllocator>
fn __init__(
    this &ConstDequeIterator<T, A>,
    data *const T,
    cap usize,
    front usize,
    index usize
) void
```

### `fn __inc__(this &ConstDequeIterator<T, A>) void`

```
export
template <T type, A type = DefaultAllocator>
fn __inc__(this &ConstDequeIterator<T, A>) void
```

### `fn __deref__(this &const ConstDequeIterator<T, A>) &const T`

```
export
template <T type, A type = DefaultAllocator>
fn __deref__(this &const ConstDequeIterator<T, A>) &const T
```

### `fn __add__(`

A position `offset` slots further on.

- `offset`: how many slots further along the walk to stand, added to the
  current index; it is not wrapped, so the caller stays within `end()`

```
export
template <T type, A type = DefaultAllocator>
fn __add__(
    this &const ConstDequeIterator<T, A>,
    offset usize
) ConstDequeIterator<T, A>
```

### `fn __sub__(`

A position `offset` slots before this one.

- `offset`: how many slots back along the walk to stand, subtracted from the
  current index; it is not wrapped, so the caller stops no earlier than
  `begin()`

```
export
template <T type, A type = DefaultAllocator>
fn __sub__(
    this &const ConstDequeIterator<T, A>,
    offset usize
) ConstDequeIterator<T, A>
```

### `fn __dec__(this &ConstDequeIterator<T, A>) void`

Steps back one slot.

```
export
template <T type, A type = DefaultAllocator>
fn __dec__(this &ConstDequeIterator<T, A>) void
```

### `fn __iadd__(this &ConstDequeIterator<T, A>, offset usize) void`

Moves forward by `offset` slots in one step.

- `offset`: how many slots further along the walk to stand, added to the
  current index

```
export
template <T type, A type = DefaultAllocator>
fn __iadd__(this &ConstDequeIterator<T, A>, offset usize) void
```

### `fn __isub__(this &ConstDequeIterator<T, A>, offset usize) void`

Moves back by `offset` slots in one step.

- `offset`: how many slots back along the walk to stand, subtracted from the
  current index

```
export
template <T type, A type = DefaultAllocator>
fn __isub__(this &ConstDequeIterator<T, A>, offset usize) void
```

### `fn __getitem__(this &const ConstDequeIterator<T, A>, at usize) &const T`

The element `at` slots on.

- `at`: how many slots past this position to read, not including it, so
  zero is the element the position itself stands on

```
export
template <T type, A type = DefaultAllocator>
fn __getitem__(this &const ConstDequeIterator<T, A>, at usize) &const T
```

### `fn __lt__(`

Whether this position comes before `other` in the walk.

- `other`: the position to compare with; only the distance walked is
  compared, so an `end()` of one deque can order against a position of
  another

```
export
template <T type, A type = DefaultAllocator>
fn __lt__(
    this &const ConstDequeIterator<T, A>,
    other &const ConstDequeIterator<T, A>
) bool
```

### `fn __eq__(`

Whether two positions stand on the same slot of the same buffer.

- `other`: the position to compare with, whose buffer and index must both
  match; two positions are never equal while they remain on one buffer

```
export
template <T type, A type = DefaultAllocator>
fn __eq__(
    this &const ConstDequeIterator<T, A>,
    other &const ConstDequeIterator<T, A>
) bool
```

### `fn __ne__(`

Whether two positions differ.

- `other`: the position to compare with; this is the negation of `==`, so a
  different buffer alone makes the positions unequal

```
export
template <T type, A type = DefaultAllocator>
fn __ne__(
    this &const ConstDequeIterator<T, A>,
    other &const ConstDequeIterator<T, A>
) bool
```

### `fn begin(this &Deque<T, A>) DequeIterator<T, A>`

The first position.

```
export
template <T type, A type = DefaultAllocator>
fn begin(this &Deque<T, A>) DequeIterator<T, A>
```

### `fn end(this &Deque<T, A>) DequeIterator<T, A>`

One past the last position.

```
export
template <T type, A type = DefaultAllocator>
fn end(this &Deque<T, A>) DequeIterator<T, A>
```

### `fn begin(this &const Deque<T, A>) ConstDequeIterator<T, A>`

The first position, read only.

```
export
template <T type, A type = DefaultAllocator>
fn begin(this &const Deque<T, A>) ConstDequeIterator<T, A>
```

### `fn end(this &const Deque<T, A>) ConstDequeIterator<T, A>`

One past the last position, read only.

```
export
template <T type, A type = DefaultAllocator>
fn end(this &const Deque<T, A>) ConstDequeIterator<T, A>
```

### `type Queue {`

------------------------------------------------------------------ adapters
A first-in, first-out sequence: a deque used from the front and the back.

```
export
template <T type, A type = DefaultAllocator>
type Queue {
    items Deque<T, A>;
}
```

### `fn __init__(this &Queue<T, A>) void`

```
export
template <T type, A type = DefaultAllocator>
fn __init__(this &Queue<T, A>) void
```

### `fn __init__(this &Queue<T, A>, other &const Queue<T, A>) void`

Copies another queue, values included.

- `other`: the queue to copy; its elements are copied front first, so the
  two queues share nothing afterwards

```
export
template <T type, A type = DefaultAllocator>
fn __init__(this &Queue<T, A>, other &const Queue<T, A>) void
```

### `fn __init__(this &Queue<T, A>, other &&Queue<T, A>) void`

Takes another queue's storage.

- `other`: the queue to take the buffer and allocator from, left empty

```
export
template <T type, A type = DefaultAllocator>
fn __init__(this &Queue<T, A>, other &&Queue<T, A>) void
```

### `fn __assign__(this &Queue<T, A>, other &const Queue<T, A>) void`

Copies another queue over this one.

- `other`: the queue to copy; this queue's own elements are dropped first,
  so the result holds exactly `other`'s

```
export
template <T type, A type = DefaultAllocator>
fn __assign__(this &Queue<T, A>, other &const Queue<T, A>) void
```

### `fn __assign__(this &Queue<T, A>, other &&Queue<T, A>) void`

Takes another queue's storage over this one.

- `other`: the queue to take the buffer from, left empty

```
export
template <T type, A type = DefaultAllocator>
fn __assign__(this &Queue<T, A>, other &&Queue<T, A>) void
```

### `fn __drop__(this &Queue<T, A>) void`

```
export
template <T type, A type = DefaultAllocator>
fn __drop__(this &Queue<T, A>) void
```

### `fn push(this &Queue<T, A>, value T) void`

Adds `value` at the back of the queue.

- `value`: the element to enqueue; it is moved out of the parameter's own
  storage rather than copied, and it will be the last one out

```
export
template <T type, A type = DefaultAllocator>
fn push(this &Queue<T, A>, value T) void
```

### `fn pop(this &Queue<T, A>) T`

Removes the element that has waited longest.

```
export
template <T type, A type = DefaultAllocator>
fn pop(this &Queue<T, A>) T
```

### `fn front(this &const Queue<T, A>) &const T`

Returns the element that has waited longest.

```
export
template <T type, A type = DefaultAllocator>
fn front(this &const Queue<T, A>) &const T
```

### `fn front(this &Queue<T, A>) &T`

```
export
template <T type, A type = DefaultAllocator>
fn front(this &Queue<T, A>) &T
```

### `fn length(this &const Queue<T, A>) usize`

```
export
template <T type, A type = DefaultAllocator>
fn length(this &const Queue<T, A>) usize
```

### `fn empty(this &const Queue<T, A>) bool`

```
export
template <T type, A type = DefaultAllocator>
fn empty(this &const Queue<T, A>) bool
```

### `fn clear(this &Queue<T, A>) void`

```
export
template <T type, A type = DefaultAllocator>
fn clear(this &Queue<T, A>) void
```

### `type Stack {`

A last-in, first-out sequence: a deque used only from the back.

```
export
template <T type, A type = DefaultAllocator>
type Stack {
    items Deque<T, A>;
}
```

### `fn __init__(this &Stack<T, A>) void`

```
export
template <T type, A type = DefaultAllocator>
fn __init__(this &Stack<T, A>) void
```

### `fn __init__(this &Stack<T, A>, other &const Stack<T, A>) void`

Copies another stack, values included.

- `other`: the stack to copy, bottom first; the two stacks share nothing
  afterwards

```
export
template <T type, A type = DefaultAllocator>
fn __init__(this &Stack<T, A>, other &const Stack<T, A>) void
```

### `fn __init__(this &Stack<T, A>, other &&Stack<T, A>) void`

Takes another stack's storage.

- `other`: the stack to take the buffer and allocator from, left empty

```
export
template <T type, A type = DefaultAllocator>
fn __init__(this &Stack<T, A>, other &&Stack<T, A>) void
```

### `fn __assign__(this &Stack<T, A>, other &const Stack<T, A>) void`

Copies another stack over this one.

- `other`: the stack to copy; this stack's own elements are dropped first,
  so the result holds exactly `other`'s

```
export
template <T type, A type = DefaultAllocator>
fn __assign__(this &Stack<T, A>, other &const Stack<T, A>) void
```

### `fn __assign__(this &Stack<T, A>, other &&Stack<T, A>) void`

Takes another stack's storage over this one.

- `other`: the stack to take the buffer from, left empty

```
export
template <T type, A type = DefaultAllocator>
fn __assign__(this &Stack<T, A>, other &&Stack<T, A>) void
```

### `fn __drop__(this &Stack<T, A>) void`

```
export
template <T type, A type = DefaultAllocator>
fn __drop__(this &Stack<T, A>) void
```

### `fn push(this &Stack<T, A>, value T) void`

Puts `value` on top of the stack.

- `value`: the element to push; it is moved out of the parameter's own
  storage rather than copied, and it becomes the next one `pop` returns

```
export
template <T type, A type = DefaultAllocator>
fn push(this &Stack<T, A>, value T) void
```

### `fn pop(this &Stack<T, A>) T`

Takes the value that was put on last.

```
export
template <T type, A type = DefaultAllocator>
fn pop(this &Stack<T, A>) T
```

### `fn top(this &const Stack<T, A>) &const T`

Returns the value on top.

```
export
template <T type, A type = DefaultAllocator>
fn top(this &const Stack<T, A>) &const T
```

### `fn top(this &Stack<T, A>) &T`

```
export
template <T type, A type = DefaultAllocator>
fn top(this &Stack<T, A>) &T
```

### `fn length(this &const Stack<T, A>) usize`

```
export
template <T type, A type = DefaultAllocator>
fn length(this &const Stack<T, A>) usize
```

### `fn empty(this &const Stack<T, A>) bool`

```
export
template <T type, A type = DefaultAllocator>
fn empty(this &const Stack<T, A>) bool
```

### `fn clear(this &Stack<T, A>) void`

```
export
template <T type, A type = DefaultAllocator>
fn clear(this &Stack<T, A>) void
```

### `fn cbegin(this &const Deque<T, A>) ConstDequeIterator<T, A>`

The first position, read only, named as C++ names it.

```
export
template <T type, A type>
fn cbegin(this &const Deque<T, A>) ConstDequeIterator<T, A>
```

### `fn cend(this &const Deque<T, A>) ConstDequeIterator<T, A>`

The one-past-the-end position, read only, named as C++ names it.

```
export
template <T type, A type>
fn cend(this &const Deque<T, A>) ConstDequeIterator<T, A>
```
