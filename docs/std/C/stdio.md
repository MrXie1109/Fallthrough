# `C.stdio`

Reading and writing through the C standard I/O library.

Part of the C bindings: the declarations from `stdio.h`.

This module is not meant to be imported on its own. `standard/C.ftm` gathers
it with the others and re-exports everything, so `import C;` is what a
program writes and `C.printf` is what it calls.

Every declaration is `export`, which is what makes it visible to the module
that re-exports it; a module's declarations are private by default.
The standard streams, which the C library owns.

`stdin` is a `FILE *`, which this language has no type for; a stream is
passed around opaquely, so the bindings name it `*c8` and the pointer is
never dereferenced. The declaration is `extern` because the object is in the
C library: `stdin` is not a variable of this program, it is one this program
reaches.

## Declarations
