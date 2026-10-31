# 09 Modules

> [中文](09-modules.zh.md)

## Scope

This volume specifies the importing and exporting of modules, the contents of the export
table, the rules of name identity, the module lookup order, multi-file compilation and
linking.

## Importing

### Forms

```ft
// geom.ftm
export fn area(w int, h int) int {
    return w * h;
}

export type Point {
    x int;
    y int;
}
```

```ft
// main.ft
import C;
import geom;
import geom as g;
from geom import area;
from geom import Point;

fn main() int {
    p Point = .{1, 2};
    C.printf("%d %d %d\n", area(2, 3), g.area(3, 4), p.x);
    return 0;
}
```

These two files compile together and the program prints `6 12 1`: `geom.area` is reached
through the module name and through the alias `g`, and `from geom import area` makes the
plain name usable.

A module may forward what it imports, which is how an umbrella module is assembled:

```ft
// umbrella.ftm
export from geom import *;
```

2.1.1 Importing is static: a module is loaded at compile time, and there is no run-time
module object.

2.1.2 An imported name **must** be exported by the imported module (`export`).

2.1.3 `from M import *` only brings names into the current file; it does not change the
current module's export table.

2.1.4 `export from M import …` both brings names in and adds them to the current
module's export table.

2.1.5 A dot in a module name belongs to the module name, not to a member:
`C.math.sin(…)` names nothing; a name re-exported by `C` is written `C.sin(…)`. Type
names are the exception: `std.string.String` is a legal spelling, because the whole path
is the type's name.

### Short names: type names and value names are not the same

2.2.1 After `import M;`, the **type names** exported by `M` (`type`, `union`, `enum`,
`interface`, `traits`) may be written with their short names directly.

2.2.2 **Functions, constants and module-level variables have no short-name exception**:
they must be written with a qualified name, or introduced through
`from M import name`.

2.2.3 A short name belongs only to a file that imports that module **directly**; a
transitive dependency exposes only the full name.

2.2.4 The last segment of a dotted module name is its short name, but it is valid only
for a direct importer.

## The export table

### Contents

Every kind of declaration may be exported:

| Declaration | What the importer receives |
| ---- | ---------------- |
| `export fn f(…) T { … }` | the signature and the symbol; every overload is retained |
| `export fn f(…) T;` | the declaration only; the symbol is defined elsewhere |
| `export type T { … }` | the fields (including `mutable` and `@align(..)`), template parameters, specialisations and nested aliases |
| `export type T;` | the name, as an incomplete type |
| `export type T<…>` | a type family: the parameters and all specialisations |
| `export type T = U;` | an alias |
| `export union U { … }` | the fields and the union layout |
| `export enum E { … }` | the type (the base type) and the value of each member |
| `export interface I(it T) { … }` | the method table |
| `export traits X(it T) { … }` | the name; the requirements follow the defining module's syntax tree |
| `export NAME const T = v;` | the value and the type |
| `export name T = v;` | the module-level variable itself |
| `@extern_C export fn f(…) T;` | an external function: the ABI and the symbol |
| `export name T @extern_C;` | an external variable: the name, the type and the linkage symbol |
| `export define NAME …` | the macro definition |

3.1.1 Exporting is not transitive: only `export from` puts a name into this module's
export table.

3.1.2 A type's methods **must** be `export`ed separately; when only the fields are
exported, an importer cannot call the member functions.

3.1.3 A macro definition travels with the export table; macros referenced in its body but
not exported are also available with the definition.

### Incomplete types

3.2.1 `export type Node;` exports the name rather than a layout. An importer may use
`*Node` and `&Node`, but **must not** use it by value or access its members.

3.2.2 The definition may be visible only in the defining module; this is the normative
form of an opaque handle.

3.2.3 `sizeof(Node)`, `Node x;` and `x.field` require a complete type; on an incomplete
type they are errors.

## Umbrella modules

4.1 An umbrella module consists of `export from` declarations.

4.2 Merging rules for names with the same spelling: functions merge as overloads; for
every other kind, the umbrella module's own declaration prevails, and a forwarded name
with the same spelling is ignored.

## Where a module is found

5.1 The lookup order for `import M;` is:

1. the directory containing the importer;
2. the directory named by the environment variable `FALLTHROUGH_STANDARD`, if it is set;
3. the directory containing the implementation and several levels above it;
4. the directory containing the file being compiled and several levels above it;
5. the current working directory and several levels above it.

5.2 A dot maps to a directory separator: `std.string` corresponds to `std/string.ftm`.

5.3 In each directory, the loader **must** first look for `M.<platform>.ftm`, and then
for `M.ftm`. The platform name is the target system name (such as `linux`, `windows`,
`macos`); the module name does not change because of a platform file.

5.4 The implementation **must** allow the environment variable `FALLC_TARGET_OS` to
override the platform name used for checking, so that the platform files of another
system can be checked on one system.

5.5 A module name **must** resolve to exactly one file; when two files can provide the
same module name, the implementation **must** report a diagnostic.

## Name identity

6.1 One module name corresponds to one file.

6.2 A short name belongs to the direct importer.

6.3 A local value may shadow a module name with the same spelling.

6.4 A type introduced by the same module declaration is the same type in every
translation unit; types with the same name declared separately are different types.

## Defaults cross a module boundary

7.1 A parameter default value is an expression of the module that declares it.

7.2 The declaring module **must** retain the default-value record with the exported
signature; the caller fills omitted arguments from that record.

7.3 Consequently, a cross-module call may omit an argument that has a default value.

7.4 The implementation **must not** require the caller to rewrite the default value.

## Multi-file compilation

8.1 A program may consist of several translation units, compiled and linked together.

8.2 Each `.ft` file is a translation unit; translation units do not share names except
through import.

8.3 A symbol **must** be defined at most once in the whole program; declarations may be
repeated.

8.4 An `.ftm` file takes part in compilation through `import`; it **must not** be linked
as a program on its own.

## External libraries

9.1 A `link` declaration names an external library to link:

```ft
link m dynamic from default;
link z static from "/usr/lib";
link "gcc_s" dynamic from default;

fn main() int {
    return 0;
}
```

Each of these three files compiles and links: a bare name finds the library the way the
linker does, a quoted name is passed through as written, and `from "…"` names the
directory to search.

9.2 All three parts of `link` **must** be present: the name, the link kind (`dynamic` or
`static`), and the lookup location (`default` or a directory string).

9.2.1 `dynamic` and `static` are **not** reserved words
(see [01 Lexical §5.2](01-lexical.md#keywords)). They mean the link kind only in the
position after a library name in a `link` declaration; anywhere else they are ordinary
identifiers, so a program may declare `static int = 2;` and use it. The `static` of a
declaration's storage duration is the attribute `@static`
(see [03 Declarations §4](03-declarations.md#declaration-modifiers)), which is a
different word in a different place.

9.3 When a library name contains `/` or ends in `.a`/`.so`, it is passed to the linker as
a file path; otherwise the platform convention's prefix and suffix are added.

9.4 The declarations of external functions and external variables are given in
[04 Functions](04-functions.md) and [03 Declarations](03-declarations.md#external-variables-and-functions)
respectively.

9.5 The standard library's C bindings are an ordinary module; they are imported in the
same way as any ordinary module.
