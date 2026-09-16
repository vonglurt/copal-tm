// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Paul Richeson
//! Terminal plumbing, with no dependencies.
//!
//! The same shape ascitty uses, for the same reason: raw mode is two `stty`
//! flags, the alternate screen is two escape sequences, and reading keys is a
//! thread and a channel.  Writing that down once is cheaper than carrying a
//! terminal library onto a node that cannot fetch one.
//!
//! One thing here is not ascitty's.  A task manager is a program people leave
//! running, so it has to follow a window that is resized.  Two ways are
//! offered and the good one is tried first: `CSI 18 t`, which a terminal
//! answers on the stream we are already reading, and `stty size`, which is a
//! fork and an exec.  The second is used only when the first goes unanswered,
//! and then only when the frame looks wrong.

use std::io::{Read, Write};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};

use crate::tty::keys::{Key, KeyParser};

/// Set once the terminal is in raw mode, so the restore runs exactly once
/// however the program ends.
static RAW: AtomicBool = AtomicBool::new(false);

pub struct Term {
    /// Whether this terminal answers `CSI 18 t` with its size.  When it does,
    /// following a resize costs six bytes; when it does not, it costs a fork.
    pub reports_size: bool,
    /// Whether this terminal was asked for, and agreed to, mouse reporting.
    pub mouse: bool,
}

impl Term {
    /// Enter raw mode and the alternate screen, and hide the cursor.
    pub fn enter(mouse: bool) -> std::io::Result<Term> {
        stty(&["raw", "-echo"])?;
        RAW.store(true, Ordering::SeqCst);
        let mut out = std::io::stdout();
        // 1049: alternate screen.  25l: hide the cursor.  2J: clear it.
        out.write_all(b"\x1b[?1049h\x1b[?25l\x1b[2J")?;
        if mouse {
            // 1000: press and release.  1006: SGR encoding, so a click past
            // column 95 still has coordinates that fit.
            out.write_all(b"\x1b[?1000h\x1b[?1006h")?;
        }
        out.flush()?;

        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore();
            prev(info);
        }));

        // Before the reader thread exists: the handshake reads the terminal's
        // own replies off stdin, and an unread reply is a fistful of
        // keystrokes as far as the decoder is concerned.
        let reports_size = handshake();
        Ok(Term {
            reports_size,
            mouse,
        })
    }

    /// Ask the terminal how big it is.  The answer arrives on the key stream
    /// as [`Key::Size`], which is where a terminal puts everything it says.
    pub fn ask_size(&self) {
        if self.reports_size {
            let mut out = std::io::stdout();
            let _ = out.write_all(b"\x1b[18t");
            let _ = out.flush();
        }
    }

    /// The size in cells, by forking `stty`.  The fallback path.
    pub fn size_slow() -> (i32, i32) {
        if let Ok(o) = Command::new("stty")
            .arg("size")
            .stdin(std::process::Stdio::inherit())
            .output()
        {
            let s = String::from_utf8_lossy(&o.stdout);
            let mut it = s.split_whitespace();
            if let (Some(r), Some(c)) = (it.next(), it.next()) {
                if let (Ok(r), Ok(c)) = (r.parse::<i32>(), c.parse::<i32>()) {
                    if r > 2 && c > 2 {
                        return (c, r);
                    }
                }
            }
        }
        (100, 34)
    }

    pub fn write(&self, s: &str) {
        let mut out = std::io::stdout();
        let _ = out.write_all(s.as_bytes());
        let _ = out.flush();
    }
}

impl Drop for Term {
    fn drop(&mut self) {
        restore();
    }
}

/// Undo everything [`Term::enter`] did.  Safe to call more than once, and it
/// runs from the panic hook, because a task manager that dies with echo off
/// leaves a shell nobody can type into.
pub fn restore() {
    if !RAW.swap(false, Ordering::SeqCst) {
        return;
    }
    let mut out = std::io::stdout();
    let _ = out.write_all(b"\x1b[?1006l\x1b[?1000l\x1b[0m\x1b[?25h\x1b[?1049l");
    let _ = out.flush();
    let _ = stty(&["sane"]);
}

fn stty(args: &[&str]) -> std::io::Result<()> {
    Command::new("stty")
        .args(args)
        .stdin(std::process::Stdio::inherit())
        .status()
        .map(|_| ())
}

/// Ask two questions in one round trip: how big are you, and - as the fence
/// every terminal ever made answers - what are you.  A terminal that answered
/// the second without answering the first does not report its size, which
/// turns "wait and see" into a definite answer in one round trip.
fn handshake() -> bool {
    let mut out = std::io::stdout();
    if out.write_all(b"\x1b[18t\x1b[c").is_err() || out.flush().is_err() {
        return false;
    }
    let _ = stty(&["-icanon", "min", "0", "time", "1"]);
    let mut reply = Vec::new();
    let mut chunk = [0u8; 64];
    let mut stdin = std::io::stdin();
    for _ in 0..5 {
        match stdin.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => reply.extend_from_slice(&chunk[..n]),
        }
        if reply.contains(&b'c') {
            break;
        }
    }
    let _ = stty(&["raw", "-echo"]);
    crate::tty::keys::size_reply(&reply).is_some()
}

/// Start the reader thread.  Every key, size report and mouse click the
/// terminal sends arrives on this channel already decoded.
pub fn input_channel() -> Receiver<Key> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut stdin = std::io::stdin();
        let mut parser = KeyParser::new();
        let mut buf = [0u8; 1024];
        loop {
            match stdin.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    for key in parser.feed(&buf[..n]) {
                        if tx.send(key).is_err() {
                            return;
                        }
                    }
                }
            }
        }
    });
    rx
}
