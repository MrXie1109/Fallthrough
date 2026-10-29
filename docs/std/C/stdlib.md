# `C.stdlib`

Allocation, process control, and number conversion.

Part of the C bindings: the declarations from `stdlib.h`.

This module is not meant to be imported on its own. `standard/C.ftm` gathers
it with the others and re-exports everything, so `import C;` is what a
program writes and `C.printf` is what it calls.

Every declaration is `export`, which is what makes it visible to the module
that re-exports it; a module's declarations are private by default.
The two values `exit` takes.

They are constants of the module, so a program writes `C.EXIT_SUCCESS`, and
they are exported because a program that imported the umbrella must be able
to name them.

## Declarations
