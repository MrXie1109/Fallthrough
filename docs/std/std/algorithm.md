# `std.algorithm`

Algorithms over ranges.

C++ keeps the algorithms out of the containers, and so does this library: a
container owns its storage, and its members are about that storage. Ordering,
searching and filling are about a *range*, which is a first position and a
one-past-the-end position -- exactly what `begin()` and `end()` hand back.

    values Array<int> = .{3, 1, 2};
    sort(values.begin(), values.end());

# One template per algorithm

Each algorithm is a single template over the iterator type, not one overload
per container and not one overload per kind of position. The body asks the
position for the operations the language gives an iterator -- `++` to step,
`*` to reach the element, `==`/`!=` to compare, and `+` where the algorithm
needs random access -- so an `Array` (whose positions are `*T`), a `Deque` (a
class with `__inc__` and `__add__`) and a `String` all get every algorithm
here, and a new container gets them by having `begin()` and `end()`.

The elements are compared with `<` and `==`, which is the requirement the
containers themselves put on an element type: the numbers, the strings, and
any declared type that writes `__lt__` and `__eq__` have every algorithm here.
`sort_by` and `is_sorted_by` take the order as a comparator *type* instead, the
same one the tree and the heap take, so a different order is a different type
argument rather than a second algorithm.

## Declarations

### `type Equal {`

The equality order: `==`.

The tree ships `Less` and `Greater`; equality is the third thing an algorithm
asks of its elements, so it lives beside them.

```
export
template <E type>
type Equal {
}
```

### `fn __call__(this &const Equal<E>, a &const E, b &const E) bool`

Answers whether `a` equals `b` under `==`.
- `a`: the left element
- `b`: the right element

```
export
template <E type>
fn __call__(this &const Equal<E>, a &const E, b &const E) bool
```

### `fn range_length(first It, last It) usize`

How many positions lie between `first` and `last`.

Stepping rather than subtracting keeps this available to a forward range: it
asks only for `++`, not for random access.
- `first`: where the range starts
- `last`: one past the range's end

```
@enable_if(InputIterator(It))
export
template <It type>
fn range_length(first It, last It) usize
```

### `fn swap(first It, second It) void`

Exchanges the elements two positions name.

The values travel through the element type's own copy, assignment and move,
so a type that owns storage swaps its storage rather than sharing it. A
member-wise exchange of two strings would leave both elements pointing at one
buffer.
- `first`: a position to write into
- `second`: the other position

```
@enable_if(ForwardIterator(It))
export
template <It type>
fn swap(first It, second It) void
```

### `fn sort(first It, last It) void`

Puts the range in order, smallest first.

A heap sort: `O(n log n)` in the worst case, no extra storage, and no
allocator -- which matters where an algorithm runs on a range whose size the
caller chose. It uses random access, so it takes a `*T` pair or a deque's
`begin()`/`end()`.
- `first`: where the range starts
- `last`: one past the range's end

```
@enable_if(RandomAccessIterator(It))
export
template <It type>
fn sort(first It, last It) void
```

### `fn sort_by(first It, last It, order Cmp) void`

Sorts a range in place under the comparator it is given.

`Greater` is how a descending sort is written, and a comparator a caller holds
is passed the same way:

    descending Greater<int>;
    sort_by(values.begin(), values.end(), descending);
- `first`: where the range starts
- `last`: one past the range's end
- `order`: the comparison that decides which element comes first

```
@enable_if(RandomAccessIterator(It))
export
template <It type, Cmp type>
fn sort_by(first It, last It, order Cmp) void
```

### `fn sift_down(first It, count usize, start usize) void`

Moves the element at `start` down until its children are both larger.
- `first`: the root of the heap
- `count`: how many elements the heap holds
- `start`: the element to move down

```
template <It type>
fn sift_down(first It, count usize, start usize) void
```

### `fn sift_down_by(first It, count usize, start usize, cmp Cmp) void`

`sift_down` under a comparator.
- `first`: the root of the heap
- `count`: how many elements the heap holds
- `start`: the element to move down
- `cmp`: the comparison that orders the heap

```
template <It type, Cmp type>
fn sift_down_by(first It, count usize, start usize, cmp Cmp) void
```

### `fn is_sorted(first It, last It) bool`

Whether a range is in order.
- `first`: where the range starts
- `last`: one past the range's end

```
@enable_if(ForwardIterator(It))
export
template <It type>
fn is_sorted(first It, last It) bool
```

### `fn is_sorted_by(first It, last It, order Cmp) bool`

Whether a range is in order under the comparator.
- `first`: where the range starts
- `last`: one past the range's end
- `order`: the comparison that decides the order

```
@enable_if(ForwardIterator(It))
export
template <It type, Cmp type>
fn is_sorted_by(first It, last It, order Cmp) bool
```

### `fn reverse(first It, last It) void`

Reverses a range in place.
- `first`: where the range starts
- `last`: one past the range's end

```
@enable_if(RandomAccessIterator(It))
export
template <It type>
fn reverse(first It, last It) void
```

### `fn fill(first It, last It, value T) void`

Puts `value` in every slot of a range.
- `first`: where the range starts
- `last`: one past the range's end
- `value`: what to write at every position

```
@enable_if(ForwardIterator(It))
export
template <It type, T type>
fn fill(first It, last It, value T) void
```

### `fn find(first It, last It, value &const T) It`

Returns the first position whose element equals `value`, or `last`.
- `first`: where to start looking
- `last`: one past the last position to look at
- `value`: what to look for

```
@enable_if(InputIterator(It))
export
template <It type, T type>
fn find(first It, last It, value &const T) It
```

### `fn index_of(first It, last It, value &const T) isize`

Returns the first position whose element equals `value`, or `-1`.
- `first`: where to start looking
- `last`: one past the last position to look at
- `value`: what to look for

```
@enable_if(InputIterator(It))
export
template <It type, T type>
fn index_of(first It, last It, value &const T) isize
```

### `fn count_of(first It, last It, value &const T) usize`

How many elements of a range equal `value`.
- `first`: where to start counting
- `last`: one past the last position to count
- `value`: the element to count

```
@enable_if(InputIterator(It))
export
template <It type, T type>
fn count_of(first It, last It, value &const T) usize
```
