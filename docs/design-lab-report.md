# An Instrument Panel and a Process Browser: the Design of Copal Task Manager

*Lab Report — Design Specification*

<!-- SPDX-License-Identifier: MIT -->
Copyright (c) 2026 Paul Richeson. MIT licensed — see `LICENSE`. Copal Linux is
an aggregation of Alpine Linux, not a derivative work of it; Alpine and its
packages remain under their own licences.

---

## Abstract

Copal's "full monty" install level puts a desktop on an Alpine machine and
leaves the user with `top`. This report specifies the program that should be
there instead: `copal-tm`, a task manager for the terminal, built as a Rust
Cargo workspace with no external crates, shipped as a crate, and installed by
`copal-prep.sh` at stage 17 alongside the rest of the full install.

The design is taken from one image — a macOS sensor panel — read not as a
picture to copy but as an interface to translate. Section III reads it. What
that reading yields is a **grammar**: a panel with an inset dot-matrix title
strip, a family of segmented ladder meters whose unlit track is the meter's
own hue at low opacity, strip charts with a shared time cursor and per-series
carets, and a rail of stat tiles. Sections V and VI specify every group in
that grammar down to the hex value, the glyph, the cell, and the label.

The image is a *sensor* panel; a *task* manager has to do one more thing, and
it is the thing that can go wrong: end a program. Section VIII specifies that
half — a process Browser with a nesting tree, an Inspector that decodes
`/proc/[pid]/status`'s signal masks so that a trap is *visible before it is
hit*, a Network section that joins a process's own descriptors to the kernel's
socket tables so that "what is holding port 8080" is one keystroke rather than
another program, and a halt ladder that looks for the right addressee (the child, the
wrapper, the process group, the session, or the service manager) rather than
firing `SIGKILL` at whatever the cursor happened to be on.

Section VII is the sampling schedule: what is read, from where, how often, and
why that often and not more. Section X is the build, which runs on Alpine and
on a Mac. Section XI is the phase plan and what each phase must pass.

**Index terms** — Alpine Linux, task manager, terminal user interface, procfs,
signal disposition, process groups, Rust, instrument design.

---

## I. Introduction

### A. What exists

Copal installs stock Alpine through a sixteen-stage installer and, at stage
17, a Wayland desktop themed as *Linux Antiquity* (`docs/THEME.md`). The
process tools on that machine are BusyBox `top` and, if the user installs it,
`htop`. Neither is Copal's: they do not follow the theme, they do not know
what an OpenRC service is, and neither will tell you, before you press the
key, that the process you are about to terminate has installed a handler for
`SIGTERM`.

### B. Why it is a Rust workspace with no dependencies

Three conventions hold across the sibling projects and this one adopts them
without modification:

1. **`copal-build` goes by shape.** A checkout with a `Cargo.toml` gets
   `cargo`, and what it makes goes into `~/.local/bin`. It builds on the
   machine itself and must never modify a tracked file.
2. **No external crates, and that is the design.** orrery states the reason
   — "a dependency is therefore a crate fetch that fails on the one machine
   this is meant to run on" — and ascitty's three crates depend only on each
   other. A fleet node never reaches the internet.
3. **The Makefile is the front door.** Cargo is what the Makefile calls.

### C. The four crates

| crate | what it owns | knows about |
|---|---|---|
| `copal-tm-tty` | colour, a damage-tracked cell canvas, the glyph vocabulary and its fallbacks, raw mode, key decoding | nothing above it |
| `copal-tm-probe` | every reading: `/proc`, `/sys`, and a simulated source for machines that have neither | no drawing |
| `copal-tm-ui` | the widgets of Section V: title strips, ladders, strip charts, tiles, tables, the Inspector | draws into a canvas; holds no state of its own |
| `copal-tm` | the binary: configuration, layout, the event loop, the halt ladder, the Transcript | all three |

The split is the one that makes the program testable without a terminal: the
canvas can be flushed to a string and asserted on, and the probe can be fed a
directory of fixture files instead of `/proc`.

---

## II. Objectives

1. Specify every visible group: purpose, label, placement, grouping,
   construction in cells and glyphs, colour in hex, and shade derivation.
2. Specify the sampling schedule — source, path, period, and the reason for
   the period.
3. Specify the halting model, including how the correct addressee is found
   and how a trap is reported before it is hit.
4. Specify how a process's network use is investigated from `/proc` alone —
   what the relation is, and what it honestly cannot give.
5. Degrade legibly: 132 columns down to 80, true colour down to sixteen,
   braille down to half blocks down to ASCII.
6. Build and run on Alpine (the target) and on macOS (the desk it is written
   at), from one Makefile.
7. End in a crate that can be published, and a build seen before it is.

---

## III. The reference image, read as an interface

The image is a macOS sensor panel, 2000×1254, showing ten panels on a near
black ground. What follows is what is *in* it, in interface terms, because
the translation cannot begin until the thing is named.

### A. Structure

- **Ground and cards.** A near-black desktop (~`#0E1216`) carries rounded
  rectangular **cards** of a slightly lighter charcoal (~`#141A20`), each
  with a one-pixel border a little lighter again, and an inner shadow at the
  top edge. Gutters between cards are uniform — roughly one grid unit.
- **A three-band layout.** Band 1 is three cards of unequal width (a narrow
  instrument cluster, a wide chart, a medium table). Band 2 is one full-width
  card. Band 3 is a rail of four equal tiles. The bands are ordered by
  *glance cost*: the leftmost card of band 1 is read in a fraction of a
  second, the table needs seconds, the rail is checked only when something
  else has already said to look.

### B. The title strip

The three band-1 cards carry the panel's signature element: an **inset
dot-matrix display**. A black well is let into the top of the card; inside it
the title is set in a seven-segment/dot-matrix face in cyan (~`#4FE3E8`),
widely letter-spaced; the rest of the well is filled with the *unlit cells of
the same matrix*, drawn as a dim blue-grey texture; and a single numeric
readout is right-aligned in the same face — `9.4W` for SYS, `11%` for CPU
HISTORY. It is a marquee: **title left, headline metric right, unlit matrix
between them**. The unlit filler is the whole trick — it says the display is
a physical thing with a fixed number of cells, most of which happen to be off.

Note the exception, because it is instructive: the full-width **Memory
Utilization** card does *not* use a matrix strip. Its title is a large,
sentence-case, proportional label at the top left, with `23.3 GB / 32.0 GB`
right-aligned in the same face. The wide card is a *document*; the narrow
cards are *instruments*. Two title treatments, two meanings.

### C. The ladder meter

Four vertical bars in the SYS card, and one at the left of the Memory card.
Each is a **segmented gauge** — a column of ~18 short horizontal bars with a
gap between each, in the manner of a mixing-desk VU meter or a 1970s spectrum
analyser. Three properties matter:

1. **The unlit track is the meter's own hue, darkened** — not grey. The CPU
   column's empty segments are dark green, Battery's are dark red, Temp's are
   dark olive, GPU's are dark navy. A column that is nearly empty still reads
   as *that instrument*, and the four columns remain four distinguishable
   objects at a glance.
2. **The lit run glows.** The topmost lit segments are lighter than those
   below and bleed a little light into the ground.
3. **Label above, value below.** A short caption in the series hue above the
   column (`CPU`, `Battery`, `Temp`, `GPU`), and the reading below it in the
   same hue: `10.6%`, `9.4 W`, `35.1 °C`, `8.0%`. Fixed width, fixed decimal
   places — `00.0%` — so the number does not jitter in place.

The Temp column is not a single hue: its segments run olive at the bottom
through to bright yellow at the top, so *height and colour say the same
thing twice*. The System Pressure tile does the same in green→yellow→red.

### D. The strip chart

The CPU HISTORY card is a multi-series line chart, and it carries six
distinct pieces of interface:

1. **A legend that is also a control.** Three items — `Utilization` (green),
   `Temperature` (orange), `Kernel` (red) — each with a small icon and a
   coloured label, sitting *above* the plot rather than inside it.
2. **Dual axes.** `100% / 50% / 0%` down the left, `110° / 55° / 0°` down the
   right. The right axis belongs to the temperature series alone.
3. **A grid** of faint horizontal rules at the labelled values, drawn *under*
   the series.
4. **A brushed region.** Two vertical dashed white rulers with the band
   between them lifted a shade. It is a time selection.
5. **Carets.** At the right edge, each series has a small triangle pointing
   at its current value — a playhead, in the sense a tape deck means it.
6. **A footer caption row**, left and right: `10 logical processors · 10.6% ·
   9.4 W` and `Speed Auto`. Context that would be noise inside the plot.

The Memory card's chart is the same object in **area** form: one magenta
series, filled beneath, with its own in-plot extent labels (`8.6 GB` top
left, `9.3 GB` bottom left) and a three-part footer — `Available 8.7 GB`,
`Cached 6.3 GB`, `Swap 412.0 MB` — set left, centre and right.

### E. The roll

`AVERAGE CPU USE` is a plain table: `PID`, an application icon, `Name`,
`CPU`, `Memory`. Numeric columns are right-aligned with tabular figures;
`Name` is left-aligned with the icon in a fixed gutter; rows are tall and
unstriped, separated by whitespace rather than rules; the sort is CPU
descending and is not stated anywhere, because the first row is obviously the
biggest. It is a *league table*, not a database view: it answers "what is
using the machine right now" and nothing else.

### F. The tile

Four tiles in band 3, each with the same five parts: a round icon, a name, a
right-aligned headline metric, a two-or-three-part sub-caption, and a
horizontal segmented meter along the bottom. Each tile owns an accent hue
(blue, green, amber, green). The fourth tile's headline is a *word* — `LOW` —
not a number, which is the right answer for a quantity whose exact value
nobody can act on.

### G. What the image does not have

It has no process tree, no selection, no detail view, nothing about what a
process is doing on the network, and no way to end anything. It is an
instrument panel, and half of what follows is not in it.

---

## IV. The palette and the shading system

One table, and every colour in the program comes from it. Values are the
image's, sampled and regularised.

### A. Structure

| token | hex | used for |
|---|---|---|
| `ground` | `#0E1216` | the screen behind the panels |
| `panel` | `#141A20` | panel fill |
| `panel_hi` | `#182129` | a focused panel's fill; a brushed time band |
| `rule` | `#1E2830` | panel borders, chart grid rules |
| `inset` | `#05080A` | the well of a title strip |
| `text` | `#D7E0E5` | primary text |
| `dim` | `#55646E` | captions, units, unlit matrix cells, disabled items |
| `sel` | `#1D2E3A` | the selection bar of the Browser |
| `sel_edge` | `#4FE3E8` | the selection's left edge marker |

### B. Series hues

| token | hex | assigned to |
|---|---|---|
| `cyan` | `#4FE3E8` | title text and title readouts; the program's own voice |
| `green` | `#7CF05A` | CPU utilization |
| `orange` | `#F0A03A` | temperature series |
| `red` | `#F0453A` | kernel/system time; danger |
| `battery` | `#F2402C` | the Battery ladder |
| `yellow` | `#F2E34A` | the Temp ladder's top, warnings |
| `gpu` | `#4FA8F5` | the GPU ladder |
| `magenta` | `#E83CF0` | memory |
| `net` | `#46B4F0` | network |
| `disk` | `#55D97A` | storage |
| `amber` | `#E8A020` | the configurable slot tile |

Ramps: `heat = [#55D97A, #F2E34A, #F0453A]` for temperature and pressure;
`cool = [#1E3A5F, #4FA8F5, #9BD4FF]` for the GPU column.

### C. The three shade operations

Every shade in the program is one of these applied to a series hue. There are
no other greys.

| operation | formula | where |
|---|---|---|
| **track** | `hue` mixed into `panel` at **0.16** | unlit ladder segments, unlit tile meter cells, the area fill under a chart (0.18), the unlit matrix filler (0.22 of `dim` into `inset`) |
| **bloom** | `hue` mixed toward white at **0.30** | the topmost lit segment of a ladder; the current-value caret |
| **partial** | `hue` mixed into `panel` at the segment's own fill fraction | the one segment straddling the value, so a meter moves smoothly rather than in steps |

That third one is the terminal's answer to the image's anti-aliased LEDs: a
character cell cannot be half lit, but it can be a colour halfway between lit
and unlit, and at a glance the two are the same thing.

### D. Degrading

| capability | test | fallback |
|---|---|---|
| true colour | `COLORTERM` contains `truecolor`/`24bit`, else configuration | nearest of the sixteen ANSI colours, computed per cell |
| braille | locale is UTF-8 and `TERM` is not `linux` | half blocks, then `*` and `.` |
| rounded box drawing | as above | square box drawing, then `+ - \|` |

The fallbacks are selected once at start-up, as a `Charset`, and every widget
asks it for glyphs rather than writing literals.

---

## V. The layout grammar

### A. The panel

Every group is drawn in a **panel**: a one-cell border in `rule` on `panel`
fill, with the **title strip** occupying the first interior row against
`inset`. A panel is therefore three rows taller than its body.

```
╭──────────────────────────────────────╮  border, `rule` on `panel`
│ S Y S ▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪  9.4 W │  title strip: `inset` ground
│                                      │  body
╰──────────────────────────────────────╯
```

The title strip is Section III-B translated: the title in `cyan`, bold, with
one space of letter-spacing (`S Y S`); the readout right-aligned in `cyan`;
between them, `▪` repeated in `dim` over `inset` at 0.22 — the unlit matrix.
A focused panel's border is `cyan` at 0.5 and its fill is `panel_hi`.

The wide Memory panel takes the *document* treatment instead: sentence-case
title in `text`, bold, no letter-spacing, no filler, readout still right.

### B. The bands

| band | rows | contents |
|---|---|---|
| 1 | 14 | **A** SYS (21 cols) · **B** CPU History (flex) · **C** the Roll (44 cols) |
| 2 | 9 | **D** Memory Utilization, full width |
| 3 | 6 | **E** four tiles, equal widths |
| 4 | rest, min 6 | **F** the Browser, with **G** the Inspector as a right pane or an overlay |
| 5 | 1 | **I** the status bar and the last Transcript line |

One cell of margin around the screen, one cell of gutter between panels.

### C. Breakpoints

| width | what changes |
|---|---|
| ≥ 132 | as above; the Inspector opens as a 46-column right pane of band 4 |
| 100–131 | the Roll drops its Memory column; tile sub-captions shorten; the Inspector becomes an overlay |
| 80–99 | band 1 becomes SYS + CPU History; the Roll folds into the Browser as a sort mode; band 3 becomes two tiles on two rows |
| < 80 | the dashboard collapses to a three-row **gauge rail** — four horizontal ladders and one sparkline — and the Browser takes everything else |

| height | what changes |
|---|---|
| ≥ 40 | all five bands |
| 30–39 | band 3 drops to one row per tile (no sub-caption) |
| 24–29 | bands 1 and 4 only |
| < 24 | the Browser alone |

### D. Views

`F1` dashboard only · `F2` split, the default · `F3` browser only. The split
is the default because the program's two jobs — *watch* and *act* — are meant
to be visible at the same time.

---

## VI. The groups

Each subsection gives: **purpose**, **label**, **placement**, **grouping**,
**construction**, **colour**, and **sources**. Cadences are in Section VII.

### A. SYS — the instrument cluster

**Purpose.** The half-second glance. Four quantities that say whether the
machine is healthy, in a shape the eye reads without reading.

**Label.** `SYS` in the title strip. The right-hand readout is *the selected
system stat*, configurable and cycled live with `w`:

| value | shows | note |
|---|---|---|
| `power` | `9.4 W` | from the battery's `power_now`, or current × voltage |
| `remaining` | `2:41 left` | the lap time: energy remaining ÷ current draw, the default on a laptop |
| `uptime` | `6d 04:12` | the default on a machine with no battery |
| `load` | `1.42` | one-minute load average |
| `temp` | `35.1 °C` | the hottest zone |
| `pressure` | `LOW` | the worst of the three PSI averages |

**Placement.** Band 1, far left, 21 columns. Leftmost because reading starts
there and these are the first four numbers anyone wants.

**Grouping.** Four ladder columns side by side in one panel, sharing a
baseline and a cap line, so their heights are comparable at a glance even
though their units are not. Default `cpu, battery, temp, gpu`; configurable
from a catalogue of `cpu battery temp gpu mem swap io net load pressure fan`.
On a machine with no battery the second column falls back to `mem`, and on
one with no GPU counter the fourth falls back to `io`; the program says which
in the Transcript at start-up rather than showing an empty column.

**Construction.** Each column is 4 cells wide (3 of meter, 1 of gutter) and
`body − 2` rows tall:

- **row 0** the caption — `CPU`, `Battery`, `Temp`, `GPU` — in the series hue,
  bold, centred over the column, truncated to 4;
- **rows 1..n** the ladder, one segment per row, drawn as `▄` (the lower half
  block, so the top half of each row stays dark and *is* the gap between
  segments — this is what lets a character grid wear a VU meter at all);
- **row n+1** the reading, in the series hue, centred, fixed width and fixed
  decimals: `10.6%`, `9.4 W`, `35.1 °C`, `8.0%`.

Segment *i* of *n* is lit when `(i+1)/n ≤ v`. Lit segments take the hue (or
`ramp(heat, i/n)` for Temp); the topmost lit segment takes **bloom**; the one
segment straddling `v` takes **partial**; the rest take **track**.

**Colour.** `green`, `battery`, `heat` ramp, `gpu` — Section IV-B.

**Sources.** `/proc/stat` (1 s), `/sys/class/power_supply/BAT*` (5 s),
`/sys/class/thermal/*/temp` and hwmon (2 s), GPU busy (2 s).

### B. CPU HISTORY — the strip chart

**Purpose.** Whether *now* is unusual. A meter says what is happening; a
history says whether it has been happening.

**Label.** `CPU HISTORY` in the title strip, with current total utilization
as the readout: `11%`.

**Placement.** Band 1, centre, taking all the width the other two leave.
Widest because it is the only panel whose value increases with width — every
extra column is another second of the past.

**Grouping.** Legend row on top, plot in the middle, two caption rows below;
the axis labels live in 5-column gutters either side of the plot.

**Construction.**

- **Legend row.** `◈ Utilization   ◈ Temperature   ◈ Kernel`, each in its
  series hue, each a toggle on `1`, `2`, `3` or a click; a series that is off
  is drawn in `dim` and its line is not plotted.
- **Plot.** Braille, one screen column carrying two sample columns and four
  vertical dots per row — a 20×10 plot is 40×40 dots. Series are drawn in
  their hue over a grid of `rule` rules at 0 %, 50 % and 100 %. Where two
  series occupy the same cell, the later one wins, ordered Kernel, Utilization,
  Temperature, so the hottest quantity is never hidden.
- **Axes.** Left `100% 50% 0%` in `dim`; right `110° 55° 0°` in `orange`,
  because the right axis belongs to that one series and should be its colour.
- **Brush.** Two `┊` rulers in `text` at 0.6 with the band between them
  filled `panel_hi`. It marks the last 60 seconds by default and can be
  dragged; the footer then reads the average over the band instead of the
  instant. **The brush is global** — it is the same time selection in every
  history panel, which is the improvement on the image, where each chart
  brushes alone.
- **Carets.** In the right gutter, `◀` per visible series at its current
  value's row, in the series hue at **bloom**.
- **Footer row 1.** Left: `10 logical processors · 10.6% · 9.4 W`. Right:
  the governor — `Speed: schedutil`, or `Speed Auto` when none is readable.
- **Footer row 2 — the core classes.** Not in the image, and required: one
  micro-meter per logical processor, drawn with `▁▂▃▄▅▆▇█`, grouped and
  labelled by class. Classes come from `cpu_capacity` or
  `cpufreq/cpuinfo_max_freq`: cores within 5 % of each other are one class,
  the fastest class is `P`, the next `E`, then `p2`, `p3`…; an SMT sibling is
  drawn immediately after its partner at 0.6 opacity. On a uniform machine
  the row is a single unlabelled group of *n*. This is the row that shows a
  single-threaded build pinning one big core while eleven others idle.

**Colour.** `green` utilization, `orange` temperature, `red` kernel.

**Sources.** `/proc/stat` per core (1 s), thermal (2 s), `cpufreq` (60 s),
topology (once).

### C. AVERAGE CPU USE — the Roll

**Purpose.** The five-second question: what is using the machine? Distinct
from the Browser below it, which answers "what is running and what shall I do
about it".

**Label.** `AVERAGE CPU USE`, readout = the process count: `284`.

**Placement.** Band 1, right, 44 columns.

**Construction.** Columns `PID` (7, right) · mark (2) · `Name` (flex, left) ·
`CPU` (6, right) · `Memory` (9, right). A header row in `dim` with the sort
column in `cyan`. Rows are one cell tall — a terminal has no room for the
image's generous leading, so separation is carried by the `dim` header and by
right alignment alone.

The image's per-application icon becomes a **mark glyph**, which carries more
than decoration does:

| glyph | meaning | colour |
|---|---|---|
| `◆` | an ordinary user process | by state |
| `◇` | a kernel thread | `dim` |
| `▪` | a supervised service (an OpenRC pidfile or a `system.slice` cgroup) | `net` |
| `◈` | this program | `cyan` |

and the glyph's colour is the process state: `R` `green`, `S` `text`, `D`
`orange` — *the state in which no signal will be delivered* — `T`/`t`
`yellow`, `Z` `red`.

**Stability.** Sorted by CPU descending, but a row only changes rank when it
crosses another by more than **0.3 %**, and rows fade rather than jump when
they enter or leave. An unstable table cannot be read, and reading it is the
only thing it is for.

**Sources.** `/proc/[0-9]*/stat` (1 s).

### D. MEMORY UTILIZATION — the wide band

**Purpose.** Memory is one number with three qualifications — available,
cached, swap — and the qualifications are the whole story. Given the full
width because the history matters more than the instant: a machine at 90 %
that has been at 90 % all day is fine, and one that reached it in ten seconds
is not.

**Label.** The document treatment: `Memory Utilization` in `text`, bold,
sentence case, left. Readout `23.3 GB / 32.0 GB` in `text`, right.

**Placement.** Band 2, full width, 9 rows.

**Grouping.** Ladder at the left, chart filling the rest, captions beneath —
the same reading order as the SYS panel, at a different scale, so the two
panels teach each other.

**Construction.**

- **Ladder**, 4 columns, `magenta`, with `72.7%` beneath it in `magenta`.
- **Area chart**, `magenta` line at full hue over a fill of `magenta` into
  `panel` at 0.18, drawn with `▁▂▃▄▅▆▇█` rather than braille, because an area
  wants a solid edge and braille gives it a stippled one.
- **In-plot extent labels**: the window maximum at the top left and the
  minimum at the bottom left, in `magenta` at 0.5, inside the plot, exactly
  as the image does — they cost no layout and they turn a shape into a
  quantity.
- **Footer**: `Available 8.7 GB` left, `Cached 6.3 GB` centre, `Swap
  412.0 MB` right, in `dim` with the figures in `text`. Swap turns `orange`
  once it is non-zero and growing, because that is the moment memory stops
  being a curiosity.

**Sources.** `/proc/meminfo` (1 s).

### E. The tile rail

**Purpose.** Four quantities that are watched but rarely acted on. A tile is
a *stat tile*: an identity, one headline, a line of qualification, and a
meter — the smallest thing that can still be read at a glance.

**Placement.** Band 3, four equal tiles, 6 rows each (border, title, headline
row, caption row, meter row, border).

**Construction.** Each tile:

```
╭────────────────────────────────╮
│ ◍ Network            5.97 KB/s │  name left in `dim`, headline right in hue, bold
│ 404 Mb/s · 0.0%   Scale 266 KB │  sub-caption, `dim`, left and right
│ ▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪ │  horizontal segmented meter
╰────────────────────────────────╯
```

The meter is the ladder laid on its side: one `▪` per cell, lit in the hue,
unlit at **track**, the straddling cell at **partial**.

| # | tile | headline | sub-caption | meter | hue |
|---|---|---|---|---|---|
| 1 | Network | current rate, auto-scaled `5.97 KB/s` | link speed · error rate; and the meter's full-scale value | rate ÷ scale, the scale auto-ranging up instantly and decaying over 30 s | `net` |
| 2 | Storage | `2.4%` busy | `R 86.8 KB/s · W 95.1 KB/s`; SMART verdict if `smartctl` is present | `io_ticks` delta ÷ interval | `disk` |
| 3 | Slot | configurable | — | — | `amber` |
| 4 | System Pressure | a **word**: `LOW`, `SOME`, `HIGH` | `CPU queue 4.4 · Memory low · I/O busy 2%` | `ramp(heat)` ladder lit to the worst of the three PSI averages | ramp |

Tile 3 is the image's NPU, which Alpine has no equivalent of. It is a slot,
chosen in configuration from: `gpu`, `swap-rate`, `entropy`, `load`,
`processes`, `fan`, `throttle` (thermal throttle events since boot), `bus`
(the Copal NATS bus's connection state, on a fleet node). Default `gpu` where
a GPU counter exists, `load` otherwise.

**Sources.** `/proc/net/dev`, `/proc/diskstats`, `/proc/pressure/*` (1 s);
`smartctl` out of band at 300 s if present.

### F. The Browser — the process tree

**Purpose.** The half of the program the image does not have. Named for
NeXTSTEP's File Viewer and Smalltalk's System Browser, matching the
vocabulary staticstream uses, because these are the same ideas.

**Label.** `PROCESSES` in a title strip; readout `284 · 3 selected` or the
filter text when one is active.

**Placement.** Band 4, everything left over, minimum 6 rows.

**Construction.**

- **Columns**, configurable, default: `PID` (7, right) · `USER` (9) · `PR/NI`
  (6) · `S` (1, the state glyph) · `THR` (4, right) · `CPU%` (6, right) ·
  `MEM%` (6, right) · `RSS` (8, right) · `TIME+` (9, right) · `COMMAND`
  (flex). Header in `dim`; the sort column in `cyan` with `▾`/`▴`; clicking a
  header sorts by it, clicking again reverses.
- **Tree mode** (`t` toggles; the default). Children are indented 2 per level
  with guides `│ ├ ╰ ─` in `rule`. A collapsible parent carries `▾`/`▸`, and
  a **collapsed parent rolls its descendants up into itself**: its CPU% and
  RSS become the subtree's totals, and the count of hidden descendants rides on
  the closed triangle itself — `▸9 foot` — rather than trailing the name, where
  it would read as part of the command line. This is the feature that makes a
  browser with forty renderer processes readable, and it is why the tree is
  the default rather than the flat list.
- **Selection.** A full-width bar in `sel` with `▎` in `sel_edge` at the left.
  Multi-select with `Space`, which is what makes "stop these four" one action.
- **Filtering.** `/` opens a filter over name, command line and user, matched
  as a case-insensitive substring, shown in the title strip; `u` filters to
  one user; `K` shows or hides kernel threads (hidden by default).
- **Colour.** `COMMAND` is drawn as the program name in `text` and its
  arguments in `dim`, so a long command line stays scannable. A process whose
  state is `D` has its whole row in `orange`; a zombie's in `red`; a stopped
  process's in `yellow`. The row of this program itself is `cyan`.

**Sources.** `/proc/[0-9]*/stat` every tick; `/proc/[pid]/status` for visible
rows only; everything else on selection — Section VII.

### G. The Inspector

**Purpose.** Everything about one process, including the things that decide
whether it can be stopped.

**Placement.** A 46-column right pane of band 4 at ≥ 132 columns; otherwise a
centred overlay panel, 60 % of the screen, with a one-cell drop shadow drawn
by darkening the cells beneath it 45 % toward black. Opened with `Enter` or
`i`, closed with `Esc`.

**Construction.** Five sections, switched with `1`–`5`. Four are named as
NeXTSTEP's Inspector named them; the fifth, **Network**, is added because the
question it answers — what is this process doing on the network — is one a task
manager is asked constantly and one that `/proc` will answer if the join is
made:

1. **Attributes** — pid, ppid, pgid, sid, controlling tty, user (real and
   effective), state, nice, priority, threads, start time, elapsed, CPU time,
   context switches (voluntary and involuntary).
2. **Contents** — the full command line, wrapped and with the arguments in
   `dim`; the working directory; the executable and whether it has been
   deleted since it was mapped; the cgroup; the environment's size; open file
   descriptors counted by class (files, sockets, pipes, anon).
3. **Network** — Section VI-G-1 below.
4. **Tools** — the halt plan of Section VIII, computed and shown *before*
   anything is sent.
5. **Access** — real, effective and saved uid and gid; capabilities; the
   seccomp mode; and **the signal table**, which is the section that justifies
   the Inspector's existence:

```
   SIGNAL     DISPOSITION
   TERM  15   trapped      the program has installed a handler
   INT    2   default
   HUP    1   ignored      nohup, or a daemon that has detached
   QUIT   3   default
   USR1  10   trapped
   KILL   9   default      cannot be caught, blocked or ignored
```

Read from the `SigIgn`, `SigCgt` and `SigBlk` masks in `/proc/[pid]/status` —
three hexadecimal words that every Linux process publishes and no common task
manager shows. `trapped` is `orange`, `ignored` is `dim`, `blocked` is `red`,
`default` is `green`. This is the answer to "look for the correct program to
halt to get through the traps": the traps are printed, and the plan below
routes around them.

#### G-1. The Network section, and the relation it rests on

**Purpose.** Which ports a process is listening on, who it is connected to, and
whether anything is stuck.

**The relation.** Linux does not keep a per-process network table. It keeps two
halves of one, and they are joined by an inode:

- `/proc/[pid]/fd/*` is a directory of symbolic links. A socket's link reads
  back as the literal string **`socket:[12345]`**, where the number is the
  socket's inode.
- `/proc/net/tcp`, `tcp6`, `udp`, `udp6` and `unix` each list every socket on
  the machine, one per line, with local and remote address, connection state,
  send and receive queue depths — and, in a column, **that same inode**.

Match them and a file descriptor becomes an address. That is the whole
mechanism, and it is what `ss -p`, `lsof -i` and `netstat -p` do; this program
does it inline, for one process, on selection, so no other tool has to be
installed on the node. The socket table is read once per Inspector open — five
small files — and the process's descriptors are joined against it.

The addresses themselves are hexadecimal and little-endian: `0100007F:0035` is
127.0.0.1 port 53, and an IPv6 address is four little-endian 32-bit words, with
the IPv4-mapped form shown as IPv4 because that is what it is.

**What it cannot give, and says so.** There is no per-process byte *rate* in
`/proc` at all. Per-process network accounting needs taskstats, netlink, cgroup
counters or eBPF, none of which is a file read and none of which is portable to
every architecture Copal runs on. The section therefore states, once, under its
summary line: *per-process byte rates are not in /proc; these are addresses and
queues.* Guessing would be worse than the gap.

**What it does give** turns out to answer most of the questions actually asked:

| shown | why it is the useful column |
|---|---|
| `LISTEN` rows first | "what is holding port 8080" is the most common question, and it should be the first line |
| the peer address | "who is it talking to", for the one process rather than the machine |
| the state | `TIME_WAIT` piling up, `CLOSE_WAIT` that never closes — both are diagnoses |
| `tx_queue` | a send queue that does not drain is the shape of a stuck peer; drawn in `orange` when non-zero |
| Unix sockets with their paths | how a desktop process is actually talking to its session |

**Construction.** Wide enough (≥ 58 columns) it is a table — `PROTO`, `LOCAL`,
`REMOTE`, `STATE`, with `tx` right-aligned when it is non-zero. Narrower, it is
one socket to a line with the **state first**, because in a narrow pane the
state is the column worth a glance. `LISTEN` is `green`, `ESTABLISHED` is
`text`, everything else is `dim`; the protocol is always `net`. The summary line
above reads `3 listening · 11 established · 4 unix`.

**Sources and cadence.** `/proc/[pid]/fd` (a directory scan) and the five
`/proc/net` files, **on selection and on Inspector open only** — row 9 of
Section VII-A. On a process holding ten thousand descriptors this is the most
expensive read in the program, which is exactly why it does not happen on the
tick.

**Where it leads.** The halt plan reads the same list: a process holding a
listening socket is one whose stop will free a port, and that is worth saying
before it is stopped. Deferred to after phase 5; the reading is already there.

### H. Services — the halt ladder

Specified in Section VIII, because it is the part that can do harm.

### I. The Transcript and the status bar

**Purpose.** A program whose job is to end other programs must keep a record
of what it ended.

**Construction.** The Transcript is Smalltalk's — the system's running log.
The last line of it occupies the status bar; `T` opens the whole log as an
overlay, with each entry stamped `HH:MM:SS` and coloured by kind: an action
in `text`, a refusal in `yellow`, a signal sent in `orange`, a process that
died as a result in `red`, a configuration change in `dim`. It records every
signal sent with its exact equivalent command line, every refusal and why,
every escalation, and every start-up fallback ("no battery found; column 2
shows memory instead").

**The status bar**, one row on `panel`: view name and counts at the left, the
key hints for the current context in the middle in `dim` with the keys
themselves in `cyan`, and at the right the tick period (`1.0s`), a `⏸` when
paused, and `SIMULATED` in `orange` when the probe is not reading a real
`/proc`.

---

## VII. Sources and cadence

### A. The schedule

One tick drives everything; each source has a divisor. The tick is
`refresh` in the configuration, default **1.0 s**, range 0.25–10 s.

| # | source | path | every | why that period |
|---|---|---|---|---|
| 1 | CPU jiffies, total and per core | `/proc/stat` | **1 tick** | the tick *defines* every rate in the program; one read, ~2 KB |
| 2 | memory | `/proc/meminfo` | 1 tick | one read, ~1.3 KB |
| 3 | network | `/proc/net/dev` | 1 tick | one read |
| 4 | disk | `/proc/diskstats` | 1 tick | one read; `io_ticks` is already a busy-time accumulator |
| 5 | pressure | `/proc/pressure/{cpu,memory,io}` | 1 tick | three reads; already averaged over 10 s by the kernel |
| 6 | load, uptime | `/proc/loadavg`, `/proc/uptime` | 1 tick | trivial |
| 7 | the process list | `readdir /proc` + `/proc/[pid]/stat` | 1 tick | one directory scan and one small read per process — the dominant cost, and the reason for row 8 |
| 8 | per-process status | `/proc/[pid]/status` | **visible rows only**, 1 tick; every row on a sort change | ~55 lines each. On a 400-process machine, reading it for everything is 22,000 lines a second to display 30 of them |
| 9 | command line, cgroup, exe, cwd, fds | `/proc/[pid]/{cmdline,cgroup,exe,cwd,fd}` | **on selection**, and on Inspector open | `fd` is a directory scan; on a process holding 10,000 descriptors it is the most expensive read in the program |
| 9a | the socket table, joined to those fds by inode | `/proc/net/{tcp,tcp6,udp,udp6,unix}` | with row 9 | five small files, read once per Inspector open; Section VI-G-1 |
| 10 | thermal | `/sys/class/thermal/*/temp`, `hwmon` | **2 ticks** | zones update on their own schedule, often 1–2 s; on some ARM firmware a read is a mailbox round trip |
| 11 | GPU busy | `/sys/class/drm/card*/device/gpu_busy_percent`, `/sys/class/devfreq/*/load` | 2 ticks | as above |
| 12 | battery | `/sys/class/power_supply/*/{capacity,status,power_now,current_now,voltage_now,energy_now,energy_full}` | **5 s** | the gauge chip itself updates every few seconds; sampling faster adds noise, not resolution, and on some ACPI firmware each read is a real cost to the thing being measured |
| 13 | cpufreq, governor | `/sys/devices/system/cpu/cpu*/cpufreq/scaling_cur_freq` | 2 ticks; governor 60 s | one read per core |
| 14 | topology and core classes | `.../topology/*`, `cpu_capacity`, `cpuinfo_max_freq` | **once**, then 60 s | changes only when a core is hotplugged |
| 15 | the uid→name map | `/etc/passwd` | once, then on `mtime` change | |
| 16 | OpenRC service map | `/run/openrc/started/*`, cgroup paths | 10 s | how a supervised daemon is recognised |
| 17 | SMART | `smartctl -H` if installed | **300 s**, out of band, on a worker thread | it forks, it can block on a sleeping disk, and the answer changes monthly |

### B. The rules behind the schedule

1. **Nothing forks on the tick.** Rows 1–16 are file reads. The one fork
   (row 17) runs on its own thread at 1/300 the rate, and a slow answer
   simply arrives late.
2. **Cost scales with what is shown, not with what exists.** Rows 8 and 9 are
   the statement of that: a machine with 4,000 processes costs no more per
   tick than one with 40, except for the directory scan.
3. **Every reading carries its own age.** A value sampled 4 seconds ago is
   drawn with its last sample time, and any reading older than three times
   its own period is drawn in `dim` — an instrument that has stopped must not
   look like one reading zero.
4. **The program throttles itself.** On battery below 15 %, or when its own
   CPU share exceeds 2 % for ten consecutive ticks, the tick doubles, up to
   4 s, and the Transcript says so. A task manager that is in the top five of
   its own table has failed at its job.
5. **Paused means paused.** `Space` in the dashboard freezes sampling
   entirely, for reading a chart or taking a screenshot; the status bar shows
   `⏸` and the histories keep their contents.

### C. Where the readings go

Each series is a ring buffer of `f32`, sized to the widest plot the screen
could want (1024 samples, ~17 minutes at 1 s) and sampled down for narrower
plots by taking the maximum of each bucket — the maximum and not the mean,
because a spike that is averaged away is exactly the event the history exists
to show.

### D. When there is no `/proc`

The probe has two back ends chosen at compile time. On Linux it reads the
paths above. On anything else — the Mac this is written at — a **simulated**
source produces plausible, seeded, slowly-drifting values and a synthetic
process tree, so that every widget can be developed and screenshotted without
a virtual machine. It is never silent about it: the status bar shows
`SIMULATED` in `orange`, and the first Transcript line says so.

---

## VIII. Acting: selection, Services, and the halt ladder

### A. The principle

Ending a process is the one irreversible thing this program does. Every path
to it therefore (1) states the exact equivalent command line before acting,
(2) requires a confirmation that is not the same key as the one that opened
the dialogue, and (3) writes what happened to the Transcript.

### B. Services

`k` on the selection opens the **Services** menu — NeXTSTEP's word, where an
application's selection is sent to a service:

| item | signal | note |
|---|---|---|
| Terminate | `TERM` 15 | the polite one; shown struck through when the target ignores it |
| Interrupt | `INT` 2 | what `Ctrl-C` sends |
| Hangup | `HUP` 1 | for daemons: usually *reload*, not stop |
| Quit | `QUIT` 3 | terminates *and dumps core* — labelled so |
| Stop / Continue | `STOP` / `CONT` | pause and resume; neither can be caught |
| User 1 / User 2 | `USR1` / `USR2` | whatever the program has made of them |
| Kill | `KILL` 9 | cannot be caught; the target gets no chance to clean up |
| Renice… | — | −20…19, with a note when it needs privilege |
| **Halt plan…** | — | `x`; Section VIII-D |

Each item shows the target's disposition for that signal beside it, from the
Inspector's signal table: `default`, `trapped`, `ignored`, `blocked`. An
ignored signal is not hidden — it is shown greyed with the reason, because
"why did nothing happen" is worse than a disabled menu item.

### C. Finding the right addressee

Before offering anything, the plan diagnoses the target.

1. **Refusals, absolutely.** pid 1; `kthreadd` and every kernel thread; and a
   process whose uid is not ours when we are not root — in that last case the
   plan says what privilege would be needed rather than failing at the syscall.
2. **States that change the answer.**
   - `Z` (zombie): *no signal can affect it.* It is already dead and waiting
     to be reaped. The plan offers **the parent** instead and says why.
   - `D` (uninterruptible sleep): the signal will be queued and delivered
     only when the process leaves the kernel — which, for a process stuck on
     dead storage or a hung NFS mount, may be never. The plan says so and
     does not pretend `KILL` is different.
   - `T`/`t` (stopped/traced): a stopped process will not run its own handler
     until it is continued, so `TERM` alone may appear to do nothing. The
     plan pairs `TERM` with `CONT` — which is what actually ends it.
3. **The wrapper walk.** Walk up the parent chain while the parent is a known
   wrapper — `sh`, `ash`, `bash`, `dash`, `zsh`, `busybox`, `env`, `sudo`,
   `doas`, `setsid`, `nohup`, `timeout`, `stdbuf`, `tini`, `s6-supervise`,
   `runsv`, `supervise-daemon`, `start-stop-daemon` — **and** that parent has
   this process as its only child. That is a wrapper standing in front of the
   real program. The plan then offers four addressees, in the order that
   works most often:

   | addressee | equivalent | when it is right |
   |---|---|---|
   | the process group | `kill -TERM -<pgid>` | almost always: it is what `Ctrl-C` does, and it reaches every member, so one member's trap cannot hold the others |
   | the wrapper | `kill -TERM <wrapper pid>` | when the wrapper would otherwise respawn or report the child's death as its own failure |
   | the process itself | `kill -TERM <pid>` | when it is the whole job |
   | the session | `kill -TERM -<sid>` | last, and only offered when the session leader is the wrapper: it reaches a detached job that has escaped its group |

4. **The service check.** If the target matches an OpenRC pidfile under
   `/run/openrc/started/`, or sits in a `system.slice` cgroup, **the correct
   halt is not a signal at all** — it is `rc-service <name> stop`. The plan
   says so, shows that command as the primary action, and notes that
   signalling a supervised daemon usually gets it restarted three seconds
   later. This is the most common way a kill "does not work", and it is not a
   trap at all; it is a supervisor doing its job.

### D. The ladder

Having chosen an addressee, the plan builds an escalation:

```
  1. TERM   →  wait 2.0 s   [trapped: the program has a handler; it may be
                             cleaning up, so the wait is real]
  2. INT    →  wait 2.0 s   [skipped: ignored]
  3. HUP    →  wait 2.0 s
  4. KILL                   [cannot be caught; the last step, always]
```

Rules: a signal the mask says is **ignored** is struck out and skipped. One
that is **caught** is marked *trapped* and its grace period is doubled,
because a handler that is doing real cleanup deserves the time and one that
is swallowing the signal will be found out by the next step anyway. `KILL` is
always last and always present. Between steps the plan re-reads the target;
if it is gone the ladder stops and the Transcript records which rung worked
— *which is the piece of knowledge the whole feature exists to produce.*

The ladder never runs unattended: each rung is shown with its wait counting
down, `Esc` stops it where it is, and the Transcript records both what was
sent and what stopping left behind.

### E. The confirmation

```
╭─ Halt plan ───────────────────────────────────────────╮
│ 4821  node  (server.js)                               │
│ addressee: process group 4820  (parent `sh` is a       │
│            wrapper with one child)                     │
│ TERM is trapped — the program has a handler            │
│                                                        │
│   kill -TERM -4820                                     │
│                                                        │
│ [Enter] send   [e] escalate stepwise   [Esc] cancel    │
╰────────────────────────────────────────────────────────╯
```

The command line is shown because it can be read, checked, and typed by hand
if the user would rather. A task manager that hides what it is about to do is
asking to be trusted; one that prints it is asking to be checked.

---

## IX. Configuration

`~/.config/copal/taskman.conf`, `key = value`, `#` comments — the same shape
and the same directory as the rest of Copal's user configuration.

```ini
refresh        = 1.0          # seconds per tick, 0.25 .. 10
view           = split        # split | dashboard | browser
charset        = auto         # auto | full | blocks | ascii
color          = auto         # auto | truecolor | 16
sys.meters     = cpu,battery,temp,gpu
sys.readout    = remaining    # power | remaining | uptime | load | temp | pressure
slot.tile      = gpu          # gpu | swap-rate | entropy | load | processes | fan | throttle | bus
history.span   = 300          # seconds of history to keep on screen where it fits
cpu.scale      = total        # total (100% = the whole machine) | core (100% = one core)
browser.tree   = true
browser.kernel = false
browser.sort   = cpu          # cpu | mem | pid | time | name | user
halt.grace     = 2.0          # seconds between rungs of the ladder
halt.confirm   = true         # never false without an explicit edit
theme          = copal        # copal | mono
```

Every key is also a command-line flag of the same name, and `--dump-config`
writes the effective configuration with its defaults commented — so the file
never has to be written from documentation.

---

## X. Build and packaging

### A. The Makefile, which is the front door

| target | does |
|---|---|
| `make` / `make build` | `cargo build --release`, into `target/release/copal-tm` |
| `make run` | build and run it in this terminal |
| `make debug` | `cargo build` and run under the debug profile with `COPAL_TM_LOG=debug`, logging to `/tmp/copal-tm.log` so the log does not fight the alternate screen for the terminal |
| `make test` | `cargo test --workspace` |
| `make check` | `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test` |
| `make fmt` | `cargo fmt` |
| `make demo` | run against the simulated probe regardless of platform |
| `make shot` | render **one frame** to stdout and exit — `copal-tm --frame WxH`. This is how a build is looked at without a terminal session, how every breakpoint of Section V-C is checked, and what the golden-frame tests render. `FRAME=80x24 make shot` for the narrow case; `make plain` strips the escapes |
| `make install` | `cargo install --path crates/copal-tm --root ~/.local` |
| `make dist` | `cargo package -p …` for each crate, checked but not pushed |
| `make publish` | the four `cargo publish` calls in dependency order — **and it refuses unless `make check` has passed in this tree** |
| `make clean` | `cargo clean` |
| `make help` | the list above |

### B. Both machines

The program is for Alpine and is written on a Mac. The probe's Linux back end
is `#[cfg(target_os = "linux")]`; everything else — the canvas, the widgets,
the layout, the event loop, the halt ladder's *reasoning* — is portable, and
on macOS the simulated probe feeds it. `make demo` is therefore a complete
rehearsal of the interface on a machine that has no `/proc` at all, which is
what makes the design of Sections V and VI checkable before any of it is
carried to the target.

### C. Publishing

Four crates, published in dependency order: `copal-tm-tty`, `copal-tm-probe`,
`copal-tm-ui`, `copal-tm`. Each carries `description`, `license = "MIT"`,
`repository`, `keywords` and `categories`. Nothing is published until a build
has been seen and `make check` is clean.

### D. Installing on Copal

Stage 17 of `copal-prep.sh` gains a step that builds this checkout the way
`copal-build` builds any `Cargo.toml` checkout, and a menu entry beside the
other full-install tools. The binary is one file with no shared-library needs
beyond musl's, which is the whole reason for the no-dependency rule.

---

## XI. Phases, and what each must pass

| phase | delivers | passes when |
|---|---|---|
| **0** | `copal-tm-tty`: colour, canvas, glyphs, raw mode, keys | the canvas writes only damaged cells; a sequence split across two reads decodes; braille bits match Unicode. *(Done: 11 tests.)* |
| **1** | `copal-tm-probe`: the Linux back end and the simulated one, behind one interface | a known `SigCgt` mask decodes; a hexadecimal socket address reads little-endian; the simulated tree contains every case the halt plan reasons about; a native probe reads this machine. *(Done: 28 tests.)* |
| **2** | `copal-tm-ui` groups A–E, and the layout | every breakpoint of Section V-C renders at its width without overlap or panic; a golden frame shows every group. *(Done: 28 widget tests, and 7 sizes from 40×12 to 200×60 plus the ASCII fallback.)* |
| **3** | groups F and G: the Browser, the tree, the Inspector | a collapsed parent's rolled-up RSS equals the sum of its subtree; the signal table decodes a mask; the Network section lists a listening port. *(Done.)* |
| **4** | group H: Services and the halt ladder | the addressee walk picks the process group for a `sh`-wrapped child; a zombie offers its parent; an OpenRC-supervised process offers `rc-service stop`; an ignored signal is struck out and `KILL` always remains; nothing is sent without confirmation. *(Done.)* |
| **5** | packaging | `make check` clean on both machines; a build seen; then publish. *(Build seen at 95 tests; publish pending.)* |

---

## XII. Open questions

1. **Mouse.** Reporting is enabled and clicks select rows and toggle legend
   items. Whether to support drag-to-brush on the charts is deferred to
   phase 2, when the cost is known.
2. **The bus.** On a fleet node the Copal NATS bus could carry the same
   sample the dashboard draws, making `copal-tm` a node's local face of
   `copal fleet watch`. Deferred; the slot tile reserves `bus` for it.
3. **Recording.** The ring buffers are exactly the shape Static Stream
   records. A `--record` flag writing a `.sstr` of a session's samples would
   cost little and would make a performance complaint reproducible. Deferred
   to after phase 5.

---

## References

[1] Copal Linux, `docs/THEME.md` — the full install and stage 17.
[2] Copal Linux, `docs/staticstream-project-lab-report.md` — the no-dependency
    rule, the Makefile-as-front-door convention, and the NeXTSTEP/Smalltalk
    vocabulary this program shares.
[3] ascitty, `crates/ascitty-tty/src/term.rs` — raw mode by `stty`, the
    `CSI 18 t` size handshake, and the restoring panic hook.
[4] Linux kernel, `Documentation/filesystems/proc.rst` — `/proc/[pid]/stat`,
    `status` and the `SigIgn`/`SigCgt`/`SigBlk` masks.
[5] Linux kernel, `Documentation/accounting/psi.rst` — pressure stall
    information.
[6] A. Goldberg, *Smalltalk-80: The Interactive Programming Environment*,
    Addison-Wesley, 1984 — Browser, Inspector, Transcript.
[7] NeXT Computer, *NeXTSTEP User Interface Guidelines*, 1992 — the File
    Viewer, the Inspector's sections, and Services.
