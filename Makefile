# COPAL-TM - build, run, look at, check and publish.
#
# The Makefile is the front door; cargo is what it calls.  That is the
# convention across copal, orrery, ascitty and yodacon, and it is what
# `copal-build` expects to find beside a Cargo.toml.
#
# The program targets Alpine and is written on a Mac.  Everything but the
# probe's Linux back end is portable, and `make demo` runs the whole
# interface against the simulated probe on either machine.
#
# Requires: cargo.  Nothing else, ever - see the no-dependency rule in
# docs/design-lab-report.md, Section I-B.

CARGO ?= cargo
BIN    = target/release/copal-tm
PREFIX ?= $(HOME)/.local
FRAME  ?= 132x40

# The four crates, in the order they must be published.
CRATES = copal-tm-tty copal-tm-probe copal-tm-ui copal-tm

.PHONY: all build run debug demo shot test check fmt clippy install uninstall dist publish clean help

all: build

help:
	@echo 'make build      the release binary, target/release/copal-tm'
	@echo 'make run        build it and run it in this terminal'
	@echo 'make debug      run the debug build, logging to /tmp/copal-tm.log'
	@echo 'make demo       run against the simulated probe, whatever the machine'
	@echo 'make shot       render one frame to stdout (FRAME=132x40)'
	@echo 'make test       cargo test --workspace'
	@echo 'make check      fmt --check, clippy -D warnings, and the tests'
	@echo 'make fmt        cargo fmt'
	@echo 'make install    into $(PREFIX)/bin'
	@echo 'make dist       package each crate, without pushing anything'
	@echo 'make publish    the four cargo publish calls, in dependency order'
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
	$(CARGO) test --workspace

fmt:
	$(CARGO) fmt

clippy:
	$(CARGO) clippy --workspace --all-targets -- -D warnings

check:
	$(CARGO) fmt --check
	$(CARGO) clippy --workspace --all-targets -- -D warnings
	$(CARGO) test --workspace

install: build
	$(CARGO) install --path crates/copal-tm --root $(PREFIX) --force

uninstall:
	$(CARGO) uninstall --root $(PREFIX) copal-tm

dist:
	@for c in $(CRATES); do echo "== $$c"; $(CARGO) package -p $$c --allow-dirty || exit 1; done

# Publishing is gated on a clean check, in this tree, now.  Nothing goes to
# crates.io that has not been built and looked at first.
publish: check
	@for c in $(CRATES); do \
	  echo "== publish $$c"; \
	  $(CARGO) publish -p $$c || exit 1; \
	  sleep 20; \
	done

clean:
	$(CARGO) clean
