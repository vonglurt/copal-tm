// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Paul Richeson
//! The widgets of Copal Task Manager: the grammar of Section V of the design
//! report, and nothing else.
//!
//! Everything here draws into a `Canvas` it is handed and holds no state of
//! its own. A widget is given a rectangle, a theme and the values to show; it
//! returns nothing or the rectangle it did not use. That is what makes the
//! whole interface testable without a terminal — every test in this crate
//! draws into a canvas and asserts on the cells.

pub mod graph;
pub mod ladder;
pub mod panel;
pub mod table;
pub mod theme;
pub mod tile;

pub use graph::{Axis, Series};
pub use ladder::{Hue, Ladder};
pub use panel::{Panel, Title};
pub use table::Col;
pub use theme::Theme;
pub use tile::Tile;
