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

# A scratch directory for the compiler's own smoke test.
SCRATCH ?= scratch

# One test binary is a filter; empty means every test in the workspace. The
# project's rule is to run only the tests a change touches, which is why
# `TEST=` exists and why `test-all` is a separate target.
TEST    ?=
TARGET  ?=

.PHONY: help build release check clippy fmt fmt-check test test-all std-check \
        smoke clean distclean install uninstall

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
	@echo "make smoke        compile and run tests/smoke.ft if it exists"
	@echo "make clean        remove build artifacts"
	@echo "make install      install fallc (PREFIX=$(PREFIX), default /usr/local)"
	@echo "make uninstall    remove the installed fallc"

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
	@echo "installed $(DESTDIR)$(PREFIX)/bin/fallc"

uninstall:
	rm -f $(DESTDIR)$(PREFIX)/bin/fallc
	@echo "removed $(DESTDIR)$(PREFIX)/bin/fallc"
