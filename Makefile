# Fallthrough build driver.
#
# The workspace is a normal Cargo workspace; this file exists so that the
# common things have short, memorable names and so that the compiler can be
# rebuilt and exercised without remembering crate names. Nothing here changes
# what Cargo does.
#
# Override a knob on the command line:
#
#   make JOBS=2 build
#   make TEST=integer_promotion test
#   make TARGET=windows std-check

CARGO   ?= cargo
JOBS    ?= 5
PROFILE ?= debug

# The compiler binary this tree produces.
BIN     := target/$(PROFILE)/fallc

# Where `make install` puts the compiler. `sudo make install` writes to
# /usr/local/bin; `make install PREFIX=$HOME/.local` does not need root.
PREFIX  ?= /usr/local
DESTDIR ?=

# The system decides where the library and its environment file belong. The
# layout is the Unix one on Linux and macOS; a Windows build under MSYS uses
# the same relative layout inside PREFIX.
SYSTEM  ?= $(shell uname -s 2>/dev/null || echo Windows)
STDLIB_DEST ?= $(PREFIX)/share/fallthrough/standard
ENV_FILE    ?= $(PREFIX)/etc/fallthrough/env

# A scratch directory for the compiler's own smoke test.
SCRATCH ?= scratch

# The compiler `make cache` warms the cache with: the installed one, on PATH.
# The cache belongs to a build of the compiler, so warming it with a different
# binary than the one a program is later built with would fill a cache nobody
# reads -- which is why this is the installed compiler and not `$(BIN)`.
FALLC   ?= fallc

# One test binary is a filter; empty means every test in the workspace. The
# project's rule is to run only the tests a change touches, which is why
# `TEST=` exists and why `test-all` is a separate target.
TEST    ?=
TARGET  ?=

.PHONY: help build release check clippy fmt fmt-check test test-all std-check \
        cache docs docs-check smoke clean distclean install uninstall

help:
	@echo "make build        build the workspace (debug)"
	@echo "make release      build the workspace with optimisations"
	@echo "make check        type-check the workspace without code generation"
	@echo "make clippy       lints; CI expects zero warnings"
	@echo "make fmt          format the workspace"
	@echo "make fmt-check    fail if formatting would change anything"
	@echo "make test         run tests (TEST=<name> to run one binary)"
	@echo "make test-all     run every workspace test (not the default policy)"
	@echo "make std-check    compile a program that imports the standard library"
	@echo "make cache        compile the whole standard library once with the"
	@echo "                  installed $(FALLC), so every later program builds from"
	@echo "                  the cache (run 'make release install' first)"
	@echo "make docs         write docs/std/ from the library's comments"
	@echo "make docs-check   fail if those pages are out of date"
	@echo "make smoke        compile and run tests/smoke.ft if it exists"
	@echo "make clean        remove build artifacts"
	@echo "make install      install fallc, the library and their env file"
	@echo "                  (PREFIX=$(PREFIX), $(SYSTEM))"
	@echo "make uninstall    remove them again"

build:
	$(CARGO) build -j $(JOBS)

release:
	$(CARGO) build -j $(JOBS) --release

check:
	$(CARGO) check -j $(JOBS) --workspace --all-targets

clippy:
	$(CARGO) clippy -j $(JOBS) --workspace --all-targets

fmt:
	$(CARGO) fmt --all

fmt-check:
	$(CARGO) fmt --all -- --check

# A named test binary is cheaper than the whole workspace; that is the form the
# development loop uses.
test:
ifneq ($(strip $(TEST)),)
	$(CARGO) test -j $(JOBS) -p fallc --test $(TEST) -- --test-threads $(JOBS)
else
	$(CARGO) test -j $(JOBS) --workspace -- --test-threads $(JOBS)
endif

test-all:
	$(CARGO) test -j $(JOBS) --workspace -- --test-threads $(JOBS)

# The standard library is compiled by the compiler, not by Cargo, so this is the
# check that catches a library change the Rust tests do not: it imports the
# umbrella module and touches enough of it to lower real code.
std-check: build
	@mkdir -p $(SCRATCH)
	@printf '%s\n' \
		'import std;' \
		'import C;' \
		'' \
		'fn main() int except {' \
		'    text String = "standard library";' \
		'    C.printf("%s\n", text.data());' \
		'    return 0;' \
		'> $(SCRATCH)/std-check.ft
	$(BIN) $(SCRATCH)/std-check.ft -o $(SCRATCH)/std-check
	@$(SCRATCH)/std-check

# Fills the module cache without building a program.
#
# Every module is compiled once and its products are cached: its interface, the
# compiled functions, and the templates that go with them. A build that finds
# them there reads them instead of compiling the sources again, which is what
# turns a standard-library program from seconds into a fraction of one.
#
# The cache is per compiler build -- the key includes this binary's size and
# modification time -- so a rebuild of `fallc` starts it empty. Running this
# once after `make release` puts everything a program can import into it, and
# the next program someone writes is already warm.
#
#   make release cache
#
# The program compiled here is the widest import the language has: the umbrella
# module, the C bindings, and the compile-time queries. A module no program
# imports is still compiled, because importing the umbrella is what loads it.
cache:
	@command -v $(FALLC) >/dev/null 2>&1 || { \
		echo "$(FALLC) is not installed; run 'make release install' first"; exit 1; }
	@mkdir -p $(SCRATCH)
	@printf '%s\n' \
		'import std;' \
		'import C;' \
		'' \
		'fn main() int except {' \
		'    text String = "warm";' \
		'    C.printf("%s\\n", text.data());' \
		'    return 0;' \
		'}' \
		> $(SCRATCH)/cache.ft
	@echo "warming the cache with $$(command -v $(FALLC))"
	@$(FALLC) $(SCRATCH)/cache.ft -o $(SCRATCH)/cache
	@echo "module cache is warm: the standard library is compiled"
# The library's own documentation, from the comments that already describe it.
#
# Every module opens with what it is for, and every declaration is preceded by
# what it does and what its parameters mean. Those comments are Markdown, so
# the page is a selection rather than a translation: a function keeps its
# signature and loses its body, a type keeps its fields, and the prose is the
# prose that was written.
#
# `make docs-check` is the form CI uses: it fails when a page no longer matches
# the source it came from.
PYTHON ?= python3
DOCS_TOOL := tools/std-docs.py

docs:
	$(PYTHON) $(DOCS_TOOL)

docs-check:
	$(PYTHON) $(DOCS_TOOL) --check

smoke: build
	@if [ -f tests/smoke.ft ]; then \
		$(BIN) tests/smoke.ft -o $(SCRATCH)/smoke && $(SCRATCH)/smoke; \
	else \
		echo "tests/smoke.ft does not exist; nothing to run"; \
	fi

clean:
	$(CARGO) clean

distclean: clean
	rm -rf $(SCRATCH)

install:
	@test -x target/release/fallc || { \
		echo "target/release/fallc is missing; run 'make release' first"; exit 1; }
	install -d $(DESTDIR)$(PREFIX)/bin
	install -m 755 target/release/fallc $(DESTDIR)$(PREFIX)/bin/fallc
	# The standard library travels with the compiler, but the compiler cannot
	# guess where an installed copy lives: it records the location in the
	# environment file its own documentation tells users to source.
	install -d $(DESTDIR)$(STDLIB_DEST)
	cp -R standard/. $(DESTDIR)$(STDLIB_DEST)/
	@install -d "$$(dirname '$(DESTDIR)$(ENV_FILE)')"
	@{ \
		echo "# Fallthrough environment, written by 'make install'."; \
		echo "# Source this file from your shell profile so the compiler finds"; \
		echo "# the standard library without a flag."; \
		echo "export FALLTHROUGH_STANDARD=\"$(STDLIB_DEST)\""; \
	} > $(DESTDIR)$(ENV_FILE)
	@echo "installed $(DESTDIR)$(PREFIX)/bin/fallc"
	@echo "installed $(DESTDIR)$(STDLIB_DEST)"
	@echo "wrote     $(DESTDIR)$(ENV_FILE)"
	@echo "source it, or add this to your shell profile:"
	@echo "  . $(ENV_FILE)"

uninstall:
	rm -f $(DESTDIR)$(PREFIX)/bin/fallc
	rm -rf $(DESTDIR)$(STDLIB_DEST)
	rm -f $(DESTDIR)$(ENV_FILE)
	@echo "removed $(DESTDIR)$(PREFIX)/bin/fallc"
	@echo "removed $(DESTDIR)$(STDLIB_DEST) and $(DESTDIR)$(ENV_FILE)"
