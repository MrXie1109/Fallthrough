# Fallthrough for Zed

Language support for [Fallthrough](https://github.com/MrXie1109/Fallthrough) in
[Zed](https://zed.dev).

The extension starts the compiler as a language server: `fallc --lsp`.

## Install

The extension needs the compiler. Build and install it once:

```sh
git clone https://github.com/MrXie1109/Fallthrough.git
cd Fallthrough
make release
sudo make install
fallc --version
```

Then install the extension from Zed's extension page, or load it from a
checkout while developing it — see [Developing the extension](#developing-the-extension).

## What it does

- Recognises `.ft` (a program) and `.ftm` (a module).
- Reports the compiler's diagnostics as you type: lexer, parser, type checker
  and module errors, each at the position the compiler reports, with its code
  (`S0010`, `P0002`, …).
- Withdraws the diagnostics of a closed buffer.

Completion, hover, go-to-definition and formatting are not implemented yet: the
server declares only what it does.

## Finding the compiler

In order:

1. the `fallc` on `PATH`;
2. `$FALLC`;
3. `target/release/fallc` or `target/debug/fallc` in the worktree root.

A project can name its own compiler in Zed's settings:

```jsonc
{
  "lsp": {
    "fallc": {
      "binary": {
        "path": "/path/to/fallc",
        "arguments": ["--lsp"]
      }
    }
  }
}
```

or, for the extension's own lookup:

```jsonc
{
  "fallthrough": {
    "server_path": "target/debug/fallc"
  }
}
```

## Grammars

Zed highlights with Tree-sitter. The grammar lives in
[`editors/tree-sitter-fallthrough`](../tree-sitter-fallthrough): its queries
(`highlights.scm`, `brackets.scm`, `indents.scm`, `outline.scm`,
`textobjects.scm`) sit beside `config.toml` here, and the compiled parser is
`grammars/fallthrough.wasm`.

## Developing the extension

```sh
cd editors/zed
# In Zed: Extensions → Install Dev Extension → this directory
```

Zed builds the extension from `src/lib.rs` with the `zed_extension_api` crate.
A compiler built in the same checkout is found through path 3 above, so
developing the compiler and the extension in Zed uses the local build without
installing anything.
