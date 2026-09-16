// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Paul Richeson
//! Turning the bytes a terminal sends into events.
//!
//! The decoder is incremental because a terminal is free to split an escape
//! sequence across two reads, and a half-parsed `CSI` that is thrown away
//! comes back as a stray `[` in the filter box.  State is kept between feeds
//! and a sequence is only emitted once it is complete.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Char(char),
    Ctrl(char),
    Alt(char),
    Enter,
    Esc,
    Tab,
    BackTab,
    Backspace,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Delete,
    Insert,
    F(u8),
    /// `CSI 8 ; rows ; cols t`, the terminal answering for its own size.
    Size(i32, i32),
    /// Button, column, row - all zero-based cells.  Button 64/65 are wheel up
    /// and down, which is how a long process list is scrolled.
    Mouse(u8, i32, i32),
}

pub struct KeyParser {
    buf: Vec<u8>,
}

impl Default for KeyParser {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyParser {
    pub fn new() -> KeyParser {
        KeyParser {
            buf: Vec::with_capacity(64),
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Key> {
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        loop {
            match self.step() {
                Step::Emit(k, n) => {
                    self.buf.drain(..n);
                    out.push(k);
                }
                Step::Drop(n) => {
                    self.buf.drain(..n);
                }
                Step::Incomplete => break,
                Step::Done => break,
            }
        }
        out
    }

    fn step(&mut self) -> Step {
        let b = &self.buf[..];
        if b.is_empty() {
            return Step::Done;
        }
        if b[0] != 0x1b {
            return match b[0] {
                b'\r' | b'\n' => Step::Emit(Key::Enter, 1),
                b'\t' => Step::Emit(Key::Tab, 1),
                0x7f | 0x08 => Step::Emit(Key::Backspace, 1),
                c if c < 0x20 => Step::Emit(Key::Ctrl((c + b'a' - 1) as char), 1),
                c if c < 0x80 => Step::Emit(Key::Char(c as char), 1),
                _ => {
                    // UTF-8: wait for the whole scalar before emitting it.
                    let need = utf8_len(b[0]);
                    if b.len() < need {
                        return Step::Incomplete;
                    }
                    match std::str::from_utf8(&b[..need]) {
                        Ok(s) => match s.chars().next() {
                            Some(c) => Step::Emit(Key::Char(c), need),
                            None => Step::Drop(need),
                        },
                        Err(_) => Step::Drop(1),
                    }
                }
            };
        }
        // An ESC by itself, with nothing behind it yet: a terminal sends a
        // complete sequence in one write, so a lone trailing ESC is the key.
        if b.len() == 1 {
            return Step::Emit(Key::Esc, 1);
        }
        match b[1] {
            b'[' => self.csi(),
            b'O' => {
                if b.len() < 3 {
                    return Step::Incomplete;
                }
                match b[2] {
                    b'A' => Step::Emit(Key::Up, 3),
                    b'B' => Step::Emit(Key::Down, 3),
                    b'C' => Step::Emit(Key::Right, 3),
                    b'D' => Step::Emit(Key::Left, 3),
                    b'H' => Step::Emit(Key::Home, 3),
                    b'F' => Step::Emit(Key::End, 3),
                    c @ b'P'..=b'S' => Step::Emit(Key::F(c - b'P' + 1), 3),
                    _ => Step::Drop(3),
                }
            }
            0x1b => Step::Emit(Key::Esc, 1),
            c if c < 0x80 => Step::Emit(Key::Alt(c as char), 2),
            _ => Step::Drop(2),
        }
    }

    fn csi(&self) -> Step {
        let b = &self.buf[..];
        // ESC [ <params> <final>, where final is @ through ~.
        let mut i = 2;
        while i < b.len() && !(0x40..=0x7e).contains(&b[i]) {
            i += 1;
        }
        if i >= b.len() {
            return Step::Incomplete;
        }
        let params = &b[2..i];
        let fin = b[i];
        let n = i + 1;
        let nums: Vec<i32> = std::str::from_utf8(params)
            .unwrap_or("")
            .trim_start_matches(['?', '<', '>'])
            .split(';')
            .map(|p| p.parse::<i32>().unwrap_or(0))
            .collect();
        let first = nums.first().copied().unwrap_or(0);
        match fin {
            b'A' => Step::Emit(Key::Up, n),
            b'B' => Step::Emit(Key::Down, n),
            b'C' => Step::Emit(Key::Right, n),
            b'D' => Step::Emit(Key::Left, n),
            b'H' => Step::Emit(Key::Home, n),
            b'F' => Step::Emit(Key::End, n),
            b'Z' => Step::Emit(Key::BackTab, n),
            b't' => {
                // CSI 8 ; rows ; cols t
                if first == 8 && nums.len() >= 3 {
                    Step::Emit(Key::Size(nums[2], nums[1]), n)
                } else {
                    Step::Drop(n)
                }
            }
            b'M' | b'm' if params.first() == Some(&b'<') => {
                if nums.len() >= 3 {
                    // SGR mouse is one-based; a release is the same button
                    // with an `m` final, which the app ignores.
                    if fin == b'M' {
                        Step::Emit(Key::Mouse(nums[0] as u8, nums[1] - 1, nums[2] - 1), n)
                    } else {
                        Step::Drop(n)
                    }
                } else {
                    Step::Drop(n)
                }
            }
            b'~' => match first {
                1 | 7 => Step::Emit(Key::Home, n),
                2 => Step::Emit(Key::Insert, n),
                3 => Step::Emit(Key::Delete, n),
                4 | 8 => Step::Emit(Key::End, n),
                5 => Step::Emit(Key::PageUp, n),
                6 => Step::Emit(Key::PageDown, n),
                11..=15 => Step::Emit(Key::F((first - 10) as u8), n),
                17..=21 => Step::Emit(Key::F((first - 11) as u8), n),
                23..=26 => Step::Emit(Key::F((first - 12) as u8), n),
                _ => Step::Drop(n),
            },
            _ => Step::Drop(n),
        }
    }
}

enum Step {
    Emit(Key, usize),
    Drop(usize),
    Incomplete,
    Done,
}

fn utf8_len(b: u8) -> usize {
    if b >= 0xf0 {
        4
    } else if b >= 0xe0 {
        3
    } else if b >= 0xc0 {
        2
    } else {
        1
    }
}

/// The size out of a `CSI 8 ; rows ; cols t` report, as `(cols, rows)`.
/// Used by the startup handshake, before the parser exists.
pub fn size_reply(b: &[u8]) -> Option<(i32, i32)> {
    let mut i = 0;
    while i + 4 < b.len() {
        if b[i] == 0x1b && b[i + 1] == b'[' && b[i + 2] == b'8' && b[i + 3] == b';' {
            let mut j = i + 4;
            while j < b.len() && b[j] != b't' {
                j += 1;
            }
            if j < b.len() {
                let mut it = b[i + 4..j].split(|&c| c == b';');
                let rows = std::str::from_utf8(it.next()?).ok()?.trim().parse().ok()?;
                let cols = std::str::from_utf8(it.next()?).ok()?.trim().parse().ok()?;
                return Some((cols, rows));
            }
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_keys() {
        let mut p = KeyParser::new();
        assert_eq!(p.feed(b"qk"), vec![Key::Char('q'), Key::Char('k')]);
        assert_eq!(p.feed(b"\r"), vec![Key::Enter]);
        assert_eq!(p.feed(&[0x03]), vec![Key::Ctrl('c')]);
    }

    #[test]
    fn arrows_and_function_keys() {
        let mut p = KeyParser::new();
        assert_eq!(
            p.feed(b"\x1b[A\x1b[6~\x1b[21~"),
            vec![Key::Up, Key::PageDown, Key::F(10)]
        );
    }

    #[test]
    fn a_sequence_split_across_reads_survives() {
        let mut p = KeyParser::new();
        assert_eq!(p.feed(b"\x1b["), vec![]);
        assert_eq!(p.feed(b"B"), vec![Key::Down]);
    }

    #[test]
    fn size_report_is_cols_then_rows() {
        let mut p = KeyParser::new();
        assert_eq!(p.feed(b"\x1b[8;40;120t"), vec![Key::Size(120, 40)]);
        assert_eq!(size_reply(b"junk\x1b[8;40;120t"), Some((120, 40)));
    }

    #[test]
    fn sgr_mouse_is_zero_based() {
        let mut p = KeyParser::new();
        assert_eq!(p.feed(b"\x1b[<0;10;5M"), vec![Key::Mouse(0, 9, 4)]);
    }

    #[test]
    fn utf8_split_across_reads() {
        let mut p = KeyParser::new();
        assert_eq!(p.feed(&[0xe2]), vec![]);
        assert_eq!(p.feed(&[0x94, 0x80]), vec![Key::Char('─')]);
    }
}
