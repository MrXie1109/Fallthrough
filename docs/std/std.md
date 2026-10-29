# `std`

The standard library.

This module is an umbrella: the pieces live in `standard/std/`, and this file
gathers them and re-exports everything. A program writes `import std;` and
reaches `std.String`, `std.panic`, and the allocation functions alike.

The pieces are named after what they are rather than after what they contain,
so a program that only wants allocation can `import std.util;` and get
nothing else.

## Declarations

### `export from std.fmt import format;`

Building text from a format string, without writing it anywhere:

    name String = std.format("{} has {} items", owner, count)??;

The format module's own pieces (`render`, `apply_spec`, `format_value`) are
named rather than starred, so that a program which imports `std` gets the
function it means and nothing that could collide with a name of its own.

```
export from std.fmt import format;
```

### `export type String16 = BasicString<u16>;`

The wider character strings: the same operations over a different code unit.

```
export type String16 = BasicString<u16>;
export type String32 = BasicString<u32>;
```
