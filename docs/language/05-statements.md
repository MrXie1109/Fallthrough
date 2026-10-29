# 05 Statements

> [中文](05-statements.zh.md)

## Scope

This volume specifies the forms and semantics of statements: blocks, conditions, loops
(including the range form), `switch`, jumps, deferred blocks and compile-time
assertions. Statements related to exceptions are given in [06 Exceptions](06-exceptions.md).

## Statement forms

2.1 Statements include: bindings, assignment, compound assignment, expression statements,
bare blocks, `if`, loops, `switch`, jump statements, `defer` and `static_assert`.

2.2 Any expression that has a value may be used as an expression statement; its value is
discarded.

2.3 A bare block `{ … }` introduces a scope; on leaving the scope, the bindings in it are
destroyed.

2.4 The head of a condition or loop **must** be parenthesized: `if (...)`,
`while (...)`, `for (...)`, `switch (...)`.

2.5 A control-flow block **must not** be followed by a semicolon; a declaration and an
expression statement end with `;`.

## Conditions

### Truth values

3.1.1 The condition of `if`, `while` or `for` accepts the following types:

| Operand type | Truth value |
| ---------- | ---- |
| integer, floating point, character | true when the value is non-zero |
| `bool` | its value |
| pointer | true when non-null |
| reference | true when the referenced value is true |
| array | always true |
| function | always true |
| any other nominal type | requires the declaration of `__bool__` |

3.1.2 A condition reads the truth value of the operand; it does not convert the operand
to `bool`. This adjustment belongs to the implicit adjustments listed in Section 12 of
[02 Types](02-types.md).

3.1.3 The result type of `!x` is `bool`, and it accepts the same operands as a
condition.

### `if (cond) @constexpr`

3.2.1 The condition of `if (c) @constexpr` **must** be a constant.

3.2.2 When the condition is false, the `else` branch is selected.

3.2.3 The unselected branch is **not checked**, and no code is generated for it.

3.2.4 An ordinary `if` checks both branches; when the condition happens to be constant,
only the selected branch is generated, but the other branch is still checked.

### Branch weights

3.2.5 After an `if`, `else` or `case`, a branch-weight attribute may be written:
`@likely` says the branch is usually taken, `@unlikely` says it rarely is.

```ft
import C;

fn run() int { return 1; }
fn wait() int { return 2; }

fn classify(code int) int {
    switch (code) {
        case 200 @likely:
            return 1;
        default:
            return 0;
    }
}

fn main() int {
    ready bool = true;
    if (ready) @likely {
        C.printf("run=%d\n", run());
    } else @unlikely {
        C.printf("wait=%d\n", wait());
    }
    C.printf("%d %d\n", classify(200), classify(500));
    return 0;
}
```

This program prints `run=1`, then `1 0`. The weights are advice to code generation, not
conditions: the branch that runs is decided by `ready` and by `code` alone.

3.2.6 A branch weight affects code generation only; evaluation order and result do not
change. At most one weight attribute may be written on a branch, and writing it in any
other position is an error.

3.2.7 Where a value is wanted, an `if` with both branches is itself the value: each branch
is a block, and what it gives is what the `if` gives
(see [09 Scopes §2.2](09-scopes.md#scopes)). Both branches **must** give the same type, and
an `if` without `else` gives no value. A conditional expression `cond ? a : b` says the same
thing when both sides are single expressions.

```ft
import C;
import std;

fn sign(n int) int { return if (n < 0) { -1 } else { 1 }; }

fn main() int except {
    C.printf("%d %d\n", sign(-5), sign(5));
    return 0;
}
```

This program prints `-1 1`: the `if` is the value `sign` returns, and each branch's block
gives the number written in it.

### Compile-time assertions

3.3.1 `static_assert(cond)` or `static_assert(cond, message)` checks a `bool` constant
at compile time; when the condition is false, the implementation **must** report a
diagnostic and, when a `message` is present, include it in the diagnostic.

3.3.2 `static_assert` may appear at module level and in a function body; it **must not**
appear in the body of a type definition.

3.3.3 A `static_assert` in a template is checked once per instantiation; a template that
is never instantiated does not trigger it.

3.3.4 The condition **must** be a constant that the compiler is able to evaluate.

### Probing whether a block compiles

3.4.1 `if (auto) @constexpr { … } else { … }` asks the compiler to **try** the following
block. The condition is not written: the question is whether the block compiles. When it
does, the block is the program; when it does not, the `else` block is, and the trial's
diagnostics are discarded.

3.4.2 The untaken side is not checked at all, so a block may name things that do not exist
for the types it is tried against. Only a fatal diagnostic fails the trial: a warning, such
as a variable that is never read, does not.

3.4.3 `if (auto) @constexpr` is the way to ask whether a *block* compiles, as
`@meta.compilable(T)` asks whether a *name* does
(see [07 Expressions §7.9](07-expressions.md#compile-time-queries)).

```ft
import C;
import std;

type Point { x int; y int; }

fn main() int except {
    if (auto) @constexpr {
        p Point = .{1, 2};
        n int = p.x;
        C.printf("a Point has x\n");
    } else {
        C.printf("no point here\n");
    }
    return 0;
}
```

This program prints `a Point has x`: the block compiles, so it is the one kept. A block
that named a field `Point` does not have would take the `else` instead, and nothing would
be reported about the trial.

## Loops

4.1 All three loop forms may carry an `else` clause.

```ft
import C;

fn main() int {
    n int = 0;
    while (n < 3) {
        n = n + 1;
    } else {
        C.printf("while else ran\n");
    }
    for (i int = 0; i < 2; i = i + 1) {
        C.printf("%d ", i);
    } else {
        C.printf("for else ran\n");
    }
    return 0;
}
```

This program prints `while else ran`, then `0 1 for else ran`: the loop's `else` runs when
the loop ends without `break`, which is the case for both here.

4.1.1 The `else` clause is executed when the loop ends normally: when the condition
becomes false, or when the range form reaches `end()`.

4.1.2 The following cases do **not** execute the `else` clause: `break`, `return`,
`raise`. `continue` does not constitute an interruption.

### `while`

The body is executed repeatedly while the condition is true. The rules for the condition
are the same as in Section 3.

### `for (init; cond; update)`

4.3.1 `init` may be a binding, an expression statement, or empty.

4.3.2 The scope of a binding in `init` is the entire `for` statement.

4.3.3 When `cond` is omitted, it is always true.

4.3.4 `update` is executed after each completion of the loop body (and after a
`continue`), and the condition is then tested again.

## Range loops

### Expansion rules

`for (x T : range)` requires `range` to support the following operations:

| Operation | Meaning |
| ---- | ---- |
| `r.begin()` | the starting position |
| `r.end()` | the end position: one past the last element |
| `begin != end` | whether the traversal has not finished |
| `++it` | advance one position |
| `*it` | read the current position |

A range loop **must** be expanded as follows, where names beginning with `$` are
generated by the implementation and cannot be written by a program:

```text
auto $it = r.begin();
auto $end = r.end();
for (; $it != $end; ++$it) {
    x T = *$it;
    <loop body>
} else {
    <else clause>
}
```

5.1.1 The expanded code follows all the rules for ordinary statements.

5.1.2 `range` is **evaluated only once**. When `range` is an expression that names
storage (a name, a field, a dereference, a constant subscript), the implementation may
call `begin`/`end` on it directly; otherwise it **must** first bind a temporary.

5.1.3 When the loop variable is `&T`, it is the element itself; when it is `T` or
`auto`, it is a copy.

5.1.4 During iteration, the container **must not** grow or be reallocated; such behavior
is undefined behavior.

5.1.5 A fixed-length array is walked by its elements. Its `begin()` and `end()` come
from the standard library, which declares them once for every element type and length:

```ft
export
template <T type, N usize>
fn begin(this &const [N]const T) *const T {
    return &this[0];
}
```

The length `N` is deduced from the argument's own length, so one declaration serves
every array. A string literal is a `[N]c8`, so it is walked the same way and **may** be
used directly as the range of a range loop:

```ft
import C;
import std;

fn main() int except {
    for (ch c8 : "abc") {
        C.printf("%c ", ch);
    }
    C.printf("\n");
    return 0;
}
```

This program prints `a b c`.

5.1.6 The elements of a fixed-length array are in line, so the iterators are pointers
into it and `end()` is `begin() + N`. A named array is walked in place; a literal is
bound to a temporary first, as 5.1.2 requires.

### Ranges as the subject

5.2.1 A range literal is the ordinary way to walk a series of integers. `1 .. n` walks 1
through `n - 1` and `1 ..= n` walks 1 through `n`
(see [01 Lexical §6.6](01-lexical.md#range-literals)). The loop variable is `i64`, the
element type of `Range`:

```ft
import C;
import std;

fn main() int except {
    total i64 = 0i64;
    for (i i64 : 1 ..= 10) {
        total += i;
    }
    C.printf("%ld\n", total);
    return 0;
}
```

This program prints `55`, the sum of 1 through 10: `..=` includes its high end, so the
walk reaches 10.

5.2.2 The subject of a range loop may be any value that supports the operations in the
table above, a range literal among them. A `Range` value may also be stored and walked
repeatedly, and a range used twice is walked twice: it is a value, not a cursor.

5.2.3 A fixed-length array is walked by giving `[N]T` the two operations. The length is a
template's value parameter, so one declaration serves every length, and an array literal
is a subject like any other:

```ft
import C;
import std;

template <T type, N usize>
fn begin(this &const [N]T) *const T { return &this[0]; }
template <T type, N usize>
fn end(this &const [N]T) *const T { return &this[0] + N; }

fn main() int except {
    for (ch c8 : "abc") { C.printf("%c ", ch); }
    C.printf("| ");
    for (v int : {10, 20, 30}) { C.printf("%d ", v); }
    C.printf("\n");
    return 0;
}
```

This program prints `a b c | 10 20 30 `: a string literal is a `[N]const c8` and an array
literal is a `[N]T`, and both walk through the two declarations above. Its trailing space
is the one the loop body writes after the last element.

## `switch`

6.1 `switch (v) { case C: … default: … }` selects a branch by value.

6.2 A label **must** be a constant. One label group may contain several constants
separated by commas.

6.3 Two labels in the same `switch` **must not** have the same value.

6.4 `default` may appear in any position, at most once.

6.5 The end of one branch **must not** fall into the next branch; where fallthrough is
needed, `fallthrough;` **must** be written at the end of that branch.

6.6 The statements of each branch belong to the scope of the `switch`; where an
independent scope is needed, write a bare block.

6.7 The value of the `switch` **must** be comparable with the labels.

## Jumps

| Statement | Semantics |
| ---- | ---- |
| `break;` | leave the nearest loop or `switch` |
| `continue;` | enter the next iteration of the nearest loop |
| `return;` | return from a function that returns `void` |
| `return x;` | return a value; the type **must** be compatible with the return type |
| `fallthrough;` | appears only at the end of a `switch` branch, and enters the next branch |

7.1 The target of `continue` is the nearest **loop**; a `switch` is not its target.

7.2 The target of `break` is the nearest loop or `switch`.

## `defer`

8.1 `defer { … }` records the block in the **current scope**; it runs when that scope
ends. The braces are **required**.

8.2 Several `defer`s in one scope run in **reverse declaration order**.

8.3 The block runs however the scope ends: by reaching its end, by `return`, `break`,
`continue`, or by an exception leaving the scope.

8.4 A scope entered later ends first, so a `defer` of an inner block runs before one of
an outer block.

8.5 When a scope ends, its `defer` blocks run before the scope's local bindings are
destroyed (each in reverse order). A `defer` block may declare locals and defer again;
those follow the block's own rules.

8.6 `defer` is a statement, not an expression; it has no value.
