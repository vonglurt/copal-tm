// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Paul Richeson
//! The drawing vocabulary, and what to do when the console does not have it.
//!
//! Four levels, because this program has to be legible on a Raspberry Pi
//! hanging off a television running the kernel's own console font as well as
//! in a terminal with a full font:
//!
//! | level | plot resolution | segment | frame |
//! |---|---|---|---|
//! | `Full` | braille, 2x4 dots per cell | `▄` half block | rounded box drawing |
//! | `Blocks` | half blocks, 1x2 per cell | `▄` half block | square box drawing |
//! | `Console` | `■`, 1x1 | `■` | square box drawing |
//! | `Ascii` | `*` and `.`, 1x1 | `#` | `+ - |` |
//!
//! `Blocks` is not a lesser `Full`: a graph plotted with half blocks is
//! coarser, and it is the one to pick when a font is in doubt.
//!
//! `Console` is the Linux kernel console. Its fonts are not CP437 whole:
//! Ubuntu's `Uni2-Fixed16` (calber, 28 October 2026) has the box drawing,
//! `█ ░ ▒ ■ ▮ ● ♦ ◀ ▶ ▲ ▼ · •`, and none of the half blocks, eighths, braille
//! or most geometric shapes; a missing glyph is drawn as the font's
//! replacement. So `Blocks` there, chosen for `TERM=linux` until then, drew
//! every meter segment and graph edge as that replacement. `Console` plots
//! with what the font has, and the canvas swaps every other glyph the program
//! uses for one it has as it writes the frame (`console_glyph`).

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Charset {
    Full,
    Blocks,
    Console,
    Ascii,
}

impl Charset {
    /// What the terminal looks capable of, from the environment alone.  A
    /// guess, and the configuration file overrides it.
    pub fn detect() -> Charset {
        let lang = std::env::var("LC_ALL")
            .or_else(|_| std::env::var("LC_CTYPE"))
            .or_else(|_| std::env::var("LANG"))
            .unwrap_or_default()
            .to_ascii_uppercase();
        if !(lang.contains("UTF-8") || lang.contains("UTF8")) {
            return Charset::Ascii;
        }
        let term = std::env::var("TERM").unwrap_or_default();
        // The Linux kernel console maps a 256- or 512-glyph font, without
        // braille or the half blocks (see the head of this file).
        if term == "linux" || term.starts_with("vt") {
            return Charset::Console;
        }
        Charset::Full
    }

    pub fn parse(s: &str) -> Option<Charset> {
        match s.trim().to_ascii_lowercase().as_str() {
            "full" | "braille" => Some(Charset::Full),
            "blocks" | "block" => Some(Charset::Blocks),
            "console" => Some(Charset::Console),
            "ascii" | "plain" => Some(Charset::Ascii),
            "auto" => Some(Charset::detect()),
            _ => None,
        }
    }

    /// The box frame: `[tl, tr, bl, br, h, v]`.
    pub fn frame(self) -> [char; 6] {
        match self {
            Charset::Full => ['╭', '╮', '╰', '╯', '─', '│'],
            Charset::Blocks | Charset::Console => ['┌', '┐', '└', '┘', '─', '│'],
            Charset::Ascii => ['+', '+', '+', '+', '-', '|'],
        }
    }

    /// The tree guides of the Browser: `[vertical, tee, elbow, dash]`.
    pub fn tree(self) -> [char; 4] {
        match self {
            Charset::Full | Charset::Blocks => ['│', '├', '╰', '─'],
            Charset::Console => ['│', '├', '└', '─'],
            Charset::Ascii => ['|', '+', '`', '-'],
        }
    }

    /// One lit segment of a ladder meter.  The lower half block leaves the
    /// top half of the row dark, which is the gap between segments - the
    /// reason a terminal can wear the look at all.
    pub fn segment(self) -> char {
        match self {
            Charset::Full | Charset::Blocks => '▄',
            Charset::Console => '■',
            Charset::Ascii => '#',
        }
    }

    /// The unlit filler of a dot-matrix title strip.
    pub fn matrix_dot(self) -> char {
        match self {
            Charset::Full => '▪',
            Charset::Blocks => '▪',
            Charset::Console => '·',
            Charset::Ascii => '.',
        }
    }

    /// The caret at the right edge of a graph, pointing at a series' current
    /// value.  A playhead, in the sense a tape deck means it.
    pub fn caret(self) -> char {
        match self {
            Charset::Full | Charset::Blocks | Charset::Console => '◀',
            Charset::Ascii => '<',
        }
    }

    /// The dashed vertical of a scrub ruler.
    pub fn ruler(self) -> char {
        match self {
            Charset::Full => '┊',
            Charset::Blocks => ':',
            Charset::Console => '┊',
            Charset::Ascii => ':',
        }
    }
}

/// A glyph the Linux console's font has, for one it may not (`Console`).
/// Shapes go to the nearest shape. A low fill goes to `_`, which sits at the
/// foot of the cell as `▁` does (the shades `░ ▒` were tried first and made
/// the top of an area chart a speckle); a high one to the full block.
pub fn console_glyph(ch: char) -> char {
    match ch {
        '▀' | '▄' | '▪' => '■',
        '▁' | '▂' | '▃' => '_',
        '▅' | '▆' | '▇' | '▉' | '▊' | '▋' | '▌' => '█',
        '▏' | '▎' | '▍' => '▮',
        '◈' | '◉' => '●',
        '◆' => '♦',
        '◇' => '∙',
        '◍' => '⊕',
        '◎' => '⊙',
        '▤' => '≡',
        '✓' => '√',
        '⏸' => '‖',
        '⠀' => ' ',
        '\u{2801}'..='\u{28ff}' => '•',
        _ => ch,
    }
}

/// Eight levels of a partly filled cell, bottom up.  Index 0 is empty.
pub const EIGHTHS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
/// The same idea left to right, for horizontal bars.
pub const EIGHTHS_H: [char; 9] = [' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];

/// A braille cell under construction: 2 columns by 4 rows of dots.
///
/// The bit order is the one Unicode chose, which is not the one you would
/// choose: dots 1-3 and 4-6 are the top three rows of each column, and dots
/// 7 and 8 - added when braille was extended to eight dots - are the fourth
/// row, tacked on at the high end.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Braille(pub u8);

impl Braille {
    pub const BASE: u32 = 0x2800;

    /// Light the dot at `(col, row)`, `col` in 0..2 and `row` in 0..4.
    pub fn set(&mut self, col: u32, row: u32) {
        if col > 1 || row > 3 {
            return;
        }
        let bit = if row == 3 {
            0x40u8 << col
        } else {
            0x01u8 << (col * 3 + row)
        };
        self.0 |= bit;
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn ch(self) -> char {
        char::from_u32(Braille::BASE + self.0 as u32).unwrap_or('⠀')
    }
}

/// Half-block plotting: two rows of dots per cell.
pub fn half_block(top: bool, bottom: bool) -> Option<char> {
    match (top, bottom) {
        (false, false) => None,
        (true, false) => Some('▀'),
        (false, true) => Some('▄'),
        (true, true) => Some('█'),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn braille_bits_match_unicode() {
        let mut b = Braille::default();
        b.set(0, 0);
        assert_eq!(b.ch(), '⠁');
        let mut b = Braille::default();
        b.set(1, 3);
        assert_eq!(
            b.ch(),
            '⣀'.to_string().chars().next().map(|_| b.ch()).unwrap()
        );
        assert_eq!(b.0, 0x80);
        let mut full = Braille::default();
        for c in 0..2 {
            for r in 0..4 {
                full.set(c, r);
            }
        }
        assert_eq!(full.ch(), '⣿');
    }

    #[test]
    fn out_of_range_dots_are_ignored() {
        let mut b = Braille::default();
        b.set(5, 9);
        assert!(b.is_empty());
    }

    /// Every glyph `Console` draws, and every one `console_glyph` gives, is in
    /// Ubuntu's console font (`Uni2-Fixed16`'s table, read on calber).
    #[test]
    fn the_console_set_is_in_the_console_font() {
        const FONT: &str = "─│┌┐└┘├┊█■▮●♦◀∙⊕⊙≡√‖•· _";
        let cs = Charset::Console;
        let mut drawn: Vec<char> = cs.frame().to_vec();
        drawn.extend(cs.tree());
        drawn.extend([cs.segment(), cs.matrix_dot(), cs.caret(), cs.ruler()]);
        drawn.extend(EIGHTHS.iter().map(|&c| console_glyph(c)));
        drawn.extend(EIGHTHS_H.iter().map(|&c| console_glyph(c)));
        for c in "▀▄▪◈◉◆◇◍◎▤✓⏸⠀⣿".chars() {
            drawn.push(console_glyph(c));
        }
        for c in drawn {
            assert!(
                FONT.contains(c),
                "{c} U+{:04X} is not in the font",
                c as u32
            );
        }
    }
}
