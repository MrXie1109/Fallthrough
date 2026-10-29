# `C.assert`

The assertion macro, as the function behind it.

Part of the C bindings: the declarations from `assert.h`.

This module is not meant to be imported on its own. `standard/C.ftm` gathers
it with the others and re-exports everything, so `import C;` is what a
program writes and `C.printf` is what it calls.

Every declaration is `export`, which is what makes it visible to the module
that re-exports it; a module's declarations are private by default.
--------------------------------------------------------------- assert.h
Aborts with a message if the condition is zero.

WAITING FOR SPEC: `assert` is a macro in C, so its argument would normally
be stringified at the call site. Here it is simply a function taking a flag
and a message, which is close enough for a temporary C binding.

## Declarations
