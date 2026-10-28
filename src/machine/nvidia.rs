// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Paul Richeson
//! NVIDIA's cards, which tell `/sys` nothing.
//!
//! The proprietary driver has no `gpu_busy_percent`; the load, the memory and
//! the processes on the card come from NVML, and NVML from a C library this
//! crate will not link (no external crates, no FFI to a library that may not
//! be there). `nvidia-smi` is NVML's own front end and ships with every
//! driver, so it is asked instead, but not once a tick: starting it costs
//! tens of milliseconds and initialises the driver each time. Two copies are
//! started once and left running, each printing a line a card or a process
//! every two seconds:
//!
//! - `nvidia-smi dmon -s pucm -o T`: each card's busy share (`sm`), memory
//!   in use (`fb`), power and temperature;
//! - `nvidia-smi pmon -s um -o T`: each process on a card, its share and its
//!   memory, `C` for compute (CUDA) or `G` for graphics.
//!
//! A thread reads each and keeps the latest. Columns are found by the names
//! in the header line, not by position, because drivers add columns (580
//! has `jpg`, `ofa` and `ccpm`; older ones do not). When copal-tm exits the
//! pipes close and each `nvidia-smi` ends at its next line (SIGPIPE), even
//! if copal-tm was killed and `Drop` never ran.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::machine::types::{GpuCard, GpuUse};

/// Seconds between `nvidia-smi`'s lines.
const EVERY: u32 = 2;

#[derive(Default)]
struct Latest {
    /// Per card index: (busy 0..1, memory used MB, watts, degrees).
    cards: HashMap<u32, (Option<f32>, Option<f32>, Option<f32>, Option<f32>)>,
    cards_at: Option<Instant>,
    procs: HashMap<i32, GpuUse>,
    procs_at: Option<Instant>,
}

pub struct Nvidia {
    latest: Arc<Mutex<Latest>>,
    children: Vec<Child>,
    name: String,
    total_mb: Option<f32>,
}

impl Nvidia {
    /// `None` on a machine without the NVIDIA driver or `nvidia-smi`.
    pub fn start() -> Option<Nvidia> {
        if !std::path::Path::new("/proc/driver/nvidia").exists() {
            return None;
        }
        let q = Command::new("nvidia-smi")
            .args([
                "--query-gpu=name,memory.total",
                "--format=csv,noheader,nounits",
            ])
            .stderr(Stdio::null())
            .output()
            .ok()?;
        if !q.status.success() {
            return None;
        }
        let first = String::from_utf8_lossy(&q.stdout);
        let first = first.lines().next().unwrap_or("");
        let mut f = first.split(',').map(str::trim);
        let name = f
            .next()
            .unwrap_or("NVIDIA")
            .trim_start_matches("NVIDIA ")
            .trim_start_matches("GeForce ")
            .to_string();
        let total_mb = f.next().and_then(|v| v.parse().ok());

        let latest = Arc::new(Mutex::new(Latest::default()));
        let mut children = Vec::new();
        let d = EVERY.to_string();
        for (args, card) in [
            (vec!["dmon", "-s", "pucm", "-d", &d, "-o", "T"], true),
            (vec!["pmon", "-s", "um", "-d", &d, "-o", "T"], false),
        ] {
            let Ok(mut child) = Command::new("nvidia-smi")
                .args(&args)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
            else {
                continue;
            };
            let Some(out) = child.stdout.take() else {
                continue;
            };
            let l = Arc::clone(&latest);
            let spawned = std::thread::Builder::new()
                .name(if card { "nvidia-dmon" } else { "nvidia-pmon" }.into())
                .spawn(move || {
                    if card {
                        read_cards(BufReader::new(out), &l)
                    } else {
                        read_procs(BufReader::new(out), &l)
                    }
                });
            if spawned.is_ok() {
                children.push(child);
            } else {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        if children.is_empty() {
            return None;
        }
        Some(Nvidia {
            latest,
            children,
            name,
            total_mb,
        })
    }

    /// The busiest card's share, and the card as a whole, if read lately.
    pub fn card(&self) -> Option<(f32, GpuCard)> {
        let l = self.latest.lock().ok()?;
        if !fresh(l.cards_at) {
            return None;
        }
        let mut busy: Option<f32> = None;
        let mut card = GpuCard {
            name: self.name.clone(),
            total_mb: self.total_mb,
            ..Default::default()
        };
        for (b, fb, w, c) in l.cards.values() {
            if let Some(b) = b {
                busy = Some(busy.map_or(*b, |x| x.max(*b)));
            }
            card.used_mb = add(card.used_mb, *fb);
            card.watts = add(card.watts, *w);
            card.celsius = match (card.celsius, c) {
                (Some(a), Some(b)) => Some(a.max(*b)),
                (a, b) => a.or(*b),
            };
        }
        card.cards = l.cards.len();
        Some((busy?, card))
    }

    /// The processes on a card, by pid, if read lately.
    pub fn procs(&self) -> HashMap<i32, GpuUse> {
        match self.latest.lock() {
            Ok(l) if fresh(l.procs_at) => l.procs.clone(),
            _ => HashMap::new(),
        }
    }
}

impl Drop for Nvidia {
    fn drop(&mut self) {
        for c in self.children.iter_mut() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

/// Three of its own periods, the rule every instrument here keeps.
fn fresh(at: Option<Instant>) -> bool {
    at.is_some_and(|t| t.elapsed().as_secs_f32() <= (EVERY * 3) as f32)
}

fn add(a: Option<f32>, b: Option<f32>) -> Option<f32> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a + b),
        (a, b) => a.or(b),
    }
}

/// The column names from `nvidia-smi`'s first header line (`#Time gpu pwr
/// ...`), lower case, the `#` dropped.
fn header(line: &str) -> Option<Vec<String>> {
    let rest = line.strip_prefix('#')?;
    let cols: Vec<String> = rest
        .split_whitespace()
        .map(|s| s.to_ascii_lowercase())
        .collect();
    // The second header line holds the units (`#HH:MM:SS Idx W ...`).
    if cols.first().map(String::as_str) == Some("time") {
        Some(cols)
    } else {
        None
    }
}

/// A number, or `None` for `-`.
fn num(fields: &[&str], cols: &[String], name: &str) -> Option<f32> {
    let i = cols.iter().position(|c| c == name)?;
    fields.get(i)?.parse().ok()
}

fn read_cards(r: impl BufRead, latest: &Mutex<Latest>) {
    let mut cols: Vec<String> = Vec::new();
    for line in r.lines() {
        let Ok(line) = line else { return };
        if line.starts_with('#') {
            if let Some(c) = header(&line) {
                cols = c;
            }
            continue;
        }
        let f: Vec<&str> = line.split_whitespace().collect();
        if cols.is_empty() || f.len() < cols.len() {
            continue;
        }
        let Some(idx) = num(&f, &cols, "gpu") else {
            continue;
        };
        let reading = (
            num(&f, &cols, "sm").map(|v| (v / 100.0).clamp(0.0, 1.0)),
            num(&f, &cols, "fb"),
            num(&f, &cols, "pwr"),
            num(&f, &cols, "gtemp"),
        );
        if let Ok(mut l) = latest.lock() {
            l.cards.insert(idx as u32, reading);
            l.cards_at = Some(Instant::now());
        }
    }
}

fn read_procs(r: impl BufRead, latest: &Mutex<Latest>) {
    let mut cols: Vec<String> = Vec::new();
    // One sample is every line with the same time; it is published when
    // the next time begins.
    let mut time = String::new();
    let mut group: HashMap<i32, GpuUse> = HashMap::new();
    for line in r.lines() {
        let Ok(line) = line else { return };
        if line.starts_with('#') {
            if let Some(c) = header(&line) {
                cols = c;
            }
            continue;
        }
        let f: Vec<&str> = line.split_whitespace().collect();
        if cols.is_empty() || f.is_empty() {
            continue;
        }
        if f[0] != time {
            if let Ok(mut l) = latest.lock() {
                l.procs = std::mem::take(&mut group);
                l.procs_at = Some(Instant::now());
            }
            time = f[0].to_string();
        }
        // `-` in the pid column: no process on that card.
        let Some(pid) = num(&f, &cols, "pid") else {
            continue;
        };
        let kind = cols
            .iter()
            .position(|c| c == "type")
            .and_then(|i| f.get(i))
            .and_then(|s| s.chars().next())
            .unwrap_or('?');
        let u = group.entry(pid as i32).or_default();
        u.compute |= kind == 'C';
        u.busy = add(u.busy, num(&f, &cols, "sm").map(|v| v / 100.0));
        u.mem_mb = add(u.mem_mb, num(&f, &cols, "fb"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DMON: &str = "\
#Time         gpu    pwr  gtemp  mtemp     sm    mem    enc    dec    jpg    ofa   mclk   pclk     fb   bar1   ccpm
#HH:MM:SS     Idx      W      C      C      %      %      %      %      %      %    MHz    MHz     MB     MB     MB
 01:44:16       0     63     35      -     87      0      0      0      -      -    810    139   1200      5      0
";

    // Two samples: a CUDA process, then nothing (`-`), then a third time
    // so the second is published.
    const PMON: &str = "\
#Time         gpu         pid   type     sm    mem    enc    dec    jpg    ofa     fb   ccpm    command
#HH:MM:SS     Idx           #    C/G      %      %      %      %      %      %     MB     MB    name
 01:44:21       0       4242     C     55      3      -      -      -      -    900      0    treadcore-bc
 01:44:23       0          -     -      -      -      -      -      -      -      -      -    -
 01:44:25       0          -     -      -      -      -      -      -      -      -      -    -
";

    /// calber's own lines (driver 580): the busy share, the memory and the
    /// watts, found by name.
    #[test]
    fn a_dmon_line_is_read_by_its_column_names() {
        let l = Mutex::new(Latest::default());
        read_cards(DMON.as_bytes(), &l);
        let l = l.lock().unwrap();
        let (b, fb, w, c) = l.cards[&0];
        assert_eq!(b, Some(0.87));
        assert_eq!(fb, Some(1200.0));
        assert_eq!(w, Some(63.0));
        assert_eq!(c, Some(35.0));
    }

    #[test]
    fn a_pmon_sample_is_every_line_of_one_time() {
        let l = Mutex::new(Latest::default());
        let mut lines = PMON.lines();
        let two: String = lines.by_ref().take(4).map(|s| format!("{s}\n")).collect();
        read_procs(two.as_bytes(), &l);
        {
            let l = l.lock().unwrap();
            let u = l.procs[&4242];
            assert!(u.compute);
            assert_eq!(u.busy, Some(0.55));
            assert_eq!(u.mem_mb, Some(900.0));
        }
        let l2 = Mutex::new(Latest::default());
        read_procs(PMON.as_bytes(), &l2);
        assert!(l2.lock().unwrap().procs.is_empty(), "the process ended");
    }
}
