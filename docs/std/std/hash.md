# `std.hash`

Hashing: a hasher type, the hash of the built-in types, and the four hash
containers.

# The hasher is a type

`Hash<K>` turns a key into a number, and it is a *type* for the same reason
the order of a tree is: which one applies is decided while compiling, and no
table stands between the container and the hasher. `Hash<K>` asks the key
itself -- `value.__hash__()` -- so a type that declares `__hash__` is
hashable with no further ceremony, and `__call__` is what the containers call.

# Collisions are handled by chaining

A bucket holds the head of a chain, so an entry never moves once it is in the
table, an erase is an unlink, and a key may appear any number of times -- which
is what the multi containers need. A table grows when it holds more entries
than buckets, and the chains are relinked into the larger array.

## Declarations

### `export traits Hashable(it T) {`

What a type has to provide to be used as a hash key.

Both methods are the language's own special methods, so a type declares them
the way it declares any other: `fn __hash__(this &const Thing) usize` and
`fn __eq__(this &const Thing, other &const Thing) bool`.

The two belong together: the hash decides which chain a key is looked for in,
and `__eq__` decides whether an entry in that chain is the key. A type with a
hash and no equality is not a key, which is why both are here.

```
export traits Hashable(it T) {
    it.__hash__() -> usize,
    it == it -> bool,
}
```

### `fn mix(number usize) usize`

Spreads the bits of an integer, so that keys which differ only in a high
digit do not all land in the same bucket.

- `number`: the value to hash

```
fn mix(number usize) usize
```

### `type Hash {`

The hasher for every type that says how to hash itself.

```
export
template <T type>
type Hash {
}
```

### `fn __call__(this &const Hash<T>, value &const T) usize`

Turns `value` into a number by asking the type.

- `value`: the value to hash

```
export
template <T type>
fn __call__(this &const Hash<T>, value &const T) usize
```

### `export fn __hash__(this &const i8) usize {`

------------------------------------------------------------------ builtins
The integers are hashed by their value, spread out.

Every width has its own spelling and so its own method: a `byte` is not a
`u8`, and `char` names `char`, which is why each one is written out.

```
export fn __hash__(this &const i8) usize {
    return mix(this as usize);
}
```

### `export fn __hash__(this &const i16) usize {`

```
export fn __hash__(this &const i16) usize {
    return mix(this as usize);
}
```

### `export fn __hash__(this &const int) usize {`

```
export fn __hash__(this &const int) usize {
    return mix(this as usize);
}
```

### `export fn __hash__(this &const i64) usize {`

```
export fn __hash__(this &const i64) usize {
    return mix(this as usize);
}
```

### `export fn __hash__(this &const i128) usize {`

```
export fn __hash__(this &const i128) usize {
    return mix((this as i64) as usize);
}
```

### `export fn __hash__(this &const u8) usize {`

```
export fn __hash__(this &const u8) usize {
    return mix(this as usize);
}
```

### `export fn __hash__(this &const u16) usize {`

```
export fn __hash__(this &const u16) usize {
    return mix(this as usize);
}
```

### `export fn __hash__(this &const uint) usize {`

```
export fn __hash__(this &const uint) usize {
    return mix(this as usize);
}
```

### `export fn __hash__(this &const u64) usize {`

```
export fn __hash__(this &const u64) usize {
    return mix(this as usize);
}
```

### `export fn __hash__(this &const u128) usize {`

```
export fn __hash__(this &const u128) usize {
    return mix((this as u64) as usize);
}
```

### `export fn __hash__(this &const byte) usize {`

```
export fn __hash__(this &const byte) usize {
    return mix(this as usize);
}
```

### `export fn __hash__(this &const bool) usize {`

```
export fn __hash__(this &const bool) usize {
    return mix(this as int as usize);
}
```

### `export fn __hash__(this &const char) usize {`

```
export fn __hash__(this &const char) usize {
    return mix(this as usize);
}
```

### `export fn __hash__(this &const c16) usize {`

```
export fn __hash__(this &const c16) usize {
    return mix(this as usize);
}
```

### `export fn __hash__(this &const c32) usize {`

```
export fn __hash__(this &const c32) usize {
    return mix(this as usize);
}
```

### `export fn __hash__(this &const String) usize {`

A string is hashed by its characters, not by the address of its buffer: two
strings with the same text are one key wherever they were built.

```
export fn __hash__(this &const String) usize {
    hash usize = 1469598103934665603usize;
    bytes *const char = this.data();
    for (i usize = 0; i < this.length(); i += 1) {
        hash = (hash ^ ((bytes[i] as usize))) * 1099511628211usize;
    }
    return hash;
}
```

### `type HashEntry {`

-------------------------------------------------------------------- table
One entry in a bucket's chain.

```
export
template <K type, V type>
type HashEntry {
    // The key, which is what the bucket was chosen by.
    key K;
    // The mapped value. A set stores a `Unit` here.
    value V;
    // The key's hash, kept so that a comparison starts with it.
    hash usize;
    // The next entry in the same bucket, or null.
    next *HashEntry<K, V>;
}
```

### `INITIAL_BUCKETS const usize = 8usize;`

How many buckets a table starts with.

```
INITIAL_BUCKETS const usize = 8usize;
```

### `type HashTable {`

A hash table: buckets, chains, and one entry per key.

This is the structure the four containers share; a program writes `HMap`,
`HSet`, `HMMap` or `HMSet`, which wrap it.

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
type HashTable {
    // The buckets, each the head of a chain: `*HashEntry<K, V>` per slot.
    //
    // An array of addresses rather than of entries, so that an entry keeps its
    // own address while the table grows, and so that a chain can be walked
    // without knowing a bucket's index.
    _buckets *byte;
    // How many buckets there are.
    _capacity usize;
    // How many entries there are.
    _count usize;
    // The allocator the entries and the buckets come from.
    mem A;
    // The hasher.
    hasher H;
}
```

### `fn __init__(this &HashTable<K, V, H, A>) void`

Creates an empty table.

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn __init__(this &HashTable<K, V, H, A>) void
```

### `fn __init__(this &HashTable<K, V, H, A>, hash H, memory A) void`

Creates an empty table with the caller's hasher and allocator.

- `hash`: the hash that turns a key into a bucket index
- `memory`: the allocator the buckets and entries are drawn from

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn __init__(this &HashTable<K, V, H, A>, hash H, memory A) void
```

### `fn heads(this &const HashTable<K, V, H, A>) **HashEntry<K, V>`

The buckets as an array of chain heads.

```
fn heads(this &const HashTable<K, V, H, A>) **HashEntry<K, V>
```

### `fn clear_buckets(this &HashTable<K, V, H, A>) void`

Empties every bucket, so that a freshly allocated array reads as no chains.

```
fn clear_buckets(this &HashTable<K, V, H, A>) void
```

### `fn bucket_of(this &const HashTable<K, V, H, A>, hash usize) usize`

The bucket a hash belongs in.

- `hash`: the hash function to use

```
fn bucket_of(this &const HashTable<K, V, H, A>, hash usize) usize
```

### `fn length(this &const HashTable<K, V, H, A>) usize`

Returns how many entries the table holds.

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn length(this &const HashTable<K, V, H, A>) usize
```

### `fn empty(this &const HashTable<K, V, H, A>) bool`

Returns `true` when the table holds nothing.

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn empty(this &const HashTable<K, V, H, A>) bool
```

### `fn grow(this &HashTable<K, V, H, A>) void`

Doubles the buckets and relinks every chain into the larger array.

The entries themselves do not move: only the heads are rewritten, which is
what makes growth safe to do while a value lives in the table.

```
fn grow(this &HashTable<K, V, H, A>) void
```

### `fn entry_of(this &HashTable<K, V, H, A>, key &const K) *HashEntry<K, V>`

Finds the entry holding `key`, or null.

- `key`: the key to look for

```
fn entry_of(this &HashTable<K, V, H, A>, key &const K) *HashEntry<K, V>
```

### `fn entry_of_const(this &const HashTable<K, V, H, A>, key &const K) *const HashEntry<K, V>`

Finds the entry holding `key`, or null, for a table read through a constant.

- `key`: the key to look for

```
fn entry_of_const(this &const HashTable<K, V, H, A>, key &const K) *const HashEntry<K, V>
```

### `fn insert(this &HashTable<K, V, H, A>, key K, value V) void`

Adds an entry, replacing the value when the key is already there.

- `key`: the key to place
- `value`: the value to store with it

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn insert(this &HashTable<K, V, H, A>, key K, value V) void
```

### `fn insert_new(this &HashTable<K, V, H, A>, key K, value V) *HashEntry<K, V>`

Adds an entry that is known to be new.

- `key`: the key to place
- `value`: the value to store with it

```
fn insert_new(this &HashTable<K, V, H, A>, key K, value V) *HashEntry<K, V>
```

### `fn insert_multi(this &HashTable<K, V, H, A>, key K, value V) *HashEntry<K, V>`

Adds an entry without looking for the key, so a key may appear many times.

- `key`: the key to place
- `value`: the value to store with it

```
fn insert_multi(this &HashTable<K, V, H, A>, key K, value V) *HashEntry<K, V>
```

### `fn find(this &HashTable<K, V, H, A>, key &const K) *V`

Returns the value stored for `key`, or null.

- `key`: the key to look for

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn find(this &HashTable<K, V, H, A>, key &const K) *V
```

### `fn find(this &const HashTable<K, V, H, A>, key &const K) *const V`

Returns the value stored for `key`, or null, for a constant table.

- `key`: the key to look for

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn find(this &const HashTable<K, V, H, A>, key &const K) *const V
```

### `fn contains(this &const HashTable<K, V, H, A>, key &const K) bool`

Whether `key` is in the table.

- `key`: the key to look for

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn contains(this &const HashTable<K, V, H, A>, key &const K) bool
```

### `fn count(this &const HashTable<K, V, H, A>, key &const K) usize`

How many entries hold `key`.

- `key`: the key to look for

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn count(this &const HashTable<K, V, H, A>, key &const K) usize
```

### `fn erase(this &HashTable<K, V, H, A>, key &const K) bool`

Removes one entry holding `key`, and answers whether there was one.

- `key`: the key to look for

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn erase(this &HashTable<K, V, H, A>, key &const K) bool
```

### `fn erase_all(this &HashTable<K, V, H, A>, key &const K) usize`

Removes every entry holding `key`, and answers how many there were.

- `key`: the key to look for

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn erase_all(this &HashTable<K, V, H, A>, key &const K) usize
```

### `fn release(this &HashTable<K, V, H, A>, entry *HashEntry<K, V>) void`

Destroys one entry that is no longer in any chain.

- `entry`: the entry to free

```
fn release(this &HashTable<K, V, H, A>, entry *HashEntry<K, V>) void
```

### `fn clear(this &HashTable<K, V, H, A>) void`

Removes every entry.

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn clear(this &HashTable<K, V, H, A>) void
```

### `fn copy_entries(this &HashTable<K, V, H, A>, other &const HashTable<K, V, H, A>) void`

Copies every entry of `other` into this empty table.

- `other`: the table to copy

```
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn copy_entries(this &HashTable<K, V, H, A>, other &const HashTable<K, V, H, A>) void
```

### `fn __init__(this &HashTable<K, V, H, A>, other &const HashTable<K, V, H, A>) void`

Copies another table, chains and all.

- `other`: the table to copy

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn __init__(this &HashTable<K, V, H, A>, other &const HashTable<K, V, H, A>) void
```

### `fn __init__(this &HashTable<K, V, H, A>, other &&HashTable<K, V, H, A>) void`

Takes another table's chains.

- `other`: the table to copy

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn __init__(this &HashTable<K, V, H, A>, other &&HashTable<K, V, H, A>) void
```

### `fn __assign__(this &HashTable<K, V, H, A>, other &const HashTable<K, V, H, A>) void`

Copies another table over this one.

- `other`: the table to copy

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn __assign__(this &HashTable<K, V, H, A>, other &const HashTable<K, V, H, A>) void
```

### `fn __assign__(this &HashTable<K, V, H, A>, other &&HashTable<K, V, H, A>) void`

Takes another table's chains over this one.

- `other`: the table to copy

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn __assign__(this &HashTable<K, V, H, A>, other &&HashTable<K, V, H, A>) void
```

### `fn __drop__(this &HashTable<K, V, H, A>) void`

Destroys the table.

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn __drop__(this &HashTable<K, V, H, A>) void
```

### `type HashIterator {`

----------------------------------------------------------------- iterators
A position in a table, which walks every entry.

The bucket the entry is in is remembered, so stepping is "the rest of this
chain, or the next bucket that has one" and needs no second scan.

```
export
template <K type, V type>
type HashIterator {
    // The buckets being walked, or null at the end.
    buckets *byte;
    // How many buckets there are.
    capacity usize;
    // The bucket the current entry is in.
    index usize;
    // The entry the iterator stands on, or null at the end.
    entry *HashEntry<K, V>;
}
```

### `fn __init__(this &HashIterator<K, V>) void`

Creates an iterator that stands nowhere, which is how `end()` is written.

```
export
template <K type, V type>
fn __init__(this &HashIterator<K, V>) void
```

### `fn __init__(this &HashIterator<K, V>, buckets *byte, capacity usize) void`

Creates an iterator over a table's buckets, standing on the first entry.

- `buckets`: the bucket array, or null for a position with none
- `capacity`: how many buckets the array has; a power of two

```
export
template <K type, V type>
fn __init__(this &HashIterator<K, V>, buckets *byte, capacity usize) void
```

### `fn seek(this &HashIterator<K, V>) void`

Finds the first entry from the current bucket on.

```
fn seek(this &HashIterator<K, V>) void
```

### `fn __inc__(this &HashIterator<K, V>) void`

Moves to the next entry.

```
export
template <K type, V type>
fn __inc__(this &HashIterator<K, V>) void
```

### `fn __deref__(this &HashIterator<K, V>) &V`

The mapped value at this position, for writing.

```
export
template <K type, V type>
fn __deref__(this &HashIterator<K, V>) &V
```

### `fn key(this &const HashIterator<K, V>) &const K`

The key at this position, for reading.

```
export
template <K type, V type>
fn key(this &const HashIterator<K, V>) &const K
```

### `fn __eq__(this &const HashIterator<K, V>, other &const HashIterator<K, V>) bool`

Whether two positions are the same.

- `other`: the table to copy

```
export
template <K type, V type>
fn __eq__(this &const HashIterator<K, V>, other &const HashIterator<K, V>) bool
```

### `fn __ne__(this &const HashIterator<K, V>, other &const HashIterator<K, V>) bool`

Whether two positions differ.

- `other`: the table to copy

```
export
template <K type, V type>
fn __ne__(this &const HashIterator<K, V>, other &const HashIterator<K, V>) bool
```

### `type ConstHashIterator {`

A position in a table, read only.

```
export
template <K type, V type>
type ConstHashIterator {
    // The buckets being walked, or null at the end.
    buckets *byte;
    // How many buckets there are.
    capacity usize;
    // The bucket the current entry is in.
    index usize;
    // The entry the iterator stands on, or null at the end.
    entry *const HashEntry<K, V>;
}
```

### `fn __init__(this &ConstHashIterator<K, V>) void`

Creates an iterator that stands nowhere.

```
export
template <K type, V type>
fn __init__(this &ConstHashIterator<K, V>) void
```

### `fn __init__(this &ConstHashIterator<K, V>, buckets *byte, capacity usize) void`

Creates an iterator over a table's buckets.

- `buckets`: the bucket array, or null for a position with none
- `capacity`: how many buckets the array has; a power of two

```
export
template <K type, V type>
fn __init__(this &ConstHashIterator<K, V>, buckets *byte, capacity usize) void
```

### `fn seek(this &ConstHashIterator<K, V>) void`

Finds the first entry from the current bucket on.

```
fn seek(this &ConstHashIterator<K, V>) void
```

### `fn __inc__(this &ConstHashIterator<K, V>) void`

Moves to the next entry.

```
export
template <K type, V type>
fn __inc__(this &ConstHashIterator<K, V>) void
```

### `fn __deref__(this &const ConstHashIterator<K, V>) &const V`

The mapped value at this position, for reading.

```
export
template <K type, V type>
fn __deref__(this &const ConstHashIterator<K, V>) &const V
```

### `fn key(this &const ConstHashIterator<K, V>) &const K`

The key at this position, for reading.

```
export
template <K type, V type>
fn key(this &const ConstHashIterator<K, V>) &const K
```

### `fn __eq__(this &const ConstHashIterator<K, V>, other &const ConstHashIterator<K, V>) bool`

Whether two positions are the same.

- `other`: the table to copy

```
export
template <K type, V type>
fn __eq__(this &const ConstHashIterator<K, V>, other &const ConstHashIterator<K, V>) bool
```

### `fn __ne__(this &const ConstHashIterator<K, V>, other &const ConstHashIterator<K, V>) bool`

Whether two positions differ.

- `other`: the table to copy

```
export
template <K type, V type>
fn __ne__(this &const ConstHashIterator<K, V>, other &const ConstHashIterator<K, V>) bool
```

### `type SetHashIterator {`

A position in a set, which walks the keys.

A set stores a `Unit` as its value, so the iterator shared with a map would
hand out `&Unit`; this one hands out the key, which is what a set has.

```
export
template <K type>
type SetHashIterator {
    // The buckets being walked, or null at the end.
    buckets *byte;
    // How many buckets there are.
    capacity usize;
    // The bucket the current entry is in.
    index usize;
    // The entry the iterator stands on, or null at the end.
    entry *HashEntry<K, Unit>;
}
```

### `fn __init__(this &SetHashIterator<K>) void`

Creates an iterator that stands nowhere.

```
export
template <K type>
fn __init__(this &SetHashIterator<K>) void
```

### `fn __init__(this &SetHashIterator<K>, buckets *byte, capacity usize) void`

Creates an iterator over a table's buckets.

- `buckets`: the bucket array, or null for a position with none
- `capacity`: how many buckets the array has; a power of two

```
export
template <K type>
fn __init__(this &SetHashIterator<K>, buckets *byte, capacity usize) void
```

### `fn seek(this &SetHashIterator<K>) void`

Finds the first entry from the current bucket on.

```
fn seek(this &SetHashIterator<K>) void
```

### `fn __inc__(this &SetHashIterator<K>) void`

Moves to the next key.

```
export
template <K type>
fn __inc__(this &SetHashIterator<K>) void
```

### `fn __deref__(this &const SetHashIterator<K>) &const K`

The key at this position.

```
export
template <K type>
fn __deref__(this &const SetHashIterator<K>) &const K
```

### `fn __eq__(this &const SetHashIterator<K>, other &const SetHashIterator<K>) bool`

Whether two positions are the same.

- `other`: the table to copy

```
export
template <K type>
fn __eq__(this &const SetHashIterator<K>, other &const SetHashIterator<K>) bool
```

### `fn __ne__(this &const SetHashIterator<K>, other &const SetHashIterator<K>) bool`

Whether two positions differ.

- `other`: the table to copy

```
export
template <K type>
fn __ne__(this &const SetHashIterator<K>, other &const SetHashIterator<K>) bool
```

### `type ConstSetHashIterator {`

A position in a set, read only.

```
export
template <K type>
type ConstSetHashIterator {
    buckets *byte;
    capacity usize;
    index usize;
    entry *const HashEntry<K, Unit>;
}
```

### `fn __init__(this &ConstSetHashIterator<K>) void`

Creates an iterator that stands nowhere.

```
export
template <K type>
fn __init__(this &ConstSetHashIterator<K>) void
```

### `fn __init__(this &ConstSetHashIterator<K>, buckets *byte, capacity usize) void`

Creates an iterator over a table's buckets.

- `buckets`: the bucket array, or null for a position with none
- `capacity`: how many buckets the array has; a power of two

```
export
template <K type>
fn __init__(this &ConstSetHashIterator<K>, buckets *byte, capacity usize) void
```

### `fn seek(this &ConstSetHashIterator<K>) void`

Finds the first entry from the current bucket on.

```
fn seek(this &ConstSetHashIterator<K>) void
```

### `fn __inc__(this &ConstSetHashIterator<K>) void`

Moves to the next key.

```
export
template <K type>
fn __inc__(this &ConstSetHashIterator<K>) void
```

### `fn __deref__(this &const ConstSetHashIterator<K>) &const K`

The key at this position.

```
export
template <K type>
fn __deref__(this &const ConstSetHashIterator<K>) &const K
```

### `fn __eq__(this &const ConstSetHashIterator<K>, other &const ConstSetHashIterator<K>) bool`

Whether two positions are the same.

- `other`: the table to copy

```
export
template <K type>
fn __eq__(this &const ConstSetHashIterator<K>, other &const ConstSetHashIterator<K>) bool
```

### `fn __ne__(this &const ConstSetHashIterator<K>, other &const ConstSetHashIterator<K>) bool`

Whether two positions differ.

- `other`: the table to copy

```
export
template <K type>
fn __ne__(this &const ConstSetHashIterator<K>, other &const ConstSetHashIterator<K>) bool
```

### `type HMap {`

--------------------------------------------------------------- containers
A map from keys to values, with no order.

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
type HMap {
    table HashTable<K, V, H, A>;
}
```

### `fn __init__(this &HMap<K, V, H, A>) void`

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn __init__(this &HMap<K, V, H, A>) void
```

### `fn __init__(this &HMap<K, V, H, A>, other &const HMap<K, V, H, A>) void`

Copies another hmap through its table.

- `other`: the table to copy

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn __init__(this &HMap<K, V, H, A>, other &const HMap<K, V, H, A>) void
```

### `fn __init__(this &HMap<K, V, H, A>, other &&HMap<K, V, H, A>) void`

Takes another hmap's table.

- `other`: the table to copy

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn __init__(this &HMap<K, V, H, A>, other &&HMap<K, V, H, A>) void
```

### `fn __assign__(this &HMap<K, V, H, A>, other &const HMap<K, V, H, A>) void`

Copies another hmap over this one.

- `key`: the key to look for

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn __assign__(this &HMap<K, V, H, A>, other &const HMap<K, V, H, A>) void
```

### `fn __init__(this &HMap<K, V, H, A>, hash H, memory A) void`

Creates an empty table with the caller's hash and allocator.

- `hash`: the hash function the table uses
- `memory`: the allocator the buckets and entries are drawn from

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn __init__(this &HMap<K, V, H, A>, hash H, memory A) void
```

### `fn __drop__(this &HMap<K, V, H, A>) void`

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn __drop__(this &HMap<K, V, H, A>) void
```

### `fn insert(this &HMap<K, V, H, A>, key K, value V) void`

Adds an entry, replacing the value when the key is already there.

- `key`: the key to place, moved in
- `value`: the value to store with it, moved in

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn insert(this &HMap<K, V, H, A>, key K, value V) void
```

### `fn find(this &HMap<K, V, H, A>, key &const K) *V`

Looks the key up in the table.

- `key`: the key to look for

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn find(this &HMap<K, V, H, A>, key &const K) *V
```

### `fn find(this &const HMap<K, V, H, A>, key &const K) *const V`

Looks the key up in the table.

- `key`: the key to look for

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn find(this &const HMap<K, V, H, A>, key &const K) *const V
```

### `fn at(this &HMap<K, V, H, A>, key &const K) &V`

Returns the value for a key, and aborts when there is none.

The same rule the ordered map follows: `m[key]` names a value that must be
there, and `find` is the form that answers "is it?".

- `key`: the key to look for

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn at(this &HMap<K, V, H, A>, key &const K) &V
```

### `fn at(this &const HMap<K, V, H, A>, key &const K) &const V`

Returns the value for a key, and aborts when there is none, for reading.

- `key`: the key to look for

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn at(this &const HMap<K, V, H, A>, key &const K) &const V
```

### `fn __getitem__(this &HMap<K, V, H, A>, key &const K) &V`

Indexes the map: `m[key]` is the value, and a missing key aborts.

- `key`: the key to look for

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn __getitem__(this &HMap<K, V, H, A>, key &const K) &V
```

### `fn contains(this &const HMap<K, V, H, A>, key &const K) bool`

Looks the key up in the table.

- `key`: the key to look for

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn contains(this &const HMap<K, V, H, A>, key &const K) bool
```

### `fn count(this &const HMap<K, V, H, A>, key &const K) usize`

Looks the key up in the table.

- `key`: the key to look for

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn count(this &const HMap<K, V, H, A>, key &const K) usize
```

### `fn erase(this &HMap<K, V, H, A>, key &const K) bool`

Looks the key up in the table.

- `key`: the key to look for

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn erase(this &HMap<K, V, H, A>, key &const K) bool
```

### `fn erase_all(this &HMap<K, V, H, A>, key &const K) usize`

Looks the key up in the table.

- `key`: the key to look for

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn erase_all(this &HMap<K, V, H, A>, key &const K) usize
```

### `fn clear(this &HMap<K, V, H, A>) void`

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn clear(this &HMap<K, V, H, A>) void
```

### `fn length(this &const HMap<K, V, H, A>) usize`

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn length(this &const HMap<K, V, H, A>) usize
```

### `fn empty(this &const HMap<K, V, H, A>) bool`

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn empty(this &const HMap<K, V, H, A>) bool
```

### `fn begin(this &HMap<K, V, H, A>) HashIterator<K, V>`

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn begin(this &HMap<K, V, H, A>) HashIterator<K, V>
```

### `fn end(this &HMap<K, V, H, A>) HashIterator<K, V>`

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn end(this &HMap<K, V, H, A>) HashIterator<K, V>
```

### `fn begin(this &const HMap<K, V, H, A>) ConstHashIterator<K, V>`

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn begin(this &const HMap<K, V, H, A>) ConstHashIterator<K, V>
```

### `fn end(this &const HMap<K, V, H, A>) ConstHashIterator<K, V>`

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn end(this &const HMap<K, V, H, A>) ConstHashIterator<K, V>
```

### `type HSet {`

A set of keys, with no order.

```
export
template <K type, H type = Hash<K>, A type = DefaultAllocator>
type HSet {
    table HashTable<K, Unit, H, A>;
}
```

### `fn __init__(this &HSet<K, H, A>) void`

```
export
template <K type, H type = Hash<K>, A type = DefaultAllocator>
fn __init__(this &HSet<K, H, A>) void
```

### `fn __init__(this &HSet<K, H, A>, other &const HSet<K, H, A>) void`

Copies another hset through its table.

- `other`: the table to copy

```
export
template <K type, H type = Hash<K>, A type = DefaultAllocator>
fn __init__(this &HSet<K, H, A>, other &const HSet<K, H, A>) void
```

### `fn __init__(this &HSet<K, H, A>, other &&HSet<K, H, A>) void`

Takes another hset's table.

- `other`: the table to copy

```
export
template <K type, H type = Hash<K>, A type = DefaultAllocator>
fn __init__(this &HSet<K, H, A>, other &&HSet<K, H, A>) void
```

### `fn __assign__(this &HSet<K, H, A>, other &const HSet<K, H, A>) void`

Copies another hset over this one.

- `key`: the key to look for

```
export
template <K type, H type = Hash<K>, A type = DefaultAllocator>
fn __assign__(this &HSet<K, H, A>, other &const HSet<K, H, A>) void
```

### `fn __init__(this &HSet<K, H, A>, hash H, memory A) void`

Creates an empty table with the caller's hash and allocator.

- `hash`: the hash function the table uses
- `memory`: the allocator the buckets and entries are drawn from

```
export
template <K type, H type = Hash<K>, A type = DefaultAllocator>
fn __init__(this &HSet<K, H, A>, hash H, memory A) void
```

### `fn __drop__(this &HSet<K, H, A>) void`

```
export
template <K type, H type = Hash<K>, A type = DefaultAllocator>
fn __drop__(this &HSet<K, H, A>) void
```

### `fn insert(this &HSet<K, H, A>, key K) void`

Looks the key up in the table.

- `key`: the key to look for

```
export
template <K type, H type = Hash<K>, A type = DefaultAllocator>
fn insert(this &HSet<K, H, A>, key K) void
```

### `fn contains(this &const HSet<K, H, A>, key &const K) bool`

Looks the key up in the table.

- `key`: the key to look for

```
export
template <K type, H type = Hash<K>, A type = DefaultAllocator>
fn contains(this &const HSet<K, H, A>, key &const K) bool
```

### `fn count(this &const HSet<K, H, A>, key &const K) usize`

Looks the key up in the table.

- `key`: the key to look for

```
export
template <K type, H type = Hash<K>, A type = DefaultAllocator>
fn count(this &const HSet<K, H, A>, key &const K) usize
```

### `fn erase(this &HSet<K, H, A>, key &const K) bool`

Looks the key up in the table.

- `key`: the key to look for

```
export
template <K type, H type = Hash<K>, A type = DefaultAllocator>
fn erase(this &HSet<K, H, A>, key &const K) bool
```

### `fn erase_all(this &HSet<K, H, A>, key &const K) usize`

Looks the key up in the table.

- `key`: the key to look for

```
export
template <K type, H type = Hash<K>, A type = DefaultAllocator>
fn erase_all(this &HSet<K, H, A>, key &const K) usize
```

### `fn clear(this &HSet<K, H, A>) void`

```
export
template <K type, H type = Hash<K>, A type = DefaultAllocator>
fn clear(this &HSet<K, H, A>) void
```

### `fn length(this &const HSet<K, H, A>) usize`

```
export
template <K type, H type = Hash<K>, A type = DefaultAllocator>
fn length(this &const HSet<K, H, A>) usize
```

### `fn empty(this &const HSet<K, H, A>) bool`

```
export
template <K type, H type = Hash<K>, A type = DefaultAllocator>
fn empty(this &const HSet<K, H, A>) bool
```

### `fn begin(this &HSet<K, H, A>) SetHashIterator<K>`

```
export
template <K type, H type = Hash<K>, A type = DefaultAllocator>
fn begin(this &HSet<K, H, A>) SetHashIterator<K>
```

### `fn end(this &HSet<K, H, A>) SetHashIterator<K>`

```
export
template <K type, H type = Hash<K>, A type = DefaultAllocator>
fn end(this &HSet<K, H, A>) SetHashIterator<K>
```

### `fn begin(this &const HSet<K, H, A>) ConstSetHashIterator<K>`

```
export
template <K type, H type = Hash<K>, A type = DefaultAllocator>
fn begin(this &const HSet<K, H, A>) ConstSetHashIterator<K>
```

### `fn end(this &const HSet<K, H, A>) ConstSetHashIterator<K>`

```
export
template <K type, H type = Hash<K>, A type = DefaultAllocator>
fn end(this &const HSet<K, H, A>) ConstSetHashIterator<K>
```

### `type HMMap {`

A map that keeps every value inserted for a key.

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
type HMMap {
    table HashTable<K, V, H, A>;
}
```

### `fn __init__(this &HMMap<K, V, H, A>) void`

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn __init__(this &HMMap<K, V, H, A>) void
```

### `fn __init__(this &HMMap<K, V, H, A>, other &const HMMap<K, V, H, A>) void`

Copies another hmmap through its table.

- `other`: the table to copy

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn __init__(this &HMMap<K, V, H, A>, other &const HMMap<K, V, H, A>) void
```

### `fn __init__(this &HMMap<K, V, H, A>, other &&HMMap<K, V, H, A>) void`

Takes another hmmap's table.

- `other`: the table to copy

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn __init__(this &HMMap<K, V, H, A>, other &&HMMap<K, V, H, A>) void
```

### `fn __assign__(this &HMMap<K, V, H, A>, other &const HMMap<K, V, H, A>) void`

Copies another hmmap over this one.

- `key`: the key to look for

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn __assign__(this &HMMap<K, V, H, A>, other &const HMMap<K, V, H, A>) void
```

### `fn __init__(this &HMMap<K, V, H, A>, hash H, memory A) void`

Creates an empty table with the caller's hash and allocator.

- `hash`: the hash function the table uses
- `memory`: the allocator the buckets and entries are drawn from

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn __init__(this &HMMap<K, V, H, A>, hash H, memory A) void
```

### `fn __drop__(this &HMMap<K, V, H, A>) void`

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn __drop__(this &HMMap<K, V, H, A>) void
```

### `fn insert(this &HMMap<K, V, H, A>, key K, value V) void`

Adds an entry, replacing the value when the key is already there.

- `key`: the key to place, moved in
- `value`: the value to store with it, moved in

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn insert(this &HMMap<K, V, H, A>, key K, value V) void
```

### `fn find(this &HMMap<K, V, H, A>, key &const K) *V`

Looks the key up in the table.

- `key`: the key to look for

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn find(this &HMMap<K, V, H, A>, key &const K) *V
```

### `fn find(this &const HMMap<K, V, H, A>, key &const K) *const V`

Looks the key up in the table.

- `key`: the key to look for

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn find(this &const HMMap<K, V, H, A>, key &const K) *const V
```

### `fn contains(this &const HMMap<K, V, H, A>, key &const K) bool`

Looks the key up in the table.

- `key`: the key to look for

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn contains(this &const HMMap<K, V, H, A>, key &const K) bool
```

### `fn count(this &const HMMap<K, V, H, A>, key &const K) usize`

Looks the key up in the table.

- `key`: the key to look for

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn count(this &const HMMap<K, V, H, A>, key &const K) usize
```

### `fn erase(this &HMMap<K, V, H, A>, key &const K) bool`

Looks the key up in the table.

- `key`: the key to look for

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn erase(this &HMMap<K, V, H, A>, key &const K) bool
```

### `fn erase_all(this &HMMap<K, V, H, A>, key &const K) usize`

Looks the key up in the table.

- `key`: the key to look for

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn erase_all(this &HMMap<K, V, H, A>, key &const K) usize
```

### `fn clear(this &HMMap<K, V, H, A>) void`

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn clear(this &HMMap<K, V, H, A>) void
```

### `fn length(this &const HMMap<K, V, H, A>) usize`

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn length(this &const HMMap<K, V, H, A>) usize
```

### `fn empty(this &const HMMap<K, V, H, A>) bool`

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn empty(this &const HMMap<K, V, H, A>) bool
```

### `fn begin(this &HMMap<K, V, H, A>) HashIterator<K, V>`

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn begin(this &HMMap<K, V, H, A>) HashIterator<K, V>
```

### `fn end(this &HMMap<K, V, H, A>) HashIterator<K, V>`

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn end(this &HMMap<K, V, H, A>) HashIterator<K, V>
```

### `fn begin(this &const HMMap<K, V, H, A>) ConstHashIterator<K, V>`

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn begin(this &const HMMap<K, V, H, A>) ConstHashIterator<K, V>
```

### `fn end(this &const HMMap<K, V, H, A>) ConstHashIterator<K, V>`

```
export
template <K type, V type, H type = Hash<K>, A type = DefaultAllocator>
fn end(this &const HMMap<K, V, H, A>) ConstHashIterator<K, V>
```

### `type HMSet {`

A set that keeps a key every time it is inserted.

```
export
template <K type, H type = Hash<K>, A type = DefaultAllocator>
type HMSet {
    table HashTable<K, Unit, H, A>;
}
```

### `fn __init__(this &HMSet<K, H, A>) void`

```
export
template <K type, H type = Hash<K>, A type = DefaultAllocator>
fn __init__(this &HMSet<K, H, A>) void
```

### `fn __init__(this &HMSet<K, H, A>, other &const HMSet<K, H, A>) void`

Copies another hmset through its table.

- `other`: the table to copy

```
export
template <K type, H type = Hash<K>, A type = DefaultAllocator>
fn __init__(this &HMSet<K, H, A>, other &const HMSet<K, H, A>) void
```

### `fn __init__(this &HMSet<K, H, A>, other &&HMSet<K, H, A>) void`

Takes another hmset's table.

- `other`: the table to copy

```
export
template <K type, H type = Hash<K>, A type = DefaultAllocator>
fn __init__(this &HMSet<K, H, A>, other &&HMSet<K, H, A>) void
```

### `fn __assign__(this &HMSet<K, H, A>, other &const HMSet<K, H, A>) void`

Copies another hmset over this one.

- `key`: the key to look for

```
export
template <K type, H type = Hash<K>, A type = DefaultAllocator>
fn __assign__(this &HMSet<K, H, A>, other &const HMSet<K, H, A>) void
```

### `fn __init__(this &HMSet<K, H, A>, hash H, memory A) void`

Creates an empty table with the caller's hash and allocator.

- `hash`: the hash function the table uses
- `memory`: the allocator the buckets and entries are drawn from

```
export
template <K type, H type = Hash<K>, A type = DefaultAllocator>
fn __init__(this &HMSet<K, H, A>, hash H, memory A) void
```

### `fn __drop__(this &HMSet<K, H, A>) void`

```
export
template <K type, H type = Hash<K>, A type = DefaultAllocator>
fn __drop__(this &HMSet<K, H, A>) void
```

### `fn insert(this &HMSet<K, H, A>, key K) void`

Looks the key up in the table.

- `key`: the key to look for

```
export
template <K type, H type = Hash<K>, A type = DefaultAllocator>
fn insert(this &HMSet<K, H, A>, key K) void
```

### `fn contains(this &const HMSet<K, H, A>, key &const K) bool`

Looks the key up in the table.

- `key`: the key to look for

```
export
template <K type, H type = Hash<K>, A type = DefaultAllocator>
fn contains(this &const HMSet<K, H, A>, key &const K) bool
```

### `fn count(this &const HMSet<K, H, A>, key &const K) usize`

Looks the key up in the table.

- `key`: the key to look for

```
export
template <K type, H type = Hash<K>, A type = DefaultAllocator>
fn count(this &const HMSet<K, H, A>, key &const K) usize
```

### `fn erase(this &HMSet<K, H, A>, key &const K) bool`

Looks the key up in the table.

- `key`: the key to look for

```
export
template <K type, H type = Hash<K>, A type = DefaultAllocator>
fn erase(this &HMSet<K, H, A>, key &const K) bool
```

### `fn erase_all(this &HMSet<K, H, A>, key &const K) usize`

Looks the key up in the table.

- `key`: the key to look for

```
export
template <K type, H type = Hash<K>, A type = DefaultAllocator>
fn erase_all(this &HMSet<K, H, A>, key &const K) usize
```

### `fn clear(this &HMSet<K, H, A>) void`

```
export
template <K type, H type = Hash<K>, A type = DefaultAllocator>
fn clear(this &HMSet<K, H, A>) void
```

### `fn length(this &const HMSet<K, H, A>) usize`

```
export
template <K type, H type = Hash<K>, A type = DefaultAllocator>
fn length(this &const HMSet<K, H, A>) usize
```

### `fn empty(this &const HMSet<K, H, A>) bool`

```
export
template <K type, H type = Hash<K>, A type = DefaultAllocator>
fn empty(this &const HMSet<K, H, A>) bool
```

### `fn begin(this &HMSet<K, H, A>) SetHashIterator<K>`

```
export
template <K type, H type = Hash<K>, A type = DefaultAllocator>
fn begin(this &HMSet<K, H, A>) SetHashIterator<K>
```

### `fn end(this &HMSet<K, H, A>) SetHashIterator<K>`

```
export
template <K type, H type = Hash<K>, A type = DefaultAllocator>
fn end(this &HMSet<K, H, A>) SetHashIterator<K>
```

### `fn begin(this &const HMSet<K, H, A>) ConstSetHashIterator<K>`

```
export
template <K type, H type = Hash<K>, A type = DefaultAllocator>
fn begin(this &const HMSet<K, H, A>) ConstSetHashIterator<K>
```

### `fn end(this &const HMSet<K, H, A>) ConstSetHashIterator<K>`

```
export
template <K type, H type = Hash<K>, A type = DefaultAllocator>
fn end(this &const HMSet<K, H, A>) ConstSetHashIterator<K>
```

### `fn cbegin(this &const HMap<K, V, H, A>) ConstHashIterator<K, V>`

The first position, read only, named as C++ names it.

```
export
template <K type, V type, H type, A type>
fn cbegin(this &const HMap<K, V, H, A>) ConstHashIterator<K, V>
```

### `fn cend(this &const HMap<K, V, H, A>) ConstHashIterator<K, V>`

The one-past-the-end position, read only, named as C++ names it.

```
export
template <K type, V type, H type, A type>
fn cend(this &const HMap<K, V, H, A>) ConstHashIterator<K, V>
```

### `fn cbegin(this &const HMMap<K, V, H, A>) ConstHashIterator<K, V>`

The first position, read only, named as C++ names it.

```
export
template <K type, V type, H type, A type>
fn cbegin(this &const HMMap<K, V, H, A>) ConstHashIterator<K, V>
```

### `fn cend(this &const HMMap<K, V, H, A>) ConstHashIterator<K, V>`

The one-past-the-end position, read only, named as C++ names it.

```
export
template <K type, V type, H type, A type>
fn cend(this &const HMMap<K, V, H, A>) ConstHashIterator<K, V>
```

### `fn cbegin(this &const HSet<K, H, A>) ConstSetHashIterator<K>`

The first position, read only, named as C++ names it.

```
export
template <K type, H type, A type>
fn cbegin(this &const HSet<K, H, A>) ConstSetHashIterator<K>
```

### `fn cend(this &const HSet<K, H, A>) ConstSetHashIterator<K>`

The one-past-the-end position, read only, named as C++ names it.

```
export
template <K type, H type, A type>
fn cend(this &const HSet<K, H, A>) ConstSetHashIterator<K>
```

### `fn cbegin(this &const HMSet<K, H, A>) ConstSetHashIterator<K>`

The first position, read only, named as C++ names it.

```
export
template <K type, H type, A type>
fn cbegin(this &const HMSet<K, H, A>) ConstSetHashIterator<K>
```

### `fn cend(this &const HMSet<K, H, A>) ConstSetHashIterator<K>`

The one-past-the-end position, read only, named as C++ names it.

```
export
template <K type, H type, A type>
fn cend(this &const HMSet<K, H, A>) ConstSetHashIterator<K>
```
