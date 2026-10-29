# `std.prelude`

The names a small program wants without hunting for their modules.

    import std.prelude;

This module re-exports the parts of the library that are in every program:
the error and optional types, strings and arrays, the collections a first
program reaches for, and the printing and formatting entry points. It is a
convenience, not a different library: every name here is the one its own
module defines, and `import std;` remains the full umbrella.

Nothing in the prelude is *removed* from its module; importing both is fine.

## Declarations
