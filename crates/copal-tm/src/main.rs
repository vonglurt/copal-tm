// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Paul Richeson
//! copal-tm — the Copal task manager.
//!
//! An instrument panel and a process browser, in one terminal window, on a
//! machine that cannot fetch a crate. The design it follows is written down in
//! `docs/design-lab-report.md`, group by group and cell by cell.

mod app;
mod config;
mod draw;
mod transcript;

use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use copal_tm_tty::{input_channel, Canvas, Key, Term};

use crate::app::App;
use crate::config::{Config, USAGE};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{USAGE}");
        return;
    }
    if args.iter().any(|a| a == "--version") {
        println!("copal-tm {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let (cfg, notes) = Config::load(&args);
    if args.iter().any(|a| a == "--dump-config") {
        print!("{}", cfg.dump());
        return;
    }
    // `--frame WxH` renders one frame to stdout and exits: how a build is
    // looked at without a terminal session, how the layout is checked at every
    // breakpoint, and what `make shot` uses.
    if let Some(spec) = args.iter().find_map(|a| a.strip_prefix("--frame")) {
        let spec = spec.trim_start_matches('=').trim();
        let spec = if spec.is_empty() {
            args.iter()
                .skip_while(|a| !a.starts_with("--frame"))
                .nth(1)
                .cloned()
                .unwrap_or_default()
        } else {
            spec.to_string()
        };
        let (w, h) = spec
            .split_once(['x', 'X'])
            .and_then(|(a, b)| Some((a.trim().parse().ok()?, b.trim().parse().ok()?)))
            .unwrap_or((132, 40));
        print!("{}", still(cfg, notes, w, h));
        return;
    }
    if let Err(e) = run(cfg, notes) {
        copal_tm_tty::restore();
        eprintln!("copal-tm: {e}");
        std::process::exit(1);
    }
}

/// One frame, as the escape sequence that would draw it. Always against the
/// simulated probe on a machine with no `/proc`, and always after enough ticks
/// to give the histories something to show.
fn still(mut cfg: Config, notes: Vec<String>, w: i32, h: i32) -> String {
    let truecolor = cfg.truecolor;
    cfg.mouse = false;
    let mut app = App::new(cfg, notes);
    for _ in 0..240 {
        app.tick();
    }
    let mut canvas = Canvas::new(w, h);
    canvas.truecolor = truecolor;
    let mut out = String::new();
    draw::frame(&mut app, &mut canvas);
    canvas.flush(&mut out);
    out.push_str("\x1b[0m\n");
    out
}

fn run(cfg: Config, notes: Vec<String>) -> std::io::Result<()> {
    let truecolor = cfg.truecolor;
    let mouse = cfg.mouse;
    let mut app = App::new(cfg, notes);

    let term = Term::enter(mouse)?;
    let keys = input_channel();
    let (w, h) = Term::size_slow();
    let mut canvas = Canvas::new(w, h);
    canvas.truecolor = truecolor;

    app.tick();
    let mut out = String::with_capacity(1 << 16);
    let mut next = app.probe.now();
    let mut size_check = 0.0f64;

    while !app.quit {
        let now = app.probe.now();
        if now >= next {
            app.tick();
            next = now + app.period;
        }
        // Follow the window. A terminal that answers `CSI 18 t` tells us on
        // the stream we are already reading; one that does not is asked the
        // slow way, and only every other second.
        if term.reports_size {
            term.ask_size();
        } else if now - size_check > 2.0 {
            size_check = now;
            let (w, h) = Term::size_slow();
            if w != canvas.w || h != canvas.h {
                canvas.resize(w, h);
            }
        }

        out.clear();
        draw::frame(&mut app, &mut canvas);
        canvas.flush(&mut out);
        term.write(&out);

        let wait = (next - app.probe.now()).clamp(0.02, 0.25);
        match keys.recv_timeout(Duration::from_secs_f64(wait)) {
            Ok(k) => {
                handle(&mut app, &mut canvas, k);
                // Drain whatever else arrived in the same burst, so holding a
                // key scrolls rather than queueing frames.
                while let Ok(k) = keys.try_recv() {
                    handle(&mut app, &mut canvas, k);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(())
}

fn handle(app: &mut App, canvas: &mut Canvas, k: Key) {
    match k {
        Key::Size(w, h) => {
            if w != canvas.w || h != canvas.h {
                canvas.resize(w, h);
            }
        }
        // Wheel up and down, which is how a long process list is scrolled.
        Key::Mouse(64, _, _) => app.move_sel(-3),
        Key::Mouse(65, _, _) => app.move_sel(3),
        Key::Mouse(..) => {}
        other => app.key(other),
    }
}
