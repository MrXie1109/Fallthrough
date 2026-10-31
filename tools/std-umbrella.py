#!/usr/bin/env python3
"""Checks that `std` exports every module of the library.

`standard/std.ftm` is the umbrella: `import std;` is meant to bring the whole
library in, so a module that exists and is not exported from it is a name a
program cannot reach without knowing which file defines it. That is exactly the
kind of thing that goes wrong silently -- a module is added, everything
compiles, and the name is simply absent from `std`.

Three modules on disk are not exported from it:

    error.macos, error.windows   the Darwin and Windows implementations of the
                                 three functions `std.error` defines, under the
                                 same names; exporting them would put three
                                 definitions of each name in one namespace
    prelude                      a subset of the library, whose own comment says
                                 `import std;` remains the full umbrella

Usage:
    tools/std-umbrella.py           report and exit non-zero on a gap
"""

from __future__ import annotations

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
LIBRARY = ROOT / "standard" / "std"
UMBRELLA = ROOT / "standard" / "std.ftm"

# Modules that exist and are deliberately not part of the umbrella.
NOT_EXPORTED = {
    "error.macos": "the Darwin implementation of the names `std.error` defines",
    "error.windows": "the Windows implementation of the names `std.error` defines",
    "prelude": "a deliberate subset of the library, not a second umbrella",
}


def exported_modules(text: str) -> set[str]:
    """The modules an umbrella re-exports, by their name under `std`."""
    return set(
        re.findall(
            r"^export\s+from\s+std\.([A-Za-z_][A-Za-z0-9_.]*)\s+import",
            text,
            re.MULTILINE,
        )
    )


def main() -> int:
    modules = {path.stem for path in LIBRARY.glob("*.ftm")}
    exported = exported_modules(UMBRELLA.read_text(encoding="utf-8"))

    missing = sorted(modules - exported - set(NOT_EXPORTED))
    unknown = sorted(exported - modules)
    stale = sorted(set(NOT_EXPORTED) - modules)

    if unknown:
        for name in unknown:
            print(f"std.{name} is exported, and there is no such module")
    if stale:
        for name in stale:
            print(f"std.{name} is listed as deliberately unexported, and does not exist")
    if missing:
        for name in missing:
            print(f"std.{name} is a module, and the umbrella does not export it")

    if missing or unknown or stale:
        return 1

    print(
        f"std exports {len(exported)} of {len(modules)} modules; "
        f"{len(NOT_EXPORTED)} are deliberately left out"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
