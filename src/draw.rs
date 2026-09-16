// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Paul Richeson
//! Every cell this program writes.
//!
//! One entry point, `frame`, which cuts the screen into the bands of Section
//! V-B, draws the groups of Section VI into them, and puts any overlay on top.
//! Nothing here decides anything: it reads `App` and writes cells.

use copal_tm::machine::fmt;
use copal_tm::machine::halt::Severity;
use copal_tm::machine::signals::Disposition;
use copal_tm::machine::types::Supervision;
use copal_tm::tty::{Canvas, Rect, Rgb, BOLD, DIM};
use copal_tm::ui::graph::{self, Axis, Series};
use copal_tm::ui::ladder::{self, Hue, Ladder};
use copal_tm::ui::panel::{self, Panel};
use copal_tm::ui::table::{self, Col};
use copal_tm::ui::tile::{self, Tile};

use crate::app::{App, Focus, Overlay};
use crate::config::View;
use crate::transcript::Kind;

const SYS_W: i32 = 22;
const ROLL_W: i32 = 44;

pub fn frame(app: &mut App, c: &mut Canvas) {
    let t = app.theme.clone();
    c.clear(t.ground);
    let all = c.area();
    let (status, rest) = all.cut_bottom(1);
    let area = rest.pad(0, 1, 0, 1);

    let show_dash = app.cfg.view != View::Browser && area.h >= 8;
    let show_browser = app.cfg.view != View::Dashboard;
    let narrow = area.w < 80;

    let mut r = area;
    if show_dash {
        let reserve = if show_browser { 8 } else { 0 };
        let budget = (r.h - reserve).max(0);
        if narrow || budget < 12 {
            // Under 80 columns the dashboard collapses to a gauge rail.
            let (rail, rest) = r.cut_top(budget.min(5));
            gauge_rail(app, c, rail);
            r = rest;
        } else {
            let h1 = budget.clamp(9, 14);
            let (band1, rest) = r.cut_top(h1);
            band_one(app, c, band1);
            r = rest;
            let left = (r.h - reserve).max(0);
            if left >= 8 {
                let (band2, rest) = r.cut_top(9.min(left));
                memory(app, c, band2);
                r = rest;
            }
            let left = (r.h - reserve).max(0);
            if left >= 5 {
                let (band3, rest) = r.cut_top(5.min(left));
                tiles(app, c, band3);
                r = rest;
            }
        }
    }
    if show_browser && r.h >= 4 {
        let body = if app.overlay == Overlay::Inspector && area.w >= 132 {
            let (pane, left) = r.cut_right(46);
            inspector(app, c, pane);
            left
        } else {
            r
        };
        browser(app, c, body);
    }
    status_bar(app, c, status);

    match app.overlay {
        Overlay::Inspector if area.w < 132 => {
            inspector(app, c, centred(all, 72, 22));
        }
        Overlay::Services => services(app, c, centred(all, 58, 14)),
        Overlay::Halt => halt(app, c, centred(all, 74, 20)),
        Overlay::Transcript => transcript(app, c, centred(all, 84, 24)),
        Overlay::Help => help(app, c, centred(all, 66, 22)),
        _ => {}
    }
}

fn centred(area: Rect, w: i32, h: i32) -> Rect {
    let w = w.min(area.w - 2).max(20);
    let h = h.min(area.h - 2).max(6);
    Rect::new(area.x + (area.w - w) / 2, area.y + (area.h - h) / 2, w, h)
}

// ---- band 1 -----------------------------------------------------------

fn band_one(app: &mut App, c: &mut Canvas, r: Rect) {
    let roll_w = if r.w >= 132 { ROLL_W } else { 34 };
    let show_roll = r.w >= 100;
    let (sys, rest) = r.cut_left(SYS_W.min(r.w / 3));
    sys_panel(app, c, sys);
    if show_roll && rest.w > roll_w + 24 {
        let (roll, hist) = rest.pad(0, 0, 0, 1).cut_right(roll_w);
        cpu_history(app, c, hist.pad(0, 1, 0, 0));
        roll_panel(app, c, roll);
    } else {
        cpu_history(app, c, rest.pad(0, 0, 0, 1));
    }
}

/// Group A — the instrument cluster.
fn sys_panel(app: &mut App, c: &mut Canvas, r: Rect) {
    let t = app.theme.clone();
    let readout = app.sys_readout();
    let focused = app.focus == Focus::Dashboard;
    let body = panel::draw(c, &t, r, &Panel::new("SYS", &readout).focused(focused));
    if body.is_empty() {
        return;
    }
    let meters = app.cfg.meters.clone();
    let n = meters.len() as i32;
    let cols = body.columns(&vec![1; meters.len()], 1);
    for (m, col) in meters.iter().zip(cols.iter()) {
        let (v, text, label) = app.meter(*m);
        let hue = match label {
            "CPU" => Hue::Solid(t.green),
            "Batt" => Hue::Solid(t.battery),
            "Temp" | "PSI" => Hue::Ramp(&t.heat),
            "GPU" => Hue::Ramp(&t.cool),
            "Mem" | "Swap" => Hue::Solid(t.magenta),
            "Net" => Hue::Solid(t.net),
            _ => Hue::Solid(t.disk),
        };
        let l = Ladder {
            label,
            readout: &text,
            value: v,
            hue,
            stale: false,
        };
        ladder::vertical(c, &t, *col, &l);
    }
    let _ = n;
}

/// Group B — the strip chart, its legend, axes, brush, carets and footers.
fn cpu_history(app: &mut App, c: &mut Canvas, r: Rect) {
    let t = app.theme.clone();
    let readout = format!("{:.0}%", app.snap.cpu.total * 100.0);
    let focused = app.focus == Focus::Dashboard;
    let body = panel::draw(
        c,
        &t,
        r,
        &Panel::new("CPU HISTORY", &readout).focused(focused),
    );
    if body.w < 20 || body.h < 4 {
        return;
    }
    let (legend_row, rest) = body.cut_top(1);
    let foot_rows = if rest.h >= 6 { 2 } else { 1 };
    let (foot, plot_area) = rest.cut_bottom(foot_rows);
    let (left_gut, rest) = plot_area.cut_left(5);
    let (right_gut, plot) = rest.cut_right(5);

    let span = app.cfg.history_span;
    let cols = graph::dot_columns(t.cs, plot.w);
    let util = app.hist.util.bucket_max(span, cols);
    let kern = app.hist.kernel.bucket_max(span, cols);
    let temp = app.hist.temp.bucket_max(span, cols);
    let tmax = app.snap.thermal.scale_max.max(1.0);

    // Drawn in this order so the hottest quantity is never the one hidden.
    let series = [
        Series {
            name: "Kernel",
            color: t.red,
            data: &kern,
            max: 1.0,
            visible: app.series_on[2],
            axis: Axis::Left,
        },
        Series {
            name: "Utilization",
            color: t.green,
            data: &util,
            max: 1.0,
            visible: app.series_on[0],
            axis: Axis::Left,
        },
        Series {
            name: "Temperature",
            color: t.orange,
            data: &temp,
            max: tmax,
            visible: app.series_on[1],
            axis: Axis::Right,
        },
    ];
    // The legend is in reading order, not drawing order.
    let legend = [&series[1], &series[2], &series[0]];
    let legend_owned: Vec<Series> = legend
        .iter()
        .map(|s| Series {
            name: s.name,
            color: s.color,
            data: s.data,
            max: s.max,
            visible: s.visible,
            axis: s.axis,
        })
        .collect();
    graph::legend(c, &t, legend_row, &legend_owned);

    // The brush: the most recent sixty seconds, shared with every history.
    let brush = if cols > 8 {
        let secs = 60.0 / app.period.max(0.1);
        let frac = (secs / span as f64).clamp(0.05, 1.0);
        let from = ((1.0 - frac) * cols as f64) as usize;
        Some((from, cols - 1))
    } else {
        None
    };
    graph::plot(c, &t, plot, &series, brush);
    graph::carets(c, &t, plot, right_gut.x, &series);

    for (f, label) in [(1.0f32, "100%"), (0.5, "50%"), (0.0, "0%")] {
        let y = plot.y + ((1.0 - f) * (plot.h - 1) as f32).round() as i32;
        c.text_right(left_gut.right() - 1, y, 5, label, t.dim, t.panel, DIM);
        let right = format!("{:.0}\u{b0}", tmax * f);
        c.text(
            right_gut.x + 1,
            y,
            4,
            &right,
            t.orange.on(t.panel, 0.7),
            t.panel,
            DIM,
        );
    }

    let n = app.snap.cpu.logical();
    let power = app
        .snap
        .power
        .watts
        .map(|w| format!(" \u{b7} {}", fmt::watts(w)))
        .unwrap_or_default();
    let left = format!(
        "{n} logical processor{} \u{b7} {}{}",
        if n == 1 { "" } else { "s" },
        fmt::pct(app.snap.cpu.total),
        power
    );
    c.text(foot.x, foot.y, foot.w - 14, &left, t.dim, t.panel, DIM);
    let gov = app
        .snap
        .cpu
        .governor
        .clone()
        .map(|g| format!("Speed: {g}"))
        .unwrap_or_else(|| "Speed Auto".into());
    c.text_right(foot.right(), foot.y, 20, &gov, t.dim, t.panel, DIM);

    if foot_rows == 2 {
        core_strip(app, c, Rect::new(foot.x, foot.y + 1, foot.w, 1));
    }
}

/// The per-core strip: one micro-meter per logical processor, grouped by
/// class. This is the row that shows a single-threaded build pinning one big
/// core while eleven others idle.
fn core_strip(app: &mut App, c: &mut Canvas, r: Rect) {
    let t = app.theme.clone();
    let cores = &app.snap.cpu.per_core;
    if cores.is_empty() || r.w < 8 {
        return;
    }
    let classes = &app.snap.cpu.classes;
    let names = &app.snap.cpu.class_names;
    let mut x = r.x;
    let mut last_class: Option<u8> = None;
    for (i, v) in cores.iter().enumerate() {
        if x >= r.right() - 1 {
            break;
        }
        let cls = classes.get(i).copied().unwrap_or_default();
        if last_class != Some(cls.class) {
            if let Some(name) = names.get(cls.class as usize) {
                if x + 2 < r.right() {
                    if last_class.is_some() {
                        x += 1;
                    }
                    c.text(x, r.y, 2, name, t.dim, t.panel, DIM);
                    x += name.chars().count() as i32;
                }
            }
            last_class = Some(cls.class);
        }
        let hue = if cls.class == 0 {
            t.green
        } else {
            t.green.mix(t.gpu, 0.45)
        };
        ladder::micro(c, &t, x, r.y, *v, hue, cls.sibling);
        x += 1;
    }
    let text = fmt::pct(app.snap.cpu.iowait);
    c.text_right(
        r.right(),
        r.y,
        14,
        &format!("iowait {text}"),
        t.dim,
        t.panel,
        DIM,
    );
}

/// Group C — the Roll.
fn roll_panel(app: &mut App, c: &mut Canvas, r: Rect) {
    let t = app.theme.clone();
    let count = format!("{}", app.snap.procs.len());
    // Letter-spacing costs a column a character; the narrow form of the title
    // is a shorter title, not a smaller one.
    let title = if r.w >= 40 {
        "AVERAGE CPU USE"
    } else {
        "CPU USE"
    };
    let body = panel::draw(c, &t, r, &Panel::new(title, &count));
    if body.h < 2 {
        return;
    }
    // The mark is its own column: crowding it into the PID column's gutter
    // costs a digit on a five-figure pid.
    let cols: Vec<Col> = if body.w >= 40 {
        vec![
            Col::num("PID", 7),
            Col::fixed("", 1),
            Col::flex("Name", 8),
            Col::num("CPU", 6),
            Col::num("Memory", 9),
        ]
    } else {
        vec![
            Col::num("PID", 7),
            Col::fixed("", 1),
            Col::flex("Name", 8),
            Col::num("CPU", 6),
        ]
    };
    let lay = table::layout(&cols, body.w, 1);
    table::header(c, &t, body, &cols, &lay, Some((3, true)));
    for (n, &i) in app.roll.iter().take((body.h - 1) as usize).enumerate() {
        let Some(p) = app.snap.procs.get(i) else {
            continue;
        };
        let y = body.y + 1 + n as i32;
        let (mark, mcol) = mark_of(app, p, &t);
        table::cell(
            c,
            body.x + lay[0].0,
            y,
            lay[0].1,
            &p.pid.to_string(),
            t.dim,
            t.panel,
            true,
            0,
        );
        if lay[1].1 > 0 {
            c.put(body.x + lay[1].0, y, mark, mcol, t.panel, 0);
        }
        table::cell(
            c,
            body.x + lay[2].0,
            y,
            lay[2].1,
            &fmt::elide(&p.name, lay[2].1.max(0) as usize),
            t.text,
            t.panel,
            false,
            0,
        );
        let cpu = fmt::pct(p.cpu);
        let cpu_col = if p.cpu > 0.10 { t.green } else { t.text };
        table::cell(
            c,
            body.x + lay[3].0,
            y,
            lay[3].1,
            &cpu,
            cpu_col,
            t.panel,
            true,
            0,
        );
        if lay.len() > 4 && lay[4].1 > 0 {
            table::cell(
                c,
                body.x + lay[4].0,
                y,
                lay[4].1,
                &fmt::kb(p.rss_kb),
                t.dim,
                t.panel,
                true,
                0,
            );
        }
    }
}

/// The mark glyph that replaces the reference image's application icon, and
/// carries more than decoration does: what kind of process, and what state.
fn mark_of(app: &App, p: &copal_tm::machine::Proc, t: &copal_tm::ui::Theme) -> (char, Rgb) {
    let glyph = if p.pid == app.machine.me {
        '\u{25c8}'
    } else if p.kernel_thread {
        '\u{25c7}'
    } else if p.supervision != Supervision::None {
        '\u{25aa}'
    } else {
        '\u{25c6}'
    };
    let colour = match p.state {
        _ if p.pid == app.machine.me => t.cyan,
        'R' => t.green,
        'D' => t.orange,
        'T' | 't' => t.yellow,
        'Z' => t.red,
        _ if p.kernel_thread => t.dim,
        _ if p.supervision != Supervision::None => t.net,
        _ => t.text,
    };
    (glyph, colour)
}

// ---- band 2 -----------------------------------------------------------

/// Group D — the wide band. Document title, ladder, area chart, captions.
fn memory(app: &mut App, c: &mut Canvas, r: Rect) {
    let t = app.theme.clone();
    let m = app.snap.mem;
    let readout = format!("{} / {}", fmt::kb(m.used_kb()), fmt::kb(m.total_kb));
    let body = panel::draw(
        c,
        &t,
        r,
        &Panel::new("Memory Utilization", &readout).document(),
    );
    if body.h < 3 || body.w < 24 {
        return;
    }
    let (foot, upper) = body.cut_bottom(1);
    let (gauge, rest) = upper.cut_left(6);
    let pct = fmt::pct(m.frac());
    ladder::vertical(
        c,
        &t,
        gauge,
        &Ladder {
            label: "",
            readout: &pct,
            value: m.frac(),
            hue: Hue::Solid(t.magenta),
            stale: false,
        },
    );
    let plot = rest.pad(0, 0, 0, 1);
    let data = app
        .hist
        .mem
        .bucket_max(app.cfg.history_span, plot.w.max(1) as usize);
    graph::area(c, &t, plot, &data, 1.0, t.magenta);

    // The in-plot extent labels of the reference image: they cost no layout
    // and they turn a shape into a quantity.
    let (lo, hi) = app.hist.mem.extent(app.cfg.history_span);
    // Bright, and over whatever is already there: a label that punched a
    // panel-coloured hole in the fill would damage the shape it describes.
    let label = t.bloom(t.magenta);
    c.text_keep_bg(
        plot.x + 1,
        plot.y,
        10,
        &fmt::kb((hi * m.total_kb as f32) as u64),
        label,
        DIM,
    );
    c.text_keep_bg(
        plot.x + 1,
        plot.bottom() - 1,
        10,
        &fmt::kb((lo * m.total_kb as f32) as u64),
        label,
        DIM,
    );

    let avail = format!("Available {}", fmt::kb(m.avail_kb));
    let cached = format!("Cached {}", fmt::kb(m.cached_kb));
    let swap = format!("Swap {}", fmt::kb(m.swap_used_kb));
    c.text(foot.x, foot.y, foot.w / 3, &avail, t.dim, t.panel, DIM);
    c.text_center(foot, foot.y, &cached, t.dim, t.panel, DIM);
    // Swap turns orange once it is non-zero: the moment memory stops being a
    // curiosity.
    let swap_col = if m.swap_used_kb > 0 { t.orange } else { t.dim };
    c.text_right(
        foot.right(),
        foot.y,
        foot.w / 3,
        &swap,
        swap_col,
        t.panel,
        DIM,
    );
}

// ---- band 3 -----------------------------------------------------------

fn tiles(app: &mut App, c: &mut Canvas, r: Rect) {
    let t = app.theme.clone();
    let n = if r.w >= 120 { 4 } else { 2 };
    let cols = r.columns(&vec![1; n], 1);
    let s = &app.snap;

    let net_head = fmt::rate(s.net.total_bps());
    let net_sub = format!(
        "{} \u{b7} {}",
        s.net
            .link_mbps
            .map(|m| format!("{m} Mb/s"))
            .unwrap_or_else(|| s.net.iface.clone()),
        fmt::pct(s.net.err_rate)
    );
    let net_scale = format!("Scale {}", fmt::rate(app.net_scale));
    tile::draw(
        c,
        &t,
        cols[0],
        &Tile {
            name: "Network",
            icon: '\u{25cd}',
            headline: &net_head,
            sub_left: &net_sub,
            sub_right: &net_scale,
            value: (s.net.total_bps() / app.net_scale.max(1.0)) as f32,
            hue: Hue::Solid(t.net),
            stale: false,
        },
    );

    let disk_head = fmt::pct(s.disk.busy);
    let disk_sub = format!(
        "R {} \u{b7} W {}",
        fmt::rate(s.disk.read_bps),
        fmt::rate(s.disk.write_bps)
    );
    let smart = s
        .disk
        .smart
        .clone()
        .map(|v| format!("SMART: {v}"))
        .unwrap_or_default();
    let disk_name = if s.disk.name.is_empty() {
        "Storage".to_string()
    } else {
        s.disk.name.to_uppercase()
    };
    tile::draw(
        c,
        &t,
        cols[1],
        &Tile {
            name: &disk_name,
            icon: '\u{25a4}',
            headline: &disk_head,
            sub_left: &disk_sub,
            sub_right: &smart,
            value: s.disk.busy,
            hue: Hue::Solid(t.disk),
            stale: false,
        },
    );

    if n < 4 {
        return;
    }

    let (slot_v, slot_head, slot_name, slot_sub) = slot(app);
    tile::draw(
        c,
        &t,
        cols[2],
        &Tile {
            name: &slot_name,
            icon: '\u{25c9}',
            headline: &slot_head,
            sub_left: &slot_sub,
            sub_right: "",
            value: slot_v,
            hue: Hue::Solid(t.amber),
            stale: false,
        },
    );

    let psi = s.psi;
    let psi_sub = format!(
        "CPU {:.1} \u{b7} Mem {:.1} \u{b7} I/O {:.1}",
        psi.cpu10, psi.mem10, psi.io10
    );
    tile::draw(
        c,
        &t,
        cols[3],
        &Tile {
            name: "System Pressure",
            icon: '\u{25ce}',
            headline: psi.verdict(),
            sub_left: &psi_sub,
            sub_right: if psi.present { "" } else { "no PSI" },
            value: (psi.worst() / 40.0).clamp(0.0, 1.0),
            hue: Hue::Ramp(&t.heat),
            stale: false,
        },
    );
}

/// The configurable third tile — the reference image's NPU, which Alpine has
/// no equivalent of.
fn slot(app: &App) -> (f32, String, String, String) {
    let s = &app.snap;
    match app.cfg.slot.as_str() {
        "load" => {
            let n = s.cpu.logical().max(1) as f32;
            (
                (s.load[0] / n).clamp(0.0, 1.0),
                format!("{:.2}", s.load[0]),
                "Load".into(),
                format!("{:.2} \u{b7} {:.2} over {n:.0} cores", s.load[1], s.load[2]),
            )
        }
        "processes" => {
            let n = s.procs.len();
            let running = s.procs.iter().filter(|p| p.state == 'R').count();
            (
                (n as f32 / 512.0).clamp(0.0, 1.0),
                n.to_string(),
                "Processes".into(),
                format!("{running} running"),
            )
        }
        "swap-rate" => {
            let f = if s.mem.swap_total_kb == 0 {
                0.0
            } else {
                s.mem.swap_used_kb as f32 / s.mem.swap_total_kb as f32
            };
            (f, fmt::pct(f), "Swap".into(), fmt::kb(s.mem.swap_used_kb))
        }
        _ => match s.gpu {
            Some(g) => (g, fmt::pct(g), "GPU".into(), "render busy".into()),
            None => (
                s.disk.busy,
                fmt::pct(s.disk.busy),
                "I/O".into(),
                "no GPU counter on this machine".into(),
            ),
        },
    }
}

/// Under 80 columns: four horizontal ladders and the headline figures.
fn gauge_rail(app: &mut App, c: &mut Canvas, r: Rect) {
    let t = app.theme.clone();
    let readout = app.sys_readout();
    let body = panel::draw(c, &t, r, &Panel::new("SYS", &readout));
    if body.is_empty() {
        return;
    }
    let meters = app.cfg.meters.clone();
    for (n, m) in meters.iter().take(body.h as usize).enumerate() {
        let (v, text, label) = app.meter(*m);
        let y = body.y + n as i32;
        c.text(body.x, y, 5, label, t.text, t.panel, 0);
        let bar = Rect::new(body.x + 5, y, (body.w - 5 - 8).max(1), 1);
        let hue = match label {
            "CPU" => Hue::Solid(t.green),
            "Batt" => Hue::Solid(t.battery),
            "Temp" | "PSI" => Hue::Ramp(&t.heat),
            "GPU" => Hue::Ramp(&t.cool),
            _ => Hue::Solid(t.magenta),
        };
        ladder::horizontal(c, &t, bar, v, hue, false);
        c.text_right(body.right(), y, 8, &text, hue.top(), t.panel, 0);
    }
}

// ---- band 4: the Browser ---------------------------------------------

fn browser(app: &mut App, c: &mut Canvas, r: Rect) {
    let t = app.theme.clone();
    let readout = if app.filter.is_empty() {
        let m = app.marked.len();
        if m > 0 {
            format!("{} \u{b7} {m} marked", app.rows.len())
        } else {
            format!("{}", app.rows.len())
        }
    } else {
        format!("/{}", app.filter)
    };
    let focused = app.focus == Focus::Browser;
    let body = panel::draw(
        c,
        &t,
        r,
        &Panel::new("PROCESSES", &readout).focused(focused),
    );
    if body.h < 2 {
        return;
    }
    app.rows_visible = (body.h - 1).max(1) as usize;

    let wide = body.w >= 100;
    let cols: Vec<Col> = if wide {
        vec![
            Col::num("PID", 7),
            Col::fixed("USER", 9),
            Col::fixed("S", 1),
            Col::num("THR", 4),
            Col::num("CPU%", 6),
            Col::num("MEM%", 6),
            Col::num("RSS", 9),
            Col::num("TIME+", 9),
            Col::flex("COMMAND", 16),
        ]
    } else {
        vec![
            Col::num("PID", 7),
            Col::fixed("S", 1),
            Col::num("CPU%", 6),
            Col::num("RSS", 9),
            Col::flex("COMMAND", 12),
        ]
    };
    let lay = table::layout(&cols, body.w, 1);
    let sort_col = cols
        .iter()
        .position(|c| {
            c.head.starts_with(match app.cfg.sort {
                crate::config::Sort::Cpu => "CPU",
                crate::config::Sort::Mem => "MEM",
                crate::config::Sort::Pid => "PID",
                crate::config::Sort::Time => "TIME",
                crate::config::Sort::Name => "COMMAND",
                crate::config::Sort::User => "USER",
            })
        })
        .unwrap_or(0);
    table::header(c, &t, body, &cols, &lay, Some((sort_col, app.desc)));

    let total_mem = app.snap.mem.total_kb.max(1) as f32;
    for n in 0..app.rows_visible {
        let Some(row) = app.rows.get(app.scroll + n).cloned() else {
            break;
        };
        let Some(p) = app.snap.procs.get(row.idx).cloned() else {
            continue;
        };
        let y = body.y + 1 + n as i32;
        let selected = app.scroll + n == app.sel;
        let mut bg = t.panel;
        if selected {
            table::selection(c, &t, body, y, focused);
            bg = if focused { t.sel } else { t.sel.dark(0.35) };
        } else if app.marked.contains(&p.pid) {
            bg = t.sel.dark(0.6);
            c.fill_bg(Rect::new(body.x, y, body.w, 1), bg);
        }
        // A row whose state is remarkable is coloured entire.
        let row_fg = match p.state {
            'D' => t.orange,
            'Z' => t.red,
            'T' | 't' => t.yellow,
            _ if p.pid == app.machine.me => t.cyan,
            _ => t.text,
        };

        let mut put = |i: usize, s: &str, fg: Rgb| {
            if let Some(&(x, w)) = lay.get(i) {
                table::cell(c, body.x + x, y, w, s, fg, bg, cols[i].right, 0);
            }
        };
        let mut i = 0;
        put(i, &p.pid.to_string(), if selected { row_fg } else { t.dim });
        i += 1;
        if wide {
            put(i, &fmt::elide(&p.user, 9), t.dim);
            i += 1;
        }
        put(i, &p.state.to_string(), mark_of(app, &p, &t).1);
        i += 1;
        if wide {
            put(i, &p.threads.to_string(), t.dim);
            i += 1;
        }
        let cpu = fmt::pct(row.cpu);
        put(i, &cpu, if row.cpu > 0.10 { t.green } else { row_fg });
        i += 1;
        if wide {
            put(i, &fmt::pct(row.rss_kb as f32 / total_mem), t.dim);
            i += 1;
        }
        put(i, &fmt::kb(row.rss_kb), t.dim);
        i += 1;
        if wide {
            put(i, &fmt::cputime(p.cpu_time), t.dim);
            i += 1;
        }

        // COMMAND, with the tree guides, the disclosure arrow and the
        // roll-up count.
        let (cx, cw) = lay[i];
        let x0 = body.x + cx;
        let used = if app.cfg.tree && app.filter.is_empty() {
            table::guides(
                c,
                &t,
                x0,
                y,
                cw,
                &row.ancestors_last,
                row.is_last,
                if row.expandable {
                    Some((row.open, row.hidden))
                } else {
                    None
                },
                bg,
            )
        } else {
            0
        };
        // If the guides did not fit, the depth still has to show, or a deep
        // subtree reads as a flat list at a narrow width.
        let used = if app.cfg.tree && app.filter.is_empty() && used == 0 && row.depth > 0 {
            (row.depth as i32).min(cw / 3)
        } else {
            used
        };
        let left = cw - used;
        if left > 0 {
            let name_w = (p.name.chars().count() as i32).min(left);
            c.text(x0 + used, y, name_w, &p.name, row_fg, bg, BOLD);
            let x = x0 + used + name_w;
            // The arguments in dim, so a long command line stays scannable.
            if x + 2 < x0 + cw && !p.cmdline.is_empty() {
                let args = p.cmdline.split_once(' ').map(|(_, a)| a).unwrap_or("");
                if !args.is_empty() {
                    c.text(x + 1, y, x0 + cw - x - 1, args, t.dim, bg, DIM);
                }
            }
        }
    }
}

// ---- overlays ---------------------------------------------------------

/// Group G — the Inspector.
fn inspector(app: &mut App, c: &mut Canvas, r: Rect) {
    let t = app.theme.clone();
    let Some(pid) = app.selected_pid() else {
        return;
    };
    let Some(p) = app.proc_of(pid).cloned() else {
        return;
    };
    panel::shadow(c, r);
    let title = format!("{} {}", pid, fmt::elide(&p.name, 18));
    let tabs = ["Attributes", "Contents", "Network", "Tools", "Access"];
    let tab = app.inspector_tab.clamp(1, 5) as usize;
    let body = panel::draw(c, &t, r, &Panel::new("INSPECT", &title).focused(true));
    if body.h < 4 {
        return;
    }
    let (tabrow, rest) = body.cut_top(1);
    let mut x = tabrow.x;
    for (i, name) in tabs.iter().enumerate() {
        let on = i + 1 == tab;
        let label = format!("{} {}", i + 1, name);
        let w = label.chars().count() as i32 + 2;
        if x + w > tabrow.right() {
            break;
        }
        c.text(
            x,
            tabrow.y,
            w,
            &label,
            if on { t.cyan } else { t.dim },
            t.panel,
            if on { BOLD } else { DIM },
        );
        x += w;
    }
    let d = app.detail.clone();
    let mut y = rest.y;
    let mut line = |c: &mut Canvas, k: &str, v: &str, col: Rgb| {
        if y >= rest.bottom() {
            return;
        }
        c.text(rest.x, y, 14, k, t.dim, t.panel, DIM);
        c.text(rest.x + 14, y, rest.w - 14, v, col, t.panel, 0);
        y += 1;
    };
    match tab {
        1 => {
            line(
                c,
                "state",
                &format!("{} ({})", p.state, p.state_name()),
                t.text,
            );
            line(c, "parent", &format!("{}", p.ppid), t.text);
            line(c, "group", &format!("{}", p.pgid), t.text);
            line(c, "session", &format!("{}", p.sid), t.text);
            line(c, "user", &p.user, t.text);
            line(c, "threads", &p.threads.to_string(), t.text);
            line(
                c,
                "nice / prio",
                &format!("{} / {}", p.nice, p.prio),
                t.text,
            );
            line(c, "cpu", &fmt::pct(p.cpu), t.text);
            line(c, "cpu time", &fmt::cputime(p.cpu_time), t.text);
            line(
                c,
                "rss / vsz",
                &format!("{} / {}", fmt::kb(p.rss_kb), fmt::kb(p.vsz_kb)),
                t.text,
            );
            let age = (app.snap.uptime - p.start_s).max(0.0);
            line(c, "elapsed", &fmt::uptime(age), t.text);
            if let Supervision::OpenRc(n) = &p.supervision {
                line(c, "service", &format!("OpenRC: {n}"), t.net);
            }
        }
        2 => {
            if let Some(d) = &d {
                line(
                    c,
                    "command",
                    &fmt::elide(&d.cmdline, (rest.w - 14).max(8) as usize),
                    t.text,
                );
                line(
                    c,
                    "executable",
                    &fmt::elide(&d.exe, (rest.w - 14).max(8) as usize),
                    if d.exe_deleted { t.orange } else { t.text },
                );
                if d.exe_deleted {
                    line(c, "", "replaced on disk since it was mapped", t.orange);
                }
                line(
                    c,
                    "cwd",
                    &fmt::elide(&d.cwd, (rest.w - 14).max(8) as usize),
                    t.text,
                );
                line(
                    c,
                    "cgroup",
                    &fmt::elide(&d.cgroup, (rest.w - 14).max(8) as usize),
                    t.text,
                );
                line(c, "environ", &format!("{} variables", d.env_count), t.text);
                line(
                    c,
                    "open fds",
                    &format!(
                        "{} files \u{b7} {} sockets \u{b7} {} pipes \u{b7} {} other",
                        d.fds.0, d.fds.1, d.fds.2, d.fds.3
                    ),
                    t.text,
                );
            } else {
                line(c, "", "not read", t.dim);
            }
        }
        3 => network_section(app, c, rest, &d),
        4 => tools_section(app, c, rest, pid),
        _ => {
            if let Some(d) = &d {
                line(
                    c,
                    "uid",
                    &format!("real {} \u{b7} effective {}", d.uid_real, d.uid_eff),
                    t.text,
                );
                line(
                    c,
                    "gid",
                    &format!("real {} \u{b7} effective {}", d.gid_real, d.gid_eff),
                    t.text,
                );
                line(c, "capabilities", &format!("{:#x}", d.cap_eff), t.text);
                line(
                    c,
                    "seccomp",
                    ["disabled", "strict", "filter"][(d.seccomp as usize).min(2)],
                    t.text,
                );
                line(
                    c,
                    "switches",
                    &format!("{} vol \u{b7} {} invol", d.ctx_vol, d.ctx_invol),
                    t.text,
                );
                y += 1;
                c.text(
                    rest.x,
                    y,
                    rest.w,
                    "SIGNAL     DISPOSITION",
                    t.dim,
                    t.panel,
                    DIM,
                );
                y += 1;
                for s in copal_tm::machine::signals::TABLE {
                    if y >= rest.bottom() {
                        break;
                    }
                    let disp = d.sig.disposition(s.num);
                    let (word, col) = match disp {
                        Disposition::Trapped => ("trapped", t.orange),
                        Disposition::Ignored => ("ignored", t.dim),
                        Disposition::Blocked => ("blocked", t.red),
                        _ => ("default", t.green),
                    };
                    c.text(rest.x, y, 6, s.name, t.text, t.panel, 0);
                    c.text_right(rest.x + 9, y, 4, &s.num.to_string(), t.dim, t.panel, DIM);
                    c.text(rest.x + 11, y, 10, word, col, t.panel, 0);
                    let note = match disp {
                        Disposition::Trapped => "a handler is installed",
                        Disposition::Ignored => "sending it does nothing",
                        Disposition::Blocked => "delivered when unblocked",
                        Disposition::Uncatchable => "cannot be caught",
                        Disposition::Default => "",
                    };
                    c.text(rest.x + 22, y, rest.w - 22, note, t.dim, t.panel, DIM);
                    y += 1;
                }
            } else {
                line(c, "", "not read", t.dim);
            }
        }
    }
}

/// The Inspector's Network section: every socket the process holds, joined
/// from its own descriptors. See `machine::types::Socket` for the relation.
fn network_section(app: &App, c: &mut Canvas, r: Rect, d: &Option<copal_tm::machine::Detail>) {
    let t = app.theme.clone();
    let Some(d) = d else {
        c.text(r.x, r.y, r.w, "not read", t.dim, t.panel, DIM);
        return;
    };
    c.text(r.x, r.y, r.w, &d.socket_summary(), t.text, t.panel, 0);
    // Said once, here, because it is the question everyone asks next.
    c.text(
        r.x,
        r.y + 1,
        r.w,
        "per-process byte rates are not in /proc; these are addresses and queues",
        t.dim,
        t.panel,
        DIM,
    );
    let head = r.y + 3;
    // Wide enough for a table; narrower, one socket to a line.
    let wide = r.w >= 58;
    if wide {
        c.text(
            r.x,
            head - 1,
            r.w,
            "PROTO  LOCAL                REMOTE               STATE",
            t.dim,
            t.panel,
            DIM,
        );
    }
    for (n, s) in d.sockets_ranked().iter().enumerate() {
        let y = head + n as i32;
        if y >= r.bottom() {
            let more = d.sockets.len() - n;
            c.text(
                r.x,
                r.bottom() - 1,
                r.w,
                &format!("\u{2026} {more} more"),
                t.dim,
                t.panel,
                DIM,
            );
            break;
        }
        let col = match s.state {
            "LISTEN" => t.green,
            "ESTABLISHED" => t.text,
            _ => t.dim,
        };
        if wide {
            c.text(r.x, y, 6, s.proto, t.net, t.panel, 0);
            c.text(r.x + 7, y, 20, &fmt::elide(&s.local, 20), col, t.panel, 0);
            c.text(
                r.x + 28,
                y,
                20,
                &fmt::elide(&s.remote, 20),
                t.dim,
                t.panel,
                0,
            );
            c.text(r.x + 49, y, r.w - 49, s.state, col, t.panel, 0);
            if s.tx_queue > 0 && r.w > 62 {
                c.text_right(
                    r.right(),
                    y,
                    14,
                    &format!("tx {}", fmt::bytes(s.tx_queue)),
                    t.orange,
                    t.panel,
                    DIM,
                );
            }
        } else {
            // The state first, because in a narrow pane it is the column that
            // is worth a glance; then the address, elided from the middle.
            c.text(r.x, y, 12, s.state, col, t.panel, 0);
            let addr = if s.remote.is_empty() {
                format!("{} {}", s.proto, s.local)
            } else {
                format!("{} {} \u{2192} {}", s.proto, s.local, s.remote)
            };
            c.text(
                r.x + 12,
                y,
                r.w - 12,
                &fmt::elide(&addr, (r.w - 12).max(4) as usize),
                t.text,
                t.panel,
                0,
            );
        }
    }
}

/// The Inspector's Tools section: the halt plan, computed and shown before
/// anything is sent.
fn tools_section(app: &App, c: &mut Canvas, r: Rect, pid: i32) {
    let t = app.theme.clone();
    let plan = app.machine.plan(&app.snap, pid, app.cfg.grace);
    let mut y = r.y;
    for w in &plan.warnings {
        if y >= r.bottom() {
            return;
        }
        let col = match w.severity {
            Severity::Refuse => t.red,
            Severity::Caution => t.orange,
            Severity::Note => t.dim,
        };
        y += wrap(c, Rect::new(r.x, y, r.w, r.bottom() - y), &w.text, col);
    }
    if plan.refused {
        return;
    }
    y += 1;
    if let Some(a) = plan.recommended() {
        c.text(
            r.x,
            y,
            r.w,
            &format!("addressee: {}", a.describe()),
            t.text,
            t.panel,
            0,
        );
        y += 1;
        c.text(
            r.x + 2,
            y,
            r.w - 2,
            &a.command(&copal_tm::machine::signals::SIGTERM),
            t.cyan,
            t.panel,
            BOLD,
        );
        y += 2;
    }
    for rung in &plan.ladder {
        if y >= r.bottom() {
            return;
        }
        let (col, attr) = if rung.skipped {
            (t.dim, DIM)
        } else {
            (t.text, 0)
        };
        let mark = if rung.skipped { '\u{2717}' } else { '\u{2192}' };
        c.put(r.x, y, mark, col, t.panel, 0);
        c.text(r.x + 2, y, 6, rung.sig.name, col, t.panel, attr);
        if rung.grace > 0.0 && !rung.skipped {
            c.text(
                r.x + 9,
                y,
                10,
                &format!("wait {:.1}s", rung.grace),
                t.dim,
                t.panel,
                DIM,
            );
        }
        c.text(r.x + 20, y, r.w - 20, &rung.why, t.dim, t.panel, DIM);
        y += 1;
    }
}

fn wrap(c: &mut Canvas, r: Rect, text: &str, col: Rgb) -> i32 {
    let w = r.w.max(8) as usize;
    let mut y = 0;
    let mut line = String::new();
    for word in text.split_whitespace() {
        if line.chars().count() + word.chars().count() + 1 > w {
            if y >= r.h {
                return y;
            }
            c.text(
                r.x,
                r.y + y,
                r.w,
                &line,
                col,
                c.get(r.x, r.y + y).map(|x| x.bg).unwrap_or(col),
                0,
            );
            y += 1;
            line.clear();
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() && y < r.h {
        let bg = c.get(r.x, r.y + y).map(|x| x.bg).unwrap_or(col);
        c.text(r.x, r.y + y, r.w, &line, col, bg, 0);
        y += 1;
    }
    y
}

/// Group H — Services.
fn services(app: &mut App, c: &mut Canvas, r: Rect) {
    let t = app.theme.clone();
    panel::shadow(c, r);
    let Some(pid) = app.selected_pid() else {
        return;
    };
    let name = app.proc_of(pid).map(|p| p.name.clone()).unwrap_or_default();
    let title = format!("{pid} {name}");
    let body = panel::draw(c, &t, r, &Panel::new("SERVICES", &title).focused(true));
    let sig = app.proc_of(pid).and_then(|p| p.sig);
    for (i, s) in app.services().iter().enumerate() {
        let y = body.y + i as i32;
        if y >= body.bottom() {
            break;
        }
        let selected = i == app.service_sel;
        let bg = if selected { t.sel } else { t.panel };
        if selected {
            c.fill_bg(Rect::new(body.x, y, body.w, 1), bg);
            c.put(body.x, y, '\u{258e}', t.sel_edge, bg, BOLD);
        }
        let disp = sig.map(|m| m.disposition(s.num));
        let (word, col) = match disp {
            Some(Disposition::Trapped) => ("trapped", t.orange),
            Some(Disposition::Ignored) => ("ignored", t.dim),
            Some(Disposition::Blocked) => ("blocked", t.red),
            _ => ("", t.text),
        };
        let fg = if word == "ignored" { t.dim } else { t.text };
        c.text(
            body.x + 2,
            y,
            14,
            s.label,
            fg,
            bg,
            if selected { BOLD } else { 0 },
        );
        c.text(body.x + 16, y, 6, s.name, t.dim, bg, DIM);
        c.text(body.x + 23, y, 9, word, col, bg, 0);
        c.text(body.x + 33, y, body.w - 34, s.note, t.dim, bg, DIM);
    }
}

/// Group H — the halt plan and its confirmation.
fn halt(app: &mut App, c: &mut Canvas, r: Rect) {
    let t = app.theme.clone();
    panel::shadow(c, r);
    let Some(run) = &app.run else { return };
    let plan = run.plan.clone();
    let addressee = run.addressee.clone();
    let step = run.step;
    let title = format!("{} {}", plan.pid, plan.name);
    let body = panel::draw(c, &t, r, &Panel::new("HALT PLAN", &title).focused(true));
    let mut y = body.y;
    for w in &plan.warnings {
        if y >= body.bottom() - 4 {
            break;
        }
        let col = match w.severity {
            Severity::Refuse => t.red,
            Severity::Caution => t.orange,
            Severity::Note => t.dim,
        };
        y += wrap(
            c,
            Rect::new(body.x, y, body.w, body.bottom() - y),
            &w.text,
            col,
        );
    }
    if plan.refused {
        // Still name the right command, when there is one: knowing what would
        // have worked is most of what the plan is for.
        if let Some(a) = plan.recommended() {
            y += 1;
            c.text(body.x, y, body.w, "the right stop is:", t.dim, t.panel, DIM);
            c.text(
                body.x + 2,
                y + 1,
                body.w - 2,
                &a.command(&copal_tm::machine::signals::SIGTERM),
                t.cyan,
                t.panel,
                BOLD,
            );
        }
        c.text(
            body.x,
            body.bottom() - 1,
            body.w,
            "[Esc] close",
            t.dim,
            t.panel,
            DIM,
        );
        return;
    }
    y += 1;
    c.text(body.x, y, body.w, "addressee", t.dim, t.panel, DIM);
    y += 1;
    for (i, ch) in plan.choices.iter().enumerate() {
        if y >= body.bottom() - 3 {
            break;
        }
        let selected = i == app.choice_sel;
        let bg = if selected { t.sel } else { t.panel };
        if selected {
            c.fill_bg(Rect::new(body.x, y, body.w, 1), bg);
            c.put(body.x, y, '\u{258e}', t.sel_edge, bg, BOLD);
        }
        c.text(
            body.x + 2,
            y,
            body.w - 2,
            &ch.describe(),
            t.text,
            bg,
            if selected { BOLD } else { 0 },
        );
        y += 1;
    }
    y += 1;
    let sig = plan
        .ladder
        .get(step.min(plan.ladder.len() - 1))
        .map(|r| r.sig)
        .unwrap_or(copal_tm::machine::signals::SIGTERM);
    let cmd = addressee.command(&sig);
    c.text(body.x + 2, y, body.w - 2, &cmd, t.cyan, t.panel, BOLD);
    y += 2;
    for (i, rung) in plan.ladder.iter().enumerate() {
        if y >= body.bottom() - 1 {
            break;
        }
        let done = i < step;
        let (col, attr) = if rung.skipped {
            (t.dim, DIM)
        } else if done {
            (t.orange, 0)
        } else {
            (t.text, 0)
        };
        let mark = if rung.skipped {
            '\u{2717}'
        } else if done {
            '\u{2713}'
        } else {
            '\u{2192}'
        };
        c.put(body.x, y, mark, col, t.panel, 0);
        c.text(body.x + 2, y, 6, rung.sig.name, col, t.panel, attr);
        c.text(body.x + 9, y, body.w - 9, &rung.why, t.dim, t.panel, DIM);
        y += 1;
    }
    let hint = "[Enter] send  [e] step  [\u{2191}\u{2193}] addressee  [Esc] cancel";
    c.text(body.x, body.bottom() - 1, body.w, hint, t.dim, t.panel, DIM);
}

fn transcript(app: &mut App, c: &mut Canvas, r: Rect) {
    let t = app.theme.clone();
    panel::shadow(c, r);
    let n = format!("{}", app.log.len());
    let body = panel::draw(c, &t, r, &Panel::new("TRANSCRIPT", &n).focused(true));
    let entries = app.log.entries();
    let take = (body.h as usize).min(entries.len());
    for (i, e) in entries[entries.len() - take..].iter().enumerate() {
        let y = body.y + i as i32;
        let col = match e.kind {
            Kind::Action => t.text,
            Kind::Refusal => t.yellow,
            Kind::Signal => t.orange,
            Kind::Death => t.red,
            Kind::Note => t.dim,
        };
        c.text(body.x, y, 9, &e.stamp, t.dim, t.panel, DIM);
        c.text(body.x + 9, y, body.w - 9, &e.text, col, t.panel, 0);
    }
}

fn help(app: &mut App, c: &mut Canvas, r: Rect) {
    let t = app.theme.clone();
    panel::shadow(c, r);
    let body = panel::draw(c, &t, r, &Panel::new("KEYS", "?").focused(true));
    const KEYS: &[(&str, &str)] = &[
        ("F1 F2 F3", "dashboard / split / browser"),
        ("Tab", "move focus"),
        ("\u{2191}\u{2193} PgUp PgDn", "move the selection"),
        ("\u{2190} \u{2192}", "collapse / expand a subtree"),
        ("Enter, i", "Inspector; 1-5 choose its section"),
        ("Space", "mark a process"),
        ("t / s / r", "tree / sort / reverse"),
        ("/ , K", "filter; show kernel threads"),
        ("k", "Services - send one signal"),
        ("x", "Halt plan - the considered one"),
        ("T", "Transcript"),
        ("w", "cycle the SYS readout"),
        ("1 2 3", "toggle a history series"),
        ("p", "pause sampling"),
        ("+ -", "faster / slower tick"),
        ("q", "quit"),
    ];
    for (i, (k, v)) in KEYS.iter().enumerate() {
        let y = body.y + i as i32;
        if y >= body.bottom() {
            break;
        }
        c.text(body.x, y, 18, k, t.cyan, t.panel, 0);
        c.text(body.x + 19, y, body.w - 19, v, t.text, t.panel, 0);
    }
}

fn status_bar(app: &mut App, c: &mut Canvas, r: Rect) {
    let t = app.theme.clone();
    c.fill(r, ' ', t.text, t.panel);
    if app.overlay == Overlay::Filter {
        c.text(r.x + 1, r.y, 8, "filter /", t.cyan, t.panel, BOLD);
        let n = c.text(r.x + 9, r.y, r.w - 12, &app.filter, t.text, t.panel, 0);
        c.put(r.x + 9 + n, r.y, '\u{2588}', t.cyan, t.panel, 0);
        return;
    }
    let view = match app.cfg.view {
        View::Split => "split",
        View::Dashboard => "dashboard",
        View::Browser => "browser",
    };
    let left = format!(" {view} \u{b7} {} processes", app.snap.procs.len());
    c.text(r.x, r.y, r.w / 3, &left, t.cyan, t.panel, 0);

    if let Some(e) = app.log.last() {
        let col = match e.kind {
            Kind::Refusal => t.yellow,
            Kind::Signal => t.orange,
            Kind::Death => t.red,
            _ => t.dim,
        };
        let mid = Rect::new(r.x + r.w / 3, r.y, r.w / 3, 1);
        let text = fmt::elide(&e.text, mid.w.max(4) as usize);
        c.text(mid.x, mid.y, mid.w, &text, col, t.panel, DIM);
    }

    // The right end says, in order of how much it matters: whether the
    // readings are real, whether sampling is stopped, and how fast it ticks.
    let narrow = r.w < 60;
    let mut right = if narrow {
        format!("{:.1}s ", app.period)
    } else {
        format!("{:.1}s  ? keys ", app.period)
    };
    if app.paused {
        right = format!("\u{23f8} {right}");
    }
    if app.snap.simulated {
        right = format!("{}  {right}", if narrow { "SIM" } else { "SIMULATED" });
    }
    let col = if app.snap.simulated { t.orange } else { t.dim };
    c.text_right(r.right(), r.y, r.w / 2, &right, col, t.panel, DIM);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    /// A frame, as plain characters, row by row.
    fn render(w: i32, h: i32, f: impl FnOnce(&mut App)) -> Vec<String> {
        let cfg = Config {
            simulate: true,
            charset: copal_tm::tty::Charset::Full,
            ..Config::default()
        };
        let mut app = App::new(cfg, Vec::new());
        for _ in 0..90 {
            app.tick();
        }
        f(&mut app);
        let mut c = Canvas::new(w, h);
        frame(&mut app, &mut c);
        (0..h)
            .map(|y| {
                (0..w)
                    .map(|x| c.get(x, y).map(|c| c.ch).unwrap_or(' '))
                    .collect()
            })
            .collect()
    }

    fn has(rows: &[String], needle: &str) -> bool {
        rows.iter().any(|r| r.contains(needle))
    }

    #[test]
    fn every_breakpoint_renders_without_overlap_or_panic() {
        for (w, h) in [
            (80, 24),
            (100, 30),
            (132, 40),
            (160, 50),
            (200, 60),
            (60, 20),
            (40, 12),
        ] {
            let rows = render(w, h, |_| {});
            assert_eq!(rows.len(), h as usize);
            for r in &rows {
                assert_eq!(r.chars().count(), w as usize, "{w}x{h} row is not {w} wide");
            }
            // The status bar is the last row and is always there.
            assert!(rows[h as usize - 1].contains("SIM"), "{w}x{h}");
        }
    }

    #[test]
    fn the_full_dashboard_shows_every_group() {
        let rows = render(140, 44, |_| {});
        assert!(has(&rows, "S Y S"), "group A");
        assert!(has(&rows, "C P U   H I S T O R Y"), "group B");
        assert!(has(&rows, "Utilization"), "group B's legend");
        assert!(has(&rows, "logical processor"), "group B's footer");
        assert!(has(&rows, "A V E R A G E"), "group C");
        assert!(has(&rows, "Memory Utilization"), "group D");
        assert!(has(&rows, "Available"), "group D's captions");
        assert!(has(&rows, "Network"), "group E");
        assert!(has(&rows, "System Pressure"), "group E");
        assert!(has(&rows, "P R O C E S S E S"), "group F");
    }

    #[test]
    fn under_eighty_columns_the_dashboard_becomes_a_gauge_rail() {
        let rows = render(72, 24, |_| {});
        assert!(has(&rows, "S Y S"));
        assert!(
            !has(&rows, "C P U   H I S T O R Y"),
            "the strip chart is dropped"
        );
        assert!(
            has(&rows, "P R O C E S S E S"),
            "the browser keeps the room"
        );
    }

    #[test]
    fn the_browser_nests_and_marks_a_collapsed_subtree() {
        let rows = render(140, 60, |a| {
            let pid = a
                .snap
                .procs
                .iter()
                .find(|p| p.name == "firefox")
                .unwrap()
                .pid;
            a.collapsed.insert(pid);
            a.rebuild_rows();
            // Put the cursor on it, which is what scrolls it into view.
            a.sel = a.rows.iter().position(|r| r.pid == pid).unwrap();
        });
        assert!(has(&rows, "\u{251c}\u{2500}"), "tree guides");
        assert!(
            has(&rows, "\u{25b8}2"),
            "the roll-up count of a collapsed parent"
        );
    }

    #[test]
    fn the_inspectors_network_section_lists_ports() {
        let rows = render(140, 44, |a| {
            let pid = a.snap.procs.iter().find(|p| p.name == "nginx").unwrap().pid;
            a.sel = a.rows.iter().position(|r| r.pid == pid).unwrap();
            a.load_detail(pid);
            a.inspector_tab = 3;
            a.overlay = Overlay::Inspector;
        });
        assert!(has(&rows, "listening"), "the summary");
        assert!(has(&rows, "0.0.0.0:443"), "a listening port");
        assert!(has(&rows, "LISTEN"));
        assert!(
            has(&rows, "per-process byte rates are not in /proc"),
            "the caveat, said once"
        );
    }

    #[test]
    fn the_inspectors_access_section_decodes_the_signal_masks() {
        let rows = render(140, 60, |a| {
            let pid = a.snap.procs.iter().find(|p| p.name == "node").unwrap().pid;
            a.sel = a.rows.iter().position(|r| r.pid == pid).unwrap();
            a.load_detail(pid);
            a.inspector_tab = 5;
            a.overlay = Overlay::Inspector;
        });
        assert!(has(&rows, "DISPOSITION"));
        assert!(
            has(&rows, "trapped"),
            "the simulated node has a TERM handler"
        );
        assert!(
            has(&rows, "cannot be caught"),
            "KILL is named for what it is"
        );
    }

    #[test]
    fn the_halt_plan_shows_the_command_before_it_is_sent() {
        let rows = render(140, 44, |a| {
            let pid = a.snap.procs.iter().find(|p| p.name == "node").unwrap().pid;
            a.sel = a.rows.iter().position(|r| r.pid == pid).unwrap();
            a.open_halt();
        });
        assert!(has(&rows, "H A L T   P L A N"));
        assert!(
            has(&rows, "kill -TERM -4820"),
            "the group, and the exact command"
        );
        assert!(has(&rows, "process group 4820"));
        assert!(has(&rows, "[Enter] send"));
    }

    #[test]
    fn a_supervised_service_is_offered_its_service_manager() {
        let rows = render(140, 44, |a| {
            let pid = a.snap.procs.iter().find(|p| p.name == "nginx").unwrap().pid;
            a.sel = a.rows.iter().position(|r| r.pid == pid).unwrap();
            a.open_halt();
        });
        assert!(has(&rows, "rc-service nginx stop"));
    }

    #[test]
    fn the_services_menu_marks_what_would_do_nothing() {
        let rows = render(140, 44, |a| {
            let pid = a
                .snap
                .procs
                .iter()
                .find(|p| p.name == "stubborn")
                .unwrap()
                .pid;
            a.sel = a.rows.iter().position(|r| r.pid == pid).unwrap();
            a.service_sel = 0;
            a.overlay = Overlay::Services;
        });
        assert!(has(&rows, "Terminate"));
        assert!(
            has(&rows, "ignored"),
            "the simulated `stubborn` ignores TERM"
        );
    }

    #[test]
    fn the_ascii_fallback_draws_the_same_interface() {
        let cfg = Config {
            simulate: true,
            charset: copal_tm::tty::Charset::Ascii,
            ..Config::default()
        };
        let mut app = App::new(cfg, Vec::new());
        for _ in 0..30 {
            app.tick();
        }
        let mut c = Canvas::new(132, 40);
        frame(&mut app, &mut c);
        let rows: Vec<String> = (0..40)
            .map(|y| (0..132).map(|x| c.get(x, y).unwrap().ch).collect())
            .collect();
        assert!(has(&rows, "S Y S"));
        assert!(has(&rows, "+---"), "square corners");
        // The plot falls all the way back to `*`, and no braille survives.
        assert!(
            rows.iter()
                .all(|r| !r.chars().any(|ch| ('\u{2800}'..='\u{28ff}').contains(&ch))),
            "no braille in the ASCII fallback"
        );
        assert!(has(&rows, "*"), "the ASCII plot glyph");
    }
}
