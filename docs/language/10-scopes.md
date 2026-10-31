# 10 Scopes

> [中文](10-scopes.zh.md)

## Scope

This volume specifies the scopes of names, the rules for shadowing and repeated
declarations, and the order in which bindings are destroyed.

## Scopes

2.1 Scopes are **block**-level. Each of the following constructs introduces a scope:

- a bare block `{ … }`;
- each branch block of an `if`, and its `else` block;
- the body of a `while`, a `for`, or a range `for`;
- the `for` statement itself: a binding in its initial clause belongs to that `for`;
- `switch` itself: all branches share this scope;
- each block of `try`, `except`, `else` and `finally`;
- a function body;
- a module.

2.2 A name is visible within its scope; after leaving the scope it is no longer
visible.

2.2.1 A block standing where a value is wanted is that value: the last expression in it
without a `;` is what the block gives, and a block whose last statement ends in `;` gives
nothing (`void`). Its bindings are its own, so the value may be computed from names that do
not exist outside:

```ft
import C;
import std;

fn f() int {
    return {
        x int = 2;
        x + 3
    };
}

fn main() int except {
    C.printf("%d\n", f());
    return 0;
}
```

This program prints `5`: the block is the value of the `return`, and `x` belongs to the
block alone.

2.2.2 A bare `{ … }` in statement position is a statement block, not a value: the
distinction is the position it stands in.

2.3 Each branch of a `switch` does not introduce a separate scope; where an independent
scope is needed, write a bare block.

## Shadowing and repeated declarations

3.1 A declaration in an inner scope **may** shadow a binding of the same name in an outer
scope; after leaving the inner scope, the outer binding becomes visible again.

3.2 Declaring a binding of the same name twice in the same scope is an error.

3.3 Parameters belong to the outermost scope of the function; the top level of the
function body **must not** declare a binding with the same name as a parameter.

3.4 A local binding may shadow a module-level name.

3.5 The rules for repeated declaration of `_` are given in Section 4.

## `_`

4.1 `_` is the only identifier that may be declared repeatedly in the same scope; each
occurrence is an independent binding.

4.2 `_` is readable; it is not a placeholder that discards a value.

4.3 The scope of a `_` binding lasts to the end of the block, or until the **next `_`
declaration** in the same scope.

4.4 Each `_` binding has independent storage and is destroyed when its scope ends.

4.5 A parameter may be named `_`, in order to ignore the corresponding argument.

## Lifetime and destruction

5.1 The lifetime of a binding begins when its declaration completes and ends when its
scope ends.

5.2 When a scope ends (normal completion, or leaving through `return`, `break`,
`continue` or `raise`), the bindings already constructed in that scope **must** be
destroyed in reverse order of declaration.

5.3 Destruction is performed through the type's `__drop__` or through the destruction of
its members; the rules are given in [07 Expressions](08-expressions.md#special-methods).

5.4 The order of destruction is the same on the exception path and the normal path; an
exception raised in a `try` block completes the destruction of that block before the
`except` block is entered.

5.5 A temporary is destroyed at the end of the full expression in which it appears; a
moved-from object is still destroyed when its scope ends (in the empty state).

5.6 A program **must not** rely on the relative order among multiple destructions whose
order is not specified, unless this standard specifies otherwise.

5.7 When a scope is left, the `defer` blocks of that scope
(see [05 Statements §8](06-statements.md#defer)) run **before** the destruction of the
scope's bindings. Both run in reverse declaration order, and an inner scope's `defer` runs
before anything the enclosing scope does.
