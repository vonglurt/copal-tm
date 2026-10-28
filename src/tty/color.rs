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

    /// Nearest of the sixteen ANSI colours, for a foreground on a terminal
    /// that has no more.
    ///
    /// Greys go by brightness. The channel test below sends any grey whose
    /// three channels sit above half its maximum to 7, light grey, and a
    /// panel's dark ground is exactly such a grey: on calber's console
    /// (`TERM=linux`, 28 October 2026) every slightly-lighter-than-black
    /// ground came out as a light-grey block. A colour with little hue in
    /// it is placed by luma instead: black, dark grey, light grey, white.
    pub fn ansi16(self) -> u8 {
        let (r, g, b) = (self.0 as i32, self.1 as i32, self.2 as i32);
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        // Dark, or nearly without hue: a grey by brightness. A meter's unlit
        // track (its hue at a sixth over the panel) lands here as dark grey,
        // which is what an unlit segment should look like with sixteen colours.
        if max < 90 || max - min < 40 {
            let l = self.luma();
            return if l < 0.12 {
                0
            } else if l < 0.45 {
                8
            } else if l < 0.85 {
                7
            } else {
                15
            };
        }
        // A hue: the nearest of the six by angle. The channel test this
        // replaces sent green (7CF05A) to bright yellow and the disk's green to
        // cyan, so meters that differ on a truecolour terminal were one colour
        // on the console. Orange is colour 3, which the Linux console draws as
        // brown-orange, so orange and yellow stay apart.
        let d = (max - min) as f32;
        let h = if max == r {
            60.0 * ((g - b) as f32 / d)
        } else if max == g {
            60.0 * ((b - r) as f32 / d) + 120.0
        } else {
            60.0 * ((r - g) as f32 / d) + 240.0
        };
        let h = if h < 0.0 { h + 360.0 } else { h };
        let base = match h as u32 {
            0..=14 | 340..=360 => 1,
            15..=44 => return 3,
            45..=74 => 3,
            75..=159 => 2,
            160..=204 => 6,
            205..=259 => 4,
            _ => 5,
        };
        if max > 160 {
            base + 8
        } else {
            base
        }
    }

    /// A background on a sixteen-colour terminal: one of the eight dark
    /// colours (`40` to `47`) only, and never grey or white. The Linux
    /// console has eight background colours, and a bright one (`100` to
    /// `107`) is drawn as blink or as a bright block depending on the
    /// console. A ground with little hue (a panel, a stripe, a dimmed row)
    /// is black; a saturated one (the selection bar's blue in sixteen colours,
    /// `Theme::sixteen`) keeps its hue at normal intensity.
    pub fn ansi8_bg(self) -> u8 {
        let (r, g, b) = (self.0 as u32, self.1 as u32, self.2 as u32);
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        if max < 100 || max - min < 60 {
            return 0;
        }
        let i = self.ansi16();
        let i = if i >= 8 { i - 8 } else { i };
        if i == 7 {
            0
        } else {
            i
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A dark panel ground is black on a sixteen-colour terminal, never the
    /// light grey that the channel test gave it (calber's console).
    #[test]
    fn a_dark_grey_is_black_or_dark_grey_not_light() {
        for g in [0x1a1d22, 0x24282e, 0x30343a, 0x3a3f47] {
            let c = Rgb::hex(g);
            assert!(
                c.ansi16() == 0 || c.ansi16() == 8,
                "{:06x} -> {}",
                g,
                c.ansi16()
            );
            assert_eq!(c.ansi8_bg(), 0, "{:06x} as a background", g);
        }
        assert_eq!(Rgb::hex(0xc8c8c8).ansi16(), 7);
        assert_eq!(Rgb::hex(0xffffff).ansi16(), 15);
    }

    /// No background is ever grey or white, or bright; a saturated colour
    /// keeps its hue.
    #[test]
    fn a_background_is_one_of_the_eight_dark_colours_and_never_white() {
        for v in [
            0x000000, 0xffffff, 0xc0c0c0, 0x808080, 0x00bcd4, 0x2196f3, 0xff00ff, 0xffeb3b,
            0x101820,
        ] {
            let i = Rgb::hex(v).ansi8_bg();
            assert!(i < 7, "{:06x} -> {}", v, i);
        }
        assert_eq!(Rgb::hex(0x00bcd4).ansi8_bg(), 6, "cyan stays cyan");
        assert_eq!(
            Rgb::hex(0xffffff).ansi8_bg(),
            0,
            "white is not a background"
        );
    }

    /// The meters' hues stay apart: green is green, orange is not yellow.
    #[test]
    fn the_series_hues_map_to_their_own_colours() {
        assert_eq!(Rgb::hex(0x7CF05A).ansi16(), 10, "green");
        assert_eq!(Rgb::hex(0x55D97A).ansi16(), 10, "disk green");
        assert_eq!(Rgb::hex(0xF0A03A).ansi16(), 3, "orange");
        assert_eq!(Rgb::hex(0xF2E34A).ansi16(), 11, "yellow");
        assert_eq!(Rgb::hex(0xF0453A).ansi16(), 9, "red");
        assert_eq!(Rgb::hex(0x4FE3E8).ansi16(), 14, "cyan");
        assert_eq!(Rgb::hex(0x4FA8F5).ansi16(), 12, "gpu blue");
        assert_eq!(Rgb::hex(0xE83CF0).ansi16(), 13, "magenta");
    }
}
