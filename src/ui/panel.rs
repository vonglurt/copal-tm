// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Paul Richeson
//! The panel and its title strip — Section V-A of the design report.
//!
//! Two title treatments, because the reference image has two and the
//! difference carries meaning:
//!
//! - **Matrix**, for the narrow cards: an inset well, the title letter-spaced
//!   in cyan, the unlit cells of the matrix filling the gap, and one readout
//!   right-aligned. These are *instruments*.
//! - **Document**, for the wide card: a sentence-case label in plain text, no
//!   filler, readout still right. This is a *document*.

use crate::tty::{Canvas, Rect, Rgb, BOLD};

use crate::ui::theme::Theme;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Title {
    Matrix,
    Document,
}

pub struct Panel<'a> {
    pub title: &'a str,
    /// The headline metric, right-aligned in the strip.
    pub readout: &'a str,
    pub style: Title,
    /// The colour of the title and readout; `None` takes the theme's cyan.
    pub accent: Option<Rgb>,
    pub focused: bool,
}

impl<'a> Panel<'a> {
    pub fn new(title: &'a str, readout: &'a str) -> Panel<'a> {
        Panel {
            title,
            readout,
            style: Title::Matrix,
            accent: None,
            focused: false,
        }
    }

    pub fn document(mut self) -> Self {
        self.style = Title::Document;
        self
    }

    pub fn accent(mut self, c: Rgb) -> Self {
        self.accent = Some(c);
        self
    }

    pub fn focused(mut self, f: bool) -> Self {
        self.focused = f;
        self
    }
}

/// Draw the frame and the title strip; return the body rectangle inside.
///
/// A panel is three rows taller than its body: border, strip, body…, border.
pub fn draw(c: &mut Canvas, t: &Theme, r: Rect, p: &Panel) -> Rect {
    if r.w < 4 || r.h < 3 {
        return Rect::ZERO;
    }
    let fill = if p.focused { t.panel_hi } else { t.panel };
    let border = if p.focused {
        t.cyan.on(t.rule, 0.5)
    } else {
        t.rule
    };
    let accent = p.accent.unwrap_or(t.cyan);
    let [tl, tr, bl, br, h, v] = t.cs.frame();

    c.fill(r, ' ', t.text, fill);
    c.hline(r.x + 1, r.y, r.w - 2, h, border, fill, 0);
    c.hline(r.x + 1, r.bottom() - 1, r.w - 2, h, border, fill, 0);
    c.vline(r.x, r.y + 1, r.h - 2, v, border, fill, 0);
    c.vline(r.right() - 1, r.y + 1, r.h - 2, v, border, fill, 0);
    c.put(r.x, r.y, tl, border, fill, 0);
    c.put(r.right() - 1, r.y, tr, border, fill, 0);
    c.put(r.x, r.bottom() - 1, bl, border, fill, 0);
    c.put(r.right() - 1, r.bottom() - 1, br, border, fill, 0);

    let strip = Rect::new(r.x + 1, r.y + 1, r.w - 2, 1);
    title_strip(c, t, strip, p, accent, fill);
    Rect::new(r.x + 1, r.y + 2, r.w - 2, r.h - 3)
}

fn title_strip(c: &mut Canvas, t: &Theme, s: Rect, p: &Panel, accent: Rgb, fill: Rgb) {
    if s.is_empty() {
        return;
    }
    let ground = if p.style == Title::Matrix {
        t.inset
    } else {
        fill
    };
    c.fill(s, ' ', t.text, ground);

    let readout_w = p.readout.chars().count() as i32;
    let right = s.right();

    match p.style {
        Title::Matrix => {
            // Letter-spaced, as a dot-matrix marquee sets a label.
            let spaced: String = p
                .title
                .chars()
                .flat_map(|ch| [ch, ' '])
                .collect::<String>()
                .trim_end()
                .to_string();
            // The readout is the panel's headline and always wins; a title
            // that does not fit beside it is clipped, never overlapped.
            let room = (s.w - 2 - readout_w - 1).max(0);
            let tw = c.text(s.x + 1, s.y, room, &spaced, accent, ground, BOLD);
            // The unlit cells of the matrix: the whole trick of the strip.
            let gap_x = s.x + 1 + tw + 1;
            let gap_w = right - readout_w - 1 - gap_x;
            if gap_w > 0 {
                let dot = t.cs.matrix_dot();
                for i in 0..gap_w {
                    c.put(gap_x + i, s.y, dot, t.matrix_unlit(), ground, 0);
                }
            }
        }
        Title::Document => {
            c.text(
                s.x + 1,
                s.y,
                (s.w - 2 - readout_w).max(0),
                p.title,
                t.text,
                ground,
                BOLD,
            );
        }
    }
    if readout_w > 0 {
        c.text_right(right, s.y, s.w / 2, p.readout, accent, ground, BOLD);
    }
}

/// A one-cell drop shadow: darken what is under it rather than draw over it,
/// so an overlay reads as floating above the panels it covers.
pub fn shadow(c: &mut Canvas, r: Rect) {
    for y in r.y + 1..r.bottom() + 1 {
        for x in r.x + 1..r.right() + 1 {
            if r.contains(x, y) {
                continue;
            }
            if let Some(mut cell) = c.get(x, y) {
                cell.bg = cell.bg.dark(0.45);
                cell.fg = cell.fg.dark(0.45);
                c.set(x, y, cell);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tty::Charset;

    #[test]
    fn a_panel_body_is_three_rows_shorter_than_its_frame() {
        let t = Theme::copal(Charset::Full);
        let mut c = Canvas::new(40, 10);
        let body = draw(
            &mut c,
            &t,
            Rect::new(0, 0, 40, 10),
            &Panel::new("SYS", "9.4 W"),
        );
        assert_eq!(body, Rect::new(1, 2, 38, 7));
    }

    #[test]
    fn the_title_is_letter_spaced_and_the_readout_is_right_aligned() {
        let t = Theme::copal(Charset::Full);
        let mut c = Canvas::new(30, 4);
        draw(
            &mut c,
            &t,
            Rect::new(0, 0, 30, 4),
            &Panel::new("SYS", "9.4 W"),
        );
        let row: String = (0..30).map(|x| c.get(x, 1).unwrap().ch).collect();
        assert!(row.contains("S Y S"), "{row}");
        assert!(row.contains("9.4 W"), "{row}");
        assert_eq!(
            c.get(28, 1).unwrap().ch,
            'W',
            "the readout ends at the last body cell"
        );
        assert!(
            row.contains('\u{25aa}'),
            "the unlit matrix fills the gap: {row}"
        );
    }

    #[test]
    fn a_document_title_has_no_matrix_filler() {
        let t = Theme::copal(Charset::Full);
        let mut c = Canvas::new(40, 4);
        draw(
            &mut c,
            &t,
            Rect::new(0, 0, 40, 4),
            &Panel::new("Memory Utilization", "23.3 GB / 32.0 GB").document(),
        );
        let row: String = (0..40).map(|x| c.get(x, 1).unwrap().ch).collect();
        assert!(row.contains("Memory Utilization"));
        assert!(!row.contains('\u{25aa}'));
    }

    #[test]
    fn a_panel_too_small_to_draw_returns_an_empty_body_rather_than_panicking() {
        let t = Theme::copal(Charset::Full);
        let mut c = Canvas::new(10, 10);
        assert!(draw(&mut c, &t, Rect::new(0, 0, 2, 2), &Panel::new("x", "")).is_empty());
    }
}
