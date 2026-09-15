// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Paul Richeson
//! Colour, as 24-bit triples with the two operations this program needs:
//! mixing two colours, and laying a colour on a ground at some opacity.
//!
//! The instrument panel is built almost entirely out of the second one.  An
//! unlit segment of a meter is not grey: it is the meter's own hue at about
//! fifteen percent over the panel ground, so a column that is nearly empty
//! still reads as *that* meter rather than as a dark rectangle.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Rgb(r, g, b)
    }

    /// `0xRRGGBB`, the way the palette is written down.
    pub const fn hex(v: u32) -> Self {
        Rgb(
            ((v >> 16) & 0xff) as u8,
            ((v >> 8) & 0xff) as u8,
            (v & 0xff) as u8,
        )
    }

    /// Linear blend, `t` of `other` into `self`.
    pub fn mix(self, other: Rgb, t: f32) -> Rgb {
        let t = t.clamp(0.0, 1.0);
        let f = |a: u8, b: u8| {
            (a as f32 + (b as f32 - a as f32) * t)
                .round()
                .clamp(0.0, 255.0) as u8
        };
        Rgb(f(self.0, other.0), f(self.1, other.1), f(self.2, other.2))
    }

    /// `self` drawn on `ground` at `alpha`.  The unlit-segment operation.
    pub fn on(self, ground: Rgb, alpha: f32) -> Rgb {
        ground.mix(self, alpha)
    }

    /// Toward black.  `t = 0` keeps the colour, `t = 1` puts it out.
    pub fn dark(self, t: f32) -> Rgb {
        self.mix(Rgb(0, 0, 0), t)
    }

    /// Toward white.  Used for the bloom on a lit segment's leading edge.
    pub fn light(self, t: f32) -> Rgb {
        self.mix(Rgb(255, 255, 255), t)
    }

    /// Relative luminance, for deciding whether text on this ground is dark.
    pub fn luma(self) -> f32 {
        (0.2126 * self.0 as f32 + 0.7152 * self.1 as f32 + 0.0722 * self.2 as f32) / 255.0
    }

    /// Nearest of the sixteen ANSI colours, for terminals that have no more.
    pub fn ansi16(self) -> u8 {
        let (r, g, b) = (self.0 as u32, self.1 as u32, self.2 as u32);
        let max = r.max(g).max(b);
        if max < 40 {
            return 0;
        }
        let bright = max > 160;
        let t = max / 2;
        let mut idx = 0;
        if r > t {
            idx |= 1;
        }
        if g > t {
            idx |= 2;
        }
        if b > t {
            idx |= 4;
        }
        if idx == 0 {
            idx = 7;
        }
        if bright {
            idx + 8
        } else {
            idx
        }
    }
}

/// A ramp through a list of stops, `t` in `0.0 ..= 1.0`.  The System Pressure
/// tile's green-yellow-red ladder is one of these.
pub fn ramp(stops: &[Rgb], t: f32) -> Rgb {
    if stops.is_empty() {
        return Rgb(0, 0, 0);
    }
    if stops.len() == 1 {
        return stops[0];
    }
    let t = t.clamp(0.0, 1.0) * (stops.len() - 1) as f32;
    let i = t.floor() as usize;
    if i + 1 >= stops.len() {
        return stops[stops.len() - 1];
    }
    stops[i].mix(stops[i + 1], t - i as f32)
}
