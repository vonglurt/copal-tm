// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Paul Richeson
//! frame2html: replay what copal-tm wrote to a terminal, and write the screen
//! it would have left as one HTML page, for a headless browser to photograph.
//!
//! Used by `tools/feature-guide.sh`, which compiles it with `rustc` alone.
//! It is not a target of the crate and depends on nothing but `std`.
//!
//! It is a terminal emulator only as far as copal-tm needs one: cursor
//! position (`CSI r;c H`), SGR colour and attributes (true colour or the
//! sixteen ANSI colours), erase-display, and plain text.  Every other escape
//! sequence (the alternate screen, mouse reporting, the cursor's visibility)
//! is read and ignored.  Damage-only redraws replay correctly because the
//! screen persists between frames, exactly as it does in a terminal.
//!
//!   frame2html COLS ROWS < stream.ansi > page.html
//!   frame2html COLS ROWS 'N:row,col,w,h' 'N@text' 'N#2@text~w,h' ... < stream > page.html
//!
//! Each annotation draws a numbered ring, so a caption under the picture can
//! say "1." and be found.  `N:row,col,w,h` rings that block of cells
//! (0-based).  `N@text` rings the first place `text` is on the screen;
//! `N#k@text` the k-th; and `~w,h` makes the ring w cells by h rows from
//! where the text starts.  A ring whose text is not on the screen is an
//! error, so a caption cannot quietly point at nothing.
//!
//! The stream may end with the line `script(1)` appends when it is stopped;
//! everything from "Script done on " onward is dropped.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::{Read, Write};

const CW: i32 = 10; // cell width, px
const CH: i32 = 20; // cell height, px
const PAD: i32 = 14; // margin around the screen, px

#[derive(Clone, Copy, PartialEq)]
struct Style {
    fg: (u8, u8, u8),
    bg: (u8, u8, u8),
    bold: bool,
    dim: bool,
    under: bool,
}

const DEFAULT: Style = Style {
    fg: (200, 205, 210),
    bg: (12, 14, 18),
    bold: false,
    dim: false,
    under: false,
};

/// The xterm defaults, for frames drawn with `--color 16`.
const ANSI16: [(u8, u8, u8); 16] = [
    (0, 0, 0),
    (205, 0, 0),
    (0, 205, 0),
    (205, 205, 0),
    (0, 0, 238),
    (205, 0, 205),
    (0, 205, 205),
    (229, 229, 229),
    (127, 127, 127),
    (255, 0, 0),
    (0, 255, 0),
    (255, 255, 0),
    (92, 92, 255),
    (255, 0, 255),
    (0, 255, 255),
    (255, 255, 255),
];

struct Screen {
    w: i32,
    h: i32,
    cells: Vec<(char, Style)>,
    x: i32,
    y: i32,
    pen: Style,
}

impl Screen {
    fn new(w: i32, h: i32) -> Screen {
        Screen {
            w,
            h,
            cells: vec![(' ', DEFAULT); (w * h) as usize],
            x: 0,
            y: 0,
            pen: DEFAULT,
        }
    }

    fn put(&mut self, c: char) {
        if self.x < self.w && self.y < self.h && self.x >= 0 && self.y >= 0 {
            let i = (self.y * self.w + self.x) as usize;
            self.cells[i] = (c, self.pen);
        }
        self.x += 1;
    }

    fn sgr(&mut self, params: &[i32]) {
        let params = if params.is_empty() { &[0][..] } else { params };
        let mut i = 0;
        while i < params.len() {
            let p = params[i];
            match p {
                0 => self.pen = DEFAULT,
                1 => self.pen.bold = true,
                2 => self.pen.dim = true,
                4 => self.pen.under = true,
                22 => {
                    self.pen.bold = false;
                    self.pen.dim = false
                }
                24 => self.pen.under = false,
                30..=37 => self.pen.fg = ANSI16[(p - 30) as usize],
                90..=97 => self.pen.fg = ANSI16[(p - 90 + 8) as usize],
                40..=47 => self.pen.bg = ANSI16[(p - 40) as usize],
                100..=107 => self.pen.bg = ANSI16[(p - 100 + 8) as usize],
                39 => self.pen.fg = DEFAULT.fg,
                49 => self.pen.bg = DEFAULT.bg,
                38 | 48 if params.get(i + 1) == Some(&2) && i + 4 < params.len() => {
                    let c = (
                        params[i + 2] as u8,
                        params[i + 3] as u8,
                        params[i + 4] as u8,
                    );
                    if p == 38 {
                        self.pen.fg = c
                    } else {
                        self.pen.bg = c
                    }
                    i += 4;
                }
                _ => {}
            }
            i += 1;
        }
    }

    fn feed(&mut self, s: &str) {
        let mut it = s.chars().peekable();
        while let Some(c) = it.next() {
            match c {
                '\x1b' => match it.next() {
                    Some('[') => {
                        let mut body = String::new();
                        let mut fin = '\0';
                        for d in it.by_ref() {
                            if ('\x40'..='\x7e').contains(&d) {
                                fin = d;
                                break;
                            }
                            body.push(d);
                        }
                        if body.starts_with(['?', '>', '<', '=']) {
                            continue;
                        }
                        let nums: Vec<i32> = body
                            .split(';')
                            .map(|n| n.parse().unwrap_or(0))
                            .collect();
                        let n0 = if body.is_empty() { 0 } else { nums[0] };
                        match fin {
                            'H' | 'f' => {
                                self.y = n0.max(1) - 1;
                                self.x = nums.get(1).copied().unwrap_or(1).max(1) - 1;
                            }
                            'm' => {
                                let v = if body.is_empty() { vec![] } else { nums };
                                self.sgr(&v);
                            }
                            'J' if n0 == 2 => {
                                self.cells = vec![(' ', self.pen); (self.w * self.h) as usize]
                            }
                            _ => {}
                        }
                    }
                    Some(']') => {
                        // OSC: up to BEL or ESC \.
                        while let Some(d) = it.next() {
                            if d == '\x07' {
                                break;
                            }
                            if d == '\x1b' {
                                it.next();
                                break;
                            }
                        }
                    }
                    Some('(') | Some(')') => {
                        it.next();
                    }
                    _ => {}
                },
                '\r' => self.x = 0,
                '\n' => self.y += 1,
                c if (c as u32) < 0x20 => {}
                c => self.put(c),
            }
        }
    }
}

fn mix(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> (u8, u8, u8) {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    (f(a.0, b.0), f(a.1, b.1), f(a.2, b.2))
}

fn hex(c: (u8, u8, u8)) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}

fn esc(c: char) -> String {
    match c {
        '<' => "&lt;".into(),
        '>' => "&gt;".into(),
        '&' => "&amp;".into(),
        ' ' => "&nbsp;".into(),
        c => c.to_string(),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: frame2html COLS ROWS ['N:row,col,w,h' ...] < stream > page.html");
        std::process::exit(2);
    }
    let w: i32 = args[0].parse().expect("COLS");
    let h: i32 = args[1].parse().expect("ROWS");
    let mut raw = Vec::new();
    std::io::stdin().read_to_end(&mut raw).expect("stdin");
    let text = String::from_utf8_lossy(&raw);
    let text = match text.find("Script done on ") {
        Some(i) => &text[..i],
        None => &text[..],
    };
    let mut scr = Screen::new(w, h);
    scr.feed(text);
    let lines: Vec<Vec<char>> = (0..h)
        .map(|y| (0..w).map(|x| scr.cells[(y * w + x) as usize].0).collect())
        .collect();

    // One class per distinct style keeps the page small.
    let mut classes: HashMap<String, usize> = HashMap::new();
    let mut css = String::new();
    let mut rows = String::new();
    for y in 0..h {
        rows.push_str("<div class=r>");
        for x in 0..w {
            let (ch, st) = scr.cells[(y * w + x) as usize];
            let fg = if st.dim { mix(st.fg, st.bg, 0.45) } else { st.fg };
            let mut rule = format!("color:{};background:{}", hex(fg), hex(st.bg));
            if st.bold {
                rule.push_str(";font-weight:bold");
            }
            if st.under {
                rule.push_str(";text-decoration:underline");
            }
            let n = classes.len();
            let k = *classes.entry(rule.clone()).or_insert_with(|| {
                let _ = writeln!(css, ".s{n}{{{rule}}}");
                n
            });
            let _ = write!(rows, "<span class=s{k}>{}</span>", esc(ch));
        }
        rows.push_str("</div>\n");
    }

    let mut rings = String::new();
    for a in &args[2..] {
        let (r, c, rw, rh, label) = match a.split_once('@') {
            Some((head, rest)) if !a.contains(':') || a.find('@') < a.find(':') => {
                let (label, nth) = match head.split_once('#') {
                    Some((l, k)) => (l, k.parse::<usize>().expect("#k")),
                    None => (head, 1),
                };
                let (needle, size) = match rest.rsplit_once('~') {
                    Some((n, sz)) => (n, Some(sz)),
                    None => (rest, None),
                };
                let (r, c) = find(&lines, needle, nth).unwrap_or_else(|| {
                    eprintln!("frame2html: {needle:?} (#{nth}) is not on the screen");
                    std::process::exit(1);
                });
                let (rw, rh) = match size {
                    Some(sz) => {
                        let v: Vec<i32> = sz.split(',').map(|n| n.parse().expect("w,h")).collect();
                        (v[0], v[1])
                    }
                    None => (needle.chars().count() as i32, 1),
                };
                (r, c, rw, rh, label)
            }
            _ => {
                let (label, spec) = a.split_once(':').expect("N:row,col,w,h or N@text");
                let v: Vec<i32> = spec
                    .split(',')
                    .map(|n| n.trim().parse().expect("number"))
                    .collect();
                (v[0], v[1], v[2], v[3], label)
            }
        };
        let _ = write!(
            rings,
            "<div class=ring style=\"left:{}px;top:{}px;width:{}px;height:{}px\">\
             <b>{}</b></div>",
            PAD + c * CW - 4,
            PAD + r * CH - 3,
            rw * CW + 4,
            rh * CH + 2,
            esc_str(label)
        );
    }

    let page_bg = hex(scr.cells[0].1.bg);
    let html = format!(
        "<!doctype html><meta charset=utf-8><style>\
         html,body{{margin:0;background:{page_bg}}}\
         .t{{position:relative;padding:{PAD}px;width:{tw}px;\
           font:17px/{CH}px 'DejaVu Sans Mono',monospace}}\
         .r{{display:flex;height:{CH}px;white-space:pre}}\
         .r span{{flex:none;width:{CW}px;height:{CH}px;overflow:visible;text-align:center}}\
         .ring{{position:absolute;border:2px solid #ffcc00;border-radius:5px;\
           box-shadow:0 0 0 1px rgba(0,0,0,.7),0 0 10px rgba(255,204,0,.45)}}\
         .ring b{{position:absolute;left:-13px;top:-13px;width:22px;height:22px;\
           border-radius:11px;background:#ffcc00;color:#111;\
           font:bold 13px/22px 'DejaVu Sans',sans-serif;text-align:center;\
           box-shadow:0 0 0 2px rgba(0,0,0,.65)}}\n{css}</style>\
         <div class=t>\n{rows}{rings}</div>\n",
        tw = w * CW,
    );
    std::io::stdout().write_all(html.as_bytes()).expect("stdout");
}

/// Row and column of the k-th place `needle` is on the screen.
fn find(lines: &[Vec<char>], needle: &str, nth: usize) -> Option<(i32, i32)> {
    let n: Vec<char> = needle.chars().collect();
    let mut seen = 0;
    for (y, line) in lines.iter().enumerate() {
        if n.is_empty() || n.len() > line.len() {
            continue;
        }
        for x in 0..=line.len() - n.len() {
            if line[x..x + n.len()] == n[..] {
                seen += 1;
                if seen == nth {
                    return Some((y as i32, x as i32));
                }
            }
        }
    }
    None
}

fn esc_str(s: &str) -> String {
    s.chars().map(esc).collect()
}
