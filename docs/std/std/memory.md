# `std.memory`

Memory: ownership, allocation, and reference counting.

`Box` owns one value outright, `Rc` shares one between owners and frees it when
the last one goes, and `RcWeak` watches a shared value without owning it. Where
the memory comes from is a *type*: a container that names an allocator pays
nothing for the choice, because the call is ordinary code and an allocator with
no fields occupies no space.

`Rc` is for one thread. `Arc` is the same idea with counted atomically, which is
what makes it shareable between threads -- its counts are `Atomic<int>`, so the
cost is a lock per count and the promise is the same one every reference count
makes: the value goes when the last owner does.
The *module* is imported as well as the type: a family's members are
instantiated from its declaration, and the declaration is what an import of
the module brings.

## Declarations

### `export traits Allocator(it A) {`

--------------------------------------------------------------- allocators
What a type has to provide to be usable as an allocator.

Two methods are the whole contract: `alloc(n)` gives `n` bytes, aligned for any
type, and `free(p)` gives back a block that `alloc` returned. A trait rather
than an interface because the choice is made while compiling: a container takes
the allocator's *type*, and no table and no call through a pointer stand
between the container and the memory.

```
export traits Allocator(it A) {
    it.alloc(usize) -> *byte,
    it.free(*byte) -> void,
}
```

### `export type DefaultAllocator {`

The allocator every container uses unless it says otherwise.

It is the C heap, which is what `util.allocate` and `util.free` are.

```
export type DefaultAllocator {
}
```

### `export fn alloc(this &DefaultAllocator, size usize) *byte {`

Takes `size` bytes from the C heap.

The bytes are uninitialised: constructing a value in them is the caller's job,
exactly as `util.allocate` says.

```
export fn alloc(this &DefaultAllocator, size usize) *byte {
    return util.allocate(size);
}
```

### `export fn free(this &DefaultAllocator, block *byte) void {`

Gives a block back to the C heap.

```
export fn free(this &DefaultAllocator, block *byte) void {
    util.free(block);
}
```

### `type Box {`

---------------------------------------------------------------------- Box
An owned value, on the heap.

Moving a box moves the pointer, so it is cheap; copying one is not allowed,
because two boxes owning one value would free it twice. `get()` is how the value
is reached: the mutable form lends it out for writing, the const form for
reading, and the pointer stays valid until the box is destroyed or assigned a
new value.

```
export
template <T type>
type Box {
    // The owned value, or null in a moved-from box.
    _value *T;
}
```

### `fn __init__(this &Box<T>) void`

Creates an empty box, which owns nothing.

No allocation: an empty box is the moved-from state, so making one is the same
as making a box that has been moved from, and it is destroyed the same way.

```
export
template <T type>
fn __init__(this &Box<T>) void
```

### `fn __im_init__(this &Box<T>, value T) void`

Creates a box holding a copy of `value`.

- `value`: the value to copy into a new allocation; it is copied rather than
  taken, so the caller still owns what it passed

```
export
template <T type>
fn __im_init__(this &Box<T>, value T) void
```

### `fn __init__(this &Box<T>, other &const Box<T>) void`

Copies another box's value into a new allocation.

The two boxes own separate values afterwards, so writing through one does not
change the other. A box that already holds something releases it first.

- `other`: the box to copy; it keeps its own value

```
export
template <T type>
fn __init__(this &Box<T>, other &const Box<T>) void
```

### `fn __init__(this &Box<T>, other &&Box<T>) void`

Takes another box's value.

The pointer moves; the source is left null, so its destructor frees nothing.

- `other`: the box to take from; it is left empty and owns nothing afterwards

```
export
template <T type>
fn __init__(this &Box<T>, other &&Box<T>) void
```

### `fn __drop__(this &Box<T>) void`

Releases the value this box owns.

```
export
template <T type>
fn __drop__(this &Box<T>) void
```

### `fn clone(this &const Box<T>) Box<T>`

Returns a second box holding a copy of the value.

A `Box` is moved, not copied: `c Box<int> = b;` moves out of `b`. `clone` is
the copy spelled out, and it copies the value -- the two boxes own different
blocks.

```
export
template <T type>
fn clone(this &const Box<T>) Box<T>
```

### `fn get(this &Box<T>) &T`

Returns the value, for writing.

An empty box owns no value, so asking for one aborts with a message rather
than reading through a null pointer.

```
export
template <T type>
fn get(this &Box<T>) &T
```

### `fn get(this &const Box<T>) &const T`

Returns the value, for reading.

```
export
template <T type>
fn get(this &const Box<T>) &const T
```

### `fn empty(this &const Box<T>) bool`

Returns `true` when the box owns nothing.

```
export
template <T type>
fn empty(this &const Box<T>) bool
```

### `type RcControl {`

----------------------------------------------------------------------- Rc
The block a shared value lives in.

One allocation holds the value, both counts and the allocator that made it. The
strong count owns the value: when the last strong reference goes, the value is
destroyed. The weak count owns the block: it keeps the counts readable after the
value is gone, and the block is freed when the last weak reference goes too.

```
export
template <T type, A type>
type RcControl {
    // How many `Rc` values share the value.
    strong usize;
    // How many `Rc` and `RcWeak` values reach the block.
    weak usize;
    // The allocator the block was taken from, which is what gives it back. It is
    // a copy: a stateful allocator that counts its blocks keeps counting.
    mem A;
    // The shared value.
    value T;
}
```

### `type Rc {`

A shared owner of one value.

Copying an `Rc` copies the *pointer* and increments a count, so it is cheap;
the value is freed when the last owner goes. The count is a plain `usize`: an
`Rc` is for one thread, which is what makes it the cheap one. `nullptr` in
`_control` is the empty state -- a moved-from `Rc`, a failed `upgrade`, or one
created empty -- and it owns nothing.

```
export
template <T type, A type = DefaultAllocator>
type Rc {
    // The block the value lives in, or null when this owner owns nothing.
    _control *RcControl<T, A>;
}
```

### `type RcWeak {`

An observer of a shared value, which does not keep it alive.

`upgrade()` answers whether the value is still there; the block itself lives as
long as any weak observer does, so the counts stay readable after the last owner
is gone.

```
export
template <T type, A type = DefaultAllocator>
type RcWeak {
    // The block being observed, or null when this observer observes nothing.
    _control *RcControl<T, A>;
}
```

### `fn __init__(this &Rc<T, A>) void`

Creates an owner of nothing.

```
export
template <T type, A type = DefaultAllocator>
fn __init__(this &Rc<T, A>) void
```

### `fn __im_init__(this &Rc<T, A>, value T) void`

Creates an owner of `value`, with a strong count of one.

The value is constructed in the block, so the parameter is the last owner of
what it was given: it is moved, not copied. A `T` that is expensive to copy
therefore costs one move per `Rc`.

- `value`: the value to own; its storage moves into the new block

```
export
template <T type, A type = DefaultAllocator>
fn __im_init__(this &Rc<T, A>, value T) void
```

### `fn __im_init__(this &Rc<T, A>, value T, memory A) void`

Creates an owner of `value`, taking the block from `memory`.

This is how a stateful allocator is used: the allocator travels with the block,
so the same one gives it back however many owners the value has.

- `value`: the value to own; its storage moves into the new block
- `memory`: the allocator the block comes from; a copy is kept in the block
  and gives it back, so the caller need not outlive the value

```
export
template <T type, A type = DefaultAllocator>
fn __im_init__(this &Rc<T, A>, value T, memory A) void
```

### `fn __init__(this &Rc<T, A>, value &&T) void`

Creates an owner of a value that is dying, moving it into the block.

A value that is about to disappear is moved rather than copied, which is what
makes `Rc<Thing>(Thing(7))` cost one construction instead of two. The overload
comes after the by-value one for the reason a container's move form does: a
named value is not dying, and only a dying one may be taken apart.

- `value`: a dying value whose storage moves into the new block; it cannot be
  read afterwards

```
export
template <T type, A type = DefaultAllocator>
fn __init__(this &Rc<T, A>, value &&T) void
```

### `fn __init__(this &Rc<T, A>, value &&T, memory A) void`

Creates an owner of a dying value, taking the block from `memory`.

- `value`: a dying value whose storage moves into the new block; the block is
  built in place from it rather than copied
- `memory`: the allocator the block comes from; a copy is kept in the block
  and gives it back

```
export
template <T type, A type = DefaultAllocator>
fn __init__(this &Rc<T, A>, value &&T, memory A) void
```

### `fn __init__(this &Rc<T, A>, other &const Rc<T, A>) void`

Shares another owner's value.

The value is not copied; the count says how many owners there are.

- `other`: the owner to share with; it is copied as a pointer, and an empty
  one gives an empty owner with no counts to raise

```
export
template <T type, A type = DefaultAllocator>
fn __init__(this &Rc<T, A>, other &const Rc<T, A>) void
```

### `fn __init__(this &Rc<T, A>, other &&Rc<T, A>) void`

Takes another owner's share.

The source is left empty, so it releases nothing.

- `other`: the owner to take from; its pointer moves and it is left empty

```
export
template <T type, A type = DefaultAllocator>
fn __init__(this &Rc<T, A>, other &&Rc<T, A>) void
```

### `fn __assign__(this &Rc<T, A>, other &const Rc<T, A>) void`

Shares another owner's value, releasing what this owner held.

- `other`: the owner to share with; this owner first drops its own share, so
  assigning an owner to itself still ends with one share held

```
export
template <T type, A type = DefaultAllocator>
fn __assign__(this &Rc<T, A>, other &const Rc<T, A>) void
```

### `fn __assign__(this &Rc<T, A>, other &&Rc<T, A>) void`

Takes another owner's share, releasing what this owner held.

- `other`: the owner to take from, left empty; when both owners already share
  the same block nothing happens and neither share is released

```
export
template <T type, A type = DefaultAllocator>
fn __assign__(this &Rc<T, A>, other &&Rc<T, A>) void
```

### `fn __drop__(this &Rc<T, A>) void`

Releases this owner's share, destroying the value with the last one.

```
export
template <T type, A type = DefaultAllocator>
fn __drop__(this &Rc<T, A>) void
```

### `fn clone(this &const Rc<T, A>) Rc<T, A>`

Returns another owner of the same value.

This is the copy that a program writes on purpose, as against the one an
assignment makes: `b Rc<int> = a.clone();`.

```
export
template <T type, A type = DefaultAllocator>
fn clone(this &const Rc<T, A>) Rc<T, A>
```

### `fn get(this &const Rc<T, A>) &const T`

Returns the value, for reading.

```
export
template <T type, A type = DefaultAllocator>
fn get(this &const Rc<T, A>) &const T
```

### `fn get(this &Rc<T, A>) &T`

Returns the value, for writing.

The other owners see the write. That is the point of sharing, and it is why the
mutable form is on a mutable owner: holding `&const Rc<T>` is a promise not to
write the value through it.

```
export
template <T type, A type = DefaultAllocator>
fn get(this &Rc<T, A>) &T
```

### `fn __deref__(this &const Rc<T, A>) &const T`

Reads through the shared value: `*r` is the value.

```
export
template <T type, A type = DefaultAllocator>
fn __deref__(this &const Rc<T, A>) &const T
```

### `fn __deref__(this &Rc<T, A>) &T`

Writes through the shared value.

```
export
template <T type, A type = DefaultAllocator>
fn __deref__(this &Rc<T, A>) &T
```

### `fn empty(this &const Rc<T, A>) bool`

Returns `true` when this owner owns nothing.

```
export
template <T type, A type = DefaultAllocator>
fn empty(this &const Rc<T, A>) bool
```

### `fn strong_count(this &const Rc<T, A>) usize`

Returns how many strong owners the value has.

```
export
template <T type, A type = DefaultAllocator>
fn strong_count(this &const Rc<T, A>) usize
```

### `fn weak_count(this &const Rc<T, A>) usize`

Returns how many weak observers the value has.

```
export
template <T type, A type = DefaultAllocator>
fn weak_count(this &const Rc<T, A>) usize
```

### `fn downgrade(this &const Rc<T, A>) RcWeak<T, A>`

Returns a weak observer of this owner's value.

```
export
template <T type, A type = DefaultAllocator>
fn downgrade(this &const Rc<T, A>) RcWeak<T, A>
```

### `fn __init__(this &RcWeak<T, A>) void`

-------------------------------------------------------------------- RcWeak
Creates an observer of nothing.

```
export
template <T type, A type = DefaultAllocator>
fn __init__(this &RcWeak<T, A>) void
```

### `fn __init__(this &RcWeak<T, A>, other &const RcWeak<T, A>) void`

Observes the same value another observer does.

- `other`: the observer to share with; it is copied as a pointer and the
  block's weak count goes up, and an empty one gives an observer of nothing

```
export
template <T type, A type = DefaultAllocator>
fn __init__(this &RcWeak<T, A>, other &const RcWeak<T, A>) void
```

### `fn __init__(this &RcWeak<T, A>, other &&RcWeak<T, A>) void`

Takes another observer's share.

- `other`: the observer to take from; its pointer moves and it is left
  observing nothing, so its destructor changes no count

```
export
template <T type, A type = DefaultAllocator>
fn __init__(this &RcWeak<T, A>, other &&RcWeak<T, A>) void
```

### `fn __assign__(this &RcWeak<T, A>, other &const RcWeak<T, A>) void`

Observes another observer's value, releasing what this one observed.

- `other`: the observer to share with; this one drops its own observation
  first, so observing itself still ends with one observation held

```
export
template <T type, A type = DefaultAllocator>
fn __assign__(this &RcWeak<T, A>, other &const RcWeak<T, A>) void
```

### `fn __assign__(this &RcWeak<T, A>, other &&RcWeak<T, A>) void`

Takes another observer's share, releasing what this one observed.

- `other`: the observer to take from, left observing nothing; when both
  already observe the same block nothing happens

```
export
template <T type, A type = DefaultAllocator>
fn __assign__(this &RcWeak<T, A>, other &&RcWeak<T, A>) void
```

### `fn __drop__(this &RcWeak<T, A>) void`

Stops observing.

The block is freed when the last strong owner has already gone and this was the
last observer.

```
export
template <T type, A type = DefaultAllocator>
fn __drop__(this &RcWeak<T, A>) void
```

### `fn upgrade(this &const RcWeak<T, A>) Rc<T, A>`

Returns an owner of the value, or an empty `Rc` when it is gone.

```
export
template <T type, A type = DefaultAllocator>
fn upgrade(this &const RcWeak<T, A>) Rc<T, A>
```

### `fn expired(this &const RcWeak<T, A>) bool`

Returns `true` when the value has already been destroyed.

```
export
template <T type, A type = DefaultAllocator>
fn expired(this &const RcWeak<T, A>) bool
```

### `fn strong_count(this &const RcWeak<T, A>) usize`

Returns how many strong owners the observed value has.

```
export
template <T type, A type = DefaultAllocator>
fn strong_count(this &const RcWeak<T, A>) usize
```

### `fn weak_count(this &const RcWeak<T, A>) usize`

Returns how many weak observers the value has.

```
export
template <T type, A type = DefaultAllocator>
fn weak_count(this &const RcWeak<T, A>) usize
```

### `type ArcControl {`

---------------------------------------------------------------- Arc/ArcWeak
The block an `Arc` shares: the value, the two counts, and the allocator.

The counts are atomic, which is the one difference from an `Rc`'s block: two
threads incrementing at once must not lose each other's increment, and that is
exactly what `Atomic` promises.

```
export
template <T type, A type>
type ArcControl {
    // How many `Arc` values share the value.
    strong Atomic<int>;
    // How many `Arc` and `ArcWeak` values reach the block.
    weak Atomic<int>;
    // The allocator the block was taken from.
    mem A;
    // The shared value.
    value T;
}
```

### `type Arc {`

A shared owner of one value, shareable between threads.

```
export
template <T type, A type = DefaultAllocator>
type Arc {
    // The block the value lives in, or null when this owner owns nothing.
    _control *ArcControl<T, A>;
}
```

### `type ArcWeak {`

An observer of an `Arc`, which does not keep the value alive.

```
export
template <T type, A type = DefaultAllocator>
type ArcWeak {
    // The block being observed, or null when this observer observes nothing.
    _control *ArcControl<T, A>;
}
```

### `fn __init__(this &Arc<T, A>) void`

Creates an owner of nothing.

```
export
template <T type, A type = DefaultAllocator>
fn __init__(this &Arc<T, A>) void
```

### `fn __im_init__(this &Arc<T, A>, value T) void`

Creates an owner of `value`, with a strong count of one.

- `value`: the value to own; its storage moves into the block, so it is not
  copied even once

```
export
template <T type, A type = DefaultAllocator>
fn __im_init__(this &Arc<T, A>, value T) void
```

### `fn __im_init__(this &Arc<T, A>, value T, memory A) void`

Creates an owner of `value`, taking the block from `memory`.

- `value`: the value to own; its storage moves into the block
- `memory`: the allocator the block comes from; a copy is kept in the block
  and gives it back

```
export
template <T type, A type = DefaultAllocator>
fn __im_init__(this &Arc<T, A>, value T, memory A) void
```

### `fn __init__(this &Arc<T, A>, other &const Arc<T, A>) void`

Shares another owner's value.

- `other`: the owner to share with; the counts are raised atomically, so an
  owner that was handed to another thread may be shared from

```
export
template <T type, A type = DefaultAllocator>
fn __init__(this &Arc<T, A>, other &const Arc<T, A>) void
```

### `fn __init__(this &Arc<T, A>, other &&Arc<T, A>) void`

Takes another owner's share.

- `other`: the owner to take from; its pointer moves and it is left empty.
  Unlike sharing, nothing is counted, so neither thread may touch `other`

```
export
template <T type, A type = DefaultAllocator>
fn __init__(this &Arc<T, A>, other &&Arc<T, A>) void
```

### `fn __assign__(this &Arc<T, A>, other &const Arc<T, A>) void`

Shares another owner's value, releasing what this owner held.

- `other`: the owner to share with; this owner first drops its own share, so
  assigning an owner to itself still ends with one share held

```
export
template <T type, A type = DefaultAllocator>
fn __assign__(this &Arc<T, A>, other &const Arc<T, A>) void
```

### `fn __assign__(this &Arc<T, A>, other &&Arc<T, A>) void`

Takes another owner's share, releasing what this owner held.

- `other`: the owner to take from, left empty; when both owners already share
  the same block nothing happens and neither share is released

```
export
template <T type, A type = DefaultAllocator>
fn __assign__(this &Arc<T, A>, other &&Arc<T, A>) void
```

### `fn __drop__(this &Arc<T, A>) void`

Releases this owner's share, destroying the value with the last one.

```
export
template <T type, A type = DefaultAllocator>
fn __drop__(this &Arc<T, A>) void
```

### `fn clone(this &const Arc<T, A>) Arc<T, A>`

Returns another owner of the same value.

```
export
template <T type, A type = DefaultAllocator>
fn clone(this &const Arc<T, A>) Arc<T, A>
```

### `fn get(this &const Arc<T, A>) &const T`

Returns the value, for reading.

```
export
template <T type, A type = DefaultAllocator>
fn get(this &const Arc<T, A>) &const T
```

### `fn get(this &Arc<T, A>) &T`

Returns the value, for writing.

```
export
template <T type, A type = DefaultAllocator>
fn get(this &Arc<T, A>) &T
```

### `fn __deref__(this &const Arc<T, A>) &const T`

Reads through the shared value: `*a` is the value.

```
export
template <T type, A type = DefaultAllocator>
fn __deref__(this &const Arc<T, A>) &const T
```

### `fn __deref__(this &Arc<T, A>) &T`

```
export
template <T type, A type = DefaultAllocator>
fn __deref__(this &Arc<T, A>) &T
```

### `fn empty(this &const Arc<T, A>) bool`

Returns `true` when this owner owns nothing.

```
export
template <T type, A type = DefaultAllocator>
fn empty(this &const Arc<T, A>) bool
```

### `fn strong_count(this &const Arc<T, A>) usize`

Returns how many strong owners the value has.

```
export
template <T type, A type = DefaultAllocator>
fn strong_count(this &const Arc<T, A>) usize
```

### `fn weak_count(this &const Arc<T, A>) usize`

Returns how many weak observers the value has.

```
export
template <T type, A type = DefaultAllocator>
fn weak_count(this &const Arc<T, A>) usize
```

### `fn downgrade(this &const Arc<T, A>) ArcWeak<T, A>`

Returns a weak observer of this owner's value.

```
export
template <T type, A type = DefaultAllocator>
fn downgrade(this &const Arc<T, A>) ArcWeak<T, A>
```

### `fn __init__(this &ArcWeak<T, A>) void`

Creates an observer of nothing.

```
export
template <T type, A type = DefaultAllocator>
fn __init__(this &ArcWeak<T, A>) void
```

### `fn __init__(this &ArcWeak<T, A>, other &const ArcWeak<T, A>) void`

Observes the same value another observer does.

- `other`: the observer to share with; the weak count is raised atomically,
  and an empty one gives an observer of nothing

```
export
template <T type, A type = DefaultAllocator>
fn __init__(this &ArcWeak<T, A>, other &const ArcWeak<T, A>) void
```

### `fn __init__(this &ArcWeak<T, A>, other &&ArcWeak<T, A>) void`

Takes another observer's share.

- `other`: the observer to take from; its pointer moves and it is left
  observing nothing, so its destructor changes no count

```
export
template <T type, A type = DefaultAllocator>
fn __init__(this &ArcWeak<T, A>, other &&ArcWeak<T, A>) void
```

### `fn __init__(this &ArcWeak<T, A>, other &const Arc<T, A>) void`

Observes another owner's value.

- `other`: the owner whose value is observed; its weak count goes up so the
  block outlives the owners, and an empty one gives an observer of nothing

```
export
template <T type, A type = DefaultAllocator>
fn __init__(this &ArcWeak<T, A>, other &const Arc<T, A>) void
```

### `fn __drop__(this &ArcWeak<T, A>) void`

Stops observing.

```
export
template <T type, A type = DefaultAllocator>
fn __drop__(this &ArcWeak<T, A>) void
```

### `fn upgrade(this &const ArcWeak<T, A>) Arc<T, A>`

Returns an owner of the value, or an empty `Arc` when it is gone.

```
export
template <T type, A type = DefaultAllocator>
fn upgrade(this &const ArcWeak<T, A>) Arc<T, A>
```

### `fn expired(this &const ArcWeak<T, A>) bool`

Returns `true` when the value has already been destroyed.

```
export
template <T type, A type = DefaultAllocator>
fn expired(this &const ArcWeak<T, A>) bool
```

### `fn strong_count(this &const ArcWeak<T, A>) usize`

Returns how many strong owners the observed value has.

```
export
template <T type, A type = DefaultAllocator>
fn strong_count(this &const ArcWeak<T, A>) usize
```

### `fn weak_count(this &const ArcWeak<T, A>) usize`

Returns how many weak observers the value has.

```
export
template <T type, A type = DefaultAllocator>
fn weak_count(this &const ArcWeak<T, A>) usize
```
