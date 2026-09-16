// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Paul Richeson
//! copal-tm — the Copal task manager.
//!
//! An instrument panel and a process browser, in one terminal window, on a
//! machine that cannot fetch a crate.  The design it follows is written down
//! in `docs/design-lab-report.md`, group by group and cell by cell.
//!
//! Three modules, in the order they stack:
//!
//! - [`tty`] — colour, a damage-tracked cell canvas, the glyph vocabulary and
//!   its fallbacks, raw mode, key decoding.  Knows nothing above it.
//! - [`machine`] — every reading: `/proc`, `/sys`, and a simulated source for
//!   machines that have neither, plus signal dispositions and halt plans.
//!   Draws nothing.
//! - [`ui`] — the widgets: title strips, ladder meters, strip charts, stat
//!   tiles, tables.  Draws into a canvas and holds no state of its own.
//!
//! The split is the one that makes the program testable without a terminal:
//! the canvas can be flushed to a string and asserted on, and the readings
//! can come from the simulation instead of `/proc`.  It is a split into
//! modules, not into packages — there is one crate here and it is named
//! after the program.

pub mod machine;
pub mod tty;
pub mod ui;
