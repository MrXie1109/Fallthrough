# `std.allocators`

Allocators: where the bytes of a container come from.

An allocator is anything that can hand out a block of bytes and take it back:

    export traits Allocator(it A) {
        it.alloc(usize) -> *byte,
        it.free(*byte) -> void,
    }

Containers take one as a template parameter and default to `DefaultAllocator`,
the C heap. The allocators here are the two ends of the spectrum:

* `DefaultAllocator` is `malloc`/`free` -- every block is its own, and a
  block's lifetime is the caller's business.
* `Arena` is one block and a cursor -- allocation is a pointer bump, and
  nothing is returned until the whole arena is reset or destroyed. That is
  the right shape for a compiler pass or a document parser, where everything
  dies together.

# The block is uninitialised

`alloc` returns raw bytes, as `std.util.allocate` does: putting a value there
means constructing it with `__init__`, and taking it back means calling
`__drop__` first. An allocator knows about bytes, not about values.

## Declarations

### `export traits Allocator(it A) {`

What a type has to provide to be used as an allocator.

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

### `export fn __init__(this &DefaultAllocator) void {`

Creates the allocator. It has no state; this exists so that
`DefaultAllocator()` constructs one like any other type.

```
export fn __init__(this &DefaultAllocator) void {
}
```

### `export fn alloc(this &DefaultAllocator, size usize) *byte {`

Takes `size` bytes from the C heap.

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

### `export type Arena {`

A bump allocator: one block, a cursor, and no individual free.

`alloc` carves the next `size` bytes from the block and panics when the
block is exhausted -- an arena is sized for a job, and running out means the
estimate was wrong rather than that the caller should retry. `reset` makes
every block available again without touching the allocator's own block.

```
export type Arena {
    // The whole block, or null before the first allocation.
    _storage *byte;
    // Its size in bytes.
    _size usize;
    // How much of it is handed out.
    _used usize;
}
```

### `export fn __init__(this &Arena, size usize) void {`

Creates an arena of `size` bytes.

```
export fn __init__(this &Arena, size usize) void {
    this._storage = nullptr;
    this._size = 0;
    this._used = 0;
    if (size == 0) {
        return;
    }
    this._storage = util.allocate(size);
    this._size = size;
}
```

### `export fn __drop__(this &Arena) void {`

Releases the arena's block.

```
export fn __drop__(this &Arena) void {
    if (this._storage != nullptr) {
        util.free(this._storage);
        this._storage = nullptr;
    }
    this._size = 0;
    this._used = 0;
}
```

### `export fn alloc(this &Arena, size usize) *byte {`

Takes `size` bytes from the arena, panicking when it is exhausted.

```
export fn alloc(this &Arena, size usize) *byte {
    if (size > this._size - this._used) {
        util.panic("arena exhausted");
    }
    block *byte = this._storage + this._used;
    this._used += size;
    return block;
}
```

### `export fn free(this &Arena, block *byte) void {`

Nothing is returned: the arena owns its one block.

```
export fn free(this &Arena, block *byte) void {
    // An arena does not take blocks back one at a time. The method exists so
    // that the type models `Allocator`; `reset` is how memory is reused.
}
```

### `export fn reset(this &Arena) void {`

Makes every byte available again.

```
export fn reset(this &Arena) void {
    this._used = 0;
}
```

### `export fn available(this &const Arena) usize {`

How many bytes are still free.

```
export fn available(this &const Arena) usize {
    return this._size - this._used;
}
```
