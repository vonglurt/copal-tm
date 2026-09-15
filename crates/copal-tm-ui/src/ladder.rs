// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Paul Richeson
//! The ladder meter — Section VI-A.
//!
//! A column of segments drawn with `▄`, the lower half block. The top half of
//! each row stays dark and *is* the gap between segments: that is the whole
//! reason a character grid can wear a VU meter at all.
//!
//! Three shades do the work, all from the theme: **track** for the unlit run,
//! the hue for the lit run, **bloom** for the topmost lit segment, and
//! **partial** for the one segment straddling the value.

use copal_tm_tty::{ramp, Canvas, Rect, Rgb, BOLD};

use crate::theme::Theme;

/// A meter's colour: one hue, or a ramp that says the same thing as the
/// height does — the Temp column and the System Pressure tile.
#[derive(Clone, Copy)]
pub enum Hue<'a> {
    Solid(Rgb),
    Ramp(&'a [Rgb]),
}

impl<'a> Hue<'a> {
    /// The colour of segment `i` of `n`.
    pub fn at(&self, i: i32, n: i32) -> Rgb {
        match self {
            Hue::Solid(c) => *c,
            Hue::Ramp(stops) => ramp(
                stops,
                if n <= 1 {
                    0.0
                } else {
                    i as f32 / (n - 1) as f32
                },
            ),
        }
    }

    /// The colour of the whole thing, for a caption or a headline.
    pub fn top(&self) -> Rgb {
        match self {
            Hue::Solid(c) => *c,
            Hue::Ramp(stops) => *stops.last().unwrap_or(&Rgb(255, 255, 255)),
        }
    }
}

pub struct Ladder<'a> {
    /// The caption above the column.
    pub label: &'a str,
    /// The reading below it, already formatted and fixed-width.
    pub readout: &'a str,
    /// 0..1.
    pub value: f32,
    pub hue: Hue<'a>,
    /// A reading too old to trust is drawn washed out.
    pub stale: bool,
}

/// Draw a vertical ladder: caption, segments, reading.
pub fn vertical(c: &mut Canvas, t: &Theme, r: Rect, l: &Ladder) {
    if r.is_empty() {
        return;
    }
    let bg = t.panel;
    let mut bar = r;
    if r.h >= 3 && !l.label.is_empty() {
        c.text_center(r, r.y, &clip(l.label, r.w), l.hue.top(), bg, BOLD);
        bar = Rect::new(r.x, r.y + 1, r.w, r.h - 1);
    }
    if !l.readout.is_empty() && bar.h >= 2 {
        c.text_center(
            r,
            bar.bottom() - 1,
            &clip(l.readout, r.w),
            l.hue.top(),
            bg,
            0,
        );
        bar = Rect::new(bar.x, bar.y, bar.w, bar.h - 1);
    }
    segments(c, t, bar, l, bg);
}

fn segments(c: &mut Canvas, t: &Theme, bar: Rect, l: &Ladder, bg: Rgb) {
    let n = bar.h;
    if n <= 0 {
        return;
    }
    let seg = t.cs.segment();
    let v = l.value.clamp(0.0, 1.0);
    // Segment i counts from the bottom, so row y = bottom - 1 - i.
    for i in 0..n {
        let lo = i as f32 / n as f32;
        let hi = (i + 1) as f32 / n as f32;
        let hue = l.hue.at(i, n);
        let hue = if l.stale { t.stale(hue) } else { hue };
        let color = if v >= hi {
            // The topmost lit segment blooms.
            if v < hi + 1.0 / n as f32 {
                t.bloom(hue)
            } else {
                hue
            }
        } else if v > lo {
            t.partial(hue, (v - lo) * n as f32)
        } else {
            t.track(hue)
        };
        let y = bar.bottom() - 1 - i;
        for x in bar.x..bar.right() {
            c.put(x, y, seg, color, bg, 0);
        }
    }
}

/// The same instrument on its side: the meter along the bottom of a tile.
pub fn horizontal(c: &mut Canvas, t: &Theme, r: Rect, value: f32, hue: Hue, stale: bool) {
    if r.is_empty() {
        return;
    }
    let bg = t.panel;
    let n = r.w;
    let v = value.clamp(0.0, 1.0);
    let dot = t.cs.matrix_dot();
    for i in 0..n {
        let lo = i as f32 / n as f32;
        let hi = (i + 1) as f32 / n as f32;
        let h = hue.at(i, n);
        let h = if stale { t.stale(h) } else { h };
        let color = if v >= hi {
            if v < hi + 1.0 / n as f32 {
                t.bloom(h)
            } else {
                h
            }
        } else if v > lo {
            t.partial(h, (v - lo) * n as f32)
        } else {
            t.track(h)
        };
        for y in r.y..r.bottom() {
            c.put(r.x + i, y, dot, color, bg, 0);
        }
    }
}

/// A one-row micro-meter, drawn with the block eighths: the per-core strip
/// under the CPU history.
pub fn micro(c: &mut Canvas, t: &Theme, x: i32, y: i32, value: f32, hue: Rgb, dim: bool) {
    let v = value.clamp(0.0, 1.0);
    let idx = (v * 8.0).round().clamp(0.0, 8.0) as usize;
    let ch = copal_tm_tty::EIGHTHS[idx];
    let col = if dim { hue.on(t.panel, 0.55) } else { hue };
    let col = if idx == 0 { t.track(hue) } else { col };
    c.put(
        x,
        y,
        if idx == 0 { '\u{2581}' } else { ch },
        col,
        t.panel,
        0,
    );
}

fn clip(s: &str, w: i32) -> String {
    s.chars().take(w.max(0) as usize).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use copal_tm_tty::Charset;

    fn probe(value: f32) -> Vec<Rgb> {
        let t = Theme::copal(Charset::Full);
        let mut c = Canvas::new(4, 8);
        c.clear(t.panel);
        let l = Ladder {
            label: "",
            readout: "",
            value,
            hue: Hue::Solid(t.green),
            stale: false,
        };
        vertical(&mut c, &t, Rect::new(0, 0, 4, 8), &l);
        // Bottom-up.
        (0..8).rev().map(|y| c.get(0, y).unwrap().fg).collect()
    }

    #[test]
    fn an_empty_meter_is_still_its_own_colour() {
        let t = Theme::copal(Charset::Full);
        let col = probe(0.0);
        assert!(col.iter().all(|c| *c == t.track(t.green)));
        assert_ne!(col[0], t.panel, "an unlit track is not the ground");
    }

    #[test]
    fn a_full_meter_lights_every_segment() {
        let t = Theme::copal(Charset::Full);
        let col = probe(1.0);
        assert!(col.iter().all(|c| *c == t.green || *c == t.bloom(t.green)));
        assert_eq!(
            *col.last().unwrap(),
            t.bloom(t.green),
            "the top segment blooms"
        );
    }

    #[test]
    fn a_half_meter_lights_the_bottom_half_only() {
        let t = Theme::copal(Charset::Full);
        let col = probe(0.5);
        assert_eq!(col[0], t.green);
        assert_eq!(col[3], t.bloom(t.green));
        assert_eq!(col[4], t.track(t.green));
        assert_eq!(col[7], t.track(t.green));
    }

    #[test]
    fn the_segment_glyph_leaves_the_gap_above_it() {
        let t = Theme::copal(Charset::Full);
        let mut c = Canvas::new(4, 4);
        let l = Ladder {
            label: "",
            readout: "",
            value: 1.0,
            hue: Hue::Solid(t.green),
            stale: false,
        };
        vertical(&mut c, &t, Rect::new(0, 0, 4, 4), &l);
        assert_eq!(c.get(1, 1).unwrap().ch, '\u{2584}');
    }

    #[test]
    fn a_ramp_says_the_same_thing_as_the_height() {
        let t = Theme::copal(Charset::Full);
        let h = Hue::Ramp(&t.heat);
        assert_eq!(h.at(0, 3), t.heat[0]);
        assert_eq!(h.at(2, 3), t.heat[2]);
    }

    #[test]
    fn caption_and_readout_take_a_row_each() {
        let t = Theme::copal(Charset::Full);
        let mut c = Canvas::new(6, 6);
        let l = Ladder {
            label: "CPU",
            readout: "10.6%",
            value: 0.0,
            hue: Hue::Solid(t.green),
            stale: false,
        };
        vertical(&mut c, &t, Rect::new(0, 0, 6, 6), &l);
        let top: String = (0..6).map(|x| c.get(x, 0).unwrap().ch).collect();
        let bot: String = (0..6).map(|x| c.get(x, 5).unwrap().ch).collect();
        assert!(top.contains("CPU"));
        assert!(bot.contains("10.6%"));
    }
}
