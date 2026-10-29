# `std.tree`

Trees: an ordered map, the red-black tree it is built on, and the four
containers that wrap it.

A tree keeps its keys in order, so a walk visits them sorted and a lookup is
logarithmic. The order is a *type*: `Less<K>` is `<`, and any type with a
`__call__(a &const K, b &const K) bool` may stand in its place, which is what
makes a set of a type the language does not know how to compare possible.

The four containers are one tree with a different surface: `TMap` and `TSet`
hold one value per key, and `TMMap`/`TMSet` hold any number of them.
The allocator's methods are named by the bodies below -- `this.mem.alloc(..)` --
and a name has to be in scope where it is written. An imported *type* does not
bring its methods: the family that names them imports them too.

## Declarations

### `export traits Compare(it C, value E) {`

------------------------------------------------------------------- order
What a type has to provide to order the elements of a container.

One method is the whole contract: `less(a, b)` answers whether `a` comes
before `b`. A strict weak order is what the tree needs, so `less(a, a)` has to
be false and `less(a, b) && less(b, c)` has to imply `less(a, c)`; the tree
relies on it rather than checking it.

```
export traits Compare(it C, value E) {
    it(*const E, *const E) -> bool,
}
```

### `type Less {`

The default order: `<`.

Every type that takes part in `<` -- the integers, the floats, `String`,
pointers -- is ordered by it, and a declared type joins in by defining
`__lt__`.

```
export
template <E type>
type Less {
}
```

### `fn __call__(this &const Less<E>, a &const E, b &const E) bool`

Answers whether `a` comes before `b` under `<`.

- `a`: the left element
- `b`: the right element

```
export
template <E type>
fn __call__(this &const Less<E>, a &const E, b &const E) bool
```

### `type Greater {`

The reverse order: the larger element comes first.

A container takes its order as a type, so this is how a descending map is
spelled: `TMap<String, int, Greater<String>>`.

```
export
template <E type>
type Greater {
}
```

### `fn __call__(this &const Greater<E>, a &const E, b &const E) bool`

Answers whether `a` comes after `b`.

- `a`: the left element
- `b`: the right element

```
export
template <E type>
fn __call__(this &const Greater<E>, a &const E, b &const E) bool
```

### `type TreeNode {`

-------------------------------------------------------------------- tree
One node of the tree.

A link is a pointer to a node, and a pointer needs only the name: the member
being built is already a type when its fields are resolved, incomplete, which
is all a pointee has to be. The key and the value are constructed in place,
because the node's storage is raw memory from the allocator.

```
export
template <K type, V type>
type TreeNode {
    // The node's key, which is what the order is about.
    key K;
    // The node's value. A set stores a `Unit` here.
    value V;
    // Whether the node is red. A black node is the default, so a new node is red
    // and the fix-up decides where that is allowed.
    red bool;
    // The left child, or null.
    left *TreeNode<K, V>;
    // The right child, or null.
    right *TreeNode<K, V>;
    // The parent, or null at the root.
    parent *TreeNode<K, V>;
}
```

### `type RBTree {`

An ordered map from keys to values, as a red-black tree.

This is the structure the four containers share; a program that wants a map
writes `TMap`, which wraps it.

The invariants are the classical ones: every node is red or black, the root is
black, a red node's children are black, and every path from a node to a null
child has the same number of black nodes. Together they bound the height at
twice the black height, which is what keeps every operation logarithmic.

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
type RBTree {
    // The root, or null in an empty tree.
    root *TreeNode<K, V>;
    // How many entries the tree holds.
    _count usize;
    // The allocator the nodes come from.
    mem A;
    // The order.
    cmp Cmp;
}
```

### `fn __init__(this &RBTree<K, V, Cmp, A>) void`

Creates an empty tree.

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn __init__(this &RBTree<K, V, Cmp, A>) void
```

### `fn __init__(this &RBTree<K, V, Cmp, A>, order Cmp, memory A) void`

Creates an empty tree with an order and an allocator of its caller's.

- `order`: the comparison that decides where a key goes
- `memory`: the allocator to draw nodes from

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn __init__(this &RBTree<K, V, Cmp, A>, order Cmp, memory A) void
```

### `fn clear(this &RBTree<K, V, Cmp, A>) void`

Destroys every node.

The walk is iterative, and it is deliberately so: a recursive helper would be
a template that calls itself, and a member of a family is instantiated by the
call that needs it, so the body would be checked again before the instantiation
was recorded.

Children are taken before the node they hang from, and the link that led down
is cut on the way back up, so each node is visited once.

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn clear(this &RBTree<K, V, Cmp, A>) void
```

### `fn copy_nodes(this &RBTree<K, V, Cmp, A>, node *const TreeNode<K, V>) void`

Copies every entry of `node`'s subtree into this tree.

- `node`: the root of the subtree to copy

```
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn copy_nodes(this &RBTree<K, V, Cmp, A>, node *const TreeNode<K, V>) void
```

### `fn __init__(this &RBTree<K, V, Cmp, A>, other &const RBTree<K, V, Cmp, A>) void`

Copies another tree, nodes and all.

- `other`: the tree to copy

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn __init__(this &RBTree<K, V, Cmp, A>, other &const RBTree<K, V, Cmp, A>) void
```

### `fn __init__(this &RBTree<K, V, Cmp, A>, other &&RBTree<K, V, Cmp, A>) void`

Takes another tree's nodes.

- `other`: the tree to take from, left empty

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn __init__(this &RBTree<K, V, Cmp, A>, other &&RBTree<K, V, Cmp, A>) void
```

### `fn __assign__(this &RBTree<K, V, Cmp, A>, other &const RBTree<K, V, Cmp, A>) void`

Copies another tree over this one.

- `other`: the tree to copy

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn __assign__(this &RBTree<K, V, Cmp, A>, other &const RBTree<K, V, Cmp, A>) void
```

### `fn __assign__(this &RBTree<K, V, Cmp, A>, other &&RBTree<K, V, Cmp, A>) void`

Takes another tree's nodes over this one.

- `other`: the tree to take from, left empty

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn __assign__(this &RBTree<K, V, Cmp, A>, other &&RBTree<K, V, Cmp, A>) void
```

### `fn __drop__(this &RBTree<K, V, Cmp, A>) void`

Destroys the tree with its nodes.

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn __drop__(this &RBTree<K, V, Cmp, A>) void
```

### `fn length(this &const RBTree<K, V, Cmp, A>) usize`

Returns how many entries the tree holds.

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn length(this &const RBTree<K, V, Cmp, A>) usize
```

### `fn empty(this &const RBTree<K, V, Cmp, A>) bool`

Returns `true` when the tree holds nothing.

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn empty(this &const RBTree<K, V, Cmp, A>) bool
```

### `fn rotate_left(this &RBTree<K, V, Cmp, A>, node *TreeNode<K, V>) void`

Moves `node` up over its parent, which is the left rotation.

The subtree's in-order sequence is unchanged: the parent's right child becomes
the node's left child, and the node takes the parent's place.

- `node`: the node the walk starts or pivots on

```
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn rotate_left(this &RBTree<K, V, Cmp, A>, node *TreeNode<K, V>) void
```

### `fn rotate_right(this &RBTree<K, V, Cmp, A>, node *TreeNode<K, V>) void`

Moves `node` up over its parent, the other way.

- `node`: the node the walk starts or pivots on

```
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn rotate_right(this &RBTree<K, V, Cmp, A>, node *TreeNode<K, V>) void
```

### `fn insert_fixup(this &RBTree<K, V, Cmp, A>, node *TreeNode<K, V>) void`

Restores the red-black invariants after an insertion.

A new node is red. A red node under a red parent is the only violation, and it
is repaired by recolouring the uncle or by rotating the parent and the
grandparent; each step moves the violation up, so the walk ends at the root,
which is painted black.

- `node`: the node the walk starts or pivots on

```
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn insert_fixup(this &RBTree<K, V, Cmp, A>, node *TreeNode<K, V>) void
```

### `fn insert(this &RBTree<K, V, Cmp, A>, key K, value V) *TreeNode<K, V>`

Adds one entry, replacing the value when the key is already there.

Returns the node that holds the key. The key itself is not replaced: a map's
keys are what the order is about, and replacing one could move it.

- `key`: the key to place
- `value`: the value to store with it

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn insert(this &RBTree<K, V, Cmp, A>, key K, value V) *TreeNode<K, V>
```

### `fn insert_multi(this &RBTree<K, V, Cmp, A>, key K, value V) *TreeNode<K, V>`

Adds one entry whether or not the key is already there.

This is what a multi-container is: one entry per call, in order.

- `key`: the key to place
- `value`: the value to store with it

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn insert_multi(this &RBTree<K, V, Cmp, A>, key K, value V) *TreeNode<K, V>
```

### `fn insert_new(this &RBTree<K, V, Cmp, A>, key K, value V) *TreeNode<K, V>`

Adds an entry that is known to be new.

- `key`: the key to place
- `value`: the value to store with it

```
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn insert_new(this &RBTree<K, V, Cmp, A>, key K, value V) *TreeNode<K, V>
```

### `fn find(this &const RBTree<K, V, Cmp, A>, key &const K) *const TreeNode<K, V>`

Returns the node holding `key`, or null.

- `key`: the key to look for

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn find(this &const RBTree<K, V, Cmp, A>, key &const K) *const TreeNode<K, V>
```

### `fn find(this &RBTree<K, V, Cmp, A>, key &const K) *TreeNode<K, V>`

Returns the node holding `key`, for writing, or null.

- `key`: the key to look for

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn find(this &RBTree<K, V, Cmp, A>, key &const K) *TreeNode<K, V>
```

### `fn contains(this &const RBTree<K, V, Cmp, A>, key &const K) bool`

Returns `true` when the key is there.

- `key`: the key to look for

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn contains(this &const RBTree<K, V, Cmp, A>, key &const K) bool
```

### `fn count(this &const RBTree<K, V, Cmp, A>, key &const K) usize`

Returns how many entries have this key.

- `key`: the key to look for

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn count(this &const RBTree<K, V, Cmp, A>, key &const K) usize
```

### `fn lower_bound(this &const RBTree<K, V, Cmp, A>, key &const K) *TreeNode<K, V>`

Returns the first node whose key is not less than `key`, or null.

- `key`: the key to look for

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn lower_bound(this &const RBTree<K, V, Cmp, A>, key &const K) *TreeNode<K, V>
```

### `fn upper_bound(this &const RBTree<K, V, Cmp, A>, key &const K) *TreeNode<K, V>`

Returns the first node whose key is greater than `key`, or null.

- `key`: the key to look for

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn upper_bound(this &const RBTree<K, V, Cmp, A>, key &const K) *TreeNode<K, V>
```

### `fn erase(this &RBTree<K, V, Cmp, A>, key &const K) bool`

Removes the entry with this key, and returns whether there was one.

- `key`: the key to look for

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn erase(this &RBTree<K, V, Cmp, A>, key &const K) bool
```

### `fn erase_all(this &RBTree<K, V, Cmp, A>, key &const K) usize`

Removes every entry with this key, and returns how many there were.

- `key`: the key to look for

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn erase_all(this &RBTree<K, V, Cmp, A>, key &const K) usize
```

### `fn minimum(node *TreeNode<K, V>) *TreeNode<K, V>`

The leftmost node of the subtree rooted at `node`.

- `node`: the node the walk starts or pivots on

```
template <K type, V type>
fn minimum(node *TreeNode<K, V>) *TreeNode<K, V>
```

### `fn maximum(node *TreeNode<K, V>) *TreeNode<K, V>`

The rightmost node of the subtree rooted at `node`.

- `node`: the node the walk starts or pivots on

```
template <K type, V type>
fn maximum(node *TreeNode<K, V>) *TreeNode<K, V>
```

### `fn successor(node *TreeNode<K, V>) *TreeNode<K, V>`

The node that comes after `node` in order, or null at the end.

- `node`: the node the walk starts or pivots on

```
template <K type, V type>
fn successor(node *TreeNode<K, V>) *TreeNode<K, V>
```

### `fn predecessor(node *TreeNode<K, V>) *TreeNode<K, V>`

The node that comes before `node` in order, or null at the beginning.

- `node`: the node the walk starts or pivots on

```
template <K type, V type>
fn predecessor(node *TreeNode<K, V>) *TreeNode<K, V>
```

### `fn transplant(this &RBTree<K, V, Cmp, A>, node *TreeNode<K, V>, replacement *TreeNode<K, V>) void`

Puts `replacement` where `node` is.

- `node`: the node being taken out of its place
- `replacement`: the subtree that takes its place

```
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn transplant(this &RBTree<K, V, Cmp, A>, node *TreeNode<K, V>, replacement *TreeNode<K, V>) void
```

### `fn remove_node(this &RBTree<K, V, Cmp, A>, node *TreeNode<K, V>) void`

Removes one node and restores the invariants.

Removing a black node shortens every path through it by one black node, which
the fix-up repairs by borrowing a black node from a sibling or by pushing the
deficit up. The child that takes the removed node's place may be null, so its
parent is what the walk follows.

- `node`: the node the walk starts or pivots on

```
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn remove_node(this &RBTree<K, V, Cmp, A>, node *TreeNode<K, V>) void
```

### `fn erase_fixup(this &RBTree<K, V, Cmp, A>, start *TreeNode<K, V>, start_parent *TreeNode<K, V>) void`

Restores the invariants after a removal.

- `start`: the node the double-black sits on, or null
- `start_parent`: its parent, needed when it is null

```
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn erase_fixup(this &RBTree<K, V, Cmp, A>, start *TreeNode<K, V>, start_parent *TreeNode<K, V>) void
```

### `type MapIterator {`

----------------------------------------------------------------- iterators
A position in a map, which walks the entries in key order.

`*it` is the mapped value and `it.key()` is the key, so a map is walked without
building a pair for every step. `++` and `--` are the node's own links, so a
walk needs no stack.

```
export
template <K type, V type>
type MapIterator {
    // The node the iterator stands on, or null at the end.
    node *TreeNode<K, V>;
}
```

### `fn __init__(this &MapIterator<K, V>) void`

Creates an iterator that stands nowhere, which is how `end()` is written.

```
export
template <K type, V type>
fn __init__(this &MapIterator<K, V>) void
```

### `fn __init__(this &MapIterator<K, V>, node *TreeNode<K, V>) void`

Creates an iterator standing on a node.

- `node`: the node the walk starts or pivots on

```
export
template <K type, V type>
fn __init__(this &MapIterator<K, V>, node *TreeNode<K, V>) void
```

### `fn __inc__(this &MapIterator<K, V>) void`

Moves to the next key.

```
export
template <K type, V type>
fn __inc__(this &MapIterator<K, V>) void
```

### `fn __dec__(this &MapIterator<K, V>) void`

Moves to the previous key.

```
export
template <K type, V type>
fn __dec__(this &MapIterator<K, V>) void
```

### `fn __deref__(this &MapIterator<K, V>) &V`

The mapped value at this position, for writing.

```
export
template <K type, V type>
fn __deref__(this &MapIterator<K, V>) &V
```

### `fn key(this &const MapIterator<K, V>) &const K`

The key at this position, for reading.

```
export
template <K type, V type>
fn key(this &const MapIterator<K, V>) &const K
```

### `fn __eq__(this &const MapIterator<K, V>, other &const MapIterator<K, V>) bool`

Whether two positions are the same.

- `other`: the value to copy

```
export
template <K type, V type>
fn __eq__(this &const MapIterator<K, V>, other &const MapIterator<K, V>) bool
```

### `fn __ne__(this &const MapIterator<K, V>, other &const MapIterator<K, V>) bool`

Whether two positions differ.

- `other`: the value to copy

```
export
template <K type, V type>
fn __ne__(this &const MapIterator<K, V>, other &const MapIterator<K, V>) bool
```

### `type ConstMapIterator {`

A position in a map, read only.

```
export
template <K type, V type>
type ConstMapIterator {
    // The node the iterator stands on, or null at the end.
    node *const TreeNode<K, V>;
}
```

### `fn __init__(this &ConstMapIterator<K, V>) void`

Creates an iterator that stands nowhere.

```
export
template <K type, V type>
fn __init__(this &ConstMapIterator<K, V>) void
```

### `fn __init__(this &ConstMapIterator<K, V>, node *const TreeNode<K, V>) void`

Creates an iterator standing on a node.

- `node`: the node the walk starts or pivots on

```
export
template <K type, V type>
fn __init__(this &ConstMapIterator<K, V>, node *const TreeNode<K, V>) void
```

### `fn __inc__(this &ConstMapIterator<K, V>) void`

Moves to the next key.

```
export
template <K type, V type>
fn __inc__(this &ConstMapIterator<K, V>) void
```

### `fn __dec__(this &ConstMapIterator<K, V>) void`

Moves to the previous key.

```
export
template <K type, V type>
fn __dec__(this &ConstMapIterator<K, V>) void
```

### `fn __deref__(this &const ConstMapIterator<K, V>) &const V`

The mapped value at this position, for reading.

```
export
template <K type, V type>
fn __deref__(this &const ConstMapIterator<K, V>) &const V
```

### `fn key(this &const ConstMapIterator<K, V>) &const K`

The key at this position, for reading.

```
export
template <K type, V type>
fn key(this &const ConstMapIterator<K, V>) &const K
```

### `fn __eq__(this &const ConstMapIterator<K, V>, other &const ConstMapIterator<K, V>) bool`

Whether two positions are the same.

- `other`: the value to copy

```
export
template <K type, V type>
fn __eq__(this &const ConstMapIterator<K, V>, other &const ConstMapIterator<K, V>) bool
```

### `fn __ne__(this &const ConstMapIterator<K, V>, other &const ConstMapIterator<K, V>) bool`

Whether two positions differ.

- `other`: the value to copy

```
export
template <K type, V type>
fn __ne__(this &const ConstMapIterator<K, V>, other &const ConstMapIterator<K, V>) bool
```

### `type SetIterator {`

A position in a set, which walks the keys in order.

```
export
template <K type>
type SetIterator {
    // The node the iterator stands on, or null at the end.
    node *TreeNode<K, Unit>;
}
```

### `fn __init__(this &SetIterator<K>) void`

Creates an iterator that stands nowhere.

```
export
template <K type>
fn __init__(this &SetIterator<K>) void
```

### `fn __init__(this &SetIterator<K>, node *TreeNode<K, Unit>) void`

Creates an iterator standing on a node.

- `node`: the node the walk starts or pivots on

```
export
template <K type>
fn __init__(this &SetIterator<K>, node *TreeNode<K, Unit>) void
```

### `fn __inc__(this &SetIterator<K>) void`

Moves to the next key.

```
export
template <K type>
fn __inc__(this &SetIterator<K>) void
```

### `fn __dec__(this &SetIterator<K>) void`

Moves to the previous key.

```
export
template <K type>
fn __dec__(this &SetIterator<K>) void
```

### `fn __deref__(this &SetIterator<K>) &const K`

The key at this position, for reading.

```
export
template <K type>
fn __deref__(this &SetIterator<K>) &const K
```

### `fn __eq__(this &const SetIterator<K>, other &const SetIterator<K>) bool`

Whether two positions are the same.

- `other`: the value to copy

```
export
template <K type>
fn __eq__(this &const SetIterator<K>, other &const SetIterator<K>) bool
```

### `fn __ne__(this &const SetIterator<K>, other &const SetIterator<K>) bool`

Whether two positions differ.

- `other`: the value to copy

```
export
template <K type>
fn __ne__(this &const SetIterator<K>, other &const SetIterator<K>) bool
```

### `type ConstSetIterator {`

A position in a set, read only.

```
export
template <K type>
type ConstSetIterator {
    // The node the iterator stands on, or null at the end.
    node *const TreeNode<K, Unit>;
}
```

### `fn __init__(this &ConstSetIterator<K>) void`

Creates an iterator that stands nowhere.

```
export
template <K type>
fn __init__(this &ConstSetIterator<K>) void
```

### `fn __init__(this &ConstSetIterator<K>, node *const TreeNode<K, Unit>) void`

Creates an iterator standing on a node.

- `node`: the node the walk starts or pivots on

```
export
template <K type>
fn __init__(this &ConstSetIterator<K>, node *const TreeNode<K, Unit>) void
```

### `fn __inc__(this &ConstSetIterator<K>) void`

Moves to the next key.

```
export
template <K type>
fn __inc__(this &ConstSetIterator<K>) void
```

### `fn __dec__(this &ConstSetIterator<K>) void`

Moves to the previous key.

```
export
template <K type>
fn __dec__(this &ConstSetIterator<K>) void
```

### `fn __deref__(this &const ConstSetIterator<K>) &const K`

The key at this position, for reading.

```
export
template <K type>
fn __deref__(this &const ConstSetIterator<K>) &const K
```

### `fn __eq__(this &const ConstSetIterator<K>, other &const ConstSetIterator<K>) bool`

Whether two positions are the same.

- `other`: the value to copy

```
export
template <K type>
fn __eq__(this &const ConstSetIterator<K>, other &const ConstSetIterator<K>) bool
```

### `fn __ne__(this &const ConstSetIterator<K>, other &const ConstSetIterator<K>) bool`

Whether two positions differ.

- `other`: the value to copy

```
export
template <K type>
fn __ne__(this &const ConstSetIterator<K>, other &const ConstSetIterator<K>) bool
```

### `type TMap {`

------------------------------------------------------------------ TMap
An ordered map: one value per key, walked in key order.

The wrapper adds the surface a program expects -- `insert`, `find`, `at`,
`erase`, `begin`/`end` -- and leaves the tree to the structure that holds it.

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
type TMap {
    // The tree that holds the entries.
    tree RBTree<K, V, Cmp, A>;
}
```

### `fn insert(this &TMap<K, V, Cmp, A>, key K, value V) void`

Adds an entry, replacing the value when the key is already there.

- `key`: the key to place
- `value`: the value to store with it

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn insert(this &TMap<K, V, Cmp, A>, key K, value V) void
```

### `fn find(this &TMap<K, V, Cmp, A>, key &const K) *V`

Returns the value for a key, or null.

- `key`: the key to look for

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn find(this &TMap<K, V, Cmp, A>, key &const K) *V
```

### `fn find(this &const TMap<K, V, Cmp, A>, key &const K) *const V`

Returns the value for a key, or null, for reading.

- `key`: the key to look for

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn find(this &const TMap<K, V, Cmp, A>, key &const K) *const V
```

### `fn at(this &TMap<K, V, Cmp, A>, key &const K) &V`

Returns the value for a key, and aborts when there is none.

- `key`: the key to look for

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn at(this &TMap<K, V, Cmp, A>, key &const K) &V
```

### `fn at(this &const TMap<K, V, Cmp, A>, key &const K) &const V`

Returns the value for a key, and aborts when there is none, for reading.

- `key`: the key to look for

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn at(this &const TMap<K, V, Cmp, A>, key &const K) &const V
```

### `fn __getitem__(this &TMap<K, V, Cmp, A>, key &const K) &V`

Indexes the map: `m[key]` is the value, and a missing key aborts.

- `key`: the key to look for

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn __getitem__(this &TMap<K, V, Cmp, A>, key &const K) &V
```

### `fn contains(this &const TMap<K, V, Cmp, A>, key &const K) bool`

Whether the key is there.

- `key`: the key to look for

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn contains(this &const TMap<K, V, Cmp, A>, key &const K) bool
```

### `fn count(this &const TMap<K, V, Cmp, A>, key &const K) usize`

How many entries have this key. A map has at most one.

- `key`: the key to look for

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn count(this &const TMap<K, V, Cmp, A>, key &const K) usize
```

### `fn erase(this &TMap<K, V, Cmp, A>, key &const K) bool`

Removes the entry with this key, and returns whether there was one.

- `key`: the key to look for

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn erase(this &TMap<K, V, Cmp, A>, key &const K) bool
```

### `fn clear(this &TMap<K, V, Cmp, A>) void`

Removes every entry.

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn clear(this &TMap<K, V, Cmp, A>) void
```

### `fn length(this &const TMap<K, V, Cmp, A>) usize`

How many entries the map holds.

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn length(this &const TMap<K, V, Cmp, A>) usize
```

### `fn empty(this &const TMap<K, V, Cmp, A>) bool`

Whether the map holds nothing.

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn empty(this &const TMap<K, V, Cmp, A>) bool
```

### `fn begin(this &TMap<K, V, Cmp, A>) MapIterator<K, V>`

The first entry by key.

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn begin(this &TMap<K, V, Cmp, A>) MapIterator<K, V>
```

### `fn end(this &TMap<K, V, Cmp, A>) MapIterator<K, V>`

One past the last entry.

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn end(this &TMap<K, V, Cmp, A>) MapIterator<K, V>
```

### `fn begin(this &const TMap<K, V, Cmp, A>) ConstMapIterator<K, V>`

The first entry by key, for reading.

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn begin(this &const TMap<K, V, Cmp, A>) ConstMapIterator<K, V>
```

### `fn end(this &const TMap<K, V, Cmp, A>) ConstMapIterator<K, V>`

One past the last entry, for reading.

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn end(this &const TMap<K, V, Cmp, A>) ConstMapIterator<K, V>
```

### `type TSet {`

------------------------------------------------------------------ TSet
An ordered set: one entry per key, walked in key order.

```
export
template <K type, Cmp type = Less<K>, A type = DefaultAllocator>
type TSet {
    // The tree that holds the keys, with a `Unit` where a map keeps a value.
    tree RBTree<K, Unit, Cmp, A>;
}
```

### `fn insert(this &TSet<K, Cmp, A>, key K) void`

Adds a key, and does nothing when it is already there.

- `key`: the key to look for

```
export
template <K type, Cmp type = Less<K>, A type = DefaultAllocator>
fn insert(this &TSet<K, Cmp, A>, key K) void
```

### `fn contains(this &const TSet<K, Cmp, A>, key &const K) bool`

Whether the key is there.

- `key`: the key to look for

```
export
template <K type, Cmp type = Less<K>, A type = DefaultAllocator>
fn contains(this &const TSet<K, Cmp, A>, key &const K) bool
```

### `fn count(this &const TSet<K, Cmp, A>, key &const K) usize`

How many entries have this key. A set has at most one.

- `key`: the key to look for

```
export
template <K type, Cmp type = Less<K>, A type = DefaultAllocator>
fn count(this &const TSet<K, Cmp, A>, key &const K) usize
```

### `fn erase(this &TSet<K, Cmp, A>, key &const K) bool`

Removes the key, and returns whether it was there.

- `key`: the key to look for

```
export
template <K type, Cmp type = Less<K>, A type = DefaultAllocator>
fn erase(this &TSet<K, Cmp, A>, key &const K) bool
```

### `fn clear(this &TSet<K, Cmp, A>) void`

Removes every key.

```
export
template <K type, Cmp type = Less<K>, A type = DefaultAllocator>
fn clear(this &TSet<K, Cmp, A>) void
```

### `fn length(this &const TSet<K, Cmp, A>) usize`

How many keys the set holds.

```
export
template <K type, Cmp type = Less<K>, A type = DefaultAllocator>
fn length(this &const TSet<K, Cmp, A>) usize
```

### `fn empty(this &const TSet<K, Cmp, A>) bool`

Whether the set holds nothing.

```
export
template <K type, Cmp type = Less<K>, A type = DefaultAllocator>
fn empty(this &const TSet<K, Cmp, A>) bool
```

### `fn begin(this &TSet<K, Cmp, A>) SetIterator<K>`

The first key.

```
export
template <K type, Cmp type = Less<K>, A type = DefaultAllocator>
fn begin(this &TSet<K, Cmp, A>) SetIterator<K>
```

### `fn end(this &TSet<K, Cmp, A>) SetIterator<K>`

One past the last key.

```
export
template <K type, Cmp type = Less<K>, A type = DefaultAllocator>
fn end(this &TSet<K, Cmp, A>) SetIterator<K>
```

### `fn begin(this &const TSet<K, Cmp, A>) ConstSetIterator<K>`

The first key, for reading.

```
export
template <K type, Cmp type = Less<K>, A type = DefaultAllocator>
fn begin(this &const TSet<K, Cmp, A>) ConstSetIterator<K>
```

### `fn end(this &const TSet<K, Cmp, A>) ConstSetIterator<K>`

One past the last key, for reading.

```
export
template <K type, Cmp type = Less<K>, A type = DefaultAllocator>
fn end(this &const TSet<K, Cmp, A>) ConstSetIterator<K>
```

### `type TMMap {`

----------------------------------------------------------------- TMMap
An ordered multi-map: any number of values per key, walked in key order.

A multi-container keeps the entries in insertion order within one key, because
an equal key is placed to the right of the ones already there.

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
type TMMap {
    // The tree that holds the entries.
    tree RBTree<K, V, Cmp, A>;
}
```

### `fn insert(this &TMMap<K, V, Cmp, A>, key K, value V) void`

Adds an entry, whether or not the key is already there.

- `key`: the key to place
- `value`: the value to store with it

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn insert(this &TMMap<K, V, Cmp, A>, key K, value V) void
```

### `fn find(this &TMMap<K, V, Cmp, A>, key &const K) *V`

Returns the first value for a key, or null.

- `key`: the key to look for

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn find(this &TMMap<K, V, Cmp, A>, key &const K) *V
```

### `fn contains(this &const TMMap<K, V, Cmp, A>, key &const K) bool`

Whether the key is there.

- `key`: the key to look for

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn contains(this &const TMMap<K, V, Cmp, A>, key &const K) bool
```

### `fn count(this &const TMMap<K, V, Cmp, A>, key &const K) usize`

How many entries have this key.

- `key`: the key to look for

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn count(this &const TMMap<K, V, Cmp, A>, key &const K) usize
```

### `fn erase(this &TMMap<K, V, Cmp, A>, key &const K) bool`

Removes the first entry with this key, and returns whether there was one.

- `key`: the key to look for

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn erase(this &TMMap<K, V, Cmp, A>, key &const K) bool
```

### `fn erase_all(this &TMMap<K, V, Cmp, A>, key &const K) usize`

Removes every entry with this key, and returns how many there were.

- `key`: the key to look for

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn erase_all(this &TMMap<K, V, Cmp, A>, key &const K) usize
```

### `fn clear(this &TMMap<K, V, Cmp, A>) void`

Removes every entry.

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn clear(this &TMMap<K, V, Cmp, A>) void
```

### `fn length(this &const TMMap<K, V, Cmp, A>) usize`

How many entries the map holds.

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn length(this &const TMMap<K, V, Cmp, A>) usize
```

### `fn empty(this &const TMMap<K, V, Cmp, A>) bool`

Whether the map holds nothing.

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn empty(this &const TMMap<K, V, Cmp, A>) bool
```

### `fn begin(this &TMMap<K, V, Cmp, A>) MapIterator<K, V>`

The first entry by key.

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn begin(this &TMMap<K, V, Cmp, A>) MapIterator<K, V>
```

### `fn end(this &TMMap<K, V, Cmp, A>) MapIterator<K, V>`

One past the last entry.

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn end(this &TMMap<K, V, Cmp, A>) MapIterator<K, V>
```

### `type TMSet {`

----------------------------------------------------------------- TMSet
An ordered multi-set: any number of copies of a key, walked in key order.

```
export
template <K type, Cmp type = Less<K>, A type = DefaultAllocator>
type TMSet {
    // The tree that holds the keys, with a `Unit` where a map keeps a value.
    tree RBTree<K, Unit, Cmp, A>;
}
```

### `fn insert(this &TMSet<K, Cmp, A>, key K) void`

Adds a key, whether or not it is already there.

- `key`: the key to look for

```
export
template <K type, Cmp type = Less<K>, A type = DefaultAllocator>
fn insert(this &TMSet<K, Cmp, A>, key K) void
```

### `fn contains(this &const TMSet<K, Cmp, A>, key &const K) bool`

Whether the key is there.

- `key`: the key to look for

```
export
template <K type, Cmp type = Less<K>, A type = DefaultAllocator>
fn contains(this &const TMSet<K, Cmp, A>, key &const K) bool
```

### `fn count(this &const TMSet<K, Cmp, A>, key &const K) usize`

How many copies of the key are there.

- `key`: the key to look for

```
export
template <K type, Cmp type = Less<K>, A type = DefaultAllocator>
fn count(this &const TMSet<K, Cmp, A>, key &const K) usize
```

### `fn erase(this &TMSet<K, Cmp, A>, key &const K) bool`

Removes one copy of the key, and returns whether it was there.

- `key`: the key to look for

```
export
template <K type, Cmp type = Less<K>, A type = DefaultAllocator>
fn erase(this &TMSet<K, Cmp, A>, key &const K) bool
```

### `fn erase_all(this &TMSet<K, Cmp, A>, key &const K) usize`

Removes every copy of the key, and returns how many there were.

- `key`: the key to look for

```
export
template <K type, Cmp type = Less<K>, A type = DefaultAllocator>
fn erase_all(this &TMSet<K, Cmp, A>, key &const K) usize
```

### `fn clear(this &TMSet<K, Cmp, A>) void`

Removes every key.

```
export
template <K type, Cmp type = Less<K>, A type = DefaultAllocator>
fn clear(this &TMSet<K, Cmp, A>) void
```

### `fn length(this &const TMSet<K, Cmp, A>) usize`

How many keys the set holds.

```
export
template <K type, Cmp type = Less<K>, A type = DefaultAllocator>
fn length(this &const TMSet<K, Cmp, A>) usize
```

### `fn empty(this &const TMSet<K, Cmp, A>) bool`

Whether the set holds nothing.

```
export
template <K type, Cmp type = Less<K>, A type = DefaultAllocator>
fn empty(this &const TMSet<K, Cmp, A>) bool
```

### `fn begin(this &TMSet<K, Cmp, A>) SetIterator<K>`

The first key.

```
export
template <K type, Cmp type = Less<K>, A type = DefaultAllocator>
fn begin(this &TMSet<K, Cmp, A>) SetIterator<K>
```

### `fn end(this &TMSet<K, Cmp, A>) SetIterator<K>`

One past the last key.

```
export
template <K type, Cmp type = Less<K>, A type = DefaultAllocator>
fn end(this &TMSet<K, Cmp, A>) SetIterator<K>
```

### `fn __init__(this &TMap<K, V, Cmp, A>) void`

------------------------------------------------------- container construction
Creates an empty map.

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn __init__(this &TMap<K, V, Cmp, A>) void
```

### `fn __init__(this &TMap<K, V, Cmp, A>, other &const TMap<K, V, Cmp, A>) void`

- Copies another map through its tree.

- The tree copies every node, so the two maps share nothing afterwards.

- `other`: the value to copy

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn __init__(this &TMap<K, V, Cmp, A>, other &const TMap<K, V, Cmp, A>) void
```

### `fn __init__(this &TMap<K, V, Cmp, A>, other &&TMap<K, V, Cmp, A>) void`

- Takes another map's tree.

- The tree is taken rather than copied, so `other` is left with an empty
  one.

- `other`: the value to copy

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn __init__(this &TMap<K, V, Cmp, A>, other &&TMap<K, V, Cmp, A>) void
```

### `fn __assign__(this &TMap<K, V, Cmp, A>, other &const TMap<K, V, Cmp, A>) void`

- Copies another map over this one through its tree.

- This map's own entries are destroyed first, so afterwards the two share
  nothing.

- `other`: the value to copy

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn __assign__(this &TMap<K, V, Cmp, A>, other &const TMap<K, V, Cmp, A>) void
```

### `fn __assign__(this &TMap<K, V, Cmp, A>, other &&TMap<K, V, Cmp, A>) void`

- Takes another map's tree over this one.

- This map's own entries are destroyed first, then the tree is taken rather
  than copied, so `other` is left with an empty one.

- `other`: the value to copy

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn __assign__(this &TMap<K, V, Cmp, A>, other &&TMap<K, V, Cmp, A>) void
```

### `fn __init__(this &TMap<K, V, Cmp, A>, order Cmp, memory A) void`

- Creates an empty map with an order and an allocator of its caller's.

- The order decides where every key that is inserted later goes, and the
  allocator is where the tree's nodes come from; both are copied.

- `order`: the comparison that decides where a key goes
- `memory`: the allocator the nodes are drawn from

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn __init__(this &TMap<K, V, Cmp, A>, order Cmp, memory A) void
```

### `fn __init__(this &TSet<K, Cmp, A>) void`

Creates an empty set.

```
export
template <K type, Cmp type = Less<K>, A type = DefaultAllocator>
fn __init__(this &TSet<K, Cmp, A>) void
```

### `fn __init__(this &TSet<K, Cmp, A>, other &const TSet<K, Cmp, A>) void`

- Copies another set through its tree.

- The tree copies every node, so the two sets share nothing afterwards.

- `other`: the value to copy

```
export
template <K type, Cmp type = Less<K>, A type = DefaultAllocator>
fn __init__(this &TSet<K, Cmp, A>, other &const TSet<K, Cmp, A>) void
```

### `fn __init__(this &TSet<K, Cmp, A>, other &&TSet<K, Cmp, A>) void`

- Takes another set's tree.

- The tree is taken rather than copied, so `other` is left with an empty
  one.

- `other`: the value to copy

```
export
template <K type, Cmp type = Less<K>, A type = DefaultAllocator>
fn __init__(this &TSet<K, Cmp, A>, other &&TSet<K, Cmp, A>) void
```

### `fn __assign__(this &TSet<K, Cmp, A>, other &const TSet<K, Cmp, A>) void`

- Copies another set over this one through its tree.

- This set's own entries are destroyed first, so afterwards the two share
  nothing.

- `other`: the value to copy

```
export
template <K type, Cmp type = Less<K>, A type = DefaultAllocator>
fn __assign__(this &TSet<K, Cmp, A>, other &const TSet<K, Cmp, A>) void
```

### `fn __assign__(this &TSet<K, Cmp, A>, other &&TSet<K, Cmp, A>) void`

- Takes another set's tree over this one.

- This set's own entries are destroyed first, then the tree is taken rather
  than copied, so `other` is left with an empty one.

- `other`: the value to copy

```
export
template <K type, Cmp type = Less<K>, A type = DefaultAllocator>
fn __assign__(this &TSet<K, Cmp, A>, other &&TSet<K, Cmp, A>) void
```

### `fn __init__(this &TSet<K, Cmp, A>, order Cmp, memory A) void`

- Creates an empty set with an order and an allocator of its caller's.

- The order decides where every key that is inserted later goes, and the
  allocator is where the tree's nodes come from; both are copied.

- `order`: the comparison that decides where a key goes
- `memory`: the allocator the nodes are drawn from

```
export
template <K type, Cmp type = Less<K>, A type = DefaultAllocator>
fn __init__(this &TSet<K, Cmp, A>, order Cmp, memory A) void
```

### `fn __init__(this &TMMap<K, V, Cmp, A>) void`

Creates an empty multi-map.

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn __init__(this &TMMap<K, V, Cmp, A>) void
```

### `fn __init__(this &TMMap<K, V, Cmp, A>, other &const TMMap<K, V, Cmp, A>) void`

- Copies another multi-map through its tree.

- The tree copies every node, so the two multi-maps share nothing
  afterwards.

- `other`: the value to copy

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn __init__(this &TMMap<K, V, Cmp, A>, other &const TMMap<K, V, Cmp, A>) void
```

### `fn __init__(this &TMMap<K, V, Cmp, A>, other &&TMMap<K, V, Cmp, A>) void`

- Takes another multi-map's tree.

- The tree is taken rather than copied, so `other` is left with an empty
  one.

- `other`: the value to copy

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn __init__(this &TMMap<K, V, Cmp, A>, other &&TMMap<K, V, Cmp, A>) void
```

### `fn __assign__(this &TMMap<K, V, Cmp, A>, other &const TMMap<K, V, Cmp, A>) void`

- Copies another multi-map over this one through its tree.

- This multi-map's own entries are destroyed first, so afterwards the two
  share nothing.

- `other`: the value to copy

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn __assign__(this &TMMap<K, V, Cmp, A>, other &const TMMap<K, V, Cmp, A>) void
```

### `fn __assign__(this &TMMap<K, V, Cmp, A>, other &&TMMap<K, V, Cmp, A>) void`

- Takes another multi-map's tree over this one.

- This multi-map's own entries are destroyed first, then the tree is taken
  rather than copied, so `other` is left with an empty one.

- `other`: the value to copy

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn __assign__(this &TMMap<K, V, Cmp, A>, other &&TMMap<K, V, Cmp, A>) void
```

### `fn __init__(this &TMMap<K, V, Cmp, A>, order Cmp, memory A) void`

- Creates an empty multi-map with an order and an allocator of its caller's.

- The order decides where every key that is inserted later goes, and the
  allocator is where the tree's nodes come from; both are copied.

- `order`: the comparison that decides where a key goes
- `memory`: the allocator the nodes are drawn from

```
export
template <K type, V type, Cmp type = Less<K>, A type = DefaultAllocator>
fn __init__(this &TMMap<K, V, Cmp, A>, order Cmp, memory A) void
```

### `fn __init__(this &TMSet<K, Cmp, A>) void`

Creates an empty multi-set.

```
export
template <K type, Cmp type = Less<K>, A type = DefaultAllocator>
fn __init__(this &TMSet<K, Cmp, A>) void
```

### `fn __init__(this &TMSet<K, Cmp, A>, other &const TMSet<K, Cmp, A>) void`

- Copies another multi-set through its tree.

- The tree copies every node, so the two multi-sets share nothing
  afterwards.

- `other`: the value to copy

```
export
template <K type, Cmp type = Less<K>, A type = DefaultAllocator>
fn __init__(this &TMSet<K, Cmp, A>, other &const TMSet<K, Cmp, A>) void
```

### `fn __init__(this &TMSet<K, Cmp, A>, other &&TMSet<K, Cmp, A>) void`

- Takes another multi-set's tree.

- The tree is taken rather than copied, so `other` is left with an empty
  one.

- `other`: the value to copy

```
export
template <K type, Cmp type = Less<K>, A type = DefaultAllocator>
fn __init__(this &TMSet<K, Cmp, A>, other &&TMSet<K, Cmp, A>) void
```

### `fn __assign__(this &TMSet<K, Cmp, A>, other &const TMSet<K, Cmp, A>) void`

- Copies another multi-set over this one through its tree.

- This multi-set's own entries are destroyed first, so afterwards the two
  share nothing.

- `other`: the value to copy

```
export
template <K type, Cmp type = Less<K>, A type = DefaultAllocator>
fn __assign__(this &TMSet<K, Cmp, A>, other &const TMSet<K, Cmp, A>) void
```

### `fn __assign__(this &TMSet<K, Cmp, A>, other &&TMSet<K, Cmp, A>) void`

- Takes another multi-set's tree over this one.

- This multi-set's own entries are destroyed first, then the tree is taken
  rather than copied, so `other` is left with an empty one.

- `other`: the value to copy

```
export
template <K type, Cmp type = Less<K>, A type = DefaultAllocator>
fn __assign__(this &TMSet<K, Cmp, A>, other &&TMSet<K, Cmp, A>) void
```

### `fn __init__(this &TMSet<K, Cmp, A>, order Cmp, memory A) void`

- Creates an empty multi-set with an order and an allocator of its caller's.

- The order decides where every key that is inserted later goes, and the
  allocator is where the tree's nodes come from; both are copied.

- `order`: the comparison that decides where a key goes
- `memory`: the allocator the nodes are drawn from

```
export
template <K type, Cmp type = Less<K>, A type = DefaultAllocator>
fn __init__(this &TMSet<K, Cmp, A>, order Cmp, memory A) void
```

### `fn cbegin(this &const TMap<K, V, Cmp, A>) ConstMapIterator<K, V>`

The first position, read only, named as C++ names it.

```
export
template <K type, V type, Cmp type, A type>
fn cbegin(this &const TMap<K, V, Cmp, A>) ConstMapIterator<K, V>
```

### `fn cend(this &const TMap<K, V, Cmp, A>) ConstMapIterator<K, V>`

The one-past-the-end position, read only, named as C++ names it.

```
export
template <K type, V type, Cmp type, A type>
fn cend(this &const TMap<K, V, Cmp, A>) ConstMapIterator<K, V>
```

### `fn cbegin(this &const TMMap<K, V, Cmp, A>) ConstMapIterator<K, V>`

The first position, read only, named as C++ names it.

```
export
template <K type, V type, Cmp type, A type>
fn cbegin(this &const TMMap<K, V, Cmp, A>) ConstMapIterator<K, V>
```

### `fn cend(this &const TMMap<K, V, Cmp, A>) ConstMapIterator<K, V>`

The one-past-the-end position, read only, named as C++ names it.

```
export
template <K type, V type, Cmp type, A type>
fn cend(this &const TMMap<K, V, Cmp, A>) ConstMapIterator<K, V>
```

### `fn cbegin(this &const TSet<K, Cmp, A>) ConstSetIterator<K>`

The first position, read only, named as C++ names it.

```
export
template <K type, Cmp type, A type>
fn cbegin(this &const TSet<K, Cmp, A>) ConstSetIterator<K>
```

### `fn cend(this &const TSet<K, Cmp, A>) ConstSetIterator<K>`

The one-past-the-end position, read only, named as C++ names it.

```
export
template <K type, Cmp type, A type>
fn cend(this &const TSet<K, Cmp, A>) ConstSetIterator<K>
```

### `fn cbegin(this &const TMSet<K, Cmp, A>) ConstSetIterator<K>`

The first position, read only, named as C++ names it.

```
export
template <K type, Cmp type, A type>
fn cbegin(this &const TMSet<K, Cmp, A>) ConstSetIterator<K>
```

### `fn cend(this &const TMSet<K, Cmp, A>) ConstSetIterator<K>`

The one-past-the-end position, read only, named as C++ names it.

```
export
template <K type, Cmp type, A type>
fn cend(this &const TMSet<K, Cmp, A>) ConstSetIterator<K>
```
