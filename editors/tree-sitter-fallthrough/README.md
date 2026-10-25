# tree-sitter-fallthrough

A [Tree-sitter](https://tree-sitter.github.io) grammar for the Fallthrough
language, used by the Zed extension in `editors/zed` for syntax highlighting,
bracket matching, indentation and the outline.

## Layout

```
grammar.js          the grammar
src/                the generated parser (parser.c, node-types.json)
queries/
  highlights.scm    syntax highlighting
  brackets.scm      bracket matching
  textobjects.scm   text objects for Vim motions
  indents.scm       auto-indentation
  outline.scm       the outline panel
```

## The language, as the grammar sees it

Declarations put the name before the type (`x int`, not `int x`), so a
declaration opens with two identifiers in a row:

```ft
import std;

type Point {
    x int;
    y int;
}

fn length(this &const Point) int {
    return this.x + this.y;
}

fn main() int except {
    p Point = .{3, 4};
    std.println("{}", p.length())??;
    return 0;
}
```

## Building

The parser is committed, so Zed can load it without a build step. To regenerate
after a change to `grammar.js`:

```sh
npx tree-sitter-cli generate       # rewrites src/parser.c and node-types.json
npx tree-sitter-cli parse FILE.ft  # prints the tree; `grep ERROR` is the check
npx tree-sitter-cli query queries/highlights.scm FILE.ft
```

The wasm Zed loads is built from `src/parser.c` with the wasi-sdk clang that
`tree-sitter build --wasm` downloads:

```sh
tree-sitter build --wasm -o fallthrough.wasm .
```

If the download is unavailable, the same file comes from the compiler directly:

```sh
clang --target=wasm32-wasip2 \
      --sysroot=$HOME/.cache/tree-sitter/wasi-sdk/share/wasi-sysroot \
      -O2 -fPIC -shared -Wl,--no-entry -Wl,--export-all \
      -I./src -o fallthrough.wasm src/parser.c
```

## What the grammar does not decide

The grammar accepts every construct the compiler accepts, and does not try to
reject programs: `s String = 1;` parses as a declaration and the type checker
reports the mistake. A name is a type in one position and a value in another,
and the grammar names the type position `type_identifier` so a highlight query
can colour it as a type; which name really is a type is the checker's answer.
