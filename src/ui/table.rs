// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Paul Richeson
//! Tables — the Roll of Section VI-C and the Browser of Section VI-F.
//!
//! One column model serves both. Numeric columns are right-aligned, which is
//! the only thing that makes a column of figures scannable; the sorted column
//! is named in cyan with an arrow; and a flexible column absorbs whatever
//! width is left, so the same table renders at 44 columns and at 140.

use crate::tty::{Canvas, Rect, Rgb, BOLD, DIM};

use crate::ui::theme::Theme;

#[derive(Clone, Copy, Debug)]
pub struct Col {
    pub head: &'static str,
    /// Fixed width, or the minimum width when `flex`.
    pub w: i32,
    pub right: bool,
    pub flex: bool,
}

impl Col {
    pub const fn fixed(head: &'static str, w: i32) -> Col {
        Col {
            head,
            w,
            right: false,
            flex: false,
        }
    }
    pub const fn num(head: &'static str, w: i32) -> Col {
        Col {
            head,
            w,
            right: true,
            flex: false,
        }
    }
    pub const fn flex(head: &'static str, min: i32) -> Col {
        Col {
            head,
            w: min,
            right: false,
            flex: true,
        }
    }
}

/// Where each column starts and how wide it is, for a table `width` wide with
/// `gap` between columns. Columns that do not fit get zero width, and the
/// drawing code skips them — that is how the 100-column breakpoint drops the
/// Memory column without a second layout.
pub fn layout(cols: &[Col], width: i32, gap: i32) -> Vec<(i32, i32)> {
    let fixed: i32 = cols.iter().filter(|c| !c.flex).map(|c| c.w).sum();
    let flexes = cols.iter().filter(|c| c.flex).count() as i32;
    let gaps = gap * (cols.len() as i32 - 1).max(0);
    let spare = width - fixed - gaps;
    let per_flex = if flexes > 0 {
        (spare / flexes).max(0)
    } else {
        0
    };

    let mut out = Vec::with_capacity(cols.len());
    let mut x = 0;
    for c in cols {
        let w = if c.flex { per_flex.max(0) } else { c.w };
        if x >= width || w <= 0 {
            out.push((x.min(width), 0));
        } else {
            out.push((x, w.min(width - x)));
        }
        x += w + gap;
    }
    out
}

/// The header row. `sort` is the column index and whether it descends.
pub fn header(
    c: &mut Canvas,
    t: &Theme,
    r: Rect,
    cols: &[Col],
    lay: &[(i32, i32)],
    sort: Option<(usize, bool)>,
) {
    c.fill(Rect::new(r.x, r.y, r.w, 1), ' ', t.dim, t.panel);
    for (i, col) in cols.iter().enumerate() {
        let (x, w) = lay[i];
        if w <= 0 {
            continue;
        }
        let sorted = sort.map(|(s, _)| s == i).unwrap_or(false);
        let mark = match sort {
            Some((s, desc)) if s == i => {
                if desc {
                    "\u{25be}"
                } else {
                    "\u{25b4}"
                }
            }
            _ => "",
        };
        let fg = if sorted { t.cyan } else { t.dim };
        let text = format!("{}{}", col.head, mark);
        if col.right {
            c.text_right(r.x + x + w, r.y, w, &text, fg, t.panel, DIM);
        } else {
            c.text(r.x + x, r.y, w, &text, fg, t.panel, DIM);
        }
    }
}

/// One cell of a row.
// A cell has a position, a glyph, two colours and its attributes; a
// drawing primitive that takes them takes seven or eight arguments, and
// bundling them into a struct at every call site would cost more than it
// saved.
#[allow(clippy::too_many_arguments)]
pub fn cell(
    c: &mut Canvas,
    x: i32,
    y: i32,
    w: i32,
    text: &str,
    fg: Rgb,
    bg: Rgb,
    right: bool,
    attr: u8,
) {
    if w <= 0 {
        return;
    }
    if right {
        c.text_right(x + w, y, w, text, fg, bg, attr);
    } else {
        c.text(x, y, w, text, fg, bg, attr);
    }
}

/// The selection bar: a full-width ground with a marked left edge.
pub fn selection(c: &mut Canvas, t: &Theme, r: Rect, y: i32, focused: bool) {
    let bg = if focused { t.sel } else { t.sel.dark(0.35) };
    c.fill_bg(Rect::new(r.x, y, r.w, 1), bg);
    c.put(r.x, y, '\u{258e}', t.sel_edge, bg, BOLD);
}

/// The tree guides of the Browser, drawn to the left of a row's name.
/// `ancestors_last[i]` says whether the ancestor at depth `i` was the last of
/// its siblings — which is what decides between a continuing `│` and a blank.
// A cell has a position, a glyph, two colours and its attributes; a
// drawing primitive that takes them takes seven or eight arguments, and
// bundling them into a struct at every call site would cost more than it
// saved.
#[allow(clippy::too_many_arguments)]
pub fn guides(
    c: &mut Canvas,
    t: &Theme,
    x: i32,
    y: i32,
    max: i32,
    ancestors_last: &[bool],
    is_last: bool,
    // `Some((open, hidden))` for a row with children: the disclosure triangle,
    // and - when it is closed - how many descendants it is holding. The count
    // rides on the triangle rather than trailing the name, so it cannot be
    // mistaken for part of the command line.
    expandable: Option<(bool, usize)>,
    bg: Rgb,
) -> i32 {
    let [vert, tee, elbow, dash] = t.cs.tree();
    let mut n = 0;
    for &last in ancestors_last {
        if n + 2 > max {
            return n;
        }
        c.put(x + n, y, if last { ' ' } else { vert }, t.rule, bg, 0);
        c.put(x + n + 1, y, ' ', t.rule, bg, 0);
        n += 2;
    }
    if !ancestors_last.is_empty() || expandable.is_some() {
        // The row's own connector, when it has a parent.
    }
    if n + 2 <= max && !ancestors_last.is_empty() {
        c.put(x + n, y, if is_last { elbow } else { tee }, t.rule, bg, 0);
        c.put(x + n + 1, y, dash, t.rule, bg, 0);
        n += 2;
    }
    if let Some((open, hidden)) = expandable {
        if n + 2 <= max {
            c.put(
                x + n,
                y,
                if open { '\u{25be}' } else { '\u{25b8}' },
                t.cyan,
                bg,
                0,
            );
            n += 1;
            if !open && hidden > 0 {
                let count = hidden.to_string();
                n += c.text(x + n, y, (max - n - 1).max(0), &count, t.cyan, bg, 0);
            }
            c.put(x + n, y, ' ', t.rule, bg, 0);
            n += 1;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tty::Charset;

    const COLS: &[Col] = &[
        Col::num("PID", 7),
        Col::flex("NAME", 8),
        Col::num("CPU%", 6),
        Col::num("MEM", 9),
    ];

    #[test]
    fn the_flexible_column_absorbs_the_spare_width() {
        let lay = layout(COLS, 44, 1);
        assert_eq!(lay[0], (0, 7));
        assert_eq!(lay[1].1, 44 - 7 - 6 - 9 - 3);
        assert_eq!(lay[3].0 + lay[3].1, 44);
    }

    #[test]
    fn columns_that_do_not_fit_get_no_width_rather_than_a_negative_one() {
        let lay = layout(COLS, 14, 1);
        assert!(lay.iter().all(|(_, w)| *w >= 0));
        assert!(lay.iter().all(|(x, w)| x + w <= 14));
    }

    #[test]
    fn the_sorted_column_is_named_in_cyan_with_an_arrow() {
        let t = Theme::copal(Charset::Full);
        let mut c = Canvas::new(44, 1);
        let lay = layout(COLS, 44, 1);
        header(
            &mut c,
            &t,
            Rect::new(0, 0, 44, 1),
            COLS,
            &lay,
            Some((2, true)),
        );
        let row: String = (0..44).map(|x| c.get(x, 0).unwrap().ch).collect();
        assert!(row.contains("CPU%\u{25be}"));
        let at = row.find("CPU%").unwrap() as i32;
        assert_eq!(c.get(at, 0).unwrap().fg, t.cyan);
        assert_eq!(c.get(0, 0).unwrap().fg, t.dim);
    }

    #[test]
    fn tree_guides_continue_through_an_unfinished_ancestor() {
        let t = Theme::copal(Charset::Full);
        let mut c = Canvas::new(20, 1);
        c.clear(t.panel);
        let n = guides(&mut c, &t, 0, 0, 20, &[false, true], true, None, t.panel);
        let row: String = (0..n).map(|x| c.get(x, 0).unwrap().ch).collect();
        assert_eq!(row, "\u{2502}   \u{2570}\u{2500}");
    }

    #[test]
    fn a_collapsed_parent_shows_a_closed_disclosure_arrow() {
        let t = Theme::copal(Charset::Full);
        let mut c = Canvas::new(20, 1);
        c.clear(t.panel);
        guides(&mut c, &t, 0, 0, 20, &[], false, Some((false, 12)), t.panel);
        let row: String = (0..4).map(|x| c.get(x, 0).unwrap().ch).collect();
        assert_eq!(row, "\u{25b8}12 ", "the count rides on the triangle");
        c.clear(t.panel);
        guides(&mut c, &t, 0, 0, 20, &[], false, Some((true, 12)), t.panel);
        assert_eq!(
            c.get(0, 0).unwrap().ch,
            '\u{25be}',
            "an open parent shows no count"
        );
    }
}
