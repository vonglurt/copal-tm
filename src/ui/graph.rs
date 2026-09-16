// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Paul Richeson
//! The strip chart — Section VI-B and VI-D.
//!
//! Two forms of the same object. The **line** plot carries several series on a
//! sub-cell dot grid: braille gives 2×4 dots per character, so a 60×10 plot is
//! 120×40 dots. The **area** plot carries one series with a solid edge, drawn
//! with the block eighths, because an area wants a filled edge and braille
//! gives it a stippled one.
//!
//! The dot grid is generic over its resolution so the fallbacks are the same
//! code at a coarser scale: braille 2×4, half blocks 1×2, ASCII 1×1.

use crate::tty::{half_block, Braille, Canvas, Charset, Rect, Rgb, BOLD, DIM, EIGHTHS};

use crate::ui::theme::Theme;

/// Which axis a series is measured against. The right axis belongs to
/// temperature alone, and is drawn in temperature's colour.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    Left,
    Right,
}

pub struct Series<'a> {
    pub name: &'a str,
    pub color: Rgb,
    /// Oldest first, already resampled to the plot's dot-column count.
    pub data: &'a [f32],
    /// The value at the top of its axis.
    pub max: f32,
    pub visible: bool,
    pub axis: Axis,
}

/// The dot resolution of one character cell, by charset.
fn dots(cs: Charset) -> (i32, i32) {
    match cs {
        Charset::Full => (2, 4),
        Charset::Blocks => (1, 2),
        Charset::Ascii => (1, 1),
    }
}

/// How many samples a plot of this width wants.
pub fn dot_columns(cs: Charset, w: i32) -> usize {
    (w.max(0) * dots(cs).0) as usize
}

/// Draw the grid, the brush and every visible series.
///
/// `brush` is a pair of dot-column indices; the band between them is lifted a
/// shade and edged with dashed rulers. It is the *same* selection in every
/// history panel, which is the improvement on the reference image.
pub fn plot(c: &mut Canvas, t: &Theme, r: Rect, series: &[Series], brush: Option<(usize, usize)>) {
    if r.is_empty() {
        return;
    }
    let (dx, dy) = dots(t.cs);
    let (dw, dh) = (r.w * dx, r.h * dy);
    c.fill(r, ' ', t.text, t.panel);

    // The brushed band, under everything.
    if let Some((a, b)) = brush {
        let (a, b) = (a.min(b) as i32 / dx, b.max(a) as i32 / dx);
        let band = Rect::new(r.x + a, r.y, (b - a + 1).min(r.w - a), r.h);
        if !band.is_empty() {
            c.fill_bg(band, t.panel_hi);
        }
        let ruler = t.cs.ruler();
        let edge = t.text.on(t.panel_hi, 0.6);
        for &x in &[a, b] {
            if x >= 0 && x < r.w {
                c.vline(r.x + x, r.y, r.h, ruler, edge, t.panel_hi, 0);
            }
        }
    }

    // Grid rules at 0, 50 and 100 %, under the series.
    let hline = t.cs.frame()[4];
    for f in [0.0f32, 0.5, 1.0] {
        let yd = ((1.0 - f) * (dh - 1) as f32).round() as i32;
        let y = r.y + (yd / dy).clamp(0, r.h - 1);
        for x in r.x..r.right() {
            if let Some(cell) = c.get(x, y) {
                if cell.ch == ' ' {
                    c.put(x, y, hline, t.rule, cell.bg, 0);
                }
            }
        }
    }

    // A cell can hold one colour, so the last series to touch it wins. Drawn
    // in the order given; the caller passes the hottest quantity last so it
    // is never the one hidden.
    let mut mask = vec![0u8; (r.w * r.h) as usize];
    let mut color = vec![None::<Rgb>; (r.w * r.h) as usize];
    for s in series
        .iter()
        .filter(|s| s.visible && !s.data.is_empty() && s.max > 0.0)
    {
        let mut prev: Option<i32> = None;
        for xd in 0..dw {
            let i = (xd as usize * s.data.len()) / dw.max(1) as usize;
            let v = (s.data[i.min(s.data.len() - 1)] / s.max).clamp(0.0, 1.0);
            let yd = ((1.0 - v) * (dh - 1) as f32).round() as i32;
            // Join to the previous column so the line is continuous rather
            // than a row of disconnected dots.
            let (lo, hi) = match prev {
                Some(p) => (p.min(yd), p.max(yd)),
                None => (yd, yd),
            };
            for y in lo..=hi {
                let (cx, cy) = (xd / dx, y / dy);
                if cx >= r.w || cy >= r.h {
                    continue;
                }
                let idx = (cy * r.w + cx) as usize;
                mask[idx] |= 1 << ((y % dy) * dx + (xd % dx));
                color[idx] = Some(s.color);
            }
            prev = Some(yd);
        }
    }

    for cy in 0..r.h {
        for cx in 0..r.w {
            let idx = (cy * r.w + cx) as usize;
            if mask[idx] == 0 {
                continue;
            }
            let Some(col) = color[idx] else { continue };
            let ch = glyph(t.cs, mask[idx]);
            let bg = c.get(r.x + cx, r.y + cy).map(|c| c.bg).unwrap_or(t.panel);
            c.put(r.x + cx, r.y + cy, ch, col, bg, 0);
        }
    }
}

fn glyph(cs: Charset, mask: u8) -> char {
    match cs {
        Charset::Full => {
            let mut b = Braille::default();
            for row in 0..4u32 {
                for col in 0..2u32 {
                    if mask & (1 << (row * 2 + col)) != 0 {
                        b.set(col, row);
                    }
                }
            }
            b.ch()
        }
        Charset::Blocks => half_block(mask & 1 != 0, mask & 2 != 0).unwrap_or(' '),
        Charset::Ascii => '*',
    }
}

/// The right-hand gutter's carets: a playhead per series, pointing at its
/// current value.
pub fn carets(c: &mut Canvas, t: &Theme, plot_rect: Rect, gutter_x: i32, series: &[Series]) {
    let (_, dy) = dots(t.cs);
    let dh = plot_rect.h * dy;
    for s in series
        .iter()
        .filter(|s| s.visible && !s.data.is_empty() && s.max > 0.0)
    {
        let v = (s.data[s.data.len() - 1] / s.max).clamp(0.0, 1.0);
        let yd = ((1.0 - v) * (dh - 1) as f32).round() as i32;
        let y = plot_rect.y + (yd / dy).clamp(0, plot_rect.h - 1);
        c.put(gutter_x, y, t.cs.caret(), t.bloom(s.color), t.panel, 0);
    }
}

/// The legend row, which is also the control: a series that is off is drawn
/// dim and is not plotted.
pub fn legend(c: &mut Canvas, t: &Theme, r: Rect, series: &[Series]) {
    if r.is_empty() {
        return;
    }
    let mut x = r.x;
    for s in series {
        let w = s.name.chars().count() as i32 + 4;
        if x + w > r.right() {
            break;
        }
        let (fg, attr) = if s.visible {
            (s.color, BOLD)
        } else {
            (t.dim, DIM)
        };
        c.put(
            x,
            r.y,
            if s.visible { '\u{25c8}' } else { '\u{25c7}' },
            fg,
            t.panel,
            0,
        );
        c.text(x + 2, r.y, r.right() - x - 2, s.name, fg, t.panel, attr);
        x += w;
    }
}

/// The area form: one series, filled beneath a solid edge.
pub fn area(c: &mut Canvas, t: &Theme, r: Rect, data: &[f32], max: f32, color: Rgb) {
    if r.is_empty() || data.is_empty() || max <= 0.0 {
        return;
    }
    let wash = t.wash(color);
    c.fill(r, ' ', t.text, t.panel);
    for x in 0..r.w {
        let i = (x as usize * data.len()) / r.w.max(1) as usize;
        let v = (data[i.min(data.len() - 1)] / max).clamp(0.0, 1.0);
        let eighths = (v * (r.h * 8) as f32).round() as i32;
        let full = eighths / 8;
        let rem = (eighths % 8) as usize;
        for y in 0..r.h {
            let from_bottom = r.h - 1 - y;
            let cx = r.x + x;
            let cy = r.y + y;
            if from_bottom < full {
                c.put(cx, cy, '\u{2588}', wash, t.panel, 0);
            } else if from_bottom == full && rem > 0 {
                // The edge cell takes the full hue: the line on top of the wash.
                c.put(cx, cy, EIGHTHS[rem], color, t.panel, 0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> Theme {
        Theme::copal(Charset::Full)
    }

    #[test]
    fn a_flat_line_at_full_scale_sits_on_the_top_row() {
        let t = theme();
        let mut c = Canvas::new(10, 4);
        let data = vec![1.0f32; 20];
        let s = Series {
            name: "u",
            color: t.green,
            data: &data,
            max: 1.0,
            visible: true,
            axis: Axis::Left,
        };
        plot(&mut c, &t, Rect::new(0, 0, 10, 4), &[s], None);
        assert_eq!(c.get(0, 0).unwrap().fg, t.green);
        assert_ne!(c.get(0, 0).unwrap().ch, ' ');
        assert_eq!(
            c.get(0, 3).unwrap().fg,
            t.rule,
            "the bottom row is the 0% grid rule"
        );
    }

    #[test]
    fn a_flat_line_at_zero_sits_on_the_bottom_row() {
        let t = theme();
        let mut c = Canvas::new(10, 4);
        let data = vec![0.0f32; 20];
        let s = Series {
            name: "u",
            color: t.green,
            data: &data,
            max: 1.0,
            visible: true,
            axis: Axis::Left,
        };
        plot(&mut c, &t, Rect::new(0, 0, 10, 4), &[s], None);
        assert_eq!(c.get(5, 3).unwrap().fg, t.green);
    }

    #[test]
    fn an_invisible_series_is_not_plotted() {
        let t = theme();
        let mut c = Canvas::new(10, 4);
        let data = vec![1.0f32; 20];
        let s = Series {
            name: "u",
            color: t.green,
            data: &data,
            max: 1.0,
            visible: false,
            axis: Axis::Left,
        };
        plot(&mut c, &t, Rect::new(0, 0, 10, 4), &[s], None);
        // Row 1 carries no grid rule, so it is empty if and only if nothing
        // was plotted.
        assert_eq!(c.get(0, 1).unwrap().ch, ' ');
    }

    #[test]
    fn the_brush_lifts_its_band_and_edges_it_with_rulers() {
        let t = theme();
        let mut c = Canvas::new(10, 4);
        let data = vec![0.0f32; 20];
        let s = Series {
            name: "u",
            color: t.green,
            data: &data,
            max: 1.0,
            visible: true,
            axis: Axis::Left,
        };
        plot(&mut c, &t, Rect::new(0, 0, 10, 4), &[s], Some((8, 12)));
        assert_eq!(c.get(5, 1).unwrap().bg, t.panel_hi);
        assert_eq!(c.get(0, 1).unwrap().bg, t.panel);
        assert_eq!(c.get(4, 1).unwrap().ch, t.cs.ruler());
    }

    #[test]
    fn the_caret_points_at_the_current_value() {
        let t = theme();
        let mut c = Canvas::new(12, 4);
        let mut data = vec![0.0f32; 20];
        data[19] = 1.0;
        let s = Series {
            name: "u",
            color: t.green,
            data: &data,
            max: 1.0,
            visible: true,
            axis: Axis::Left,
        };
        carets(&mut c, &t, Rect::new(0, 0, 10, 4), 11, &[s]);
        assert_eq!(c.get(11, 0).unwrap().ch, t.cs.caret());
    }

    #[test]
    fn an_area_fills_from_the_bottom_and_edges_at_the_top() {
        let t = theme();
        let mut c = Canvas::new(8, 4);
        let data = vec![0.5f32; 8];
        area(&mut c, &t, Rect::new(0, 0, 8, 4), &data, 1.0, t.magenta);
        assert_eq!(c.get(0, 3).unwrap().fg, t.wash(t.magenta));
        assert_eq!(c.get(0, 2).unwrap().fg, t.wash(t.magenta));
        assert_eq!(c.get(0, 0).unwrap().ch, ' ');
    }

    #[test]
    fn coarser_charsets_plot_the_same_data_without_panicking() {
        for cs in [Charset::Blocks, Charset::Ascii] {
            let t = Theme::copal(cs);
            let mut c = Canvas::new(10, 4);
            let data: Vec<f32> = (0..20).map(|i| i as f32 / 20.0).collect();
            let s = Series {
                name: "u",
                color: t.green,
                data: &data,
                max: 1.0,
                visible: true,
                axis: Axis::Left,
            };
            plot(&mut c, &t, Rect::new(0, 0, 10, 4), &[s], Some((2, 6)));
            assert!((0..10).any(|x| c.get(x, 0).unwrap().fg == t.green));
        }
    }

    #[test]
    fn dot_columns_scale_with_the_charset() {
        assert_eq!(dot_columns(Charset::Full, 60), 120);
        assert_eq!(dot_columns(Charset::Blocks, 60), 60);
    }
}
