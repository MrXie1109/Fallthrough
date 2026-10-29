# `C`

The C standard library, as Fallthrough declarations.

This module is an umbrella: the declarations themselves live in one module
per C header, and this file gathers them and re-exports everything. A program
therefore writes `import C;` and calls `C.printf`, `C.sqrt`, or `C.strlen`,
without needing to know which header each came from.

## Reading this file

Each `export from .. import *` below republishes a submodule's exports as
this module's own, which is what makes the umbrella work. The `export` is
required: a plain `from .. import *` would bring the names into this file and
stop there, so what a module publishes is always something a reader can find
by looking for `export`.

The `link` instruction says what the produced binary must be linked against.
It lives here rather than in a submodule because it applies to whatever
imports the umbrella, so a program that calls `C.sqrt` gets `libm` without
writing anything itself. `libm` is separate from `libc` on Linux, and the
math functions are undefined without it.

## Caveats

This module exists because the language has no standard library yet. Every
declaration is `export`, which is what makes it visible to an importer; a
module's declarations are private by default.

A type is written the way Fallthrough writes it, and the C function is
declared with that signature. `...` marks a variadic parameter, so `printf`
may be called with any number of trailing arguments.

WAITING FOR SPEC: the C type mappings these files rely on. Two are assumed
and are the only reasonable readings, but they are assumptions:

  * a C `char *` is `*const c8`, because a C string is a run of characters;
  * a C `size_t` is `usize`, for the same reason.

Types that have no Fallthrough equivalent yet are declared with the closest
available type and marked. `FILE`, `va_list`, and function pointers are not
expressible at all, so the functions that need them are declared with `*c8`
where a pointer appears and omitted where they cannot be written honestly.

Note that an ordinary string literal is `[N]c8` and so satisfies a `*const c8`
parameter directly, terminator and all: `C.printf("hi")` needs no cast.
The submodules, one per C header. Each is re-exported wholesale.

## Declarations

### `link m dynamic from default;`

What the declarations need at link time. A system library, so the linker's
own search path is where it is found.

```
link m dynamic from default;
```
