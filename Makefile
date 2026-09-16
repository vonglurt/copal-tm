# COPAL-TM - build, run, look at, check and publish.
#
# The Makefile is the front door; cargo is what it calls.  That is the
# convention across copal, orrery, ascitty and yodacon, and it is what
# `copal-build` expects to find beside a Cargo.toml.
#
# The program targets Alpine and is written on a Mac.  Everything but the
# `machine` module's Linux back end is portable, and `make demo` runs the
# whole interface against the simulation on either machine.
#
# Requires: cargo.  Nothing else, ever - see the no-dependency rule in
# docs/design-lab-report.md, Section I-B.

CARGO ?= cargo
BIN    = target/release/copal-tm
PREFIX ?= $(HOME)/.local
FRAME  ?= 132x40

.PHONY: all build run debug demo shot plain test check fmt clippy install uninstall dist publish clean help

all: build

help:
	@echo 'make build      the release binary, target/release/copal-tm'
	@echo 'make run        build it and run it in this terminal'
	@echo 'make debug      run the debug build, logging to /tmp/copal-tm.log'
	@echo 'make demo       run against the simulation, whatever the machine'
	@echo 'make shot       render one frame to stdout (FRAME=132x40)'
	@echo 'make test       cargo test'
	@echo 'make check      fmt --check, clippy -D warnings, and the tests'
	@echo 'make fmt        cargo fmt'
	@echo 'make install    into $(PREFIX)/bin'
	@echo 'make dist       package the crate, without pushing anything'
	@echo 'make publish    the one cargo publish call'
	@echo 'make clean      cargo clean'

build $(BIN):
	$(CARGO) build --release

run: build
	$(BIN)

# The log goes to a file because a logger writing to the terminal fights the
# alternate screen for it, and the alternate screen wins.
debug:
	COPAL_TM_LOG=debug $(CARGO) run -- --refresh 0.5 2>/tmp/copal-tm.log
	@echo 'log: /tmp/copal-tm.log'

demo: build
	$(BIN) --simulate

# One frame, as the escape sequence that would draw it.  `make shot | less -R`
# to look at it, `make shot > frame.ansi` to keep it.
shot: build
	@$(BIN) --simulate --frame=$(FRAME)

# The same thing with the escapes stripped, which is what the layout is read
# from and what a golden-frame test compares.
plain: build
	@$(BIN) --simulate --frame=$(FRAME) | perl -pe 's/\e\[[0-9;]*[A-Za-z]//g'

test:
	$(CARGO) test

fmt:
	$(CARGO) fmt

clippy:
	$(CARGO) clippy --all-targets -- -D warnings

check:
	$(CARGO) fmt --check
	$(CARGO) clippy --all-targets -- -D warnings
	$(CARGO) test

install: build
	$(CARGO) install --path . --root $(PREFIX) --force

uninstall:
	$(CARGO) uninstall --root $(PREFIX) copal-tm

dist:
	$(CARGO) package --allow-dirty

# One crate, so one publish, and it is gated on a clean check in this tree,
# now.  Nothing goes to crates.io that has not been built and looked at first.
publish: check
	$(CARGO) publish

clean:
	$(CARGO) clean
