# `C.string`

Memory and string operations.

Part of the C bindings: the declarations from `string.h`.

This module is not meant to be imported on its own. `standard/C.ftm` gathers
it with the others and re-exports everything, so `import C;` is what a
program writes and `C.printf` is what it calls.

Every declaration is `export`, which is what makes it visible to the module
that re-exports it; a module's declarations are private by default.
-------------------------------------------------------------- string.h
Returns the length of a string, not counting the terminator.

## Declarations
