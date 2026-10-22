# Template parameter packs: fold, count, element selection

> [中文](templates.zh.md)

`...` has three uses in a template, and the same name `values` can be read in all three
places:

```ft
template <values ...int>
@constexpr fn sum() int {
    return (values + ...);          // fold: combines the elements in turn
}

template <values ...int>
@constexpr fn count() usize {
    return sizeof...(values);       // count: how many elements are in the pack
}

template <values ...int>
@constexpr fn second() int {
    return values[1];               // element selection: the subscript must be constant
}
```

Type packs work the same way: the `Args` of `template <Args ...type>` can be expanded
into another template's arguments (`Box<Args...>`), and `sizeof...(Args)` counts them.

## Fold

A fold has **four forms**, the same as in C++. The two unary forms take only the
elements, and the two binary forms also take a starting point; each pair differs in
**which end the combining starts from**:

With `values` being `<1, 2, 3>`, `init` being `100`, and `op` being `-`:

| Form | Association order | Expansion | Result |
| ---- | -------- | ---- | ---- |
| `(values - ...)` | Right to left: `e1 op (e2 op (… op eN))` | `1 - (2 - 3)` | `2` |
| `(... - values)` | Left to right: `((e1 op e2) op …) op eN` | `(1 - 2) - 3` | `-4` |
| `(values - ... - 100)` | Right to left, combining with `init` last | `1 - (2 - (3 - 100))` | `-98` |
| `(100 - ... - values)` | Left to right, starting from `init` | `((100 - 1) - 2) - 3` | `94` |

- **Any binary operator will do**; the operator decides "how to combine" and the form
  decides "which end to start from".
- The binary forms' **starting point is an expression**, which must be constant; it is
  read as a **unary expression**, so `(a + b + ... x)` has to be written
  `((a + b) + ... x)` — the `+` is the fold's own operator, not one inside the starting
  point.
- **Empty pack**: a unary fold needs an identity element, and the language gives only
  two — `&&` is `true`, `||` is `false`; the other operators (`+`, `-`, `*`, …) have no
  identity, and `sum<>()` reports "folding it has no value". An empty pack in a binary
  fold is simply its starting point: `(v + ... + 0)` with an empty pack is 0.
- The elements must be **values**: `(Ts + ...)` on `template <Ts ...type>` reports "its
  elements are types", because there is no number in a type.

The parentheses are part of the fold and cannot be dropped: in `values + ...` the `...`
is not an operand, and only the parentheses say "from here to there is a fold".

## Count `sizeof...(name)`

Returns `usize`, the **number of elements**, not a byte count — a pack has no memory
layout, and its length is the only thing there is to ask.

`sizeof...` and `sizeof(T)` are two different things, told apart by the `...`: the first
counts a pack, the second measures a type. An empty pack counts as **0** and is not an
error: counting needs no identity element.

Because the result is a compile-time constant, it can be an array length directly:

```ft
template <values ...int>
@constexpr fn buffer() int {
    a [sizeof...(values)]int;       // three elements are 3 i32s
    a[2] = 5;
    return a[2];
}
```

## Element selection `name[index]`

The subscript must be a **constant integer**, because the elements exist only at compile
time and nothing at run time can name them:

```ft
values[1]                         // a constant, fine
values[sizeof...(values) - 1]     // a constant expression, fine: the last element
values[i]                         // i is a run-time parameter → error
```

An out-of-range subscript is reported at compile time, together with how many elements
the pack actually has. What comes out is still the element's original type (on
`template <values ...int>` it is `int`).

## What a value parameter is in a function body

`values` in `template <values ...int>` **is not a variable**: it has no storage and no
address; it is replaced by a set of numbers at instantiation. So:

* it can be used only where compile time can answer: folds, counts, constant subscripts,
  and constant expressions;
* wherever `values[0]` appears, that number's literal value appears there.

(The same holds for a single value parameter such as `template <N int>`.)

## Usable in constant positions

A call to a `constexpr` template is a constant, so it can stand directly where a
constant is required:

```ft
template <values ...int>
@constexpr fn sum() int {
    return (values + ...);
}

fn main() int {
    a [sum<1, 2, 3>()]int;          // six ints
    x int = 0;
    if (sum<1, 2>() == 3) @constexpr {
        x = 7;                      // only this branch is generated
    }
    return x;
}
```

A position that requires a constant (an array length, an `if constexpr` condition)
**checks the expressions inside it first** and folds afterwards — which is exactly the
step that gives a template call time to be instantiated.

The precondition is that what the body has to work out is **expression-level**: a fold,
`sizeof...`, literals and arithmetic on them are reduced by the compiler, so
`return (values + ...);` can stand in a constant position. A template body with
statements -- bind a local, then add into it in a loop -- is not a constant expression
yet, and the compiler says it is not a function it can evaluate. Write the fold, or call
it as an ordinary function.

## Function parameter packs: a different thing

Everything above is about **template parameter packs**: `template <values ...int>` is a
series of compile-time values, one name standing for a series. A function can also have a
**parameter pack**, which is a series of **run-time arguments**:

```ft
template <Ts ...type>
fn total(values ...Ts) int {
    sum int = 0;
    (sum += values, ...);
    return sizeof...(values) as int + sum;
}
```

The spellings look alike, the meanings are opposite: a template parameter pack is given
at the call site (explicitly or deduced) and there is one per call; a parameter pack is
determined by **how many arguments there are**, and each count expands into its own
concrete function (the symbol carries the element types and the count). So
`sizeof...(values)` here counts "how many were passed this time", not "how many the
template listed". The full rules for parameter packs -- reference elements with
`...&Ts`, argument-position expansion `f(values...)`, the statement-only comma fold --
are in [`04-functions.md`](language/04-functions.md#parameter-packs).

## Current limitations

* **A template body with statements is not a constant expression yet** (see above).
  Calling it as an ordinary function is fine; it just cannot stand in a position that
  requires a constant.
* **The operand of `sizeof...` must be a pack**. Where there is neither a template
  parameter pack nor a parameter pack, `sizeof...(x)` says
  `x is not a template parameter pack`.

## Across modules: special methods need `export` too

When a module exports a type, that type's `__init__` / `__assign__` / `__drop__` have to
be `export`ed along with it, or no other module can call them — this is not a special
case for destructors but the visibility rule that every name follows. `std.String` is
written exactly so:

```ft
export fn __drop__(this &String) void { ... }
```

Export only `type Thing` without exporting its `__drop__` and the user takes the type to
have no destructor, so it is not called (and no error is reported). Both of std's
containers do export theirs, so destruction runs normally in any program that does
`import std;`.

## Relation to `Array`/`String`

`std.Array<T>` / `std.String` are ordinary template types; for how they are written see
[`docs/collections.md`](collections.md). A nesting such as `Array<Array<int>>`
needs no space: the `>>` in a type argument list is split into two `>`; a `>>` between
two **values** is still a right shift.
