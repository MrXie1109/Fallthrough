# The rest of the standard library

> [中文](library.zh.md)

[`collections.md`](collections.md) covers `Array<T>`, `String`, and the handful of
other `std` entry points. This document continues with the remaining modules: memory,
containers, iterators, numbers, I/O, synchronisation, and file paths. Each module is its
own file, so a program that wants only one piece imports only that piece.

- [Getting hold of them](#getting-hold-of-them)
- [Memory: allocators, `Box`, `Rc`, `Arc`](#memory-allocators-box-rc-arc)
- [Iterator traits](#iterator-traits)
- [Ordered containers: `std.tree`](#ordered-containers-stdtree)
- [Hashed containers: `std.hash`](#hashed-containers-stdhash)
- [Double-ended queue and heap: `std.deque`, `std.heap`](#double-ended-queue-and-heap-stddeque-stdheap)
- [Numbers: `std.math`](#numbers-stdmath)
- [I/O: `std.io`](#io-stdio)
- [Synchronisation: `std.sync`](#synchronisation-stdsync)
- [File paths: `std.fs`](#file-paths-stdfs)
- [Limitations](#limitations)

## Getting hold of them

```ft
import std;          // umbrella: everything below
import std.tree;     // only RBTree and TSet/TMap
import std.hash;     // only HashTable and HSet/HMap
import std.io;       // only the devices and formatted I/O
```

As `collections.md` says: an umbrella's **type names** can be written short
(`RBTree<int, int>` is `std.RBTree<int, int>`), while **functions** keep their full
name (`std.min(..)`). A container template's default arguments (comparator, hasher,
allocator) are written short inside the module and stay that way under the umbrella:
`TMap<String, int>` uses `std.Less<String>` without naming it.

## Memory: allocators, `Box`, `Rc`, `Arc`

### An allocator is a type

Allocators live in [`std/allocators.ftm`](../../../standard/std/allocators.ftm): the
`Allocator` trait (`alloc`/`free`), `DefaultAllocator` (the C heap) and the new
**`Arena`** -- one block and a cursor, where `alloc` is a pointer bump, `reset()`
makes everything available again, and running out panics. It suits work that
dies together (a compiler pass, a document parse):

```ft
import std.allocators;

arena Arena = Arena(64usize);
block *byte = arena.alloc(8usize);   // uninitialised; constructing a value is __init__
arena.reset();                        // all of it reusable
```


`std.Allocator` is a trait asking for two methods:

```ft
traits Allocator(it A) {
    fn alloc(this &A, size usize) *byte;
    fn free(this &A, block *byte) void;
}
```

`std.DefaultAllocator` is `malloc`/`free`. Containers and `Rc`/`Arc` take an allocator
as a **template argument**, defaulting to it:

```ft
type MyAllocator { };              // with the two methods implemented
xs Deque<int, MyAllocator>;        // allocated another way; Array takes only the element type
```

An allocator is a value, not a global hook. Two containers with different allocators
have nothing to do with each other's memory. Every `alloc` may return `nullptr`; a
container `panic`s with `"out of memory"` when it does, just as `std.allocate` does.

### Raw allocation

`std` exposes C's allocation functions directly; their signatures are in
[`c-bindings.md`](c-bindings.md):

| Name | Signature | Purpose |
| ---- | --------- | ------- |
| `std.alloc` | `(usize) *byte` | `malloc`; may return `nullptr` |
| `std.allocate` | `(usize) *byte` | the same, but `panic`s on failure |
| `std.calloc` | `(usize, usize) *byte` | zeroed allocation |
| `std.realloc` | `(*byte, usize) *byte` | reallocation |
| `std.free` | `(*byte) void` | release |

### `Box<T>`

One value on the heap, released when the box is destroyed.

```ft
b Box<int> = 7;        // from a value
b.get() = 9;           // get() returns &T
c Box<int> = b;        // copy: two independent values
```

`Box<T>()` is the empty box (`empty()` is `true`); `get()` on an empty box `panic`s.

### `Rc<T>` and `RcWeak<T>`

Reference counting, for sharing one value on one thread:

```ft
first Rc<String> = String("text");
second Rc<String> = first;          // count 2
weak RcWeak<String> = first.downgrade();
std.out.println(weak.upgrade().get());  // a new owner, while one is alive
```

- `strong_count()` and `weak_count()` are the current counts;
- `clone()` is the explicit strong copy, `downgrade()` produces an observer;
- the value is destroyed when the last strong owner goes, after which `upgrade()`
  returns an empty `Rc` and `expired()` is `true`;
- an empty `Rc` -- default-constructed, or the result of a failed `upgrade()` -- has no
  value to read: `get()`/`*` `panic`s with `"Rc is empty"` instead of dereferencing null;
- the control block itself is freed when the last weak observer goes.

Copy, move, assignment, and destruction are implemented as a set
(`__init__`/`__assign__`/`__drop__`), so an `Rc<T>` can live in a container and be
returned from a function.

### `Arc<T>` and `ArcWeak<T>`

The same interface as `Rc`, differing in the count: `Arc`'s is `Atomic<usize>`, so the
value can be shared across threads. Construction, copy, move, assignment, destruction,
`clone()`, `get()`, `empty()`, `strong_count()`, `weak_count()`, and `downgrade()` are
one-for-one with `Rc`, and `ArcWeak` has `upgrade()`/`expired()` too.

Note that `Atomic<T>` is currently **mutex-backed** (see
[Synchronisation](#synchronisation-stdsync)), so an `Arc`'s count is correct but not
lock-free.

## Iterator traits

`std.iterator` is traits and nothing else. Whether a container satisfies one is a
compile-time question answered by a trait query:

| Trait | Requires |
| ----- | -------- |
| `Iterable` | `begin()`/`end()` |
| `InputIterator` | `__inc__`, `__deref__`, `__eq__`/`__ne__` |
| `ForwardIterator` | multiple passes, on top of `InputIterator` |
| `BidirectionalIterator` | plus `__dec__` |
| `RandomAccessIterator` | plus `+`/`-`/`[]` |

The query is written `InputIterator(MyIterator)` and is a compile-time `bool`:

```ft
static_assert(InputIterator(SetIterator<int>));
static_assert(BidirectionalIterator(SetIterator<int>));
```

Which level holds is a fact about each type rather than a table of categories:

- `std.tree`'s iterators have `--` and are **bidirectional**;
- `std.hash`'s and `std.deque`'s iterators only go forward (`++`), so they are
  `InputIterator`s;
- `Array<T>`'s `begin()`/`end()` **are pointers**, so random access is pointer
  arithmetic; walking backwards uses `rbegin()`/`rend()`, whose iterators have
  `+`/`-`/`[]` and satisfy `RandomAccessIterator`.

## Ordered containers: `std.tree`

A red-black tree, and four containers built on it.

### Comparators

```ft
traits Compare(it C, value E) {
    fn __call__(this &const C, a &const E, b &const E) bool;
}
```

`std.Less<E>` and `std.Greater<E>` are the two ready-made implementations. A custom
comparator is a type with `__call__`, passed as a template argument:

```ft
TMap<int, String, Greater<int>> descending;
```

A comparator answers "does `a` come before `b`", not a three-way comparison: `Less` is
`a < b`, `Greater` is `b < a`.

### `RBTree<K, V, Cmp = Less<K>, A = DefaultAllocator>`

| Method | Result |
| ------ | ------ |
| `insert(key, value)` | inserts one key, returns `*TreeNode<K, V>`; replaces the value when the key is there |
| `insert_multi(key, value)` | the same key may appear many times |
| `find(key)` | `*TreeNode<K, V>` (`*const` on a const tree), or `nullptr` |
| `contains(key)`, `count(key)` | whether it is there, and how many times |
| `lower_bound(key)`, `upper_bound(key)` | the first node not less than / greater than `key` |
| `erase(key)`, `erase_all(key)` | removes one / all |
| `clear()`, `length()`, `empty()` | clearing and size |

A `TreeNode<K, V>` has `key`, `value`, and `red`, `left`, `right`, `parent`. The links
are `*TreeNode<K, V>` rather than `*byte`, so walking the tree needs no casts:

```ft
node *TreeNode<int, String> = tree.find(3);
if (node != nullptr) {
    std.out.println(node.value);
}
```

### The containers built on the tree

| Type | Holds | Interface |
| ---- | ----- | --------- |
| `TMap<K, V>` | one value per key | `insert`, `find`, `at`, `[]`, `contains`, `count`, `erase`, `clear`, `length`, `empty`, `begin`/`end` |
| `TMMap<K, V>` | many values per key | `insert`, `find`, `count`, `erase`, `erase_all`, `clear`, `length`, `empty`, `begin`/`end` |
| `TSet<K>` | keys only | `insert`, `contains`, `count`, `erase`, `clear`, `length`, `empty`, `begin`/`end` |
| `TMSet<K>` | keys that may repeat | as `TSet`, but `insert` does not deduplicate |

`at`/`[]` abort when the key is missing, while `find` returns `nullptr`: use `find` when
the answer may be "no", `at` when the key must be there.

The iterators are `MapIterator`/`ConstMapIterator`/`SetIterator`/`ConstSetIterator`:
`__inc__`/`__dec__` move, `__deref__` gives the value, `key()` gives the key. The order
is therefore the key order the comparator defines:

```ft
TMap<String, int> ages;
ages.insert(String("Ada"), 36);
ages.insert(String("Grace"), 85);
it MapIterator<String, int> = ages.begin();
stop MapIterator<String, int> = ages.end();
while (it != stop) {           // key order
    C.printf("%s %d\n", it.key().data(), *it);
    ++it;
}
```

### Ordered, with one thing left unsaid

The tree guarantees `O(log n)` insertion, lookup, and deletion, and iteration in key
order. The relative order of several entries with the same key (`insert_multi`) is **not
guaranteed**, because rebalancing moves them.

## Hashed containers: `std.hash`

Separate chaining, and four containers built on it.

### Hashers

```ft
traits Hashable(it T) {
    fn __hash__(this &const T) usize;
}
```

`std.Hash<T>` is a **type** whose `__call__` asks `value.__hash__()`. Every integer,
`byte`, `bool`, `c8`/`c16`/`c32`, and `String` already has one. Giving a type a hash
means giving it `__hash__`:

```ft
type Point { x int; y int; }

export fn __hash__(this &const Point) usize {
    hasher Hash<int>;
    return hasher(this.x) ^ hasher(this.y);
}

export fn __eq__(this &const Point, other &const Point) bool {
    return this.x == other.x && this.y == other.y;
}
```

**Both methods are needed.** The hash decides which chain to look in, and `__eq__`
decides which entry in that chain is the key; the `Hashable` trait asks for both. A
custom hasher is a type with `__call__`, passed as a template argument.

### `HashTable<K, V, H = Hash<K>, A = DefaultAllocator>`

| Method | Result |
| ------ | ------ |
| `insert(key, value)` | inserts or replaces |
| `insert_multi(key, value)` | the same key may appear many times |
| `find(key)` | `*V` / `*const V`, or `nullptr` |
| `contains(key)`, `count(key)` | whether it is there, and how many times |
| `erase(key)`, `erase_all(key)` | removes one / all |
| `clear()`, `length()`, `empty()` | clearing and size |

The table grows and rehashes by load factor, so `find` is amortised `O(1)`.

`HashEntry<K, V>` is one node in a bucket (`key`, `value`, `hash`, `next`), and the
iterators move through it. It is exported because imported signatures name it; an
ordinary program does not write the name.

### The containers built on the table

| Type | Holds | Interface |
| ---- | ----- | --------- |
| `HMap<K, V>` | one value per key | `insert`, `find`, `at`, `[]`, `contains`, `count`, `erase`, `clear`, `length`, `empty`, `begin`/`end` |
| `HMMap<K, V>` | many values per key | `insert`, `find`, `count`, `erase`, `erase_all`, `clear`, `length`, `empty` |
| `HSet<K>` | keys only | `insert`, `contains`, `count`, `erase`, `clear`, `length`, `empty` |
| `HMSet<K>` | keys that may repeat | as `HSet`, but `insert` does not deduplicate |

`HSet`/`HMap` iteration order is **not guaranteed**: it is bucket order, unrelated to
insertion order or to the keys' own order. A program that wants order uses `std.tree`.

## Double-ended queue and heap: `std.deque`, `std.heap`

### `Deque<T, A = DefaultAllocator>`

A ring buffer that accepts and yields at both ends:

| Method | Result |
| ------ | ------ |
| `push_back(value)`, `push_front(value)` | adds at one end |
| `pop_back()`, `pop_front()` | removes from one end, returns the value |
| `front()`, `back()` | `&T` / `&const T`; `panic`s when empty |
| `[]` | by index, `0` being the front |
| `reserve`, `capacity`, `length`, `empty`, `clear` | capacity and state |

`DequeIterator` only goes forward (an `InputIterator`); the elements themselves are reached by index, `d[i]`.

### `Queue<T>` and `Stack<T>`

`Queue<T>` is a thin wrapper over `Deque<T>`: `push`/`pop`/`front`/`length`/`empty`/
`clear`, first in first out. `Stack<T>` is the same shape: `push`/`pop`/`top`/`length`/
`empty`/`clear`, last in first out.

### `Heap<T, Cmp = Less<T>, A = DefaultAllocator>`

A binary heap. The comparator says who leaves first: `Less` puts the **smallest** at the
top, `Greater` the largest.

| Method | Result |
| ------ | ------ |
| `push(value)` | inserts, `O(log n)` |
| `pop()` | removes the top, `O(log n)` |
| `top()` | `&T` / `&const T`; `panic`s when empty |
| `reserve`, `capacity`, `length`, `empty`, `clear` | capacity and state |

### `PriorityQueue<T, Cmp = Less<T>, A = DefaultAllocator>`

A wrapper over `Heap` with `push`/`pop`/`top`/`length`/`empty`/`clear`. By default the
smallest comes out first; `PriorityQueue<int, Greater<int>>` pops the largest first.

## Numbers: `std.math`

### True for any number

```ft
std.min(a, b)             // the smaller
std.max(a, b)             // the larger
std.clamp(v, low, high)   // brought into [low, high]
std.abs(v)                // without its sign; unsigned types come back unchanged
std.square(v)             // v * v
std.squared_length(a, b)  // a*a + b*b
```

They are templates that ask only for `<`, `-`, and `*`, so `int`, `f64`, and `String`
(which has `<`) all work, and nothing is computed at run time for the constants.

### Real numbers

These are the C library's, taking and returning `f64`:

| Function | Result |
| -------- | ------ |
| `sqrt(x)` | square root |
| `pow(base, exponent)` | power |
| `floor(x)`, `ceil(x)`, `round(x)` | down / up / nearest integer |
| `fabs(x)` | absolute value |
| `exp(x)`, `log(x)` | `e` to the power, natural logarithm |
| `sin(x)`, `cos(x)`, `tan(x)` | trigonometry, radians |
| `asin(x)`, `acos(x)`, `atan(x)` | inverse trigonometry, radians |
| `atan2(y, x)` | the angle of the point `(x, y)` |

An `f32` is written `as f64` on the way in: the language has no implicit numeric
conversion, and these functions do not narrow.

## Random numbers: `std.random`

`std.random` is the program's one `Random`, which is MT19937: 624 words of state, a
period of 2^19937-1, and the same stream C++'s `std::mt19937` produces for a seed.
It is seeded at setup from the system's true entropy source (`/dev/urandom` on
POSIX, `BCryptGenRandom` on Windows), so two runs differ; `seed` makes a stream
repeatable.

```ft
n u32 = std.random();                 // 32 random bits
die int = std.random.uniform(1, 6);   // 1..=6, both ends included
```

| Member | Meaning |
| ------ | ------- |
| `std.random()` | the next 32-bit word |
| `seed(n)` | reseed; the same seed is the same stream |
| `seed_from_entropy()` | reseed from the system source; returns whether it worked (`except`) |
| `uniform(low, high)` | the argument type chooses the distribution: an integer type gives `[low, high]` with rejection sampling and no modulo bias; a floating type gives `[low, high)` |
| `uniform_real(low, high)` | a `f64` in `[low, high)` |
| `normal(mean, deviation)` | the normal distribution, Box-Muller |
| `bernoulli(chance)` | `true` with probability `chance` |
| `exponential(rate)` | exponential distribution, mean `1/rate` |
| `lognormal(mean, deviation)` | log-normal: `exp` of a normal draw |
| `poisson(mean)` | Poisson; Knuth for a small mean, the normal approximation for a large one |
| `binomial(trials, chance)` | binomial, one Bernoulli draw per trial |
| `geometric(chance)` | trials up to and including the first success, mean `1/chance` |
| `choice(&values)` | one element, every element equally likely |
| `weighted_choice(&values, &weights)` | one element, proportional to the weights |
| `shuffle(&array)` | in-place Fisher-Yates over a `std.Array<T>` (fixed arrays must become one), every permutation equally likely |

`Random` defines `__call__`, so `std.random()` is one draw; a program that needs a
stream of its own declares `rng Random = Random(seed)` and leaves the global one
alone. The entropy source lives in `std/internal/random_entropy.ftm` and the platform file
`std/internal/random_entropy.windows.ftm`.

## The operating system: `std.os`

```ft
import std.os;

os.pid();                 // the process id
os.getenv("HOME");        // an environment variable; KeyError when absent (has_env asks)
os.set_env("LOG", "1");   // this process's environment only
os.exit(0);               // end the process: no destructor, no `finally` runs after it
os.cwd();                 // the working directory, as a Path
os.chdir(path);
os.temp_dir();            // TMPDIR, or the platform's own when unset
os.home();                // HOME / USERPROFILE, or the current directory
os.monotonic_ns();        // nanoseconds on the monotonic clock
os.sleep_ms(10);
os.platform();            // "linux" / "windows" / "macos": the build target, at run time
os.arch();                // "x86_64" / "aarch64": likewise
```

`std.os` is the process and the system; files and paths are `std.io` and `std.fs`.
`os.platform()` and `os.arch()` come from the compiler's builtin macros, so they
describe the **build target** rather than asking the running system.

## Cross-platform

One interface, one implementation per platform: a file named
`name.<platform>.ftm` is what the loader prefers for the target platform (see
[08 Modules](language/08-modules.md#where-a-module-is-found)). Threads, mutexes
and condition variables switch between `std/internal/sync_sys.ftm` (POSIX) and
`std/internal/sync_sys.windows.ftm` (Windows: SRW locks, condition variables, `CreateThread`);
the entropy source between `std/internal/random_entropy.ftm` and
`std/internal/random_entropy.windows.ftm`; the process calls between `std/internal/os_sys.ftm` and
`std/internal/os_sys.windows.ftm`. `std.os.platform()`, `$meta.WINDOWS` and `if constexpr`
leave only the live half of a program in the build.

The compiler is cross-platform the same way: its target is the system it runs on,
linking is driven by that system's C compiler (`CC` / `FALLC_LINKER`, `clang` by
default on Windows), Windows gets `.exe` by default, and position-independent
code is only asked for on POSIX. A Windows build needs an LLVM or MinGW toolchain
on `PATH` (the link line is GNU-flavoured). `FALLC_TARGET_OS` overrides the
platform for **checking**: on Linux,
`FALLC_TARGET_OS=windows fallc --check prog.ft` parses and type-checks the whole
Windows library (it does not link or run).

## I/O: `std.io`

### One `File` type, in the terminal's style

`std.io` has one type: `File`, a C stream plus the direction it was opened for. The
standard streams, files on disk and a stream another C library hands over are all the
same thing.

```ft
std.out    File   // standard output: File(C.stdout, OpenMode.WRITE)
std.err    File   // standard error
std.in     File   // standard input: File(C.stdin, OpenMode.READ)
```

Whether a handle can be read or written is a **run-time** question, exactly as it is of
a terminal:

```ft
std.out.readable()   // false
std.out.writable()   // true
```

Writing through a read-only file is therefore not a compile error but an `IOError` at
run time. The direction check moved to run time in exchange for one type that fits every
handle.

### Opening

```ft
f    File = File("notes.txt", OpenMode.WRITE)?!;   // "wb", truncates
src  File = File("notes.txt", OpenMode.READ)?!;    // "rb"
both File = File("notes.txt", OpenMode.RW)?!;      // "r+b", the file must exist
```

`OpenMode` is C's three modes. `File()` is an invalid handle with no stream; `open(path,
mode)` gives it one, and constructing with a path means "default-construct, then `open`".
A file that cannot be opened raises `IOError`.

A C stream that is already open can be wrapped -- which is how the standard streams
themselves are made:

```ft
log File = File(C.stderr, OpenMode.WRITE);
```

| Query | Meaning |
| ----- | ------- |
| `valid()` | there is a stream behind it |
| `readable()` | the mode allows reading, and there is a stream |
| `writable()` | the mode allows writing, and there is a stream |

### Writing

```ft
std.out.print("x = ", 42, ", y = ", 2.5);   // each argument as str(argument), no separator
std.out.write("text");                 // no newline
std.out.put('x');                      // one character
std.out.flush();
```

`write` has three overloads: `&const String`, `*const char`, and `(*const byte, usize)`,
each returning the number of bytes written. `put` returns `1`.

### Formatting: `print`, `println`, `input`

Three named members do the formatting, and their names say which way they go:

```ft
std.out.print("x = ", 42, '\n');   // each argument rendered by str, then written
std.out.println("done");            // the same, then a newline
std.in.input(n, word);              // one word per reference argument, through __input__
```

- `print` / `println` take **values**: text is text, a number is its digits, a character
  is itself, and a type declaring `__str__` is whatever that returns. There is no
  argument-count limit (it is a parameter pack, see
  [`language/04-functions.md`](language/04-functions.md#parameter-packs)).
- `input` takes **references**: an element is the caller's variable, and the word read is
  stored through the type's `__input__`. A parse failure raises `ValueError`; input that
  ends early raises `EOFError`.
- Because the method name fixes the direction, "printable" needs only `str` and
  "readable" only `__input__`, and both are checked at **compile time** -- there is no
  run-time branch. A type with only `__str__` can be printed and cannot be read, which is
  a compile error at the `input` call.
- The first argument is the **format**: `{}` is replaced by the following values in
  order, as in `std.print("{} + {} = {}\n", 1, 2, 3)`. A first argument with no `{}`
  keeps the older meaning: every value rendered by `str` and written after the first.
- The braces may carry a **spec** `{:spec}`, implemented in `std/fmt.ftm` (`std.io`'s
  `print` is that module with a file behind it):
  `[[fill]align][width][.precision][type]` -- `{:>8}` right-aligned, `{:<8}` left,
  `{:^8}` centered, `{:08}` zero-filled, `{:.2f}` two decimals, `{:x}`/`{:X}`/`{:b}`/
  `{:o}` for bases, and combinations such as `{:08.2f}`.
- A type decides its own text: one declaring `__format__` is rendered by it (the
  `Formattable` requirement), otherwise `str` is used.

Two **short forms** (free functions, equal to the members on the standard streams):

```ft
std.print("x = ", 42, '\n');   // same as std.out.print(..)
line String = std.input();       // same as std.in.getline(&line): the whole line
```

`std.input()` returns the line without its terminator; the end of the input raises
`EOFError` (a blank line is an empty `String`, not the end).


### Reading

```ft
c char = std.in.get();          // one character; raises at end of input
c char;
if (std.in.get(&c)) { }        // true when read, false at end
c char = std.in.peek();         // look, do not take; raises at end
if (std.in.eof()) { }           // the next character is the end
```

`get`/`peek` **raise** at the end of input; `get(&char)` is the checking form, returning
`false` there. `eof()` asks about the *next* character, not "did the last read fail".

A whole line is `getline`:

```ft
line String;
while (std.in.getline(&line)) {
    std.out.println(line);
}
std.in.getline(&line, ';');     // a different terminator
```

`getline` clears the destination first (it answers "what is the next line", not "what
else is on this one"), consumes the terminator without putting it in the text, and
returns `false` when there was nothing to read.

### Seeking

`seek(offset, from)` moves the position and returns the new one; `from` is
`SeekFrom.START`, `CURRENT` or `END`. A terminal or a pipe raises `IOError`. A file opened
with `OpenMode.RW` can be written and then read back from the beginning. `close` on a
standard stream only forgets the handle; it does not close C's stream.

## Synchronisation: `std.sync`

### `Mutex` and `Lock`

```ft
m Mutex;
{
    _guard Lock = &m;    // locking happens in the constructor
    ...
}                        // unlocking happens in the destructor
```

A `Mutex` can be `lock()`ed/`unlock()`ed directly, but `Lock` is preferred: it ties the
unlock to the scope. A `Mutex` is a value, not a global; sharing it means passing its
address to a thread.

### `CondVar`

```ft
ready CondVar;
m Mutex;
_guard Lock = &m;
while (!ready_now) {
    ready.wait(&m);      // releases the lock while waiting, holds it again on waking
}
ready.notify_one();      // or notify_all()
```

### `Thread` and `sleep`

```ft
fn work(argument *byte) *byte {
    std.sleep(10);       // milliseconds
    return argument;
}

t Thread = work;         // from a function pointer, with one argument
t.join();
```

A `Thread`'s construction and `join` may raise `ThreadError`. The entry type is
`*fn(*byte) u32`: the argument crosses as a `*byte`, and the `u32` is the thread's exit value.

### `Atomic<T>`

```ft
count Atomic<int> = 0;
count.fetch_add(1);
count.store(7);
v int = count.load();
```

`load`/`store`/`fetch_add`/`fetch_sub`/`exchange` are atomic. **The current
implementation uses a mutex internally**, so the results are atomic but it is no faster
than locking under contention; a genuinely lock-free one waits for a language-level
atomic builtin.

## Concurrency: `std.concurrent`

Three things ([`std/concurrent.ftm`](../../../standard/std/concurrent.ftm)):

- **`Promise<T>` / `Future<T>`**: one thread produces a value, another waits.
  A promise `set`s once (a second set is a no-op) and `Future.get()` waits for
  the value; `future.ready()` does not wait. The two share one reference-counted
  control block, so either may die first.
- **`Channel<T>`**: values in a queue. `send` appends and wakes a reader; `recv`
  waits on an empty queue and raises `ChannelClosed` when the channel is closed
  and empty; `try_recv()` returns `?T` without waiting; `close()` stops new
  values and wakes every waiter.
- **`ThreadPool`**: a fixed set of workers taking jobs from a queue. It must be
  `start()`ed after construction -- the constructor runs on a temporary, and a
  thread handed the temporary's address would read the moved object. `submit(
  task, arg)` queues a job and `shutdown()` lets the queue finish and stops.

```ft
import std.concurrent;

channel Channel<int> = Channel<int>();
producer_thread = Thread(producer, &context as *byte)?!;
value int = channel.recv()?!;

promise Promise<int> = Promise<int>();
future Future<int> = promise.future();
result int = future.get()?!;

pool ThreadPool = ThreadPool(4usize);
pool.start()?!;
pool.submit(task, &state as *byte);
pool.shutdown()?!;
```

A job is a `*fn(*byte) u32`; the caller owns the argument and keeps it alive
until the task has run. The thread entry is `std.sync.Thread`'s.

## Time: `std.time`

Three questions, three types ([`std/time.ftm`](../../../standard/std/time.ftm)):
`Duration` is an amount of time (nanoseconds), `Instant` is a point on the
**monotonic** clock (for measuring how long something took), and `SystemTime` is
a point on the **wall** clock (Unix-epoch nanoseconds, for saying when).

```ft
import std;
import std.time;

start Instant = time.now();
// ... work ...
took Duration = time.since(&start);
std.print("{} {} {}\n", took.nanos(), took.millis(), took)?!;  // 0.005s

d Duration = time.from_millis(1500);   // from_nanos / from_seconds too
stamp SystemTime = time.unix_now();
std.print("{}\n", stamp.to_utc())?!;   // 2026-10-20 16:54:58
```

`Duration` supports `+`/`-`/comparison and renders as `1.500s`;
`SystemTime.to_utc()` gives a `DateTime` (`year()`/`month()`/`day()`/`hour()`/
`minute()`/`second()` and the `YYYY-MM-DD HH:MM:SS` text); `time.sleep(&duration)`
sleeps. The platform differences live in `std/internal/time_sys.ftm` (POSIX `clock_gettime`)
and `std/internal/time_sys.windows.ftm` (`GetSystemTimeAsFileTime`).

## Networking: `std.net`

`std.net` provides IPv4 TCP ([`std/net.ftm`](../../../standard/std/net.ftm)):
`connect(&"127.0.0.1", port)` / `listen(&host, port, backlog)` (port zero asks the
system, `local_port()` reports it), `accept`, `write_all(&bytes)`,
`read_into(&bytes, max)` and `read_text(max)`; `TcpStream`/`TcpListener` close in
`__drop__` and can also be closed explicitly. The handle is one `i64` on both
platforms (a POSIX fd, a Windows `SOCKET`) and the platform modules are
`std/internal/net_sys.ftm`/`std/internal/net_sys.windows.ftm`. Only dotted-quad addresses and `localhost`
are supported: DNS, IPv6 and UDP are separate pieces of work, and this module
says so instead of quietly handling a part.

## Numerics, testing and debugging: `std.math` extensions, `std.complex`, `std.matrix`, `std.test`, `std.debug`, `std.prelude`

- **math extensions** ([`std/math.ftm`](../../../standard/std/math.ftm)):
  `hypot`/`cbrt`/`trunc`/`fract`/`sign`/`signum`/`lerp`/`degrees`/`radians`/`log2`/
  `log10`/`sinh`/`cosh`/`tanh`/`asinh`/`acosh`/`atanh`/`is_nan`/`is_infinite`/
  `is_finite`, and the `IsInteger`-constrained
  `gcd`/`lcm`/`isqrt`/`is_even`/`is_odd`/`is_power_of_two`. Real constants are
  functions (`pi()`/`e()`/`tau()`/`infinity()`/`nan()`): a module constant must be
  a constant expression, and a real literal is not one.
- **complex** ([`std/complex.ftm`](../../../standard/std/complex.ftm)): the value
  type `Complex {re, im}` with
  `add`/`sub`/`mul`/`div` (division by zero raises)/`conjugate`/`abs`/`arg`/
  `norm_squared`/`pow`, `==`/`!=`, and `__str__` (`a+bi`).
- **matrix** ([`std/matrix.ftm`](../../../standard/std/matrix.ftm)):
  `matrix`/`zeros`/`identity`, `rows`/`columns`/`at`/`set`,
  `add`/`sub`/`mul` (dimensions checked)/`scale`/`transpose`, `==`/`!=`, and
  `__str__`. Inversion and decompositions are **not** provided: an inverse
  without pivoting and a tolerance is wrong on the matrices that need it most, so
  the module names the gap instead of approximating it.
- **test** ([`std/test.ftm`](../../../standard/std/test.ftm)): the `$CHECK`,
  `$CHECK_EQ`, `$CHECK_EQ_REAL`, `$CHECK_EQ_TEXT` and `$FAIL` macros; a macro
  passes the condition text and `$__builtin_FILE`/`$__builtin_LINE` to an
  assertion function, and a failure raises `TestError` with the location. The
  macros end in `?!`, so they belong in an `except` function (a test `main`).
- **debug** ([`std/debug.ftm`](../../../standard/std/debug.ftm)):
  `$ASSERT_DEBUG`, `$DEBUG_UNREACHABLE`, `print_trace(&String)` (writes stderr,
  with no dependency on the io layer) and `breakpoint()` (`abort`). The macro is
  named `ASSERT_DEBUG` because `DEBUG_ASSERT` did not become visible through the
  umbrella's macro re-export during development.
- **prelude** ([`std/prelude.ftm`](../../../standard/std/prelude.ftm)): re-exports
  of the common modules. The language's import rule still requires the module
  name for functions (`prelude.print`); types such as `String` can be written
  directly. The value of the prelude is one import for the common types and
  entry points, not a change to name lookup.

## Checksums, hashes, UUIDs and sampling: `std.crc`, `std.crypto`, `std.uuid`, `std.rand_dist`

- **crc** ([`std/crc.ftm`](../../../standard/std/crc.ftm)): `crc8` (poly 0x07, init 0),
  `crc16` (CCITT-FALSE, poly 0x1021, init 0xffff) and `crc32` (ISO-HDLC, the CRC-32
  of PNG and zip), tested with the `"123456789"` vectors.
- **crypto** ([`std/crypto.ftm`](../../../standard/std/crypto.ftm)):
  `sha256`/`sha256_hex`, `hmac_sha256`/`hmac_sha256_hex` and `hex`, implemented from
  the specification and checked against the standard vectors (`"abc"`, the empty
  message, RFC 4231). It is explicitly not a cryptography library: no
  constant-time comparisons and no key management, and SHA-1/MD5 are absent.
- **uuid** ([`std/uuid.ftm`](../../../standard/std/uuid.ftm)): `v4()?!` (version and
  RFC 4122 variant bits set), `to_string()`/`__str__`, `parse_uuid(&text)?!`,
  `version()`, `variant()` and `==`/`!=`; two `u64` fields, so copying, comparing
  and returning one needs no ownership.
- **rand_dist** ([`std/rand_dist.ftm`](../../../standard/std/rand_dist.ftm)): the
  distribution names over `std.random.Random` -- `sample_uniform`,
  `sample_uniform_real`, `sample_normal`, `sample_exponential`,
  `sample_lognormal`, `sample_poisson`, `sample_binomial`, `sample_geometric` and
  `sample_bernoulli`. The `sample_` prefix exists because a free function whose
  first parameter is the receiver is reachable through method syntax: without it,
  `generator.exponential(rate)` resolved to the free function, which then called
  itself. `choice`/`weighted_choice` stay methods on `Random`.

## System interfaces: `std.process`, `std.term`, `std.dynlib`, `std.signal`, `std.compress`, `std.regex`

- **process** ([`std/process.ftm`](../../../standard/std/process.ftm)):
  `spawn(&program, &arguments)` starts a child (`argv[0]` is filled in) and
  `wait()` returns its exit code. POSIX forks and executes (a child killed by a
  signal answers `128 + signal`, as a shell does); Windows calls
  `CreateProcessA`.
- **term** ([`std/term.ftm`](../../../standard/std/term.ftm)): ANSI SGR --
  `color_fg`/`color_bg` over eight colours,
  `style_bold`/`style_dim`/`style_underline`/`style_reset`; `supports_color()`
  is false under `NO_COLOR`, a dumb `TERM`, or an output that is not a terminal.
- **dynlib** ([`std/dynlib.ftm`](../../../standard/std/dynlib.ftm)):
  `open(&path)` gives a `Library`, `symbol(&name)` a `*byte` address for the
  caller to cast to its own function type, and `close()`; POSIX uses
  `dlopen`/`dlsym`, Windows `LoadLibraryA`/`GetProcAddress`.
- **signal** ([`std/signal.ftm`](../../../standard/std/signal.ftm)): the `Signal`
  enum (HUP/INT/TERM/USR1) with `install`/`restore`/`signal_self`, on the C
  runtime's `signal`.
- **compress** ([`std/compress.ftm`](../../../standard/std/compress.ftm)):
  `adler32`, and `deflate_stored`/`inflate_stored` for zlib **stored blocks**;
  the other deflate block kinds raise `ValueError` instead of being guessed at.
- **regex** ([`std/regex.ftm`](../../../standard/std/regex.ftm)): a backtracking
  matcher for literals, `.`, `[a-z]`/`[^0-9]`, `\d`/`\w`/`\s`, `^`/`$` and
  `*`/`+`/`?`. Groups, alternation, back-references, lazy quantifiers and
  `{n,m}` raise at `compile` time. `find` is a `?Match`
  (`start`/`end`/`length`/`matched`); `is_match` is a bool.

## Tables and configuration: `std.csv`, `std.toml`, `std.xml`

- **CSV** ([`std/csv.ftm`](../../../standard/std/csv.ftm)): `parse_rows(&text)` gives
  `Array<Array<String>>` and `write_rows(&rows)` writes it back. The rule: comma
  separated, and inside a quoted field a comma, a newline and a quote stand for
  themselves, with `""` for one quote. Bad text -- an unclosed quote, a quote in
  the middle of an unquoted field -- raises `ValueError`; writing quotes a field
  only when it must.
- **TOML** ([`std/toml.ftm`](../../../standard/std/toml.ftm)): `parse_toml(&text)`
  gives a `TomlDocument`, a flat list of entries in file order. Comments,
  `[table]`, `key = value` and strings/integers/reals/booleans are supported;
  arrays, inline tables, dates and multi-line strings are refused with
  `ValueError` rather than guessed. `get(&table, &key)` is a `?TomlValue`, and a
  value is `as_string`/`as_int`/`as_real`/`as_bool`.
- **XML** ([`std/xml.ftm`](../../../standard/std/xml.ftm)): `parse_xml(&text)` gives
  an `XmlDocument` (an array of nodes plus child indices, the same ownership shape
  as JSON), and an `XmlValue` handle offers `name`/`text`/`child_count`/`child(i)`/
  `attribute_count`/`attribute_name`/`attribute_value`/`attribute(&key)`;
  `serialize` writes it back. Elements, attributes, text, the five named entities
  and numeric entities are supported, comments and the XML declaration are
  skipped; CDATA and DTD are not.

## JSON: `std.json`

`std.JsonDocument` owns the parsed tree (every node lives in one
`Array<JsonNodeData>` and children are indices), and `JsonValue` is only a small
"document plus index" handle, so copying or passing one is cheap; a handle must
not outlive its document ([`std/json.ftm`](../../../standard/std/json.ftm)):

```ft
document JsonDocument = std.parse(&text)?!;
root JsonValue = document.root();
key String = String("name");
name String = root.member(&key).value()?!.as_text()?!;
written String = std.serialize(&root)?!;
```

`kind()` returns `NULL_VALUE`/`BOOLEAN`/`NUMBER`/`TEXT`/`ARRAY`/`OBJECT`; an
array uses `length()`/`at(i)`, an object `length()`/`member(&key)` (a
`?JsonValue`, because a missing key is an absence rather than a failure) and
`member_key(i)`/`member_value(i)`; scalars use `as_bool`/`as_number`/`as_text`,
which raise `ValueError` on the wrong kind, plus `is_null()`. Parsing failures
raise `ValueError` too. A number is always an `f64`.

## Encodings and text helpers: `std.encoding`, `std.unicode`, `std.strings`

**Encodings** ([`std/encoding.ftm`](../../../standard/std/encoding.ftm)) turn bytes into
text that can travel; both sides are `Bytes`, because the bytes need not be text:

```ft
hex String = std.hex_encode(&data);        // "4142"
back Bytes = std.hex_decode(&hex)?!;        // a ValueError on bad text
b64 String = std.base64_encode(&data);      // "QUI="
pct String = std.percent_encode(&text);     // URL: alnum -_.~ stay, the rest is %XX
```

**Unicode** ([`std/unicode.ftm`](../../../standard/std/unicode.ftm)): `encode(code_point)`
gives UTF-8 bytes (refusing >U+10FFFF and surrogates), `decode(&text)` gives
`Array<u32>` (a `ValueError` on invalid UTF-8), `valid`/`count` answer "is it valid"
and "how many code points", and `next_code_point` returns `Pair<code point, byte count>`.

**Text helpers** ([`std/strings.ftm`](../../../standard/std/strings.ftm)):
`join(&parts, &sep)`, `repeat`, `replace_all`, `lines`, `pad_start`/`pad_end`,
`upper`/`lower` (ASCII).

## Range tools: `std.iter` and `std.range`

`std.iter` ([`std/iter.ftm`](../../../standard/std/iter.ftm)) is the small moves a
loop makes on a pair of positions, each one template over the iterator type:
`advance`/`skip`/`take`/`nth` move a position and
`find_if`/`count_if`/`all_of`/`any_of`/`none_of` take a predicate (a type with
`__call__`, such as the provided `Even`/`Odd`):

```ft
first *int = std.find_if(values.begin(), values.end(), even);
n usize = std.count_if(values.begin(), values.end(), odd);
cursor *int = std.advance(values.begin(), values.end(), 2usize);
```

`std.range` ([`std/range.ftm`](../../../standard/std/range.ftm)) gives integer
ranges: `to(n)` (0..n), `up_to(low, high)`, `down_to(low, high)` and
`step_range(low, high, step)` (a zero step raises `ValueError`). `IntRange` is
both the container and its own position, so `for (value i64 : std.to(5i64))`
works; the stop is **exclusive**, and a step that does not divide the span ends
at the first position past the stop.

## Grid: `std.grid`

`Grid<T>` ([`std/grid.ftm`](../../../standard/std/grid.ftm)) is a row-major
two-dimensional array: one `Array<T>`, with element `(row, column)` at
`row * columns + column`. The constructor takes a filler because the element type
need not have a default constructor:

```ft
grid Grid<int> = Grid<int>(2usize, 3usize, 0);
grid.set(1, 2, 7);
v int = grid.at(1, 2);
rows usize = grid.rows();
```

`rows`/`columns`/`length`/`empty`, `at` (writable and read-only), `set`, `fill`
and `==`; an out-of-range `at` panics.

## Logging: `std.log`

A log line is `[LEVEL] text`, written to standard error by default. The level
filter is applied **before the text is built**, so a disabled `debug` line costs
nothing ([`std/log.ftm`](../../../standard/std/log.ftm)):

```ft
import std;
import std.log;

log.info("starting with {} items", count)?!;   // [INFO] starting with 3 items
log.set_level(log.Level.DEBUG);
log.debug("value = {}", value)?!;
log.set_level(log.Level.OFF);                  // silence everything
```

The levels are `TRACE`/`DEBUG`/`INFO`/`WARN`/`ERROR`/`OFF`; `set_level`/`level`/
`enabled`/`level_name` query them, and `set_target(&file)` sends the lines
elsewhere. The text rule is `std.print`'s (`{}` takes the following values), and
the logger supplies the newline. Writing is not synchronised here: a program
that needs whole lines from several threads calls the logger inside its own
mutex.

## File paths: `std.fs`

```ft
p Path = "/home/user/notes.txt";
p.directory();     // "/home/user"
p.file_name();     // "notes.txt"
p.extension();     // "txt"
p.join(other);     // joined, with a separator added when needed
p.exists();        // whether the file system has it
p.file(OpenMode.READ);  // open the file this path names, as a std.File
p.remove();        // whether it went away
p.rename(other);   // whether the system agreed
```

A `Path` owns its text (a `String` inside), so copy, move, assignment, and destruction
are all there; `text()` gives `*const char`, `data()` gives `&const String`, and
`__str__` gives a `String`. `directory`/`file_name`/`extension` are string operations
only -- they do not touch the file system and do not validate anything; `exists()`,
`remove()`, and `rename()` are the calls that ask the system.

## Compile-time facts: `meta`

`meta` is not part of `std`: it is the compiler's macros, and a program imports it on
its own.

```ft
import meta;
std.out.println("built from ", $meta.FILE, " on ", $meta.PLATFORM);
if ($meta.WINDOWS) @constexpr { ... } else { ... }
```

| Macro | Expands to |
| ----- | ---------- |
| `$meta.FILE` | the current file's path, as a string |
| `$meta.LINE` / `$meta.COLUMN` | the line / column of the macro, as an integer |
| `$meta.PLATFORM` | the target platform: `linux`, `windows`, `macos` |
| `$meta.ARCH` | the target architecture: `x86_64`, `aarch64` |
| `$meta.WINDOWS` / `$meta.POSIX` | `1` or `0`, directly usable in `if constexpr` |
| `$meta.DATE` / `$meta.TIME` | the build's date `YYYY-MM-DD` and time `HH:MM:SS` (UTC) |

Macros expand before parsing, so they stand anywhere a token may. They wrap the
compiler's builtin macros (`$__builtin_FILE` and friends), which are not part of the
language proper. A platform-specific library implementation lives in a
`name.platform.ftm` file, which the loader prefers for the target platform -- see
[08 Modules](language/08-modules.md#where-a-module-is-found).

## Limitations

- **`Atomic<T>` is not lock-free**, as described above.
- **Hashed containers have no order**, and a tree's entries with equal keys have no
  stable order among themselves. See the sections above.
- **`Path` does not resolve `..` or `.`.** It splits on separators; normalisation is a
  different operation.
- **A `File`'s direction is a run-time question.** Writing a read-only file, reading a
  write-only one and seeking a terminal all raise at run time; a program that wants the
  direction checked at compile time wraps it in a type of its own.
