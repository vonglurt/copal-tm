// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Paul Richeson
//! The Linux back end: `/proc` and `/sys`, on the schedule of Section VII.
//!
//! Two rules from that section are enforced here rather than by the caller,
//! because they are properties of the reading and not of the drawing:
//!
//! 1. **Nothing forks on the tick.**  Everything below is a file read.
//! 2. **Cost scales with what is shown.**  `/proc/[pid]/stat` is read for
//!    every process, because the list itself needs it; `/proc/[pid]/status`
//!    and `cgroup` are read only for the rows on screen.  On a 400-process
//!    machine the difference is 22,000 lines a second against 1,600.

use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use crate::machine::signals::SigMask;
use crate::machine::types::*;

/// `/proc` times are in USER_HZ, which is fixed at 100 on Linux whatever
/// `CONFIG_HZ` is set to.  This is the one constant that does not need
/// `sysconf`.
const USER_HZ: f64 = 100.0;

/// `PF_KTHREAD` in `/proc/[pid]/stat`'s flags field: the kernel's own answer
/// to "is this a kernel thread", and cheaper than reading an empty cmdline.
const PF_KTHREAD: u64 = 0x0020_0000;

const P_THERMAL: f64 = 2.0;
const P_GPU: f64 = 2.0;
const P_BATTERY: f64 = 5.0;
const P_FREQ: f64 = 2.0;
const P_SLOW: f64 = 60.0;

#[derive(Default, Clone, Copy)]
struct CpuTimes {
    user: u64,
    nice: u64,
    system: u64,
    idle: u64,
    iowait: u64,
    irq: u64,
    softirq: u64,
    steal: u64,
}

impl CpuTimes {
    fn total(&self) -> u64 {
        self.user
            + self.nice
            + self.system
            + self.idle
            + self.iowait
            + self.irq
            + self.softirq
            + self.steal
    }
    fn busy(&self) -> u64 {
        self.total() - self.idle - self.iowait
    }
    fn kernel(&self) -> u64 {
        self.system + self.irq + self.softirq
    }
}

pub struct Backend {
    prev_cpu: Vec<CpuTimes>,
    prev_net: (u64, u64, u64, u64),
    prev_disk: (u64, u64, u64),
    prev_proc: HashMap<i32, (f64, f64)>,
    users: HashMap<u32, String>,
    users_at: f64,
    thermal: Thermal,
    thermal_at: f64,
    gpu: Option<f32>,
    gpu_at: f64,
    power: Power,
    power_at: f64,
    freq: Vec<u64>,
    freq_at: f64,
    cpu_static: (String, Option<String>, Vec<CoreClass>, Vec<String>),
    static_at: f64,
    first: bool,
}

impl Default for Backend {
    fn default() -> Self {
        Self::new()
    }
}

impl Backend {
    pub fn new() -> Backend {
        Backend {
            prev_cpu: Vec::new(),
            prev_net: (0, 0, 0, 0),
            prev_disk: (0, 0, 0),
            prev_proc: HashMap::new(),
            users: HashMap::new(),
            users_at: f64::MIN,
            thermal: Thermal::default(),
            thermal_at: f64::MIN,
            gpu: None,
            gpu_at: f64::MIN,
            power: Power::default(),
            power_at: f64::MIN,
            freq: Vec::new(),
            freq_at: f64::MIN,
            cpu_static: (String::new(), None, Vec::new(), Vec::new()),
            static_at: f64::MIN,
            first: true,
        }
    }

    pub fn sample(&mut self, now: f64, dt: f64, visible: &[i32]) -> Snapshot {
        let mut s = Snapshot {
            t: now,
            dt,
            simulated: false,
            ..Default::default()
        };
        self.cpu(&mut s, now);
        s.mem = meminfo();
        self.net(&mut s, dt);
        self.disk(&mut s, dt);
        s.psi = pressure();
        s.load = loadavg();
        s.uptime = read_f64("/proc/uptime").unwrap_or(0.0);

        if now - self.thermal_at >= P_THERMAL {
            self.thermal = thermal();
            self.thermal_at = now;
        }
        s.thermal = self.thermal.clone();
        if now - self.gpu_at >= P_GPU {
            self.gpu = gpu_busy();
            self.gpu_at = now;
        }
        s.gpu = self.gpu;
        if now - self.power_at >= P_BATTERY {
            self.power = power();
            self.power_at = now;
        }
        s.power = self.power.clone();

        self.procs(&mut s, dt, visible, now);
        self.first = false;
        s
    }

    fn cpu(&mut self, s: &mut Snapshot, now: f64) {
        let text = fs::read_to_string("/proc/stat").unwrap_or_default();
        let mut cur: Vec<CpuTimes> = Vec::new();
        for line in text.lines() {
            if !line.starts_with("cpu") {
                break;
            }
            let mut it = line.split_whitespace();
            let _ = it.next();
            let v: Vec<u64> = it.take(8).map(|x| x.parse().unwrap_or(0)).collect();
            let mut t = CpuTimes::default();
            for (i, x) in v.iter().enumerate() {
                match i {
                    0 => t.user = *x,
                    1 => t.nice = *x,
                    2 => t.system = *x,
                    3 => t.idle = *x,
                    4 => t.iowait = *x,
                    5 => t.irq = *x,
                    6 => t.softirq = *x,
                    7 => t.steal = *x,
                    _ => {}
                }
            }
            cur.push(t);
        }
        if cur.is_empty() {
            return;
        }
        let frac = |now: &CpuTimes, was: &CpuTimes, f: fn(&CpuTimes) -> u64| -> f32 {
            let d = now.total().saturating_sub(was.total());
            if d == 0 {
                0.0
            } else {
                (f(now).saturating_sub(f(was)) as f32 / d as f32).clamp(0.0, 1.0)
            }
        };
        if self.prev_cpu.len() == cur.len() {
            s.cpu.total = frac(&cur[0], &self.prev_cpu[0], |c| c.busy());
            s.cpu.kernel = frac(&cur[0], &self.prev_cpu[0], |c| c.kernel());
            s.cpu.iowait = frac(&cur[0], &self.prev_cpu[0], |c| c.iowait);
            s.cpu.per_core = (1..cur.len())
                .map(|i| frac(&cur[i], &self.prev_cpu[i], |c| c.busy()))
                .collect();
        } else {
            s.cpu.per_core = vec![0.0; cur.len() - 1];
        }
        self.prev_cpu = cur;

        if now - self.static_at >= P_SLOW {
            self.cpu_static = cpu_static(s.cpu.per_core.len());
            self.static_at = now;
        }
        if now - self.freq_at >= P_FREQ {
            self.freq = (0..s.cpu.per_core.len())
                .map(|i| {
                    read_u64(&format!(
                        "/sys/devices/system/cpu/cpu{i}/cpufreq/scaling_cur_freq"
                    ))
                    .unwrap_or(0)
                })
                .collect();
            self.freq_at = now;
        }
        s.cpu.model = self.cpu_static.0.clone();
        s.cpu.governor = self.cpu_static.1.clone();
        s.cpu.classes = self.cpu_static.2.clone();
        s.cpu.class_names = self.cpu_static.3.clone();
        s.cpu.freq_khz = self.freq.clone();
    }

    fn net(&mut self, s: &mut Snapshot, dt: f64) {
        let text = fs::read_to_string("/proc/net/dev").unwrap_or_default();
        let (mut rx, mut tx, mut pkts, mut errs) = (0u64, 0u64, 0u64, 0u64);
        let (mut best, mut best_bytes) = (String::new(), 0u64);
        for line in text.lines().skip(2) {
            let Some((name, rest)) = line.split_once(':') else {
                continue;
            };
            let name = name.trim();
            if name == "lo" || name.starts_with("veth") || name.starts_with("docker") {
                continue;
            }
            let v: Vec<u64> = rest
                .split_whitespace()
                .map(|x| x.parse().unwrap_or(0))
                .collect();
            if v.len() < 16 {
                continue;
            }
            rx += v[0];
            tx += v[8];
            pkts += v[1] + v[9];
            errs += v[2] + v[3] + v[10] + v[11];
            if v[0] + v[8] > best_bytes {
                best_bytes = v[0] + v[8];
                best = name.to_string();
            }
        }
        let p = self.prev_net;
        if !self.first && dt > 0.0 {
            s.net.rx_bps = rx.saturating_sub(p.0) as f64 / dt;
            s.net.tx_bps = tx.saturating_sub(p.1) as f64 / dt;
            let dp = pkts.saturating_sub(p.2);
            s.net.err_rate = if dp == 0 {
                0.0
            } else {
                errs.saturating_sub(p.3) as f32 / dp as f32
            };
        }
        self.prev_net = (rx, tx, pkts, errs);
        s.net.link_mbps = read_u64(&format!("/sys/class/net/{best}/speed"));
        s.net.iface = best;
    }

    fn disk(&mut self, s: &mut Snapshot, dt: f64) {
        let text = fs::read_to_string("/proc/diskstats").unwrap_or_default();
        // Whole devices only: a partition's counters are already in its disk's,
        // and loop/ram/dm devices are not the thing anyone means by "the disk".
        let mut devs: Vec<(String, u64, u64, u64)> = Vec::new();
        for line in text.lines() {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 14 {
                continue;
            }
            let name = f[2];
            if name.starts_with("loop")
                || name.starts_with("ram")
                || name.starts_with("zram")
                || name.starts_with("dm-")
                || name.starts_with("md")
            {
                continue;
            }
            let rd: u64 = f[5].parse().unwrap_or(0);
            let wr: u64 = f[9].parse().unwrap_or(0);
            let io: u64 = f[12].parse().unwrap_or(0);
            devs.push((name.to_string(), rd, wr, io));
        }
        let parents: Vec<String> = devs.iter().map(|d| d.0.clone()).collect();
        devs.retain(|d| {
            !parents.iter().any(|p| {
                *p != d.0
                    && d.0.starts_with(p.as_str())
                    && d.0[p.len()..]
                        .chars()
                        .all(|c| c.is_ascii_digit() || c == 'p')
            })
        });
        let (mut rd, mut wr, mut io) = (0u64, 0u64, 0u64);
        let mut name = String::new();
        let mut best = 0u64;
        for d in &devs {
            rd += d.1;
            wr += d.2;
            io += d.3;
            if d.3 > best {
                best = d.3;
                name = d.0.clone();
            }
        }
        let p = self.prev_disk;
        if !self.first && dt > 0.0 {
            // Sectors are 512 bytes in `diskstats` regardless of the device's
            // own sector size; the kernel normalises them.
            s.disk.read_bps = rd.saturating_sub(p.0) as f64 * 512.0 / dt;
            s.disk.write_bps = wr.saturating_sub(p.1) as f64 * 512.0 / dt;
            // io_ticks is milliseconds with I/O in flight.
            s.disk.busy = (io.saturating_sub(p.2) as f32 / (dt as f32 * 1000.0)).clamp(0.0, 1.0);
        }
        self.prev_disk = (rd, wr, io);
        s.disk.name = name;
    }

    fn procs(&mut self, s: &mut Snapshot, dt: f64, visible: &[i32], now: f64) {
        if now - self.users_at >= P_SLOW {
            self.users = passwd();
            self.users_at = now;
        }
        let Ok(dir) = fs::read_dir("/proc") else {
            return;
        };
        let mut seen: HashMap<i32, (f64, f64)> = HashMap::new();
        let mut out = Vec::with_capacity(256);
        for e in dir.flatten() {
            let nm = e.file_name();
            let Some(nm) = nm.to_str() else { continue };
            let Ok(pid) = nm.parse::<i32>() else { continue };
            let Some(mut p) = read_stat(pid) else {
                continue;
            };
            p.uid = fs::metadata(format!("/proc/{pid}"))
                .map(|m| m.uid())
                .unwrap_or(0);
            p.user = self
                .users
                .get(&p.uid)
                .cloned()
                .unwrap_or_else(|| p.uid.to_string());
            let cpu_time = p.cpu_time;
            if let Some(&(prev_time, prev_start)) = self.prev_proc.get(&pid) {
                if prev_start == p.start_s && dt > 0.0 {
                    p.cpu = ((cpu_time - prev_time) / dt).clamp(0.0, 64.0) as f32;
                }
            }
            seen.insert(pid, (cpu_time, p.start_s));
            out.push(p);
        }
        self.prev_proc = seen;

        // Row 8 of the schedule: the expensive per-process file, for the rows
        // that are actually on screen.
        for p in out.iter_mut() {
            if visible.contains(&p.pid) {
                if let Some((mask, threads, rss)) = read_status(p.pid) {
                    p.sig = Some(mask);
                    if threads > 0 {
                        p.threads = threads;
                    }
                    if rss > 0 {
                        p.rss_kb = rss;
                    }
                }
                p.supervision = supervision(p.pid);
                if !p.kernel_thread {
                    p.cmdline = read_cmdline(p.pid);
                }
            }
        }
        s.procs = out;
    }

    pub fn detail(&mut self, pid: i32) -> Option<Detail> {
        let status = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
        let mut d = Detail {
            pid,
            ..Default::default()
        };
        let mut blk = String::new();
        let mut ign = String::new();
        let mut cgt = String::new();
        for line in status.lines() {
            let Some((k, v)) = line.split_once(':') else {
                continue;
            };
            let v = v.trim();
            let nums: Vec<u64> = v
                .split_whitespace()
                .filter_map(|x| x.parse().ok())
                .collect();
            match k {
                "Uid" => {
                    d.uid_real = *nums.first().unwrap_or(&0) as u32;
                    d.uid_eff = *nums.get(1).unwrap_or(&0) as u32;
                }
                "Gid" => {
                    d.gid_real = *nums.first().unwrap_or(&0) as u32;
                    d.gid_eff = *nums.get(1).unwrap_or(&0) as u32;
                }
                "SigBlk" => blk = v.into(),
                "SigIgn" => ign = v.into(),
                "SigCgt" => cgt = v.into(),
                "CapEff" => d.cap_eff = u64::from_str_radix(v, 16).unwrap_or(0),
                "Seccomp" => d.seccomp = v.parse().unwrap_or(0),
                "voluntary_ctxt_switches" => d.ctx_vol = v.parse().unwrap_or(0),
                "nonvoluntary_ctxt_switches" => d.ctx_invol = v.parse().unwrap_or(0),
                _ => {}
            }
        }
        d.sig = SigMask::from_hex(&blk, &ign, &cgt);
        d.cmdline = read_cmdline(pid);
        if let Ok(t) = fs::read_link(format!("/proc/{pid}/exe")) {
            let s = t.to_string_lossy().to_string();
            d.exe_deleted = s.ends_with(" (deleted)");
            d.exe = s.trim_end_matches(" (deleted)").to_string();
        }
        if let Ok(t) = fs::read_link(format!("/proc/{pid}/cwd")) {
            d.cwd = t.to_string_lossy().to_string();
        }
        d.cgroup = fs::read_to_string(format!("/proc/{pid}/cgroup"))
            .unwrap_or_default()
            .lines()
            .last()
            .and_then(|l| l.rsplit(':').next())
            .unwrap_or("")
            .to_string();
        d.env_count = fs::read(format!("/proc/{pid}/environ"))
            .map(|b| b.split(|&c| c == 0).filter(|s| !s.is_empty()).count())
            .unwrap_or(0);
        // The most expensive read in the program, and the reason it happens
        // only on selection: a process holding ten thousand descriptors makes
        // this a ten-thousand-entry directory scan.
        let table = socket_table();
        if let Ok(fds) = fs::read_dir(format!("/proc/{pid}/fd")) {
            for e in fds.flatten() {
                match fs::read_link(e.path()) {
                    Ok(t) => {
                        let s = t.to_string_lossy();
                        if let Some(inode) = s
                            .strip_prefix("socket:[")
                            .and_then(|r| r.strip_suffix(']'))
                            .and_then(|r| r.parse::<u64>().ok())
                        {
                            d.fds.1 += 1;
                            if let Some(sock) = table.get(&inode) {
                                if !d.sockets.iter().any(|x| x.inode == inode) {
                                    d.sockets.push(sock.clone());
                                }
                            }
                        } else if s.starts_with("socket:") {
                            d.fds.1 += 1;
                        } else if s.starts_with("pipe:") {
                            d.fds.2 += 1;
                        } else if s.starts_with("anon_inode:") {
                            d.fds.3 += 1;
                        } else {
                            d.fds.0 += 1;
                        }
                    }
                    Err(_) => d.fds.3 += 1,
                }
            }
        }
        Some(d)
    }
}

fn read_stat(pid: i32) -> Option<Proc> {
    let text = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // `comm` can contain spaces and parentheses, so the split is on the LAST
    // `)` and everything before it is fields 1 and 2.
    let close = text.rfind(')')?;
    let open = text.find('(')?;
    let name = text.get(open + 1..close)?.to_string();
    let rest: Vec<&str> = text.get(close + 2..)?.split_whitespace().collect();
    // After the split, index 0 is field 3.  `f(n)` reads field n.
    let f = |n: usize| -> &str { rest.get(n - 3).copied().unwrap_or("0") };
    let flags: u64 = f(9).parse().unwrap_or(0);
    let utime: f64 = f(14).parse().unwrap_or(0.0);
    let stime: f64 = f(15).parse().unwrap_or(0.0);
    let rss_pages: i64 = f(24).parse().unwrap_or(0);
    let page_kb = page_size() / 1024;
    Some(Proc {
        pid,
        state: f(3).chars().next().unwrap_or('?'),
        ppid: f(4).parse().unwrap_or(0),
        pgid: f(5).parse().unwrap_or(0),
        sid: f(6).parse().unwrap_or(0),
        tty_nr: f(7).parse().unwrap_or(0),
        name,
        prio: f(18).parse().unwrap_or(0),
        nice: f(19).parse().unwrap_or(0),
        threads: f(20).parse().unwrap_or(1),
        start_s: f(22).parse::<f64>().unwrap_or(0.0) / USER_HZ,
        vsz_kb: f(23).parse::<u64>().unwrap_or(0) / 1024,
        rss_kb: (rss_pages.max(0) as u64) * page_kb,
        cpu_time: (utime + stime) / USER_HZ,
        kernel_thread: flags & PF_KTHREAD != 0,
        ..Default::default()
    })
}

/// The three signal masks, the thread count and `VmRSS`, in one read.
fn read_status(pid: i32) -> Option<(SigMask, i32, u64)> {
    let text = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let (mut blk, mut ign, mut cgt) = (String::new(), String::new(), String::new());
    let mut threads = 0;
    let mut rss = 0;
    for line in text.lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let v = v.trim();
        match k {
            "SigBlk" => blk = v.into(),
            "SigIgn" => ign = v.into(),
            "SigCgt" => cgt = v.into(),
            "Threads" => threads = v.parse().unwrap_or(0),
            "VmRSS" => {
                rss = v
                    .split_whitespace()
                    .next()
                    .and_then(|x| x.parse().ok())
                    .unwrap_or(0)
            }
            _ => {}
        }
        if !cgt.is_empty() && threads > 0 && rss > 0 {
            break;
        }
    }
    Some((SigMask::from_hex(&blk, &ign, &cgt), threads, rss))
}

fn read_cmdline(pid: i32) -> String {
    match fs::read(format!("/proc/{pid}/cmdline")) {
        Ok(b) => {
            let parts: Vec<String> = b
                .split(|&c| c == 0)
                .filter(|s| !s.is_empty())
                .map(|s| String::from_utf8_lossy(s).to_string())
                .collect();
            parts.join(" ")
        }
        Err(_) => String::new(),
    }
}

/// Which supervisor, if any, owns this process - the check that decides
/// whether a signal is the right instrument at all.  OpenRC with cgroups
/// writes `0::/openrc.<service>`; systemd writes `…/<name>.service`.
fn supervision(pid: i32) -> Supervision {
    let text = fs::read_to_string(format!("/proc/{pid}/cgroup")).unwrap_or_default();
    for line in text.lines() {
        let path = line.rsplit(':').next().unwrap_or("");
        for seg in path.split('/') {
            if let Some(name) = seg.strip_prefix("openrc.") {
                return Supervision::OpenRc(name.to_string());
            }
            if let Some(name) = seg.strip_suffix(".service") {
                if !name.is_empty() {
                    return Supervision::Systemd(name.to_string());
                }
            }
        }
        if path.starts_with("/openrc/") {
            if let Some(name) = path.strip_prefix("/openrc/") {
                let name = name.split('/').next().unwrap_or("");
                if !name.is_empty() {
                    return Supervision::OpenRc(name.to_string());
                }
            }
        }
    }
    Supervision::None
}

fn meminfo() -> Mem {
    let text = fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let mut m = Mem::default();
    let mut swap_free = 0;
    for line in text.lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let n: u64 = v
            .split_whitespace()
            .next()
            .and_then(|x| x.parse().ok())
            .unwrap_or(0);
        match k {
            "MemTotal" => m.total_kb = n,
            // MemAvailable is the kernel's own estimate and is the only
            // honest answer; "free" has not meant anything useful for years.
            "MemAvailable" => m.avail_kb = n,
            "Cached" => m.cached_kb += n,
            "SReclaimable" => m.cached_kb += n,
            "SwapTotal" => m.swap_total_kb = n,
            "SwapFree" => swap_free = n,
            _ => {}
        }
    }
    m.swap_used_kb = m.swap_total_kb.saturating_sub(swap_free);
    m
}

fn pressure() -> Psi {
    let one = |p: &str| -> f32 {
        fs::read_to_string(p)
            .ok()
            .and_then(|t| {
                t.lines().find(|l| l.starts_with("some")).and_then(|l| {
                    l.split_whitespace()
                        .find_map(|f| f.strip_prefix("avg10="))
                        .and_then(|v| v.parse().ok())
                })
            })
            .unwrap_or(0.0)
    };
    let present = Path::new("/proc/pressure/cpu").exists();
    Psi {
        present,
        cpu10: one("/proc/pressure/cpu"),
        mem10: one("/proc/pressure/memory"),
        io10: one("/proc/pressure/io"),
    }
}

fn loadavg() -> [f32; 3] {
    let t = fs::read_to_string("/proc/loadavg").unwrap_or_default();
    let v: Vec<f32> = t
        .split_whitespace()
        .take(3)
        .filter_map(|x| x.parse().ok())
        .collect();
    [
        v.first().copied().unwrap_or(0.0),
        v.get(1).copied().unwrap_or(0.0),
        v.get(2).copied().unwrap_or(0.0),
    ]
}

fn thermal() -> Thermal {
    let mut t = Thermal {
        scale_max: 110.0,
        ..Default::default()
    };
    if let Ok(d) = fs::read_dir("/sys/class/thermal") {
        for e in d.flatten() {
            let p = e.path();
            if !p
                .file_name()
                .map(|n| n.to_string_lossy().starts_with("thermal_zone"))
                .unwrap_or(false)
            {
                continue;
            }
            let Some(milli) = read_f64(&format!("{}/temp", p.display())) else {
                continue;
            };
            let c = (milli / 1000.0) as f32;
            if !(-50.0..200.0).contains(&c) {
                continue;
            }
            let name = fs::read_to_string(format!("{}/type", p.display()))
                .unwrap_or_default()
                .trim()
                .to_string();
            t.zones.push((name, c));
        }
    }
    if t.zones.is_empty() {
        if let Ok(d) = fs::read_dir("/sys/class/hwmon") {
            for e in d.flatten() {
                let base = e.path();
                let label = fs::read_to_string(base.join("name")).unwrap_or_default();
                for i in 1..8 {
                    if let Some(milli) = read_f64(&format!("{}/temp{i}_input", base.display())) {
                        let c = (milli / 1000.0) as f32;
                        if (-50.0..200.0).contains(&c) {
                            t.zones.push((format!("{} {}", label.trim(), i), c));
                        }
                    }
                }
            }
        }
    }
    if let Some((n, c)) = t
        .zones
        .iter()
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
    {
        t.hottest_name = n.clone();
        t.hottest_c = Some(*c);
    }
    t
}

/// Every way a Linux machine will admit to a GPU load, in the order they are
/// worth trying.  `None` when none of them answers, which is when the SYS
/// panel's fourth column falls back to I/O.
fn gpu_busy() -> Option<f32> {
    if let Ok(d) = fs::read_dir("/sys/class/drm") {
        for e in d.flatten() {
            let n = e.file_name();
            let n = n.to_string_lossy();
            if !n.starts_with("card") || n.contains('-') {
                continue;
            }
            if let Some(v) = read_f64(&format!("{}/device/gpu_busy_percent", e.path().display())) {
                return Some((v / 100.0) as f32);
            }
        }
    }
    if let Ok(d) = fs::read_dir("/sys/class/devfreq") {
        for e in d.flatten() {
            if let Some(v) = read_f64(&format!("{}/load", e.path().display())) {
                return Some((v / 100.0) as f32);
            }
        }
    }
    None
}

fn power() -> Power {
    let mut p = Power::default();
    let Ok(d) = fs::read_dir("/sys/class/power_supply") else {
        return p;
    };
    for e in d.flatten() {
        let base = e.path();
        let kind = fs::read_to_string(base.join("type")).unwrap_or_default();
        if kind.trim() != "Battery" {
            continue;
        }
        p.present = true;
        p.status = fs::read_to_string(base.join("status"))
            .unwrap_or_default()
            .trim()
            .to_string();
        p.on_battery = p.status == "Discharging";
        if let Some(c) = read_f64(&format!("{}/capacity", base.display())) {
            p.charge = Some((c / 100.0) as f32);
        }
        // Micro-watts if the firmware reports energy; otherwise current times
        // voltage, both micro-units, so the product is 1e-12 W.
        let uw = read_f64(&format!("{}/power_now", base.display())).or_else(|| {
            let i = read_f64(&format!("{}/current_now", base.display()))?;
            let v = read_f64(&format!("{}/voltage_now", base.display()))?;
            Some(i * v / 1e6)
        });
        if let Some(uw) = uw {
            let w = (uw / 1e6) as f32;
            if w.abs() > 0.01 {
                p.watts = Some(w.abs());
            }
        }
        let now_uwh = read_f64(&format!("{}/energy_now", base.display())).or_else(|| {
            let ah = read_f64(&format!("{}/charge_now", base.display()))?;
            let v = read_f64(&format!("{}/voltage_now", base.display()))?;
            Some(ah * v / 1e6)
        });
        if let (Some(e), Some(w)) = (now_uwh, p.watts) {
            if p.on_battery && w > 0.1 {
                p.remaining_s = Some((e / 1e6) / w as f64 * 3600.0);
            }
        }
        break;
    }
    p
}

/// Model name, governor, and the core classes of Section VI-B's second footer
/// row.  Cores within 5 % of each other's maximum frequency are one class.
fn cpu_static(n: usize) -> (String, Option<String>, Vec<CoreClass>, Vec<String>) {
    let model = fs::read_to_string("/proc/cpuinfo")
        .unwrap_or_default()
        .lines()
        .find(|l| l.starts_with("model name") || l.starts_with("Model"))
        .and_then(|l| l.split_once(':').map(|(_, v)| v.trim().to_string()))
        .unwrap_or_default();
    let governor = fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let cap: Vec<u64> = (0..n)
        .map(|i| {
            read_u64(&format!("/sys/devices/system/cpu/cpu{i}/cpu_capacity"))
                .or_else(|| {
                    read_u64(&format!(
                        "/sys/devices/system/cpu/cpu{i}/cpufreq/cpuinfo_max_freq"
                    ))
                })
                .unwrap_or(0)
        })
        .collect();
    let mut tiers: Vec<u64> = cap.iter().copied().filter(|c| *c > 0).collect();
    tiers.sort_unstable_by(|a, b| b.cmp(a));
    tiers.dedup_by(|a, b| (*a as f64 - *b as f64).abs() / (*b as f64).max(1.0) < 0.05);
    let names: Vec<String> = if tiers.len() <= 1 {
        Vec::new()
    } else {
        (0..tiers.len())
            .map(|i| match i {
                0 => "P".to_string(),
                1 => "E".to_string(),
                k => format!("c{k}"),
            })
            .collect()
    };
    let mut classes = Vec::with_capacity(n);
    let mut seen_core: Vec<String> = Vec::new();
    // cap was collected from (0..n), so it has exactly n entries and the index
    // is still the cpu number that the sysfs paths below are named after.
    for (i, capacity) in cap.iter().enumerate() {
        let class = if tiers.len() <= 1 {
            0
        } else {
            tiers
                .iter()
                .position(|t| (*capacity as f64 - *t as f64).abs() / (*t as f64).max(1.0) < 0.05)
                .unwrap_or(tiers.len() - 1) as u8
        };
        let sibs = fs::read_to_string(format!("/sys/devices/system/cpu/cpu{i}/topology/core_id"))
            .unwrap_or_default()
            .trim()
            .to_string();
        let key = format!(
            "{}:{}",
            fs::read_to_string(format!(
                "/sys/devices/system/cpu/cpu{i}/topology/physical_package_id"
            ))
            .unwrap_or_default()
            .trim(),
            sibs
        );
        let sibling = !sibs.is_empty() && seen_core.contains(&key);
        if !sibs.is_empty() {
            seen_core.push(key);
        }
        classes.push(CoreClass { class, sibling });
    }
    (model, governor, classes, names)
}

fn passwd() -> HashMap<u32, String> {
    let mut m = HashMap::new();
    for line in fs::read_to_string("/etc/passwd")
        .unwrap_or_default()
        .lines()
    {
        let f: Vec<&str> = line.split(':').collect();
        if f.len() > 2 {
            if let Ok(uid) = f[2].parse::<u32>() {
                m.insert(uid, f[0].to_string());
            }
        }
    }
    m
}

fn read_f64(path: &str) -> Option<f64> {
    fs::read_to_string(path)
        .ok()?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

fn read_u64(path: &str) -> Option<u64> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn page_size() -> u64 {
    extern "C" {
        fn getpagesize() -> i32;
    }
    let p = unsafe { getpagesize() };
    if p > 0 {
        p as u64
    } else {
        4096
    }
}

/// The socket table, keyed by inode.
///
/// `/proc/[pid]/fd/*` reads back as `socket:[INODE]`; `/proc/net/tcp` and its
/// siblings carry that inode in a column. This builds the map once per
/// Inspector open — five small files — and `sockets_for` joins one process's
/// descriptors against it.
fn socket_table() -> HashMap<u64, Socket> {
    let mut m = HashMap::new();
    for (proto, path, v6) in [
        ("tcp", "/proc/net/tcp", false),
        ("tcp6", "/proc/net/tcp6", true),
        ("udp", "/proc/net/udp", false),
        ("udp6", "/proc/net/udp6", true),
    ] {
        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };
        for line in text.lines().skip(1) {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 10 {
                continue;
            }
            let queues = f[4].split_once(':').unwrap_or(("0", "0"));
            let inode: u64 = f[9].parse().unwrap_or(0);
            if inode == 0 {
                continue;
            }
            let state = tcp_state(f[3], proto.starts_with("udp"));
            let remote = addr(f[2], v6);
            m.insert(
                inode,
                Socket {
                    proto,
                    local: addr(f[1], v6),
                    remote: if remote.ends_with(":0") {
                        String::new()
                    } else {
                        remote
                    },
                    state,
                    tx_queue: u64::from_str_radix(queues.0, 16).unwrap_or(0),
                    rx_queue: u64::from_str_radix(queues.1, 16).unwrap_or(0),
                    inode,
                },
            );
        }
    }
    if let Ok(text) = fs::read_to_string("/proc/net/unix") {
        for line in text.lines().skip(1) {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 7 {
                continue;
            }
            let inode: u64 = f[6].parse().unwrap_or(0);
            if inode == 0 {
                continue;
            }
            // State 01 is unconnected, which for a Unix socket with a path
            // means it is listening.
            let listening = f[5] == "01" && f.len() > 7;
            m.insert(
                inode,
                Socket {
                    proto: "unix",
                    local: f.get(7).copied().unwrap_or("(anonymous)").to_string(),
                    remote: String::new(),
                    state: if listening { "LISTEN" } else { "CONNECTED" },
                    tx_queue: 0,
                    rx_queue: 0,
                    inode,
                },
            );
        }
    }
    m
}

/// `0100007F:0035` is little-endian hexadecimal: 127.0.0.1 port 53.
fn addr(s: &str, v6: bool) -> String {
    let Some((a, p)) = s.split_once(':') else {
        return s.to_string();
    };
    let port = u16::from_str_radix(p, 16).unwrap_or(0);
    if v6 {
        // Four little-endian 32-bit words, most significant group first.
        let mut groups = [0u16; 8];
        for w in 0..4 {
            let Some(word) = a.get(w * 8..w * 8 + 8) else {
                return s.to_string();
            };
            let v = u32::from_str_radix(word, 16).unwrap_or(0).swap_bytes();
            groups[w * 2] = (v >> 16) as u16;
            groups[w * 2 + 1] = (v & 0xffff) as u16;
        }
        if groups[..5].iter().all(|g| *g == 0) && groups[5] == 0xffff {
            // An IPv4-mapped address reads better as IPv4.
            return format!(
                "{}.{}.{}.{}:{port}",
                groups[6] >> 8,
                groups[6] & 0xff,
                groups[7] >> 8,
                groups[7] & 0xff
            );
        }
        let body = groups
            .iter()
            .map(|g| format!("{g:x}"))
            .collect::<Vec<_>>()
            .join(":");
        format!("[{body}]:{port}")
    } else {
        let v = u32::from_str_radix(a, 16).unwrap_or(0).swap_bytes();
        format!(
            "{}.{}.{}.{}:{port}",
            v >> 24,
            (v >> 16) & 0xff,
            (v >> 8) & 0xff,
            v & 0xff
        )
    }
}

fn tcp_state(hex: &str, udp: bool) -> &'static str {
    if udp {
        return if hex == "07" {
            "UNCONNECTED"
        } else {
            "CONNECTED"
        };
    }
    match hex {
        "01" => "ESTABLISHED",
        "02" => "SYN_SENT",
        "03" => "SYN_RECV",
        "04" => "FIN_WAIT1",
        "05" => "FIN_WAIT2",
        "06" => "TIME_WAIT",
        "07" => "CLOSE",
        "08" => "CLOSE_WAIT",
        "09" => "LAST_ACK",
        "0A" => "LISTEN",
        "0B" => "CLOSING",
        _ => "UNKNOWN",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hex_address_is_little_endian() {
        assert_eq!(addr("0100007F:0035", false), "127.0.0.1:53");
        assert_eq!(addr("00000000:1F90", false), "0.0.0.0:8080");
    }

    #[test]
    fn an_ipv4_mapped_v6_address_reads_as_ipv4() {
        assert_eq!(
            addr("0000000000000000FFFF00000100007F:0050", true),
            "127.0.0.1:80"
        );
    }

    #[test]
    fn tcp_states_are_named() {
        assert_eq!(tcp_state("0A", false), "LISTEN");
        assert_eq!(tcp_state("01", false), "ESTABLISHED");
        assert_eq!(tcp_state("07", true), "UNCONNECTED");
    }
}
