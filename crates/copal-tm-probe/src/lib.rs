// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Paul Richeson
//! Every reading `copal-tm` takes, and the reasoning about what may be done
//! with a process once it has been read.
//!
//! Two back ends behind one `Probe`: `linux`, which reads `/proc` and `/sys`
//! on the schedule of the design report's Section VII, and `sim`, which makes
//! plausible readings on a machine that has neither.  Which one is compiled
//! in is decided by the target, and the choice is visible in every snapshot -
//! `Snapshot::simulated` is not something the interface is allowed to forget.
//!
//! Nothing here draws, and the one function that changes the state of the
//! machine - `signals::send` - is called only by the binary, only after a
//! confirmation, and only with a plan from `halt` in hand.

pub mod fmt;
pub mod halt;
pub mod ring;
pub mod signals;
pub mod types;

#[cfg(target_os = "linux")]
mod linux;
mod sim;

#[cfg(target_os = "linux")]
use linux as native;

pub use halt::{Addressee, Plan, Severity, Warning};
pub use ring::Ring;
pub use signals::{Disposition, SigMask, Signal};
pub use types::*;

/// Which back end a `Probe` is using.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    /// `/proc` and `/sys` on this machine.
    Native,
    /// Made up, and saying so.
    Simulated,
}

// One of these exists per program: built once in Probe::new and held for the
// run, so the 264 bytes between the variants are never multiplied by anything.
// Boxing the native back end to even them out would put a pointer chase in
// tick(), which samples every core and every visible process on every tick.
// That is the wrong trade for an allocation made once at startup.
#[allow(clippy::large_enum_variant)]
enum Inner {
    #[cfg(target_os = "linux")]
    Native(native::Backend),
    Sim(sim::Backend),
}

pub struct Probe {
    inner: Inner,
    start: std::time::Instant,
    last: f64,
    pub source: Source,
    /// Our own pid, so the interface can mark its own row and the halt plan
    /// can warn before we stop ourselves.
    pub me: i32,
    pub uid: u32,
    pub root: bool,
}

impl Probe {
    /// The native back end where there is one, otherwise the simulation.
    pub fn new() -> Probe {
        #[cfg(target_os = "linux")]
        {
            if std::path::Path::new("/proc/stat").exists() {
                return Probe::with(Inner::Native(native::Backend::new()), Source::Native);
            }
        }
        Probe::with(Inner::Sim(sim::Backend::new()), Source::Simulated)
    }

    /// The simulation, whatever the machine.  What `make demo` runs.
    pub fn simulated() -> Probe {
        Probe::with(Inner::Sim(sim::Backend::new()), Source::Simulated)
    }

    fn with(inner: Inner, source: Source) -> Probe {
        extern "C" {
            fn getuid() -> u32;
        }
        let uid = unsafe { getuid() };
        Probe {
            inner,
            start: std::time::Instant::now(),
            last: 0.0,
            source,
            me: std::process::id() as i32,
            uid,
            root: uid == 0,
        }
    }

    /// Seconds since the probe was created.  Every timestamp in a `Snapshot`
    /// is on this clock, which is monotonic and starts at zero.
    pub fn now(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    /// One tick.  `visible` is the pids whose rows are on screen; the
    /// expensive per-process files are read for those and no others.
    pub fn tick(&mut self, visible: &[i32]) -> Snapshot {
        let now = self.now();
        let dt = (now - self.last).max(1e-6);
        self.last = now;
        let mut s = match &mut self.inner {
            #[cfg(target_os = "linux")]
            Inner::Native(b) => b.sample(now, dt, visible),
            Inner::Sim(b) => b.sample(now, dt, visible),
        };
        // The one place the whole program learns our own pid, so nothing else
        // has to ask.
        if let Some(p) = s.procs.iter_mut().find(|p| p.pid == self.me) {
            p.name = p.name.clone();
        }
        s
    }

    /// The Inspector's readings, for one process, on selection.
    pub fn detail(&mut self, pid: i32) -> Option<Detail> {
        match &mut self.inner {
            #[cfg(target_os = "linux")]
            Inner::Native(b) => b.detail(pid),
            Inner::Sim(b) => b.detail(pid),
        }
    }

    /// Build a halt plan for a process.  Decides; does not act.
    pub fn plan(&self, snap: &Snapshot, pid: i32, grace: f64) -> Plan {
        halt::plan(&snap.procs, pid, grace, self.me, self.uid, self.root)
    }
}

impl Default for Probe {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_simulated_probe_ticks_and_says_it_is_simulated() {
        let mut p = Probe::simulated();
        assert_eq!(p.source, Source::Simulated);
        let s = p.tick(&[]);
        assert!(s.simulated);
        assert_eq!(s.cpu.logical(), 10);
        assert!(!s.procs.is_empty());
        assert!(s.procs.iter().any(|p| p.name == "nginx"));
    }

    #[test]
    fn a_plan_for_a_missing_pid_refuses_rather_than_panicking() {
        let mut p = Probe::simulated();
        let s = p.tick(&[]);
        assert!(p.plan(&s, 999_999, 2.0).refused);
    }

    /// On Linux this exercises the real reader; everywhere else it is the
    /// simulation, and either way the invariants are the same.
    #[test]
    fn a_native_probe_reads_this_machine() {
        let mut p = Probe::new();
        let a = p.tick(&[]);
        let b = p.tick(&[std::process::id() as i32]);
        assert!(b.t >= a.t);
        assert!(!b.procs.is_empty(), "there is at least one process");
        assert!(b.mem.total_kb > 0 || b.simulated);
        if p.source == Source::Native {
            assert!(
                b.procs.iter().any(|x| x.pid == p.me),
                "the process table contains this test"
            );
        }
    }
}
