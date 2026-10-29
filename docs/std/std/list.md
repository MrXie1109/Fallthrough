# `std.list`

A doubly linked list: stable positions, cheap insertion at both ends.

    list List<int> = List<int>();
    list.push_back(1);
    list.push_front(0);
    for (value &T : list) { ... }

The nodes are allocated one at a time, so a position stays valid until that
element is removed, and pushing at either end costs one allocation and no
moving. The price is that a list cannot be indexed: reaching element `n` walks
from an end, which is why the algorithms that need random access are for
`Array` and `Deque`.

# Walking

`begin()`/`end()` give a `ListIterator<T>` whose `__inc__` follows `next` and
whose `__deref__` is the element. `end()` is the null position.

## Declarations

### `type ListNode {`

One node of the list.

```
export
template <T type>
type ListNode {
    // The node before this one, or null at the front.
    _previous *ListNode<T>;
    // The node after this one, or null at the back.
    _next *ListNode<T>;
    // The element.
    _value T;
}
```

### `type List {`

A doubly linked list.

```
export
template <T type>
type List {
    // The first node, or null when empty.
    _head *ListNode<T>;
    // The last node, or null when empty.
    _tail *ListNode<T>;
    // How many elements there are.
    _len usize;
}
```

### `fn __init__(this &List<T>) void`

Creates an empty list.

```
export
template <T type>
fn __init__(this &List<T>) void
```

### `fn push(this &List<T>, value T) void`

Adds `value` at the end.

The same operation as `push_back`, under the shorter name.

- `value`: the element to copy into a new node at the back

```
export
template <T type>
fn push(this &List<T>, value T) void
```

### `fn push_back(this &List<T>, value T) void`

Adds `value` at the end.

- `value`: the element to copy into a new node at the back, which becomes
  the list's last element

```
export
template <T type>
fn push_back(this &List<T>, value T) void
```

### `fn push_front(this &List<T>, value T) void`

Prepends `value`.

- `value`: the element to copy into a new node at the front, which becomes
  the list's first element

```
export
template <T type>
fn push_front(this &List<T>, value T) void
```

### `fn pop_front(this &List<T>) ?T`

Removes and returns the first element, or an absent optional.

```
export
template <T type>
fn pop_front(this &List<T>) ?T
```

### `fn pop_back(this &List<T>) ?T`

Removes and returns the last element, or an absent optional.

```
export
template <T type>
fn pop_back(this &List<T>) ?T
```

### `fn unlink(this &List<T>, node *ListNode<T>) void`

Unlinks and frees `node`, taking its value out first.

- `node`: the node to remove, which must belong to this list and not be
  null; its value is destroyed and the node is freed, so nothing may use it
  afterwards

```
template <T type>
fn unlink(this &List<T>, node *ListNode<T>) void
```

### `fn length(this &const List<T>) usize`

The number of elements.

```
export
template <T type>
fn length(this &const List<T>) usize
```

### `fn empty(this &const List<T>) bool`

Whether there are no elements.

```
export
template <T type>
fn empty(this &const List<T>) bool
```

### `fn front(this &const List<T>) ?T`

The first element, or an absent optional.

```
export
template <T type>
fn front(this &const List<T>) ?T
```

### `fn back(this &const List<T>) ?T`

The last element, or an absent optional.

```
export
template <T type>
fn back(this &const List<T>) ?T
```

### `fn begin(this &const List<T>) ListIterator<T>`

The first position.

```
export
template <T type>
fn begin(this &const List<T>) ListIterator<T>
```

### `fn end(this &const List<T>) ListIterator<T>`

The one-past-the-end position: the null node.

```
export
template <T type>
fn end(this &const List<T>) ListIterator<T>
```

### `fn clear(this &List<T>) void`

Removes every element.

```
export
template <T type>
fn clear(this &List<T>) void
```

### `fn __drop__(this &List<T>) void`

Releases the list.

```
export
template <T type>
fn __drop__(this &List<T>) void
```

### `type ListIterator {`

A position in a list.

```
export
template <T type>
type ListIterator {
    // The node it stands on, or null for the end.
    _node *ListNode<T>;
}
```

### `fn __init__(this &ListIterator<T>) void`

Creates the end position.

```
export
template <T type>
fn __init__(this &ListIterator<T>) void
```

### `fn __init__(this &ListIterator<T>, node *ListNode<T>) void`

Creates a position at `node`.

- `node`: the node the position stands on; null makes the end position, the
  same position the constructor without an argument makes

```
export
template <T type>
fn __init__(this &ListIterator<T>, node *ListNode<T>) void
```

### `fn __inc__(this &ListIterator<T>) void`

Steps to the next element.

```
export
template <T type>
fn __inc__(this &ListIterator<T>) void
```

### `fn __deref__(this &ListIterator<T>) &T`

The element the position stands on.

```
export
template <T type>
fn __deref__(this &ListIterator<T>) &T
```

### `fn __eq__(this &const ListIterator<T>, other &const ListIterator<T>) bool`

Whether two positions stand on the same node.

- `other`: the position to compare with; two end positions are equal, and a
  position equals itself

```
export
template <T type>
fn __eq__(this &const ListIterator<T>, other &const ListIterator<T>) bool
```

### `fn __ne__(this &const ListIterator<T>, other &const ListIterator<T>) bool`

Whether two positions stand on different nodes.

- `other`: the position to compare with; this is the negation of `==`, so a
  position is never unequal to itself

```
export
template <T type>
fn __ne__(this &const ListIterator<T>, other &const ListIterator<T>) bool
```

### `fn cbegin(this &const List<T>) ListIterator<T>`

The first position, read only, named as C++ names it.

```
export
template <T type>
fn cbegin(this &const List<T>) ListIterator<T>
```

### `fn cend(this &const List<T>) ListIterator<T>`

The one-past-the-end position, read only, named as C++ names it.

```
export
template <T type>
fn cend(this &const List<T>) ListIterator<T>
```
