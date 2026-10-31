# 00 General provisions

> [中文](00-start.zh.md)

## Scope

This volume specifies the composition of a translation unit and of a program, the form
of the program entry, the interface of the compilation products, and the ways in which
the diagnostics reported by an implementation are consumed. The semantics of language
constructs are given in the following volumes.

## Source files, translation units and modules

2.1 A source file is distinguished by its extension:

| Extension | Purpose |
| ------ | ---- |
| `.ft` | a translation unit; it may be compiled as a program on its own, or may form a program together with other `.ft` files |
| `.ftm` | a module; it may be used only through `import`, and must not be compiled as an executable program on its own |

2.2 A `.ft` file **must not** be imported; an implementation must reject such an import
and issue a diagnostic.

2.3 A module name is determined by the path of its `.ftm` file: with the extension
removed, directory separators are written `.`. `std.string` corresponds to
`std/string.ftm`. The rules for resolving a module name are given in
[08 Modules](09-modules.md).

2.4 A translation unit consists of: all top-level declarations of the `.ft` file, and
all declarations exported by the modules it statically imports.

## Program entry

3.1 A program begins execution at the function named `main`. The **parameter list** of
`main` must be empty or exactly `argc int, argv **c8`; its **return type** must be
`int`, `void`, or omitted when there is a function body.

3.2 When `main` returns `int`, the return value is the program's exit code; when it
returns `void` or omits the return type, the exit code is 0.

3.2.1 A `main` that returns `int` is treated as ending with `return 0;`: reaching the end
of its body without a `return` returns 0, as a C program does when it falls off the end of
its `main`.

3.2.2 For any other function, reaching the end of a body that declares a return value
that is not `void` is undefined behaviour: the function **should** return on every path
that can be taken. `main` is the one place the language supplies the value, because its
body is where a reader first meets the question.

3.3 `main` may be declared as possibly raising (`except`). An uncaught exception is
reported by the implementation; the format of the report and the exit code are given in
[06 Exceptions](07-exceptions.md).

3.4 A `main` whose parameter list or return type does not conform to 3.1 violates a
constraint of this standard, and an implementation **must** report a diagnostic
(`S0181`).

3.5 A parameter of the form `argv **c8` follows the conventions of a C program:
`argv[0]` is the program path, and `argv[argc]` is a null pointer. `argv` is not a
standard-library container; when a container is needed, the program constructs it
itself.

## Compilation interface

4.1 An implementation **must** provide a way to compile source files into one or a
combination of the following products: an executable file, an object file, LLVM
assembly, a syntax tree, a type table, a token stream. It **may** additionally produce
a static library (an archive of the program's objects) or a shared library.

4.2 An implementation **must** support a mode that performs only lexical, syntactic and
static semantic checking without producing an executable product.

4.3 An implementation **must** terminate with exit code 0 on success. The meanings of
the following exit codes are normative:

| Exit code | Meaning |
| ------ | ---- |
| 0 | compilation succeeded |
| 1 | the program violates this standard, and diagnostics were reported |
| 2 | command-line usage error |
| 3 | the implementation encountered a construct not specified by this standard |
| 70 | the program terminated with an uncaught exception (the program's exit code, not the implementation's) |

4.4 The spelling of an implementation's command-line options is not constrained by this
standard, but their behavior must not violate the conventions above.

## Compile-time limits and implementation boundaries

5.1 An implementation may limit resource use (recursion depth, number of template
instantiations, number of diagnostics, and so on). On reaching a limit, the
implementation **must** report a diagnostic and **must not** silently accept the program
or produce an undefined result.

5.2 On encountering a construct marked *Open* in
[`../SPEC-STATUS.md`](../SPEC-STATUS.md), an implementation **must** report the
internal-error diagnostic `I0999`. That diagnostic means that the implementation is
incomplete; it does not mean that the program is wrong.

## The standard library shipped with the implementation

6.1 An implementation ships with a standard library whose modules are imported through
the `std` umbrella module or through its submodules. The standard library is not
normative content of this standard; its behavior is specified by the standard library's
own documentation.

6.2 The `std` umbrella module of the standard library **must** provide the exported
names of its submodules by re-export. The semantics of re-export are given in
[08 Modules](09-modules.md).

## When something goes wrong

7.1 Diagnostics are reported by the implementation. The text, number and location of a
diagnostic form part of the implementation's public contract; numbers are stable, while
wording may change between versions.

7.2 A diagnostic contains at least: a severity level, a stable number, a primary
location (a source-text range), and a message. A diagnostic may carry a supplementary
note stating the relevant rule or the way to correct the program.

```text
error[S0002]: cannot find `printf` in this scope
  --> hello.ft:4:5
   |
 4 |     printf("hi\n");
   |     ^~~~~~
```

7.3 A number consists of a phase letter and four digits: `D` (driver), `L` (lexing and
macros), `P` (parsing), `S` (semantics), `I` (internal error). The full meaning of the
numbers and the warning groups are given in [10 Diagnostics](11-diagnostics.md).

7.4 An implementation **must** deduplicate the same diagnostic at the same program
location; the deduplication rules are given in [10 Diagnostics](11-diagnostics.md).

7.5 A program **must not** rely on the specific wording of diagnostic text; a program
may rely on numbers and locations.
