#!/usr/bin/env python3
"""Turning the standard library's comments into a document.

The library is written the way a reader needs it: a module opens with what it
is for, and each declaration is preceded by what it does and what its
parameters mean. This script collects those comments, keeps the declarations
they describe, and writes one Markdown file per module.

The comments are already Markdown -- `# heading`, blank lines between
paragraphs, backticks around code -- so nothing is rewritten. What this adds is
the one thing a comment block cannot say: which declaration it belongs to.

    python3 tools/std-docs.py                  # writes docs/std/*.md
    python3 tools/std-docs.py --check          # fails if the file is out of date
"""

from __future__ import annotations

import argparse
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
STANDARD = ROOT / "standard"
OUTPUT = ROOT / "docs" / "std"
ENVIRONMENT = ROOT / "docs" / "std" / "README.md"


def module_name(path: pathlib.Path) -> str:
    """The name a program imports the module by."""
    relative = path.relative_to(STANDARD).with_suffix("")
    return ".".join(relative.parts)


def read_blocks(path: pathlib.Path) -> list[tuple[list[str], list[str]]]:
    """Splits a module into (comment, declaration) pairs.

    A comment block is the run of `//` lines immediately above a declaration;
    the declaration is the lines that follow it, up to the first blank line
    after one that closes a brace. A comment with no declaration under it --
    the module header, or a paragraph in the middle of a declaration -- belongs
    to whatever comes next, and a header is the one at the top of the file.
    """
    lines = path.read_text().split("\n")
    blocks: list[tuple[list[str], list[str]]] = []
    comment: list[str] = []
    declaration: list[str] = []
    depth = 0
    for line in lines:
        stripped = line.strip()
        if stripped.startswith("//") and not declaration:
            text = stripped[2:]
            comment.append(text[1:] if text.startswith(" ") else text)
            continue
        if not stripped:
            if declaration and depth == 0:
                blocks.append((comment, declaration))
                comment, declaration = [], []
            continue
        declaration.append(line)
        depth += line.count("{") - line.count("}")
        if depth < 0:
            depth = 0
    if declaration:
        blocks.append((comment, declaration))
    return blocks


def declaration_title(declaration: list[str]) -> str:
    """The line a reader looks for: what is being declared, and its name.

    A declaration may open with a prefix -- `export`, `template <..>`, an
    attribute -- and the line that says *what* is being declared is the first
    one after it. The title is that line, and the prefix is left to the code
    block below, where it is part of the shape a caller writes.
    """
    for line in declaration:
        stripped = line.strip()
        if not stripped or stripped.startswith("//"):
            continue
        if (
            stripped == "export"
            or stripped.startswith("template ")
            or stripped.startswith("@")
        ):
            continue
        return stripped.split("{", 1)[0].rstrip() if stripped.startswith("fn ") else stripped
    return ""


def is_a_function(declaration: list[str]) -> bool:
    """Whether the declaration is a function, whose body is implementation."""
    for line in declaration:
        stripped = line.strip()
        if not stripped or stripped.startswith("//") or stripped == "export":
            continue
        if stripped.startswith("template ") or stripped.startswith("@"):
            continue
        return stripped.startswith("fn ")
    return False


def signature(declaration: list[str]) -> str:
    """The declaration as a page shows it.

    A function keeps its signature and loses its body: what a caller needs is
    the name, the parameters and the return type, and how the function is
    written is not what the page is about.

    Everything else is kept whole. A type's fields are what it holds, an
    enum's members are its values, and a trait's requirements are its
    requirements -- dropping any of them would leave the page describing
    something a reader cannot see.
    """
    if not is_a_function(declaration):
        return "\n".join(declaration).rstrip()
    kept: list[str] = []
    for line in declaration:
        stripped = line.strip()
        if stripped.startswith("//"):
            continue
        if "{" in line:
            head = line.split("{", 1)[0].rstrip()
            if head:
                kept.append(head)
            break
        kept.append(line.rstrip())
    return "\n".join(kept).strip()


def render(path: pathlib.Path) -> str:
    """Renders one module as Markdown."""
    name = module_name(path)
    blocks = read_blocks(path)
    out: list[str] = [f"# `{name}`", ""]

    # The header is the comment before the first declaration. It is prose about
    # the module rather than about the declaration under it, and it is what a
    # reader arriving at the page wants first.
    header: list[str] = []
    body = blocks
    if blocks:
        first_comment, _ = blocks[0]
        header = first_comment
        body = blocks[1:]
    for line in header:
        out.append(line)
    if header:
        out.append("")
        out.append("## Declarations")
        out.append("")

    for comment, declaration in body:
        title = declaration_title(declaration)
        if not title:
            continue
        # A private field or a nested alias is part of the thing above it; the
        # page states what a reader can call, and a module's own bookkeeping is
        # not that. A declaration whose comment says what it is for is kept
        # whether or not it is exported, because a module's own methods are
        # still methods.
        out.append(f"### `{title}`")
        out.append("")
        for line in comment:
            out.append(line)
        if comment:
            out.append("")
        # The signature, so the page shows what a caller writes and not only
        # the prose about it. A body is implementation: it says how the thing
        # is done, and this page is about what it is for.
        text = signature(declaration)
        if text:
            out.append("```")
            out.append(text)
            out.append("```")
            out.append("")

    while out and out[-1] == "":
        out.pop()
    out.append("")
    return "\n".join(out)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check",
        action="store_true",
        help="fail if a generated file differs from what is on disk",
    )
    arguments = parser.parse_args()

    sources = sorted(STANDARD.glob("*.ftm")) + sorted(STANDARD.glob("*/*.ftm"))
    OUTPUT.mkdir(parents=True, exist_ok=True)
    pages: list[tuple[str, str]] = []
    stale: list[str] = []
    for source in sources:
        name = module_name(source)
        filename = name.replace(".", "/") + ".md"
        target = OUTPUT / filename
        target.parent.mkdir(parents=True, exist_ok=True)
        text = render(source)
        pages.append((name, filename))
        if arguments.check:
            if not target.is_file() or target.read_text() != text:
                stale.append(filename)
        else:
            target.write_text(text)

    if arguments.check:
        if stale:
            print("out of date:", ", ".join(stale))
            return 1
        print(f"{len(pages)} pages are up to date")
        return 0

    lines = [
        "# The standard library, module by module",
        "",
        "Generated from the comments in `standard/` by `tools/std-docs.py`.",
        "Do not edit these pages: a page follows the source, and the source is",
        "what a change belongs in.",
        "",
    ]
    for name, filename in pages:
        lines.append(f"- [`{name}`]({filename})")
    lines.append("")
    ENVIRONMENT.write_text("\n".join(lines))
    print(f"wrote {len(pages)} pages and the index to {OUTPUT.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
