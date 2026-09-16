// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Paul Richeson
//! The palette, and the three shade operations everything is built from.
//!
//! Section IV of the design report. There are no other greys in this program:
//! every dim thing on screen is a series hue laid on the panel ground at a low
//! opacity, which is why a nearly-empty meter still reads as *that* meter and
//! four columns side by side stay four distinguishable objects.

use crate::tty::{Charset, Rgb};

#[derive(Clone, Debug)]
pub struct Theme {
    // Structure.
    pub ground: Rgb,
    pub panel: Rgb,
    pub panel_hi: Rgb,
    pub rule: Rgb,
    pub inset: Rgb,
    pub text: Rgb,
    pub dim: Rgb,
    pub sel: Rgb,
    pub sel_edge: Rgb,
    // Series.
    pub cyan: Rgb,
    pub green: Rgb,
    pub orange: Rgb,
    pub red: Rgb,
    pub battery: Rgb,
    pub yellow: Rgb,
    pub gpu: Rgb,
    pub magenta: Rgb,
    pub net: Rgb,
    pub disk: Rgb,
    pub amber: Rgb,
    // Ramps.
    pub heat: [Rgb; 3],
    pub cool: [Rgb; 3],
    pub cs: Charset,
}

impl Default for Theme {
    fn default() -> Self {
        Theme::copal(Charset::detect())
    }
}

impl Theme {
    /// The palette sampled from the reference image and regularised.
    pub fn copal(cs: Charset) -> Theme {
        Theme {
            ground: Rgb::hex(0x0E1216),
            panel: Rgb::hex(0x141A20),
            panel_hi: Rgb::hex(0x182129),
            rule: Rgb::hex(0x1E2830),
            inset: Rgb::hex(0x05080A),
            text: Rgb::hex(0xD7E0E5),
            dim: Rgb::hex(0x55646E),
            sel: Rgb::hex(0x1D2E3A),
            sel_edge: Rgb::hex(0x4FE3E8),
            cyan: Rgb::hex(0x4FE3E8),
            green: Rgb::hex(0x7CF05A),
            orange: Rgb::hex(0xF0A03A),
            red: Rgb::hex(0xF0453A),
            battery: Rgb::hex(0xF2402C),
            yellow: Rgb::hex(0xF2E34A),
            gpu: Rgb::hex(0x4FA8F5),
            magenta: Rgb::hex(0xE83CF0),
            net: Rgb::hex(0x46B4F0),
            disk: Rgb::hex(0x55D97A),
            amber: Rgb::hex(0xE8A020),
            heat: [Rgb::hex(0x55D97A), Rgb::hex(0xF2E34A), Rgb::hex(0xF0453A)],
            cool: [Rgb::hex(0x1E3A5F), Rgb::hex(0x4FA8F5), Rgb::hex(0x9BD4FF)],
            cs,
        }
    }

    /// One hue and the ground, for a console with no colour worth the name.
    /// Shape still carries everything: the ladders, the plots and the tables
    /// are all legible without a single hue difference.
    pub fn mono(cs: Charset) -> Theme {
        let fg = Rgb::hex(0xC8D2D8);
        let mut t = Theme::copal(cs);
        for c in [
            &mut t.cyan,
            &mut t.green,
            &mut t.orange,
            &mut t.red,
            &mut t.battery,
            &mut t.yellow,
            &mut t.gpu,
            &mut t.magenta,
            &mut t.net,
            &mut t.disk,
            &mut t.amber,
        ] {
            *c = fg;
        }
        t.heat = [fg.dark(0.5), fg.dark(0.2), fg];
        t.cool = t.heat;
        t.sel_edge = fg;
        t
    }

    pub fn named(name: &str, cs: Charset) -> Theme {
        match name.trim().to_ascii_lowercase().as_str() {
            "mono" => Theme::mono(cs),
            _ => Theme::copal(cs),
        }
    }

    /// **track** — an unlit segment: the meter's own hue at 0.16 over the
    /// panel. Not grey, ever.
    pub fn track(&self, hue: Rgb) -> Rgb {
        hue.on(self.panel, 0.16)
    }

    /// **bloom** — the topmost lit segment, and the current-value caret.
    pub fn bloom(&self, hue: Rgb) -> Rgb {
        hue.light(0.30)
    }

    /// **partial** — the one segment straddling the value, lit in proportion.
    /// A cell cannot be half lit, but it can be a colour halfway between lit
    /// and unlit, and at a glance those are the same thing.
    pub fn partial(&self, hue: Rgb, f: f32) -> Rgb {
        hue.on(self.track(hue), f.clamp(0.0, 1.0))
    }

    /// The fill under an area chart.
    pub fn wash(&self, hue: Rgb) -> Rgb {
        hue.on(self.panel, 0.18)
    }

    /// The unlit cells of a dot-matrix title strip.
    pub fn matrix_unlit(&self) -> Rgb {
        self.dim.on(self.inset, 0.22)
    }

    /// A reading older than three of its own periods is drawn in this: an
    /// instrument that has stopped must not look like one reading zero.
    pub fn stale(&self, hue: Rgb) -> Rgb {
        hue.on(self.panel, 0.45)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_track_is_the_hue_and_not_a_grey() {
        let t = Theme::copal(Charset::Full);
        let track = t.track(t.green);
        // Greener than it is red, and darker than the lit hue: the two
        // properties that make a nearly-empty meter still read as itself.
        assert!(track.1 > track.0 && track.1 > track.2);
        assert!(track.luma() < t.green.luma());
        assert!(track.luma() > t.panel.luma());
    }

    #[test]
    fn partial_interpolates_between_track_and_hue() {
        let t = Theme::copal(Charset::Full);
        assert_eq!(t.partial(t.green, 0.0), t.track(t.green));
        assert_eq!(t.partial(t.green, 1.0), t.green);
        assert!(t.partial(t.green, 0.5).luma() > t.track(t.green).luma());
    }

    #[test]
    fn mono_keeps_the_structure_and_drops_the_hues() {
        let t = Theme::mono(Charset::Ascii);
        assert_eq!(t.green, t.magenta);
        assert_ne!(t.panel, t.ground);
        assert_ne!(t.track(t.green), t.green);
    }
}
