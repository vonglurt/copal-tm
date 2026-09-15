# copal-tm

An instrument panel and a process browser, in one terminal window.

`copal-tm` is the task manager for [Copal Linux](https://github.com/vonglurt/copal-alpine-linux)'s
full install. It is a Rust Cargo workspace of four crates with **no external
dependencies** — not one — because a Copal fleet node never reaches the
internet, and a dependency is a crate fetch that fails on the one machine this
is meant to run on.

```
 ╭────────────────────╮ ╭──────────────────────────────────────────────────────╮ ╭────────────────────────────────╮
 │ S Y S ▪▪▪ 4:08 left│ │ C P U   H I S T O R Y ▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪ 36%│ │ A V E R A G E   C P U   U S E 24│
 │CPU  Batt Temp  GPU │ │◈ Utilization  ◈ Temperature  ◈ Kernel                │ │    PID   Name              CPU▾│
 │▄▄▄▄ ▄▄▄▄ ▄▄▄▄ ▄▄▄▄▄│ │100% ──────────────────────────────────┊──────────┊110°│ │   6809 ◆ ytq              29.0%│
 │▄▄▄▄ ▄▄▄▄ ▄▄▄▄ ▄▄▄▄▄│ │                        ⢀⣤     ⣠⣄       ┊  ⣠⣄⣀  ⡇⠘⠦⡄   │ │   4821 ◆ node             20.7%│
 │▄▄▄▄ ▄▄▄▄ ▄▄▄▄ ▄▄▄▄▄│ │ 50% ──────────⣠⢤──────⢸⠈⢳⡀──⣰⠃⠈⠙⠲⠤⠤⠤⣤⠤⣼⠁─⠈⠉⢹⠧⠤⠞⢻◀55°│ │    310 ▪ nginx            11.8%│
 │▄▄▄▄ ▄▄▄▄ ▄▄▄▄ ▄▄▄▄▄│ │  0% ⠽⠯⠭⠭⠤⠤⠤⢤⣀⣀⣀⡤⠤⠴⠦⠞⠉⠙⠒⠦⣄⣤⣀⣈⡥⠭⠷⠧⠤⠤⢤⣤⣠⢤⣀⡤⣄⣠⠤⠤⣄⡤⠤⢤◀0° │ │   7204 ◆ dd                1.1%│
 │36.0 12.5 52.5 13.4%│ │P▄▂▅▅▂▃ E▃▂▆▄                      Speed: schedutil│ │   6411 ◆ rustc             0.5%│
 ╰────────────────────╯ ╰──────────────────────────────────────────────────────╯ ╰────────────────────────────────╯
```

`make shot` prints a full frame; the picture above is a trimmed one.

## What it is

Two halves, visible at the same time.

**The instrument panel** is a translation of a sensor dashboard into character
cells: an inset dot-matrix title strip with the headline metric right-aligned;
segmented ladder meters whose *unlit* segments are the meter's own hue at low
opacity, so a nearly-empty gauge still reads as that gauge; a braille strip
chart with a legend that is also a control, dual axes, a brushed time band
shared by every history, and a caret per series at its current value; a
full-width memory band; and a rail of stat tiles. Every group, colour, glyph
and sampling period is specified in [docs/design-lab-report.md](docs/design-lab-report.md).

**The process browser** is the half a sensor panel does not have:

- a **nesting tree** where a collapsed parent rolls its whole subtree's CPU and
  memory up into itself — which is how forty renderer processes become one
  readable row;
- an **Inspector** with five sections, including a **Network** section that
  joins `/proc/[pid]/fd` to `/proc/net/{tcp,tcp6,udp,udp6,unix}` and shows
  every port the process is listening on and every peer it is talking to;
- and an **Access** section that decodes the `SigIgn`, `SigCgt` and `SigBlk`
  masks every Linux process publishes — so a trap is visible *before* it is hit:

```
   SIGNAL     DISPOSITION
   TERM  15   trapped      a handler is installed
   INT    2   default
   HUP    1   ignored      sending it does nothing
   KILL   9   default      cannot be caught
```

## The halt ladder

Ending a process is the one irreversible thing this program does, so it is the
part that reasons hardest. `x` on a selection builds a plan and shows it before
anything is sent:

- **a zombie** cannot be signalled at all; the plan offers its parent and says
  why;
- **uninterruptible sleep** is named, and `KILL` is not promised to be
  different;
- **a stopped process** gets `CONT` with every rung, because it will not run its
  own handler until it is continued;
- **a shell-wrapped child** is addressed by its **process group** —
  `kill -TERM -4820`, which is what Ctrl-C sends and what gets past a trap held
  by one member;
- **a supervised OpenRC daemon** is not signalled at all: the plan says
  `rc-service nginx stop`, because signalling it just gets it restarted;
- and the ladder — TERM → INT → HUP → KILL — **skips what the mask says is
  ignored**, doubles the grace on what is trapped, always ends on `KILL`, and
  records in the Transcript **which rung worked**.

Nothing is sent without a confirmation, and the exact equivalent command line
is always on screen first.

## Build

```sh
make            # the release binary, target/release/copal-tm
make run        # build and run it here
make demo       # run against the simulated probe, on any machine
make shot       # render one frame to stdout, FRAME=132x40
make test       # 95 tests, no terminal needed
make check      # fmt, clippy -D warnings, tests
make install    # into ~/.local/bin
```

It builds on Alpine (the target) and on macOS (the desk it was written at).
There is no `/proc` on a Mac, so the probe has a second back end that makes
plausible readings and a synthetic process tree containing every case the halt
ladder reasons about — a wrapped child, a zombie, a `D`-state, a stopped
process, a supervised daemon, and one that ignores `SIGTERM`. It never pretends:
the status bar says `SIMULATED` and so does the first line of the Transcript.

## Keys

| | |
|---|---|
| `F1` `F2` `F3` | dashboard / split / browser |
| `↑↓` `PgUp` `PgDn` | move the selection |
| `←` `→` | collapse / expand a subtree |
| `Enter`, `i` | Inspector; `1`–`5` choose its section |
| `Space` | mark a process |
| `t` `s` `r` | tree / sort / reverse |
| `/` `K` | filter; show kernel threads |
| `k` | Services — send one signal |
| `x` | Halt plan — the considered one |
| `T` | Transcript |
| `w` | cycle the SYS readout |
| `1` `2` `3` | toggle a history series |
| `p`, `+`/`-` | pause; faster / slower tick |
| `?` `q` | keys; quit |

## Configuration

`~/.config/copal/taskman.conf`, `key = value`, in the same directory as the rest
of Copal's user configuration. Every key is also a flag of the same name, and
`copal-tm --dump-config` writes the effective configuration as a file that can
be saved as-is.

## The crates

| crate | what it owns |
|---|---|
| `copal-tm-tty` | colour, a damage-tracked cell canvas, the glyph vocabulary and its fallbacks, raw mode, key decoding |
| `copal-tm-probe` | every reading — `/proc`, `/sys`, and the simulation — plus signal dispositions and halt plans |
| `copal-tm-ui` | the widgets: title strips, ladder meters, strip charts, stat tiles, tables |
| `copal-tm` | the binary: configuration, layout, the event loop, the Transcript |

They depend only on each other and on `std`.

## Degrading

| | |
|---|---|
| **width** | 132+ full; 100–131 narrower Roll; 80–99 no Roll; under 80 a gauge rail |
| **colour** | true colour, or the nearest of sixteen ANSI colours, computed per cell |
| **glyphs** | braille (2×4 dots per cell) → half blocks (1×2) → `*` and `+ - \|` |

Set `charset = blocks` on a Linux console whose font has no braille, or
`theme = mono` where colour is not to be trusted. Shape carries everything:
the ladders, the plots and the tables are legible without a single hue.

---

MIT licensed — see [LICENSE](LICENSE). Copyright (c) 2026 Paul Richeson.
