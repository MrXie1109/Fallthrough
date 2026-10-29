# `C.ctype`

Character classification.

Part of the C bindings: the declarations from `ctype.h`.

This module is not meant to be imported on its own. `standard/C.ftm` gathers
it with the others and re-exports everything, so `import C;` is what a
program writes and `C.printf` is what it calls.

Every declaration is `export`, which is what makes it visible to the module
that re-exports it; a module's declarations are private by default.
---------------------------------------------------------------- ctype.h
Reports whether a character is alphabetic.

## Declarations
