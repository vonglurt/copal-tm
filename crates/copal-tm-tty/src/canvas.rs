// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Paul Richeson
//! A cell canvas with damage tracking.
//!
//! Everything draws into `cur`.  `flush` walks `cur` against `prev` and writes
//! only the cells that changed, coalescing runs so that a panel whose numbers
//! tick once a second costs a few dozen bytes rather than a full screen.

use crate::color::Rgb;
use std::fmt::Write as _;

pub const BOLD: u8 = 1;
pub const DIM: u8 = 2;
pub const UNDER: u8 = 4;

#[derive(Clone, Copy, PartialEq)]
pub struct Cell {
    pub ch: char,
    pub fg: Rgb,
    pub bg: Rgb,
    pub attr: u8,
}

impl Cell {
    pub fn blank(bg: Rgb) -> Cell {
        Cell {
            ch: ' ',
            fg: Rgb(0, 0, 0),
            bg,
            attr: 0,
        }
    }
}

/// A rectangle in cells.  All layout is done by cutting these up.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const ZERO: Rect = Rect {
        x: 0,
        y: 0,
        w: 0,
        h: 0,
    };

    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect {
            x,
            y,
            w: w.max(0),
            h: h.max(0),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }

    pub fn right(&self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }

    /// Shrink on every side.
    pub fn inset(&self, dx: i32, dy: i32) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.w - 2 * dx, self.h - 2 * dy)
    }

    pub fn pad(&self, top: i32, right: i32, bottom: i32, left: i32) -> Rect {
        Rect::new(
            self.x + left,
            self.y + top,
            self.w - left - right,
            self.h - top - bottom,
        )
    }

    /// Take `n` rows off the top; returns (taken, rest).
    pub fn cut_top(&self, n: i32) -> (Rect, Rect) {
        let n = n.clamp(0, self.h);
        (
            Rect::new(self.x, self.y, self.w, n),
            Rect::new(self.x, self.y + n, self.w, self.h - n),
        )
    }

    pub fn cut_bottom(&self, n: i32) -> (Rect, Rect) {
        let n = n.clamp(0, self.h);
        (
            Rect::new(self.x, self.bottom() - n, self.w, n),
            Rect::new(self.x, self.y, self.w, self.h - n),
        )
    }

    pub fn cut_left(&self, n: i32) -> (Rect, Rect) {
        let n = n.clamp(0, self.w);
        (
            Rect::new(self.x, self.y, n, self.h),
            Rect::new(self.x + n, self.y, self.w - n, self.h),
        )
    }

    pub fn cut_right(&self, n: i32) -> (Rect, Rect) {
        let n = n.clamp(0, self.w);
        (
            Rect::new(self.right() - n, self.y, n, self.h),
            Rect::new(self.x, self.y, self.w - n, self.h),
        )
    }

    /// Cut into columns by weight, leaving `gap` between each.
    pub fn columns(&self, weights: &[i32], gap: i32) -> Vec<Rect> {
        let total: i32 = weights.iter().sum::<i32>().max(1);
        let avail = self.w - gap * (weights.len() as i32 - 1).max(0);
        let mut out = Vec::with_capacity(weights.len());
        let mut x = self.x;
        let mut used = 0;
        for (i, wgt) in weights.iter().enumerate() {
            let w = if i + 1 == weights.len() {
                avail - used
            } else {
                avail * wgt / total
            };
            out.push(Rect::new(x, self.y, w, self.h));
            x += w + gap;
            used += w;
        }
        out
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }
}

pub struct Canvas {
    pub w: i32,
    pub h: i32,
    cur: Vec<Cell>,
    prev: Vec<Cell>,
    force: bool,
    /// Set false for terminals without direct colour; cells fall back to ANSI 16.
    pub truecolor: bool,
}

impl Canvas {
    pub fn new(w: i32, h: i32) -> Canvas {
        let n = (w.max(1) * h.max(1)) as usize;
        Canvas {
            w: w.max(1),
            h: h.max(1),
            cur: vec![Cell::blank(Rgb(0, 0, 0)); n],
            prev: vec![Cell::blank(Rgb(1, 1, 1)); n],
            force: true,
            truecolor: true,
        }
    }

    pub fn resize(&mut self, w: i32, h: i32) {
        if w == self.w && h == self.h {
            return;
        }
        self.w = w.max(1);
        self.h = h.max(1);
        let n = (self.w * self.h) as usize;
        self.cur = vec![Cell::blank(Rgb(0, 0, 0)); n];
        self.prev = vec![Cell::blank(Rgb(1, 1, 1)); n];
        self.force = true;
    }

    /// Ask for the next flush to write every cell, after the screen was
    /// disturbed by something that is not us.
    pub fn invalidate(&mut self) {
        self.force = true;
    }

    pub fn area(&self) -> Rect {
        Rect::new(0, 0, self.w, self.h)
    }

    fn idx(&self, x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            None
        } else {
            Some((y * self.w + x) as usize)
        }
    }

    pub fn clear(&mut self, bg: Rgb) {
        for c in self.cur.iter_mut() {
            *c = Cell::blank(bg);
        }
    }

    pub fn set(&mut self, x: i32, y: i32, cell: Cell) {
        if let Some(i) = self.idx(x, y) {
            self.cur[i] = cell;
        }
    }

    pub fn get(&self, x: i32, y: i32) -> Option<Cell> {
        self.idx(x, y).map(|i| self.cur[i])
    }

    pub fn put(&mut self, x: i32, y: i32, ch: char, fg: Rgb, bg: Rgb, attr: u8) {
        self.set(x, y, Cell { ch, fg, bg, attr });
    }

    /// Paint the ground of a rectangle without touching its glyphs' shape.
    pub fn fill(&mut self, r: Rect, ch: char, fg: Rgb, bg: Rgb) {
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                self.put(x, y, ch, fg, bg, 0);
            }
        }
    }

    pub fn fill_bg(&mut self, r: Rect, bg: Rgb) {
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                if let Some(i) = self.idx(x, y) {
                    self.cur[i].bg = bg;
                }
            }
        }
    }

    /// Draw `s` clipped to `max` columns; returns columns drawn.
    // A cell has a position, a glyph, two colours and its attributes; a
    // drawing primitive that takes them takes seven or eight arguments, and
    // bundling them into a struct at every call site would cost more than it
    // saved.
    #[allow(clippy::too_many_arguments)]
    pub fn text(&mut self, x: i32, y: i32, max: i32, s: &str, fg: Rgb, bg: Rgb, attr: u8) -> i32 {
        let mut n = 0;
        for ch in s.chars() {
            if n >= max {
                break;
            }
            self.put(x + n, y, ch, fg, bg, attr);
            n += 1;
        }
        n
    }

    /// Draw `s` so that its last column is `right - 1`.  Numeric readouts are
    /// all right-aligned, which is what makes a column of figures scannable.
    // A cell has a position, a glyph, two colours and its attributes; a
    // drawing primitive that takes them takes seven or eight arguments, and
    // bundling them into a struct at every call site would cost more than it
    // saved.
    #[allow(clippy::too_many_arguments)]
    pub fn text_right(
        &mut self,
        right: i32,
        y: i32,
        max: i32,
        s: &str,
        fg: Rgb,
        bg: Rgb,
        attr: u8,
    ) -> i32 {
        let len = s.chars().count() as i32;
        let len = len.min(max);
        let start = right - len;
        let skip = s.chars().count() as i32 - len;
        let tail: String = s.chars().skip(skip.max(0) as usize).collect();
        self.text(start, y, len, &tail, fg, bg, attr)
    }

    pub fn text_center(&mut self, r: Rect, y: i32, s: &str, fg: Rgb, bg: Rgb, attr: u8) -> i32 {
        let len = (s.chars().count() as i32).min(r.w);
        self.text(r.x + (r.w - len) / 2, y, len, s, fg, bg, attr)
    }

    // A cell has a position, a glyph, two colours and its attributes; a
    // drawing primitive that takes them takes seven or eight arguments, and
    // bundling them into a struct at every call site would cost more than it
    // saved.
    #[allow(clippy::too_many_arguments)]
    pub fn hline(&mut self, x: i32, y: i32, w: i32, ch: char, fg: Rgb, bg: Rgb, attr: u8) {
        for i in 0..w {
            self.put(x + i, y, ch, fg, bg, attr);
        }
    }

    // A cell has a position, a glyph, two colours and its attributes; a
    // drawing primitive that takes them takes seven or eight arguments, and
    // bundling them into a struct at every call site would cost more than it
    // saved.
    #[allow(clippy::too_many_arguments)]
    pub fn vline(&mut self, x: i32, y: i32, h: i32, ch: char, fg: Rgb, bg: Rgb, attr: u8) {
        for i in 0..h {
            self.put(x, y + i, ch, fg, bg, attr);
        }
    }

    /// Diff against the last frame and append the escape sequence.
    pub fn flush(&mut self, out: &mut String) {
        let mut cx: i32 = -9;
        let mut cy: i32 = -9;
        let mut fg: Option<Rgb> = None;
        let mut bg: Option<Rgb> = None;
        let mut attr: u8 = 0xff;
        for y in 0..self.h {
            for x in 0..self.w {
                let i = (y * self.w + x) as usize;
                let c = self.cur[i];
                if !self.force && c == self.prev[i] {
                    continue;
                }
                if cy != y || cx != x {
                    let _ = write!(out, "\x1b[{};{}H", y + 1, x + 1);
                    cx = x;
                    cy = y;
                }
                if attr != c.attr {
                    out.push_str("\x1b[0m");
                    fg = None;
                    bg = None;
                    if c.attr & BOLD != 0 {
                        out.push_str("\x1b[1m");
                    }
                    if c.attr & DIM != 0 {
                        out.push_str("\x1b[2m");
                    }
                    if c.attr & UNDER != 0 {
                        out.push_str("\x1b[4m");
                    }
                    attr = c.attr;
                }
                if fg != Some(c.fg) {
                    if self.truecolor {
                        let _ = write!(out, "\x1b[38;2;{};{};{}m", c.fg.0, c.fg.1, c.fg.2);
                    } else {
                        let a = c.fg.ansi16();
                        let _ = if a < 8 {
                            write!(out, "\x1b[{}m", 30 + a)
                        } else {
                            write!(out, "\x1b[{}m", 90 + a - 8)
                        };
                    }
                    fg = Some(c.fg);
                }
                if bg != Some(c.bg) {
                    if self.truecolor {
                        let _ = write!(out, "\x1b[48;2;{};{};{}m", c.bg.0, c.bg.1, c.bg.2);
                    } else {
                        let a = c.bg.ansi16();
                        let _ = if a < 8 {
                            write!(out, "\x1b[{}m", 40 + a)
                        } else {
                            write!(out, "\x1b[{}m", 100 + a - 8)
                        };
                    }
                    bg = Some(c.bg);
                }
                out.push(c.ch);
                cx += 1;
                self.prev[i] = c;
            }
        }
        self.force = false;
        out.push_str("\x1b[0m");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_use_every_cell() {
        let r = Rect::new(0, 0, 100, 10);
        let cols = r.columns(&[1, 2, 1], 1);
        assert_eq!(cols.len(), 3);
        assert_eq!(cols[2].right(), 100);
        for c in &cols {
            assert!(c.w > 0);
        }
    }

    #[test]
    fn flush_writes_only_damage() {
        let mut c = Canvas::new(10, 2);
        c.clear(Rgb::hex(0x101010));
        let mut s = String::new();
        c.flush(&mut s);
        assert!(!s.is_empty());
        let mut s2 = String::new();
        c.flush(&mut s2);
        assert_eq!(s2, "\x1b[0m", "an unchanged frame costs one reset");
        c.put(3, 1, 'x', Rgb(255, 255, 255), Rgb::hex(0x101010), 0);
        let mut s3 = String::new();
        c.flush(&mut s3);
        assert!(s3.contains('x') && s3.contains("\x1b[2;4H"));
    }

    #[test]
    fn right_aligned_text_ends_where_asked() {
        let mut c = Canvas::new(20, 1);
        c.clear(Rgb(0, 0, 0));
        c.text_right(10, 0, 20, "42%", Rgb(255, 255, 255), Rgb(0, 0, 0), 0);
        assert_eq!(c.get(9, 0).unwrap().ch, '%');
        assert_eq!(c.get(7, 0).unwrap().ch, '4');
    }
}
