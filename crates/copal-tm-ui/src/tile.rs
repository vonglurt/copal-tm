// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Paul Richeson
//! The stat tile — Section VI-E.
//!
//! The smallest thing that can still be read at a glance: an identity, one
//! headline, a line of qualification, and a meter. Four of them make the rail
//! along the bottom of the dashboard.

use copal_tm_tty::{Canvas, Rect, BOLD, DIM};

use crate::ladder::{self, Hue};
use crate::panel::{self, Panel, Title};
use crate::theme::Theme;

pub struct Tile<'a> {
    pub name: &'a str,
    /// A single-width mark, never an emoji: a double-width glyph would push
    /// every cell after it out of place.
    pub icon: char,
    /// The right-aligned headline. A word is a fine headline for a quantity
    /// nobody can act on — System Pressure's is `LOW`.
    pub headline: &'a str,
    pub sub_left: &'a str,
    pub sub_right: &'a str,
    /// 0..1 for the meter.
    pub value: f32,
    pub hue: Hue<'a>,
    pub stale: bool,
}

/// Draw a tile into `r`, frame and all.
pub fn draw(c: &mut Canvas, t: &Theme, r: Rect, tile: &Tile) {
    if r.w < 8 || r.h < 3 {
        return;
    }
    let hue = tile.hue.top();
    let mut p = Panel::new("", "");
    p.style = Title::Document;
    let body = panel::draw(c, t, Rect::new(r.x, r.y, r.w, r.h), &p);
    if body.is_empty() {
        return;
    }

    // Row 0: icon, name, headline.
    let y = body.y - 1;
    c.put(body.x, y, tile.icon, hue, t.panel, 0);
    let head_w = tile.headline.chars().count() as i32;
    c.text(
        body.x + 2,
        y,
        (body.w - head_w - 3).max(0),
        tile.name,
        t.text,
        t.panel,
        0,
    );
    c.text_right(body.right(), y, body.w, tile.headline, hue, t.panel, BOLD);

    // Row 1: the sub-caption, left and right.
    if body.h >= 1 {
        let ry = body.y;
        let rw = tile.sub_right.chars().count() as i32;
        c.text(
            body.x,
            ry,
            (body.w - rw - 1).max(0),
            tile.sub_left,
            t.dim,
            t.panel,
            DIM,
        );
        c.text_right(
            body.right(),
            ry,
            body.w,
            tile.sub_right,
            t.dim,
            t.panel,
            DIM,
        );
    }

    // Row 2: the meter.
    if body.h >= 2 {
        ladder::horizontal(
            c,
            t,
            Rect::new(body.x, body.y + 1, body.w, 1),
            tile.value,
            tile.hue,
            tile.stale,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use copal_tm_tty::Charset;

    #[test]
    fn a_tile_puts_its_headline_at_the_right_edge_and_its_meter_at_the_foot() {
        let t = Theme::copal(Charset::Full);
        let mut c = Canvas::new(32, 6);
        let tile = Tile {
            name: "Network",
            icon: '\u{25cd}',
            headline: "5.97 KB/s",
            sub_left: "404 Mb/s \u{b7} 0.0%",
            sub_right: "Scale 266 KB",
            value: 0.5,
            hue: Hue::Solid(t.net),
            stale: false,
        };
        draw(&mut c, &t, Rect::new(0, 0, 32, 6), &tile);
        // Row 1 is the head, row 2 the sub-caption, row 3 the meter.
        let r0: String = (0..32).map(|x| c.get(x, 1).unwrap().ch).collect();
        let r1: String = (0..32).map(|x| c.get(x, 2).unwrap().ch).collect();
        assert!(r0.contains("Network"), "{r0}");
        assert!(r0.contains("5.97 KB/s"), "{r0}");
        assert!(
            r1.contains("404 Mb/s") && r1.contains("Scale 266 KB"),
            "{r1}"
        );
        // The meter: half lit, half in track.
        assert_eq!(c.get(1, 3).unwrap().fg, t.net);
        assert_eq!(c.get(29, 3).unwrap().fg, t.track(t.net));
    }

    #[test]
    fn a_tile_too_small_to_draw_is_skipped_rather_than_panicking() {
        let t = Theme::copal(Charset::Full);
        let mut c = Canvas::new(32, 6);
        let tile = Tile {
            name: "x",
            icon: '\u{25cd}',
            headline: "",
            sub_left: "",
            sub_right: "",
            value: 0.0,
            hue: Hue::Solid(t.net),
            stale: false,
        };
        draw(&mut c, &t, Rect::new(0, 0, 4, 2), &tile);
    }
}
