// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Paul Richeson
//! The terminal layer of Copal Task Manager.
//!
//! Four things and nothing else: colour, a damage-tracked cell canvas, the
//! glyph vocabulary and its fallbacks, and the terminal itself.  It knows
//! nothing about processes, meters or graphs - `copal-tm-ui` draws those into
//! the canvas this crate owns.

pub mod canvas;
pub mod color;
pub mod glyphs;
pub mod keys;
pub mod term;

pub use canvas::{Canvas, Cell, Rect, BOLD, DIM, UNDER};
pub use color::{ramp, Rgb};
pub use glyphs::{half_block, Braille, Charset, EIGHTHS, EIGHTHS_H};
pub use keys::{Key, KeyParser};
pub use term::{input_channel, restore, Term};
