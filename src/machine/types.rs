// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Paul Richeson
//! What a reading is.
//!
//! One `Snapshot` per tick, and every widget in `copal-tm-ui` draws from it.
//! Nothing here reads a file; the back ends in `linux` and `sim` fill these in.

use crate::machine::signals::SigMask;

/// Everything sampled on one tick.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    /// Monotonic seconds since the machine started.
    pub t: f64,
    /// Seconds since the previous snapshot; every rate is divided by it.
    pub dt: f64,
    /// True when this came from the simulated back end, and said so loudly.
    pub simulated: bool,
    pub cpu: Cpu,
    pub mem: Mem,
    pub net: Net,
    pub disk: Disk,
    pub power: Power,
    pub thermal: Thermal,
    /// 0..1, when the machine has a counter that answers.
    pub gpu: Option<f32>,
    pub psi: Psi,
    pub load: [f32; 3],
    pub uptime: f64,
    pub procs: Vec<Proc>,
}

/// A reading's age, so an instrument that has stopped does not look like one
/// reading zero.  Section VII-B, rule 3.
#[derive(Clone, Copy, Debug, Default)]
pub struct Aged<T> {
    pub value: T,
    /// Machine time at which this was actually read.
    pub at: f64,
}

impl<T: Copy> Aged<T> {
    pub fn new(value: T, at: f64) -> Self {
        Aged { value, at }
    }
    pub fn age(&self, now: f64) -> f64 {
        (now - self.at).max(0.0)
    }
    /// Stale once three of its own periods have gone by unrefreshed.
    pub fn stale(&self, now: f64, period: f64) -> bool {
        self.age(now) > period * 3.0
    }
}

#[derive(Clone, Debug, Default)]
pub struct Cpu {
    /// Busy fraction of the whole machine, 0..1.
    pub total: f32,
    /// System, IRQ and softirq as a fraction of the whole machine.
    pub kernel: f32,
    pub iowait: f32,
    /// Busy fraction per logical processor, 0..1.
    pub per_core: Vec<f32>,
    /// Current frequency per logical processor, in kHz, where readable.
    pub freq_khz: Vec<u64>,
    pub model: String,
    pub governor: Option<String>,
    /// One entry per logical processor: which class it belongs to.
    pub classes: Vec<CoreClass>,
    /// The class table itself, fastest first.
    pub class_names: Vec<String>,
}

impl Cpu {
    pub fn logical(&self) -> usize {
        self.per_core.len()
    }
}

/// A logical processor's place in the machine's topology.  A big.LITTLE Pi or
/// a modern x86 has more than one kind of core, and a build pinning one fast
/// core while eleven idle is invisible in an average.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CoreClass {
    /// Index into `Cpu::class_names`; 0 is the fastest class.
    pub class: u8,
    /// True when this logical processor shares a physical core with an
    /// earlier one - an SMT sibling, drawn dimmer beside its partner.
    pub sibling: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Mem {
    pub total_kb: u64,
    pub avail_kb: u64,
    pub cached_kb: u64,
    pub swap_total_kb: u64,
    pub swap_used_kb: u64,
}

impl Mem {
    pub fn used_kb(&self) -> u64 {
        self.total_kb.saturating_sub(self.avail_kb)
    }
    /// 0..1 of physical memory in use, the figure under the ladder.
    pub fn frac(&self) -> f32 {
        if self.total_kb == 0 {
            0.0
        } else {
            self.used_kb() as f32 / self.total_kb as f32
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Net {
    pub rx_bps: f64,
    pub tx_bps: f64,
    pub link_mbps: Option<u64>,
    /// Errors and drops as a fraction of packets this tick.
    pub err_rate: f32,
    pub iface: String,
}

impl Net {
    pub fn total_bps(&self) -> f64 {
        self.rx_bps + self.tx_bps
    }
}

#[derive(Clone, Debug, Default)]
pub struct Disk {
    pub name: String,
    /// Fraction of the tick the device had I/O in flight, 0..1.
    pub busy: f32,
    pub read_bps: f64,
    pub write_bps: f64,
    /// From `smartctl -H`, out of band; `None` until it answers, if ever.
    pub smart: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Power {
    pub present: bool,
    pub on_battery: bool,
    /// Charge, 0..1.
    pub charge: Option<f32>,
    pub watts: Option<f32>,
    /// The lap time: seconds of use left at the current draw.
    pub remaining_s: Option<f64>,
    pub status: String,
}

#[derive(Clone, Debug, Default)]
pub struct Thermal {
    pub hottest_c: Option<f32>,
    pub hottest_name: String,
    pub zones: Vec<(String, f32)>,
    /// The temperature the top of the Temp ladder means, in °C.
    pub scale_max: f32,
}

/// Pressure stall information: the fraction of the last ten seconds in which
/// at least one task was stalled on the resource.
#[derive(Clone, Copy, Debug, Default)]
pub struct Psi {
    pub present: bool,
    pub cpu10: f32,
    pub mem10: f32,
    pub io10: f32,
}

impl Psi {
    pub fn worst(&self) -> f32 {
        self.cpu10.max(self.mem10).max(self.io10)
    }
    /// The word the System Pressure tile shows instead of a number.
    pub fn verdict(&self) -> &'static str {
        let w = self.worst();
        if w >= 20.0 {
            "HIGH"
        } else if w >= 5.0 {
            "SOME"
        } else {
            "LOW"
        }
    }
}

/// How a process came to be supervised, which decides whether a signal is the
/// right way to stop it at all.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Supervision {
    #[default]
    None,
    /// An OpenRC service, by pidfile or cgroup.  Stopping it is
    /// `rc-service NAME stop`; signalling it gets it restarted.
    OpenRc(String),
    /// A systemd unit, for completeness on machines that are not Copal.
    Systemd(String),
}

#[derive(Clone, Debug, Default)]
pub struct Proc {
    pub pid: i32,
    pub ppid: i32,
    pub pgid: i32,
    pub sid: i32,
    pub tty_nr: i32,
    /// `R S D T t Z X`, from `/proc/[pid]/stat` field 3.
    pub state: char,
    /// The `comm`, which is the executable's basename truncated to 15 bytes.
    pub name: String,
    /// The full command line, arguments joined with spaces.  Read on
    /// selection only; empty until then, and empty forever for a kernel
    /// thread, which is how a kernel thread is recognised.
    pub cmdline: String,
    pub uid: u32,
    pub user: String,
    pub nice: i32,
    pub prio: i32,
    pub threads: i32,
    pub rss_kb: u64,
    pub vsz_kb: u64,
    /// Share of the machine, 0..1, over the last tick.
    pub cpu: f32,
    /// Total CPU seconds used since it started.
    pub cpu_time: f64,
    /// Seconds since boot at which it started; with `uptime`, its age.
    pub start_s: f64,
    pub kernel_thread: bool,
    pub supervision: Supervision,
    /// Signal dispositions.  Filled for visible rows only - Section VII-A,
    /// row 8 - so `None` means "not looked at", never "nothing to report".
    pub sig: Option<SigMask>,
}

impl Proc {
    /// What the Browser's COMMAND column shows: the command line if we have
    /// it, else the `comm` in brackets for a kernel thread.
    pub fn display(&self) -> String {
        if !self.cmdline.is_empty() {
            self.cmdline.clone()
        } else if self.kernel_thread {
            format!("[{}]", self.name)
        } else {
            self.name.clone()
        }
    }

    pub fn state_name(&self) -> &'static str {
        match self.state {
            'R' => "running",
            'S' => "sleeping",
            'D' => "uninterruptible sleep",
            'T' => "stopped",
            't' => "traced",
            'Z' => "zombie",
            'X' | 'x' => "dead",
            'I' => "idle",
            _ => "unknown",
        }
    }
}

/// One socket a process holds open.
///
/// The join that produces these is the only way `/proc` will answer "what is
/// this process doing on the network": `/proc/[pid]/fd/*` reads back as
/// `socket:[INODE]`, and `/proc/net/{tcp,tcp6,udp,udp6,unix}` carry that same
/// inode in a column. Match them and a descriptor becomes an address.
///
/// What this cannot give is a byte *rate* per process: Linux keeps no
/// per-process network counters in `/proc` at all (they need taskstats,
/// netlink or eBPF). What it does give is every address, the connection
/// state, and the queue depths - which is enough to answer the questions that
/// are actually asked of a task manager: what is it listening on, who is it
/// talking to, and is its send queue backing up.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Socket {
    /// `tcp`, `tcp6`, `udp`, `udp6` or `unix`.
    pub proto: &'static str,
    /// `0.0.0.0:8080`, or a filesystem path for a Unix socket.
    pub local: String,
    /// Empty when there is no peer - a listener, or an unconnected datagram.
    pub remote: String,
    /// `LISTEN`, `ESTABLISHED`, `TIME_WAIT`, …
    pub state: &'static str,
    /// Bytes waiting to be sent, and waiting to be read. A send queue that
    /// does not drain is the shape of a stuck connection.
    pub tx_queue: u64,
    pub rx_queue: u64,
    pub inode: u64,
}

impl Socket {
    /// The port a listener is on, for the summary line.
    pub fn port(&self) -> Option<u16> {
        self.local.rsplit(':').next()?.parse().ok()
    }

    pub fn is_listening(&self) -> bool {
        self.state == "LISTEN"
    }
}

/// The expensive readings, taken for one process when it is selected.
#[derive(Clone, Debug, Default)]
pub struct Detail {
    pub pid: i32,
    pub cmdline: String,
    pub exe: String,
    /// True when the executable has been replaced or removed since it was
    /// mapped - an upgraded package whose daemon has not been restarted.
    pub exe_deleted: bool,
    pub cwd: String,
    pub cgroup: String,
    pub env_count: usize,
    /// Open descriptors: (files, sockets, pipes, other).
    pub fds: (usize, usize, usize, usize),
    pub uid_real: u32,
    pub uid_eff: u32,
    pub gid_real: u32,
    pub gid_eff: u32,
    pub cap_eff: u64,
    pub seccomp: u8,
    pub ctx_vol: u64,
    pub ctx_invol: u64,
    pub sig: SigMask,
    /// Every socket this process holds, joined from its descriptors.
    pub sockets: Vec<Socket>,
}

impl Detail {
    /// Listeners first, then established connections, then the rest: the
    /// order the question is usually asked in.
    pub fn sockets_ranked(&self) -> Vec<&Socket> {
        let mut v: Vec<&Socket> = self.sockets.iter().collect();
        v.sort_by_key(|s| {
            (
                match s.state {
                    "LISTEN" => 0,
                    "ESTABLISHED" => 1,
                    _ => 2,
                },
                s.port().unwrap_or(u16::MAX),
            )
        });
        v
    }

    /// `3 listening \u{b7} 11 established \u{b7} 4 unix`, the summary line.
    pub fn socket_summary(&self) -> String {
        let listen = self.sockets.iter().filter(|s| s.is_listening()).count();
        let est = self
            .sockets
            .iter()
            .filter(|s| s.state == "ESTABLISHED")
            .count();
        let unix = self.sockets.iter().filter(|s| s.proto == "unix").count();
        let mut parts = Vec::new();
        if listen > 0 {
            parts.push(format!("{listen} listening"));
        }
        if est > 0 {
            parts.push(format!("{est} established"));
        }
        if unix > 0 {
            parts.push(format!("{unix} unix"));
        }
        if parts.is_empty() {
            "no sockets".into()
        } else {
            parts.join(" \u{b7} ")
        }
    }
}
