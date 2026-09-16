// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Paul Richeson
//! The Transcript — Smalltalk's, the system's running log, and Section VI-I.
//!
//! A program whose job is to end other programs must keep a record of what it
//! ended. Every signal sent with its exact equivalent command line, every
//! refusal and why, every escalation, every start-up fallback.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// Something the user did.
    Action,
    /// Something the program declined to do, and why.
    Refusal,
    /// A signal actually sent.
    Signal,
    /// A process that died as a result.
    Death,
    /// A setting changed, or a start-up fallback taken.
    Note,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub stamp: String,
    pub kind: Kind,
    pub text: String,
}

pub struct Transcript {
    entries: Vec<Entry>,
    /// Seconds east of UTC, found once at start-up by asking `date`. One
    /// fork, at start-up, never on the tick.
    offset: i64,
    cap: usize,
}

impl Default for Transcript {
    fn default() -> Self {
        Self::new()
    }
}

impl Transcript {
    pub fn new() -> Transcript {
        Transcript {
            entries: Vec::new(),
            offset: local_offset(),
            cap: 2000,
        }
    }

    pub fn log(&mut self, kind: Kind, text: impl Into<String>) {
        let stamp = self.stamp();
        self.entries.push(Entry {
            stamp,
            kind,
            text: text.into(),
        });
        if self.entries.len() > self.cap {
            let drop = self.entries.len() - self.cap;
            self.entries.drain(..drop);
        }
    }

    pub fn action(&mut self, t: impl Into<String>) {
        self.log(Kind::Action, t)
    }
    pub fn refusal(&mut self, t: impl Into<String>) {
        self.log(Kind::Refusal, t)
    }
    pub fn signal(&mut self, t: impl Into<String>) {
        self.log(Kind::Signal, t)
    }
    pub fn death(&mut self, t: impl Into<String>) {
        self.log(Kind::Death, t)
    }
    pub fn note(&mut self, t: impl Into<String>) {
        self.log(Kind::Note, t)
    }

    pub fn last(&self) -> Option<&Entry> {
        self.entries.last()
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    fn stamp(&self) -> String {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
            + self.offset;
        let day = secs.rem_euclid(86400);
        format!("{:02}:{:02}:{:02}", day / 3600, (day % 3600) / 60, day % 60)
    }
}

/// `date +%z` gives `+0100`; one fork at start-up beats carrying a timezone
/// database, and beats logging in UTC on a machine whose user is not.
fn local_offset() -> i64 {
    let Ok(out) = std::process::Command::new("date").arg("+%z").output() else {
        return 0;
    };
    let s = String::from_utf8_lossy(&out.stdout);
    let s = s.trim();
    if s.len() < 5 {
        return 0;
    }
    let sign = if s.starts_with('-') { -1 } else { 1 };
    let h: i64 = s[1..3].parse().unwrap_or(0);
    let m: i64 = s[3..5].parse().unwrap_or(0);
    sign * (h * 3600 + m * 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_are_stamped_and_kept_in_order() {
        let mut t = Transcript::new();
        t.note("started");
        t.signal("kill -TERM -4820");
        assert_eq!(t.len(), 2);
        assert_eq!(t.last().unwrap().kind, Kind::Signal);
        assert_eq!(t.entries()[0].stamp.len(), 8);
    }

    #[test]
    fn the_log_does_not_grow_without_bound() {
        let mut t = Transcript::new();
        t.cap = 10;
        for i in 0..50 {
            t.action(format!("{i}"));
        }
        assert_eq!(t.len(), 10);
        assert_eq!(t.last().unwrap().text, "49");
        assert_eq!(t.entries()[0].text, "40");
    }
}
