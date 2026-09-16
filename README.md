<h1 align="center">copal-tm</h1>

<p align="center">
  <b>An instrument panel and a process browser, in one terminal window.</b><br>
  <sub>The task manager for <a href="https://github.com/vonglurt/copal">Copal Linux</a>'s full install — one Rust crate, zero dependencies.</sub>
</p>

<p align="center">
  <a href="https://crates.io/crates/copal-tm"><img src="https://img.shields.io/crates/v/copal-tm.svg?style=flat-square" alt="copal-tm on crates.io"></a>
  <a href="https://github.com/vonglurt/copal-tm/blob/main/LICENSE"><img src="https://img.shields.io/crates/l/copal-tm.svg?style=flat-square" alt="MIT licence"></a>
  <img src="https://img.shields.io/badge/dependencies-0-informational?style=flat-square" alt="zero dependencies">
</p>

<p align="center">
  <img src="https://raw.githubusercontent.com/vonglurt/copal-tm/main/docs/media/copal-tm-demo.png" alt="copal-tm running: the SYS cluster, CPU history, the Roll, the memory band, the tile rail and the process tree" width="100%">
</p>

<p align="center"><sub><code>make demo</code> — every reading above is simulated, and it says so.</sub></p>

---

## Why

Copal's full install gives you a themed Wayland desktop and then leaves you with
BusyBox `top`.

`top` will not tell you that the process you are about to terminate has
installed a handler for `SIGTERM`. It will not tell you that the daemon you keep
killing is supervised and gets restarted three seconds later. It will not tell
you that the thing you are aiming at is a zombie, and that no signal you send
can possibly affect it. It will not tell you which of your forty Firefox
processes is the one eating the machine, or what is holding port 8080.

Every one of those facts is in `/proc`. This program reads them.

## Two halves, at the same time

### The instrument panel

A sensor dashboard, translated into character cells rather than approximated by
them. The design is not decoration — each part earns its place, and every hex
value, glyph and sampling period is written down in
**[docs/design-lab-report.md](https://github.com/vonglurt/copal-tm/blob/main/docs/design-lab-report.md)** before it was
written in code.

- **Dot-matrix title strips.** Title letter-spaced in cyan, headline metric
  right-aligned, and *the unlit cells of the matrix* between them — which is
  what makes the panel read as a physical display with a fixed number of cells,
  most of which happen to be off.
- **Ladder meters whose empty segments are still their own colour.** An unlit
  CPU segment is dark green, not grey; Battery's is dark red. A gauge at 3 %
  still reads as *that* gauge, and four columns side by side stay four
  distinguishable objects at a glance. The topmost lit segment blooms; the one
  segment straddling the value is lit in proportion, so the meter moves smoothly
  instead of stepping.
- **A braille strip chart**, 2×4 dots per cell, with a legend that is also a
  control, dual axes (the right one belongs to Temperature alone, and is drawn
  in its colour), a brushed time band **shared by every history on screen**, and
  a caret per series pointing at its current value.
- **A per-core strip** grouped by class — `P▄▂▅▅▂▃ E▃▂▆▄` — because a
  single-threaded build pinning one big core while eleven others idle is
  invisible in an average.
- **A tile rail** where the headline can be a word (`LOW`) when that is the
  honest answer for a quantity nobody can act on.

### The process browser

The half a sensor panel does not have.

```
    PID USER      S  THR  CPU%▾   MEM%       RSS     TIME+ COMMAND
      1 root      S    4   0.2%   0.0%    3.7 MB   0:00.70 ▾ init
    310 root      S    4  11.8%   0.1%   17.7 MB   3:37.00   ├─▾ nginx master process
    311 nginx     S    4   0.2%   0.1%   21.8 MB   3:37.70   │ ├─nginx worker process
   7420 nginx     Z    4   0.0%   0.0%       0 B   1h26:34   │ ╰─cgi-worker
    402 root      S    4   0.3%   0.4%    146 MB   4:41.40   ├─Xorg :0 -seat seat0
    610 copal     S    4   0.1%   1.0%    314 MB   7:07.00   ├─▾ hyprland
   1127 copal     S    4  92.9%  15.5%    4.9 GB  13:08.90   │ ├─▸9 foot -e /bin/ash
   7880 copal     R    4   0.6%   0.0%    8.7 MB   1h31:56   │ ├─copal-tm
   5140 copal     S    4   0.2%   3.5%    1.1 GB  59:58.00   │ ╰─▸2 firefox
   7001 chrony    S    4   0.0%   0.0%    5.8 MB   1h21:40   ╰─chronyd -f /etc/chrony/chrony.conf
```

A **nesting tree**, and a collapsed parent **rolls its whole subtree up into
itself**. `▸9 foot` is a terminal holding nine descendants: its 92.9 % and its
4.9 GB are the *subtree's* totals, not its own — which is how forty renderer
processes become one readable row, and why the tree is the default rather than
the flat list. The count rides on the disclosure triangle so it can never be
mistaken for part of the command line.

The row colour is the state: `D` uninterruptible in orange, `Z` zombie in red,
`T` stopped in yellow. The program's own row is cyan. Arguments are drawn dim
behind the program name, so a long command line stays scannable.

## The part nobody else shows you

Every Linux process publishes three hexadecimal words in `/proc/[pid]/status`:
`SigIgn`, `SigCgt` and `SigBlk` — the signals it ignores, the ones it has
installed a handler for, and the ones it has blocked. No common task manager
shows them. They are the difference between *"terminate did not work"* and
*"terminate was never going to work"*.

```
   SIGNAL     DISPOSITION
   TERM  15   trapped      a handler is installed
   INT    2   default
   HUP    1   ignored      sending it does nothing
   QUIT   3   default
   KILL   9   default      cannot be caught
```

`trapped` in orange, `ignored` in dim, `blocked` in red, `default` in green.
The trap is visible **before** you hit it — and so it is in the menu that sends
them, where an ignored signal is greyed with the reason rather than quietly
doing nothing:

```
╭────────────────────────────────────────────────────────╮
│ S E R V I C E S ▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪ 7551 stubborn│
│▎ Terminate     TERM   ignored   the polite one         │
│  Interrupt     INT    trapped   what Ctrl-C sends      │
│  Hangup        HUP              for a daemon this usua │
│  Quit          QUIT             terminates AND dumps c │
│  Stop          STOP             pause it; cannot be ca │
│  Continue      CONT             resume a stopped proce │
│  User 1        USR1             whatever the program m │
│  User 2        USR2             whatever the program m │
│  Kill          KILL             cannot be caught, bloc │
╰────────────────────────────────────────────────────────╯
```

### And then it routes around them

`x` on a selection builds a halt plan and shows it. Nothing is sent until you
confirm, and the exact equivalent command line is always on screen first —
because a task manager that hides what it is about to do is asking to be
trusted, and one that prints it is asking to be checked.

```
╭────────────────────────────────────────────────────────────────────────╮
│ H A L T   P L A N ▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪ 4821 node│
│the parent sh (4820) is a wrapper with one child; the group reaches both│
│it has handlers installed; a polite stop may be a clean shutdown, or it │
│may be swallowed. The ladder finds out.                                 │
│                                                                        │
│addressee                                                               │
│▎ process group 4820                                                    │
│  the wrapper sh (4820)                                                 │
│  process 4821                                                          │
│                                                                        │
│  kill -TERM -4820                                                      │
│                                                                        │
│→ TERM   TERM is trapped; the handler gets twice the grace              │
│→ INT                                                                   │
│→ HUP    HUP is trapped; the handler gets twice the grace               │
│→ KILL   cannot be caught; the last rung, always                        │
│                                                                        │
│[Enter] send  [e] step  [↑↓] addressee  [Esc] cancel                    │
╰────────────────────────────────────────────────────────────────────────╯
```

<sub>Rendered by the program, not drawn by hand: <code>copal-tm --simulate --frame=92x26 --view browser --open halt</code>.</sub>

What it works out before offering anything:

| the case | what the plan says |
|---|---|
| a **zombie** | no signal can affect it — it is already dead and waiting to be reaped. Offers the **parent** instead. |
| **uninterruptible sleep** (`D`) | the signal is delivered only when it leaves the kernel, which on dead storage may be never — **and `KILL` is no different**. It says so rather than promising otherwise. |
| a **stopped** process (`T`) | it will not run its own handler until it is continued, so every rung sends `CONT` straight after. |
| a **shell-wrapped child** | signalling the child leaves the wrapper. The group — `kill -TERM -4820` — is what `Ctrl-C` sends, and it reaches every member, so one member's trap cannot hold the others. |
| a **supervised OpenRC daemon** | a signal is the wrong instrument however well aimed. The stop is `rc-service nginx stop`; killing it just gets it restarted. *This is the most common way a kill "does not work", and it is not a trap at all.* |
| **pid 1, kernel threads, another user's process** | refused, with what privilege would have been needed. |

The ladder then runs **TERM → INT → HUP → KILL**, striking out anything the mask
says is ignored, doubling the grace on anything trapped, always ending on
`KILL` — and the Transcript records **which rung actually worked**, which is the
piece of knowledge the whole feature exists to produce.

## What is it doing on the network?

Linux keeps no per-process network table. It keeps two halves of one, joined by
an inode: `/proc/[pid]/fd/*` reads back as `socket:[12345]`, and
`/proc/net/{tcp,tcp6,udp,udp6,unix}` carry that same inode in a column. Match
them and a file descriptor becomes an address.

```
 I N S P E C T ▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪ 310 nginx
 1 Attributes  2 Contents  3 Network  4 Tools  5 Access

 3 listening · 1 established · 1 unix
 per-process byte rates are not in /proc; these are addresses and queues

 PROTO  LOCAL                REMOTE               STATE
 tcp    0.0.0.0:80                                LISTEN
 tcp    0.0.0.0:443                               LISTEN
 unix   /run/nginx…ginx.sock                      LISTEN
 tcp    10.0.0.7:443         10.0.0.31:51820      ESTABLISHED
```

"What is holding port 8080" is one keystroke, not another program that has to be
installed on the node. And the caveat is stated once, plainly: per-process byte
*rates* need taskstats, netlink or eBPF, none of which is a file read. Guessing
would be worse than the gap.

## Honest about what it costs

The sampling schedule is specified, not incidental —
[Section VII](https://github.com/vonglurt/copal-tm/blob/main/docs/design-lab-report.md) of the design report gives every source,
its period and the reason for the period.

- **Nothing forks on the tick.** Every reading is a file read. The one fork
  (`smartctl`) runs on its own thread at 1/300 the rate.
- **Cost scales with what is shown, not what exists.** `/proc/[pid]/status` is
  read for the rows on screen and no others: on a 400-process machine that is
  1,600 lines a second instead of 22,000. The expensive reads — command line,
  cgroup, the fd directory, the socket join — happen **on selection only**.
- **It throttles itself.** On battery below 15 %, or if its own CPU share passes
  2 % for ten ticks, the period doubles and the Transcript says why. *A task
  manager that appears in the top five of its own table has failed at its job.*
- **Every reading carries its age.** A value older than three of its own periods
  is drawn washed out, because an instrument that has stopped must not look like
  one reading zero.

## Zero dependencies, and that is the design

One crate, depending on nothing but `std`. Not one external crate,
ever — orrery states the reason and ascitty follows it: **a Copal fleet node
never reaches the internet, so a dependency is a crate fetch that fails on the
one machine this is meant to run on.**

So the terminal layer is ours: raw mode is two `stty` flags, the alternate
screen is two escape sequences, the size comes back on the key stream from
`CSI 18 t`, and the canvas writes only the cells that changed.

One crate named after the program, and three modules inside it:

| module | what it owns |
|---|---|
| `tty` | colour, a damage-tracked cell canvas, the glyph vocabulary and its fallbacks, raw mode, key decoding |
| `machine` | every reading — `/proc`, `/sys`, and the simulation — plus signal dispositions and halt plans |
| `ui` | the widgets: title strips, ladder meters, strip charts, stat tiles, tables |

The binary — configuration, layout, the event loop, the Transcript — is the
same crate's `main.rs`. There is one name on crates.io, one version, and
nothing to publish in dependency order.

## Install

It is published on [crates.io](https://crates.io/crates/copal-tm), and it has no
dependencies, so `cargo install` fetches exactly one crate and builds it:

```sh
cargo install copal-tm      # builds and installs into ~/.cargo/bin
copal-tm                    # ready, if ~/.cargo/bin is on your PATH
```

Every release is a published version there. `cargo install copal-tm` takes the
newest one; `cargo install copal-tm --version 0.1.2` pins a particular one, and
`cargo install copal-tm --force` upgrades an existing install in place.

It needs Rust 1.70 or newer and nothing else: no `-sys` crate, no `pkg-config`,
no C library to find — the only thing cargo downloads is this. To put the binary
somewhere other than `~/.cargo/bin`, give cargo a root:

```sh
cargo install --root /usr/local copal-tm     # /usr/local/bin/copal-tm
cargo uninstall --root /usr/local copal-tm   # and back out again
```

`cargo uninstall copal-tm`, with no `--root`, removes the one in
`~/.cargo/bin`: the root has to match the one it was installed under.

### Without a Rust toolchain

Every tagged release carries a statically linked musl binary for x86_64 and
aarch64 — one file, no shared libraries, nothing to install beside it:

```sh
tar xzf copal-tm-0.1.2-x86_64-unknown-linux-musl.tar.gz
install -Dm755 copal-tm-*/copal-tm ~/.local/bin/copal-tm
```

They are on the [releases page](https://github.com/vonglurt/copal-tm/releases),
each with a `.sha256` beside it. `make dist-bin` builds the same tarball for
whatever machine you are sitting at.

### On Alpine

`packaging/alpine/APKBUILD` is an aport for this program. With `alpine-sdk`
installed and a signing key made (`abuild-keygen -a -i`), `abuild -r` in that
directory builds an `.apk` from the tagged source — the ordinary Alpine
packaging path, and the one to send upstream to aports.

### From a checkout

The Makefile is the front door, and it installs into
`~/.local/bin` — where `copal-build` puts everything else on a Copal machine:

```sh
make install                    # ~/.local/bin/copal-tm
PREFIX=/usr/local make install  # anywhere else
make uninstall
```

## Build

```sh
make            # the release binary — one file, no shared libraries but musl's
make run        # build it and run it here
make demo       # the whole interface against the simulation, on any machine
make shot       # render one frame to stdout; FRAME=80x24 make shot
make test       # 98 tests, no terminal required
make check      # fmt, clippy -D warnings, tests
make install    # into ~/.local/bin
```

It targets Alpine and was written on a Mac. Everything but the `machine`
module's Linux back end is portable, and on a machine with no `/proc` it makes plausible
readings over a synthetic process tree that contains — deliberately — every case
the halt ladder reasons about: a wrapped child, a zombie, a `D`-state, a stopped
process, a supervised daemon, and one that ignores `SIGTERM`. It never pretends:
the status bar says `SIMULATED` and so does the first line of the Transcript.

`--frame WxH` renders a single frame to stdout and exits. That is how the build
is looked at without a terminal session, and it is what the golden-frame tests
render at seven sizes from 40×12 to 200×60. `--open halt|services|network|transcript|help`
puts an overlay up first — every boxed picture in this README
was produced that way, so none of them can drift from the program.

## Keys

| | |
|---|---|
| `F1` `F2` `F3` | dashboard / split / browser |
| `↑↓` `PgUp` `PgDn` | move the selection |
| `←` `→` | collapse / expand a subtree |
| `Enter`, `i` | Inspector; `1`–`5` for Attributes, Contents, Network, Tools, Access |
| `Space` | mark a process |
| `t` `s` `r` | tree / sort / reverse |
| `/` `K` | filter; show kernel threads |
| **`k`** | **Services** — send one signal, with its disposition beside it |
| **`x`** | **Halt plan** — the considered one |
| `T` | Transcript — every signal sent, every refusal, and why |
| `w` | cycle the SYS readout: time remaining, power, uptime, load, temp, pressure |
| `1` `2` `3` | toggle a history series |
| `p`, `+`/`-` | pause; faster / slower tick |
| `?` `q` | keys; quit |

## Configuration

`~/.config/copal/taskman.conf`, `key = value`, in the same directory as the rest
of Copal's user configuration. Every key is also a flag of the same name, and
`copal-tm --dump-config` writes the effective configuration as a file you can
save as-is.

```ini
refresh        = 1.0          # seconds per tick, 0.25 .. 10
sys.meters     = cpu,battery,temp,gpu
sys.readout    = remaining    # the lap time, by default
slot.tile      = gpu          # gpu | swap-rate | load | processes | throttle
cpu.scale      = total        # 100% is the whole machine, or one core
halt.grace     = 2.0          # seconds between rungs of the ladder
halt.confirm   = true         # never false without an explicit edit
charset        = auto         # full | blocks | ascii
theme          = copal        # copal | mono
```

## It degrades legibly

| | |
|---|---|
| **width** | 132+ full; 100–131 a narrower Roll; 80–99 no Roll; under 80 the dashboard becomes a gauge rail and the browser takes the room |
| **colour** | true colour, or the nearest of sixteen ANSI colours computed per cell |
| **glyphs** | braille (2×4 dots) → half blocks (1×2) → `*` and `+ - \|` |

`charset = blocks` for a Linux console whose font has no braille; `theme = mono`
where colour is not to be trusted. Shape carries everything: the ladders, the
plots and the tables are all legible without a single hue difference.

---

MIT licensed — see [LICENSE](https://github.com/vonglurt/copal-tm/blob/main/LICENSE). Copyright (c) 2026 Paul Richeson.
Copal Linux is an aggregation of Alpine Linux, not a derivative work of it.
