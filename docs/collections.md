# `std.Array` and `std.String`

> [中文](collections.zh.md)

The standard library's two containers. They own their memory, which is why most of this
document is about **who is responsible for which piece of memory**.

- [Getting hold of them](#getting-hold-of-them)
- [`Array<T>`](#arrayt)
- [`String`](#string)
- [Iteration](#iteration)
- [Move: construction and assignment](#move-construction-and-assignment)
- [References and pointers](#references-and-pointers)
- [Multiple files](#multiple-files)
- [Limitations](#limitations)

## Getting hold of them

```ft
import std;              // umbrella module: std.Array<T>, std.String, std.Box<T>
import std.array;        // only Array
import std.string;       // only String (Exception is here as well)
import std.memory;       // memory, panic, Box, and the fake/move probes
import std.algorithm;    // algorithms: sort, reverse, find, fill...
```

The examples below print with `C.printf`, so they also need `import C;`: `std` does not
forward the C bindings -- the two libraries are separate.

The **type names** an umbrella re-exports can be written short:

```ft
import std;
fn main() int {
    xs Array<int>;       // the same type as std.Array<int>
    s String;
    return 0;
}
```

**Functions cannot**: after `import std;` write `std.out.print(s)`, not `print(s)`. Why is
in [08 Modules](language/08-modules.md#short-names-type-names-and-value-names-are-not-the-same).

### The other entry points

The names the language reference mentions that are neither `Array` nor `String` all live in
`std`:

| Name | Signature | Purpose |
| ---- | --------- | ------- |
| `std.panic` | `(*const c8) void` | the message on standard error, then abort |
| `std.alloc` | `(usize) *byte` | `malloc`; may return `nullptr` |
| `std.allocate` | `(usize) *byte` | the same, but `panic`s when it fails |
| `std.calloc` | `(usize, usize) *byte` | zeroed allocation |
| `std.realloc` | `(*byte, usize) *byte` | reallocate |
| `std.free` | `(*byte) void` | release |
| `std.move` | `<T> (&T) &&T` | hand a named value to a constructor as a dying value: `b String = std.move(a);` |
| `std.fake` | `<T> () T` | only in compile-time queries (`except(std.fake<&T>().__drop__())`); calling it at run time aborts |
| `util.__ft_moves` | `<T> () bool` | the compile-time question "can moving a `T` raise?", which `Array` asks to choose between moving and copying |
| `std.Box<T>` | `__init__(&Box<T>)`, `__init__(&Box<T>, T)`, `get()`, `empty()`, `clone()` | one value on the heap, freed on destruction; **move** semantics, use `clone()` to copy |
| `std.Exception` / `std.exception()` | `name()` and `what()`, each returning a `String` | the exception being handled, see [06 Exceptions](language/06-exceptions.md) |

**Module names collide**: an imported name is looked up as a file on the search path by
module name, so a module of your own called `math` collides with
`standard/std/math.ftm`, and the symptom is usually
``module `math` does not export `square` ``. The fix is to rename your own module, or to
use a name with a directory such as `import geom.math;`.

## `Array<T>`

`Array<T>` is a family of types, one member per element type: `Array<int>` holds `int`,
`Array<Point>` holds `Point`. The index is an **element subscript**, not a byte offset.

```ft
import std;
fn main() int {
    xs Array<int>;
    xs.push(3);
    xs.push(4);
    xs.push(5);
    return xs[0] + xs[2];        // 8
}
```

### Construction

| Form | Meaning |
| ---- | ---- |
| `a Array<int>;` | Empty array, allocates nothing (capacity 0) |
| `b Array<int> = Array<int>(3, 7);` | Three 7s |
| `c Array<int> = a;` | **Copy** construction: the elements are copied one by one, the buffer is new |
| `d Array<int> = make();` | **Move** construction: takes over `make()`'s buffer |

The layout of `Array<T>` is "buffer pointer + length + capacity", so

```ft
sizeof(Array<int>)      // 24
sizeof(*int)            // 8, one pointer
```

### Elements, length, capacity

| Member | Description |
| ---- | ---- |
| `length()` / `capacity()` | Number of elements / allocated capacity |
| `empty()` | `length() == 0` |
| `data()` | `*const T`, lends out the buffer; invalid once the array is changed or destroyed |
| `a[i]` | Element reference: `&T` (`&const T` on a const array) |
| `front()` / `back()` | First/last element reference; an error on an empty array |
| `push(v)` | Appends at the end |
| `pop()` | Pops the last element and **returns** it (by value) |
| `insert(pos, v)` / `erase(pos)` | Insert / erase in the middle, O(n); `pos` is an iterator (`begin() + i`), not a subscript |
| `reserve(n)` / `shrink_to_fit()` | Reserve / shrink to just enough |
| `resize(n, filler)` / `clear()` | Change the length / empty it |

### Algorithms

Sorting, searching, reversing and filling live in
[`std/algorithm.ftm`](../../standard/std/algorithm.ftm) as **free functions**, not
members of `Array` -- the C++ arrangement: a container owns its storage, and an
algorithm works on a **range** (the pair `begin()`..`end()`).

Each algorithm is **one template over the iterator type**, not one overload per
container: the body uses only the operations the language gives a position (`++`,
`*`, `==`/`!=`, and `+` where it needs random access), so an `Array`'s `*T`
positions, a `String`'s bytes and a `Deque`'s iterator all call the same
`std.sort`. `@enable_if` writes the requirement down: `sort`/`reverse` ask for
`RandomAccessIterator`, `is_sorted`/`fill`/`swap` for `ForwardIterator`, and
`find`/`index_of`/`count_of` for `InputIterator`.

```ft
import std;
fn main() int {
    values Array<int>;
    values.push(3);
    values.push(1);
    values.push(2);
    descending Greater<int>;
    std.sort(values.begin(), values.end());            // 1 2 3
    std.sort_by(values.begin(), values.end(), descending); // 3 2 1
    return 0;
}
```

| Free function | Meaning |
| ------------- | ------- |
| `sort(first, last)` | orders by `<`; heap sort, `O(n log n)` worst case, no allocation |
| `sort_by(first, last, order)` | orders by a comparator value, e.g. `Greater<int>` |
| `is_sorted(first, last)` / `is_sorted_by(first, last, order)` | whether the range is ordered |
| `reverse(first, last)` | in-place reversal |
| `fill(first, last, value)` | writes `value` in every slot |
| `index_of(first, last, value)` / `count_of(first, last, value)` | first equal position (`-1` if absent) / how many equal |
| `find(first, last, value)` | position of the first equal element, or `last` |
| `swap(first, second)` | exchanges the elements at two positions through copy, assign and move |

A container whose position is a class (`Deque`) calls the same algorithms with its
iterator pair: `std.sort(d.begin(), d.end())`, `std.is_sorted`, `std.reverse`,
`std.fill`, `std.index_of`, `std.count_of`.

A swap runs through the element type's own copy and move, so an `Array<String>`
still owns exactly one buffer per element after sorting.A swap runs through the element type's own copy and move, so an `Array<String>`
still owns exactly one buffer per element after sorting.

### Elements are constructed, not copied in

A slot is **raw memory** until the first value is put into it. Building a value in one
calls the element type's `__init__(value)`; assigning to an element that is already
alive calls `__assign__`; destroying calls `__drop__`. That is how `Array`'s own
implementation is written:

```ft
slot *T = this._data + this._len;
slot->__init__(value);          // construct in the raw slot
```

Primitive types need not declare the three — the compiler gives types such as `int`
built-in versions: `__init__`/`__assign__` are a single store and `__drop__` does
nothing. So the same code holds for `Array<int>` and for `Array<String>`.

The result is that **`Array<String>` and `Array<Array<int>>` copy deeply**: each element
has its own buffer, and changing the source does not affect the element.

```ft
import std;
fn main() int {
    a String = "hello";
    xs Array<String>;
    xs.push(a);                  // the buffer in the element is another one
    a.push('!');                 // change the source
    return xs[0][0] as int;      // 104, 'h': the element is still "hello"
}
```

Nested template arguments **need no space**: the `>>` in `Array<Array<int>>` is split
into two `>` in a type argument list; a `>>` between two values is still a right shift.

## `String`

`String` is three words (`sizeof(String)` = 24) and always has a `'\0'` at the end, so
`data()` can be handed to C directly.

**Growth moves elements**: when `Array` grows (`reserve`/`shrink_to_fit`) and when it
does `insert`, it moves elements across rather than copying them if the element type's
move construction cannot raise — a `String`'s buffer therefore changes hands in place,
at the same address. The compiler answers the question at instantiation
(`util.__ft_moves<T>()`), so each element type chooses for itself; a move construction
that can raise falls back to a copy, so that an exception does not find half the
elements of the old buffer already moved out. `if constexpr` decides which branch
exists.

**Allocation failure is not an exception**: `push`, `insert`, `reserve`, `resize`,
`shrink_to_fit`, `+=`, `+` and `*` are not marked `except`. When memory runs out they go
to `std.panic` (abort with a message) — a failed allocation has no value to hand back
and nowhere to recover to, and making it a caller's obligation would only wrap every
`push` in a `try`. The exception system is left for errors the program `raise`s itself.

**Short strings do not allocate (SSO)**: text of no more than 22 bytes is stored
directly in the `String` object's own three words — the high bit of the last byte marks
it "inline", the remaining bits store the length. Such a string touches no allocator
when it is created, copied or destroyed; `capacity()` reports the capacity the object
itself provides (24, terminator included).

```ft
import std;
fn main() int {
    small String = "hi";                 // inline, no malloc
    large String = "this text is long enough to allocate for sure";
    C.printf("%lu %lu\n", small.capacity(), large.capacity());   // 23 46
    small.shrink_to_fit();               // already inline, does nothing
    large.clear();
    large.push('a');
    large.shrink_to_fit();               // short enough for the object, so it moves back in
    C.printf("%lu\n", large.capacity());  // 23
    return 0;
}
```

This program prints `23 46`, then `23`: a `String` holds twenty-two bytes inside the
object itself, so `"hi"` never allocates, and after being emptied and shrunk the long one
moves back in.

| Member | Description |
| ---- | ---- |
| `length()` / `capacity()` / `empty()` | `capacity()` means the same in both forms: the number of bytes it can hold (terminator included) |
| `data()` | `*const char`, a C string; never null (an empty string has a terminator of its own) |
| `s[i]` | `&char`, readable and writable |
| `push(ch)` / `push(&const String)` / `push(*const char)` | Append a character / a string / a C string |
| `insert(i, ch)` / `insert(i, &const String)` | Insert in the middle |
| `erase(i)` | Remove the `i`th character and **return** it |
| `reserve` / `shrink_to_fit` / `resize` / `clear` | As in `Array` |

Built-in operators:

```ft
import std;
fn main() int {
    a String = "ab";
    b String = "cd";
    c String = a + b;            // __add__, "abcd"
    a += '!';                     // __iadd__, "ab!"
    d String = b * 2;             // __mul__, "cdcd"
    say String = str(c);          // __str__, for people and for `std.out`
    return c.length() as int;     // 4
}
```

Comparison (`==`, `<` and so on) takes `&const String`, so either side may be a
temporary or a name. `repr` is for debugging and is quoted; `str` is not.

### Searching and splitting

The text's own queries are member functions, and every argument is `&const String`, so a
name and a temporary both work:

```ft
import std;
fn main() int {
    text String = "  hello world  ";
    where isize = text.find(String("world"));      // 8
    missing isize = text.find(String("zzz"));      // -1
    has bool = text.contains(String("hello"));     // true
    starts bool = text.starts_with(String("  he"));
    ends bool = text.ends_with(String("  "));
    trimmed String = text.trimmed();               // "hello world"
    piece String = text.substring(2, 5);           // "hello"
    return 0;
}
```

`find` returns `isize`, because "not there" has to be a value no position can take; ask
`contains` when only the answer matters. `substring(start, count)` clamps the range to the
text: asking past the end gives what is there, and a start past the end gives an empty
string -- a reader asking for "the last line" of a file that ends without a newline asks
for more than there is.

`split(separator char, into &Array<String>)` **appends** each piece to the array. Between
two separators, before the first, and after the last all count as pieces, even when empty:
so `"a,b,"` is three pieces and the empty string is one:

```ft
parts Array<String>;
String("a,b,,c").split(',', parts);    // ["a", "b", "", "c"]
```


### Conversions

Reading text as a number goes through `as`: `String` provides `__as__` conversions to
`int`, `usize` and `f64`. The **whole** text must be that number: `"12x"`, `""` and
`"1 2"` raise `ValueError`, and the integer conversions refuse a value that does not
fit instead of wrapping. A conversion may fail, so the call site follows the ordinary
exception rules (`??` to pass it on, or `try`/`except` to handle it):

```ft
n int = String("-42") as int??;            // -42
size usize = String("100") as usize??;     // 100
real f64 = String("2.5e2") as f64??;       // 250.0
```

`std.in.input(n)` uses the same conversion, so a failed input raises `ValueError`.
To ask *whether* the text is a number instead of requiring one, catch the
`ValueError` with `try`/`except`, or check the text before converting.

## Iteration

`Array<T>` and `String` can both be walked directly by `for (x T : ...)`: they provide
`begin()`/`end()`, and the iterators they return provide `__inc__`, `__deref__` and
`__ne__`. Those are the language's five operations and there is nothing else —
`std/iterator.ftm` merely gives them names (`Iterable`, `InputIterator`), and the
compiler does not require you to import any trait to write a loop.

```ft
for (x int : xs) {          // a copy
    C.printf("%d\n", x);
}
for (x &int : xs) {         // the element itself: writes back to the array
    x = x * 2;
}
for (c char : s) {          // String's bytes
    C.printf("%c", c);
}
```

An iterator **is a pointer**: `Array<T>`'s `begin()`/`end()` return `*T` and `String`'s
return `*char`. The elements are contiguous in memory, so the "position" is itself an
address, and `++`, `*`, `!=`, `[]` and comparisons are all the language's pointer
operations, with no wrapper type to write. `std/iterator.ftm` files them at the top of
the `Iterable → InputIterator → ForwardIterator → BidirectionalIterator →
RandomAccessIterator` chain.

These types each have a name, and the names are in the container:

| Name | What it is |
| ---- | ------ |
| `Array<T>.Iterator` | `*T`: forward, writable |
| `Array<T>.ConstIterator` | `*const T`: forward, read-only |
| `Array<T>.ReverseIterator` | A reverse position wrapping a `*T`; `++` walks backwards |
| `Array<T>.ConstReverseIterator` | As above, read-only |

The reverse iterator follows the C++ convention: it **points after** the element it
yields, so `rbegin()` is where a forward walk ends, `*it` is the previous element, and
`++it` walks towards the beginning. It is a dozen or so bytes and one pointer
subtraction.

```ft
it Array<int>.ReverseIterator = a.rbegin();
for (; it != a.rend(); ++it) {
    C.printf("%d ", *it);
}
```

### Modifying by position

`insert` and `erase` take a **position** (that is, an iterator), not a subscript:

```ft
a.insert(a.begin() + 1, 9);   // insert before the second element
gone int = a.erase(a.begin()); // remove the first, returning it
```

`String` takes both: `s.insert(3, 'x')` uses a byte subscript and
`s.insert(s.begin() + 3, 'x')` uses a position; the same for `erase`. A program usually
has a subscript in hand, whereas loops and iterators give positions.

Two rules follow from this:

- **Do not grow** the container while walking it — growth replaces the buffer, and the
  pointer then points at the old one;
- You **may** change elements while walking it: neither the buffer nor the positions of
  the elements have moved.

`begin()`/`end()` accept only a writable container (`this &Array<T>`, `this &String`):
walking a `&const Array<T>` needs a const iterator set, which does not exist yet. A
short string with inline storage (its bytes are in the object) is walked through the
object itself, so it must not be made longer during the walk either.

## Move: construction and assignment

Whether a value is "moved" rather than "copied" depends only on **whether it is dying**:
a call's result and a constructed temporary are dying; a named variable is not.

```ft
import std;

fn make() String {
    s String = "hello, this is long enough to need a heap buffer";
    return s;                     // s moves out of the function
}

fn main() int {
    a String = "named";
    b String = a;                 // copy: a is still alive
    c String = make();            // move: takes over make's buffer
    d String; d = make();         // move assignment: d's old buffer is freed first
    a = b;                        // copy assignment
    return 0;
}
```

The rule is: **a dying argument selects the `&&` form first, otherwise the `&const`
form**. This holds for constructors and for `__assign__` alike — `d = make()` takes move
assignment and `a = b` takes copy assignment. That is why in `Array`/`String` the
copying form of `__assign__` is declared **before** the moving form, in keeping with
this rule.

After a move the source is **empty**: the buffer is null, the length 0, the capacity 0.
That state is still a valid value: `data()` returns `""`, `push` can still be called,
and it can be destroyed normally (freeing null is safe).

## References and pointers

The language keeps these two apart, and the containers' interfaces are written
accordingly:

- `&x` yields a **pointer**, `*T`, never a reference;
- a reference (`&T`) appears only in the way a type is written, meaning "this name is
  that storage", with no `&` applied;
- `__getitem__` returns `&T`, so `a[i] = v` works directly and there is no
  `__setitem__`:

```ft
import std;
fn main() int {
    xs Array<int>;
    xs.push(1);
    xs[0] = 7;                    // the element is &int, and takes an assignment
    return xs[0];                 // 7
}
```

For the same reason, the reference `front()`/`back()` return can be assigned to
directly, while `pop()`/`erase()` **return by value** — they take the element out of the
array, and the array no longer owns it.

A method can also be called directly on the result of an expression, for which the
compiler arranges temporary storage:

```ft
import std;
fn main() int {
    xs Array<String>;
    a String = "hello";
    xs.push(a);
    return xs.pop().length() as int;    // 5, pops once
}
```

When the element is a structure, `a[i].field` works directly — `a[i]` is that element
itself, not a copy of it:

```ft
import std;
type Point { x int; y int; }
fn main() int {
    ps Array<Point>;
    first Point = .{1, 2};
    ps.push(first);
    ps[0].x = 10;                 // writes the element's own field
    return ps[0].x + ps[0].y;     // 12
}
```

## Multiple files

When a module exports a type, its special methods have to be exported along with it, or
no other module can call them:

```ft
// thing.ftm
export type Thing { v int; }
export fn make_thing(v int) Thing {
    return .{ v };
}
export fn __drop__(this &Thing) void { }
```

```ft
// main.ft
import C;
import thing;
fn main() int {
    t Thing = thing.make_thing(9);
    C.printf("%d\n", t.v);
    return 0;
}
```

A name that is not exported is the module's own affair —
`__init__`/`__assign__`/`__drop__` included. A missing export is no error; the user
simply "takes the type to have no destructor".

## Limitations

These are things that cannot be done now, not bugs waiting to be fixed:

- **`insert` and `erase` take a position (an iterator), not a subscript**:
  `a.insert(a.begin() + 1, v)`. Giving an array a subscript is taken as an address, and
  the failure is at run time rather than at compile time. `String` is the exception:
  `s.insert(1, 'X')` and `s.erase(1)` both take a subscript.
- **`.{ ... }` cannot be a `push` argument directly** (`xs.push(.{1, 2})` says "the
  context has to give the type"). Bind a name first and then push; `a[i].field = v` and
  `a[i] = v` both work.
- The pointer `data()` lends out is invalid once the array is changed, and the language
  does not check this.
- Iterating needs a **writable** container: `begin()` has no `&const` overload, so `for`
  cannot walk a `&const Array<T>`.

For finer detail on templates and compile time (parameter packs, folds, `sizeof...`) see
[`docs/templates.md`](templates.md); for design questions not yet settled see
[`docs/SPEC-STATUS.md`](SPEC-STATUS.md).
