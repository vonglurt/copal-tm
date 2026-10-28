// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Paul Richeson
//! macOS: the same readings as `linux`, from the kernel's own calls instead of
//! `/proc`.  No crates; every call is declared here, beside the layout it
//! fills, so the offsets can be checked against `<sys/proc_info.h>`,
//! `<mach/vm_statistics.h>` and `<net/if_var.h>` in one place.
//!
//! What macOS gives an unprivileged user, and what it does not:
//!
//! - every core's ticks (`host_processor_info`), memory (`host_statistics64`),
//!   swap, load and boot time (`sysctl`), the network (`getifaddrs`), the
//!   battery (`pmset`);
//! - every process's name, parent, group, owner and start time
//!   (`sysctl KERN_PROC`, as `ps` reads them), and for the user's own processes their CPU time,
//!   memory, threads and priority (`PROC_PIDTASKINFO`), command line and
//!   environment (`KERN_PROCARGS2`);
//! - no temperatures, GPU load, pressure stall or per-disk counters without
//!   IOKit or root.  Those stay empty, which the panel shows as absent, not
//!   as zero.

use super::types::*;
use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_void, CStr};

const P_BATTERY: f64 = 5.0;
const P_SLOW: f64 = 60.0;

extern "C" {
    static mach_task_self_: u32;
    fn mach_host_self() -> u32;
    fn host_processor_info(
        host: u32,
        flavor: c_int,
        count: *mut u32,
        info: *mut *mut i32,
        info_count: *mut u32,
    ) -> c_int;
    fn host_statistics64(host: u32, flavor: c_int, info: *mut i32, count: *mut u32) -> c_int;
    fn vm_deallocate(task: u32, addr: usize, size: usize) -> c_int;
    fn mach_timebase_info(info: *mut [u32; 2]) -> c_int;
    fn sysctlbyname(
        name: *const c_char,
        old: *mut c_void,
        oldlen: *mut usize,
        new: *const c_void,
        newlen: usize,
    ) -> c_int;
    fn sysctl(
        name: *const c_int,
        namelen: u32,
        old: *mut c_void,
        oldlen: *mut usize,
        new: *const c_void,
        newlen: usize,
    ) -> c_int;
    fn getloadavg(load: *mut f64, n: c_int) -> c_int;
    fn proc_pidinfo(pid: c_int, flavor: c_int, arg: u64, buf: *mut c_void, size: c_int) -> c_int;
    fn proc_pidpath(pid: c_int, buf: *mut c_void, size: u32) -> c_int;
    fn getsid(pid: c_int) -> c_int;
    fn getpwuid(uid: u32) -> *const Passwd;
    fn getifaddrs(out: *mut *mut IfAddrs) -> c_int;
    fn freeifaddrs(p: *mut IfAddrs);
}

// Only the first field is read; the rest of `struct passwd` is never touched.
#[repr(C)]
struct Passwd {
    pw_name: *const c_char,
}

#[repr(C)]
struct IfAddrs {
    next: *mut IfAddrs,
    name: *const c_char,
    flags: u32,
    addr: *const u8,
    netmask: *const u8,
    dstaddr: *const u8,
    data: *const u8,
}

const PROCESSOR_CPU_LOAD_INFO: c_int = 2;
const HOST_VM_INFO64: c_int = 4;
const PROC_PIDTBSDINFO: c_int = 3;
const PROC_PIDTASKINFO: c_int = 4;
const AF_LINK: u8 = 18;
const IFF_UP: u32 = 0x1;
const IFF_LOOPBACK: u32 = 0x8;

/// `struct kinfo_proc`, 648 bytes on a 64-bit Mac.
const KINFO_SIZE: usize = 648;
/// `struct proc_bsdinfo`, 136 bytes.
const BSD_SIZE: usize = 136;
/// `struct proc_taskinfo`, 96 bytes.
const TASK_SIZE: usize = 96;

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_ne_bytes(b[o..o + 4].try_into().unwrap())
}
fn i32_at(b: &[u8], o: usize) -> i32 {
    i32::from_ne_bytes(b[o..o + 4].try_into().unwrap())
}
fn u64_at(b: &[u8], o: usize) -> u64 {
    u64::from_ne_bytes(b[o..o + 8].try_into().unwrap())
}

fn sysctl_raw(name: &str, buf: &mut [u8]) -> Option<usize> {
    let c = std::ffi::CString::new(name).ok()?;
    let mut len = buf.len();
    let r = unsafe {
        sysctlbyname(
            c.as_ptr(),
            buf.as_mut_ptr() as *mut c_void,
            &mut len,
            std::ptr::null(),
            0,
        )
    };
    (r == 0).then_some(len)
}

fn sysctl_u64(name: &str) -> Option<u64> {
    let mut b = [0u8; 8];
    match sysctl_raw(name, &mut b)? {
        4 => Some(u32_at(&b, 0) as u64),
        8 => Some(u64_at(&b, 0)),
        _ => None,
    }
}

fn sysctl_str(name: &str) -> Option<String> {
    let mut b = [0u8; 256];
    let n = sysctl_raw(name, &mut b)?;
    let s = CStr::from_bytes_until_nul(&b[..n]).ok()?;
    Some(s.to_string_lossy().into_owned())
}

fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

/// A core's ticks: user, system, idle, nice.
type Ticks = [u64; 4];

pub struct Backend {
    prev_cpu: Vec<Ticks>,
    prev_net: HashMap<String, [u32; 6]>,
    prev_proc: HashMap<i32, (f64, f64)>,
    remote: HashMap<i32, (f64, Option<String>)>,
    users: HashMap<u32, String>,
    users_at: f64,
    power: Power,
    power_at: f64,
    /// Nanoseconds per unit of `pti_total_user`: 1 on Intel, 125/3 on M1.
    timebase: f64,
    boot: f64,
    page: u64,
    total_mem: u64,
    model: String,
    classes: Vec<CoreClass>,
    class_names: Vec<String>,
    argmax: usize,
}

impl Default for Backend {
    fn default() -> Self {
        Self::new()
    }
}

impl Backend {
    pub fn new() -> Backend {
        let mut tb = [0u32; 2];
        unsafe { mach_timebase_info(&mut tb) };
        let timebase = if tb[1] == 0 {
            1.0
        } else {
            tb[0] as f64 / tb[1] as f64
        };
        let mut bt = [0u8; 16];
        let boot = sysctl_raw("kern.boottime", &mut bt)
            .map(|_| u64_at(&bt, 0) as f64 + i32_at(&bt, 8) as f64 / 1e6)
            .unwrap_or(0.0);

        // Apple silicon numbers its efficiency cluster first (cpu0 to cpu3 on
        // an M1, by `ioreg`'s cluster-type), so the slower level's count of
        // logical processors leads.  Level 0 is the fastest, as in `Cpu`.
        let ncpu = sysctl_u64("hw.logicalcpu").unwrap_or(1) as usize;
        let levels = sysctl_u64("hw.nperflevels").unwrap_or(1) as usize;
        let mut class_names = Vec::new();
        let mut classes = vec![CoreClass::default(); ncpu];
        if levels > 1 {
            let mut at = 0;
            for lvl in (0..levels).rev() {
                let n = sysctl_u64(&format!("hw.perflevel{lvl}.logicalcpu")).unwrap_or(0) as usize;
                for c in classes.iter_mut().skip(at).take(n) {
                    c.class = lvl as u8;
                }
                at += n;
            }
            for lvl in 0..levels {
                class_names.push(
                    sysctl_str(&format!("hw.perflevel{lvl}.name"))
                        .unwrap_or_else(|| format!("level {lvl}")),
                );
            }
        }

        Backend {
            prev_cpu: Vec::new(),
            prev_net: HashMap::new(),
            prev_proc: HashMap::new(),
            remote: HashMap::new(),
            users: HashMap::new(),
            users_at: f64::MIN,
            power: Power::default(),
            power_at: f64::MIN,
            timebase,
            boot,
            page: sysctl_u64("hw.pagesize").unwrap_or(4096),
            total_mem: sysctl_u64("hw.memsize").unwrap_or(0),
            model: sysctl_str("machdep.cpu.brand_string").unwrap_or_default(),
            classes,
            class_names,
            argmax: sysctl_u64("kern.argmax").unwrap_or(262_144) as usize,
        }
    }

    pub fn sample(&mut self, now: f64, dt: f64, visible: &[i32]) -> Snapshot {
        let mut s = Snapshot {
            t: now,
            dt,
            simulated: false,
            ..Default::default()
        };
        self.cpu(&mut s);
        s.mem = self.mem();
        self.net(&mut s, dt);
        let mut l = [0f64; 3];
        if unsafe { getloadavg(l.as_mut_ptr(), 3) } == 3 {
            s.load = [l[0] as f32, l[1] as f32, l[2] as f32];
        }
        s.uptime = (wall() - self.boot).max(0.0);
        if now - self.power_at >= P_BATTERY {
            self.power = power();
            self.power_at = now;
        }
        s.power = self.power.clone();
        self.procs(&mut s, dt, visible, now);
        s
    }

    fn cpu(&mut self, s: &mut Snapshot) {
        let (mut n, mut info, mut len) = (0u32, std::ptr::null_mut::<i32>(), 0u32);
        let r = unsafe {
            host_processor_info(
                mach_host_self(),
                PROCESSOR_CPU_LOAD_INFO,
                &mut n,
                &mut info,
                &mut len,
            )
        };
        if r != 0 || info.is_null() {
            return;
        }
        let raw = unsafe { std::slice::from_raw_parts(info as *const u32, len as usize) };
        let cur: Vec<Ticks> = raw
            .chunks_exact(4)
            .map(|c| [c[0] as u64, c[1] as u64, c[2] as u64, c[3] as u64])
            .collect();
        unsafe { vm_deallocate(mach_task_self_, info as usize, len as usize * 4) };

        s.cpu.model = self.model.clone();
        s.cpu.class_names = self.class_names.clone();
        if !self.class_names.is_empty() && self.classes.len() == cur.len() {
            s.cpu.classes = self.classes.clone();
        }
        // The counters are 32-bit and wrap; a difference taken wrapping is
        // right across one wrap, and a tick never spans two.
        let d = |a: &Ticks, b: &Ticks, i: usize| (a[i] as u32).wrapping_sub(b[i] as u32) as u64;
        if self.prev_cpu.len() == cur.len() {
            let (mut busy, mut sys, mut all) = (0u64, 0u64, 0u64);
            for (now, was) in cur.iter().zip(&self.prev_cpu) {
                let (u, sy, id, ni) = (
                    d(now, was, 0),
                    d(now, was, 1),
                    d(now, was, 2),
                    d(now, was, 3),
                );
                let t = u + sy + id + ni;
                s.cpu.per_core.push(if t == 0 {
                    0.0
                } else {
                    ((u + sy + ni) as f32 / t as f32).clamp(0.0, 1.0)
                });
                busy += u + sy + ni;
                sys += sy;
                all += t;
            }
            if all > 0 {
                s.cpu.total = busy as f32 / all as f32;
                s.cpu.kernel = sys as f32 / all as f32;
            }
        } else {
            s.cpu.per_core = vec![0.0; cur.len()];
        }
        self.prev_cpu = cur;
    }

    /// Used is what Activity Monitor calls it: app memory, wired and
    /// compressed.  Available is the rest; cached is the file-backed pages.
    fn mem(&self) -> Mem {
        let mut v = [0i32; 38];
        let mut count = v.len() as u32;
        let mut m = Mem {
            total_kb: self.total_mem / 1024,
            ..Default::default()
        };
        let r = unsafe {
            host_statistics64(mach_host_self(), HOST_VM_INFO64, v.as_mut_ptr(), &mut count)
        };
        if r == 0 {
            let b: Vec<u8> = v.iter().flat_map(|x| x.to_ne_bytes()).collect();
            let wired = u32_at(&b, 12) as u64;
            let purgeable = u32_at(&b, 88) as u64;
            let compressed = u32_at(&b, 128) as u64;
            let external = u32_at(&b, 136) as u64;
            let internal = u32_at(&b, 140) as u64;
            let used = (internal.saturating_sub(purgeable) + wired + compressed) * self.page;
            m.avail_kb = self.total_mem.saturating_sub(used) / 1024;
            m.cached_kb = (external + purgeable) * self.page / 1024;
        }
        // struct xsw_usage: total, avail, used, then the page size.
        let mut sw = [0u8; 32];
        if sysctl_raw("vm.swapusage", &mut sw).is_some() {
            m.swap_total_kb = u64_at(&sw, 0) / 1024;
            m.swap_used_kb = u64_at(&sw, 16) / 1024;
        }
        m
    }

    /// Every interface that is up and not the loopback, summed; the busiest
    /// one names the line.  `if_data`'s byte counters are 32-bit, so each is
    /// differenced wrapping, per interface.
    fn net(&mut self, s: &mut Snapshot, dt: f64) {
        let mut head: *mut IfAddrs = std::ptr::null_mut();
        if unsafe { getifaddrs(&mut head) } != 0 {
            return;
        }
        let mut cur: HashMap<String, [u32; 6]> = HashMap::new();
        let mut link: HashMap<String, u64> = HashMap::new();
        let mut p = head;
        while !p.is_null() {
            let a = unsafe { &*p };
            p = a.next;
            if a.addr.is_null()
                || a.data.is_null()
                || a.flags & IFF_UP == 0
                || a.flags & IFF_LOOPBACK != 0
            {
                continue;
            }
            if unsafe { *a.addr.add(1) } != AF_LINK {
                continue;
            }
            let name = unsafe { CStr::from_ptr(a.name) }
                .to_string_lossy()
                .into_owned();
            // struct if_data: ipackets 20, ierrors 24, opackets 28,
            // oerrors 32, ibytes 40, obytes 44; baudrate 16.
            let d = unsafe { std::slice::from_raw_parts(a.data, 48) };
            cur.insert(
                name.clone(),
                [
                    u32_at(d, 40),
                    u32_at(d, 44),
                    u32_at(d, 20),
                    u32_at(d, 28),
                    u32_at(d, 24),
                    u32_at(d, 32),
                ],
            );
            link.insert(name, u32_at(d, 16) as u64);
        }
        unsafe { freeifaddrs(head) };

        let (mut rx, mut tx, mut pk, mut er) = (0u64, 0u64, 0u64, 0u64);
        let mut best = (0u64, String::new());
        for (name, now) in &cur {
            let Some(was) = self.prev_net.get(name) else {
                continue;
            };
            let d = |i: usize| now[i].wrapping_sub(was[i]) as u64;
            rx += d(0);
            tx += d(1);
            pk += d(2) + d(3);
            er += d(4) + d(5);
            let moved = d(0) + d(1);
            if best.1.is_empty() || moved > best.0 || (moved == best.0 && name.starts_with("en")) {
                best = (moved, name.clone());
            }
        }
        if !best.1.is_empty() && dt > 0.0 {
            s.net.rx_bps = rx as f64 / dt;
            s.net.tx_bps = tx as f64 / dt;
            s.net.err_rate = if pk == 0 { 0.0 } else { er as f32 / pk as f32 };
            s.net.link_mbps = link.get(&best.1).filter(|&&b| b > 0).map(|b| b / 1_000_000);
            s.net.iface = best.1;
        }
        self.prev_net = cur;
    }

    fn procs(&mut self, s: &mut Snapshot, dt: f64, visible: &[i32], now: f64) {
        if now - self.users_at >= P_SLOW {
            self.users.clear();
            self.users_at = now;
        }
        let all = kinfo(&[1, 14, 0]);
        let mut seen = HashMap::new();
        let mut remote = HashMap::new();
        let mut out = Vec::with_capacity(all.len() / KINFO_SIZE);
        for k in all.chunks_exact(KINFO_SIZE) {
            let mut p = self.read(k);
            let pid = p.pid;
            if let Some(&(prev_time, prev_start)) = self.prev_proc.get(&pid) {
                if prev_start == p.start_s && dt > 0.0 {
                    p.cpu = ((p.cpu_time - prev_time) / dt).clamp(0.0, 64.0) as f32;
                }
            }
            seen.insert(pid, (p.cpu_time, p.start_s));
            // Read once a process: its environment is fixed at exec.
            p.remote = match self.remote.get(&pid) {
                Some((start, r)) if *start == p.start_s => r.clone(),
                _ if p.kernel_thread => None,
                _ => self.args(pid).and_then(|(_, env)| ssh_client(&env)),
            };
            remote.insert(pid, (p.start_s, p.remote.clone()));
            if visible.contains(&pid) && !p.kernel_thread {
                p.cmdline = self
                    .args(pid)
                    .map(|(argv, _)| argv.join(" "))
                    .unwrap_or_default();
            }
            out.push(p);
        }
        self.prev_proc = seen;
        self.remote = remote;
        s.procs = out;
    }

    /// One process, from its `kinfo_proc`, which `sysctl` gives for every
    /// process to anyone; then `PROC_PIDTASKINFO` and the 32-byte name of
    /// `PROC_PIDTBSDINFO` on top, which macOS gives only for the user's own
    /// processes (or all of them to root - `ps` is setuid for this).  Another
    /// user's process is marked `unread` rather than given a guess.
    fn read(&mut self, k: &[u8]) -> Proc {
        // struct extern_proc: p_starttime 0, p_stat 36, p_pid 40, p_nice 242,
        // p_comm 243; struct eproc at 296: p_ruid 392, cr_uid 420,
        // e_ppid 560, e_pgid 564, e_tdev 572.  Checked against
        // PROC_PIDT_SHORTBSDINFO on every process of an M1.
        let pid = i32_at(k, 40);
        let uid = u32_at(k, 420);
        let tdev = u32_at(k, 572);
        let start = u64_at(k, 0) as f64 + i32_at(k, 8) as f64 / 1e6;
        let mut p = Proc {
            pid,
            ppid: i32_at(k, 560),
            pgid: i32_at(k, 564),
            sid: unsafe { getsid(pid) }.max(0),
            tty_nr: if tdev == u32::MAX { 0 } else { tdev as i32 },
            name: cstr(&k[243..260]),
            uid,
            user: self.user(uid),
            nice: k[242] as i8 as i32,
            start_s: (start - self.boot).max(0.0),
            kernel_thread: pid == 0,
            ..Default::default()
        };

        let mut b = [0u8; BSD_SIZE];
        let r = unsafe {
            proc_pidinfo(
                pid,
                PROC_PIDTBSDINFO,
                0,
                b.as_mut_ptr() as *mut c_void,
                BSD_SIZE as c_int,
            )
        };
        if r as usize == BSD_SIZE {
            let long = cstr(&b[64..96]);
            if !long.is_empty() {
                p.name = long;
            }
        }
        let mut t = [0u8; TASK_SIZE];
        let r = unsafe {
            proc_pidinfo(
                pid,
                PROC_PIDTASKINFO,
                0,
                t.as_mut_ptr() as *mut c_void,
                TASK_SIZE as c_int,
            )
        };
        let mut running = false;
        p.unread = r as usize != TASK_SIZE;
        if !p.unread {
            p.vsz_kb = u64_at(&t, 0) / 1024;
            p.rss_kb = u64_at(&t, 8) / 1024;
            p.cpu_time = (u64_at(&t, 16) + u64_at(&t, 24)) as f64 * self.timebase / 1e9;
            p.threads = i32_at(&t, 84);
            running = i32_at(&t, 88) > 0;
            p.prio = i32_at(&t, 92);
        }
        // p_stat is the process's, and is SRUN for nearly everything alive;
        // `ps` reads the threads, and so does this where it may.
        p.state = match k[36] {
            4 => 'T',
            5 => 'Z',
            _ if running => 'R',
            _ => 'S',
        };
        p
    }

    fn user(&mut self, uid: u32) -> String {
        self.users
            .entry(uid)
            .or_insert_with(|| {
                let pw = unsafe { getpwuid(uid) };
                if pw.is_null() || unsafe { (*pw).pw_name }.is_null() {
                    uid.to_string()
                } else {
                    unsafe { CStr::from_ptr((*pw).pw_name) }
                        .to_string_lossy()
                        .into_owned()
                }
            })
            .clone()
    }

    /// `KERN_PROCARGS2`: argc, the executable's path, padding, then argv and
    /// the environment, each NUL-terminated.  Only the user's own processes
    /// answer, or everyone's as root.
    fn args(&self, pid: i32) -> Option<(Vec<String>, Vec<String>)> {
        let mib = [1 as c_int, 49, pid];
        let mut buf = vec![0u8; self.argmax];
        let mut len = buf.len();
        let r = unsafe {
            sysctl(
                mib.as_ptr(),
                3,
                buf.as_mut_ptr() as *mut c_void,
                &mut len,
                std::ptr::null(),
                0,
            )
        };
        if r != 0 || len < 4 {
            return None;
        }
        let argc = i32_at(&buf, 0).max(0) as usize;
        let body = &buf[4..len];
        let mut parts = body.split(|&c| c == 0).skip(1).filter(|x| !x.is_empty());
        let argv: Vec<String> = parts
            .by_ref()
            .take(argc)
            .map(|x| String::from_utf8_lossy(x).into_owned())
            .collect();
        let env: Vec<String> = parts
            .map(|x| String::from_utf8_lossy(x).into_owned())
            .collect();
        Some((argv, env))
    }

    pub fn detail(&mut self, pid: i32) -> Option<Detail> {
        let k = kinfo(&[1, 14, 1, pid]);
        if k.len() < KINFO_SIZE {
            return None;
        }
        let mut path = [0u8; 4096];
        let n = unsafe { proc_pidpath(pid, path.as_mut_ptr() as *mut c_void, path.len() as u32) };
        let (argv, env) = self.args(pid).unwrap_or_default();
        Some(Detail {
            pid,
            cmdline: argv.join(" "),
            exe: if n > 0 {
                cstr(&path[..n as usize])
            } else {
                String::new()
            },
            env_count: env.len(),
            uid_real: u32_at(&k, 392),
            uid_eff: u32_at(&k, 420),
            gid_real: u32_at(&k, 400),
            gid_eff: u32_at(&k, 428),
            ..Default::default()
        })
    }
}

/// `sysctl` on a `CTL_KERN, KERN_PROC` name: one `kinfo_proc` a process,
/// with room for processes born between asking the size and reading.
fn kinfo(mib: &[c_int]) -> Vec<u8> {
    let mut len = 0usize;
    let n = mib.len() as u32;
    unsafe {
        sysctl(
            mib.as_ptr(),
            n,
            std::ptr::null_mut(),
            &mut len,
            std::ptr::null(),
            0,
        )
    };
    let mut buf = vec![0u8; len + 64 * KINFO_SIZE];
    let mut len = buf.len();
    let r = unsafe {
        sysctl(
            mib.as_ptr(),
            n,
            buf.as_mut_ptr() as *mut c_void,
            &mut len,
            std::ptr::null(),
            0,
        )
    };
    buf.truncate(if r == 0 { len } else { 0 });
    buf
}

fn wall() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// The client address in `SSH_CONNECTION`, as on Linux.
fn ssh_client(env: &[String]) -> Option<String> {
    env.iter()
        .find_map(|e| e.strip_prefix("SSH_CONNECTION="))
        .and_then(|v| v.split_whitespace().next())
        .map(str::to_string)
}

/// `pmset -g batt`: "Now drawing from 'Battery Power'", then a line per
/// battery, "-InternalBattery-0 (id=...)\t85%; discharging; 3:12 remaining".
/// A desktop Mac has the first line and no battery.
fn power() -> Power {
    let out = std::process::Command::new("pmset")
        .args(["-g", "batt"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let mut p = Power {
        on_battery: out.contains("'Battery Power'"),
        ..Default::default()
    };
    let Some(line) = out.lines().find(|l| l.contains("InternalBattery")) else {
        p.on_battery = false;
        return p;
    };
    p.present = true;
    let rest = line.split('\t').nth(1).unwrap_or("");
    let mut fields = rest.split(';').map(str::trim);
    p.charge = fields
        .next()
        .and_then(|c| c.strip_suffix('%'))
        .and_then(|c| c.parse::<f32>().ok())
        .map(|c| c / 100.0);
    p.status = fields.next().unwrap_or("").to_string();
    p.remaining_s = fields
        .next()
        .and_then(|r| r.split_whitespace().next())
        .and_then(|hm| hm.split_once(':'))
        .and_then(|(h, m)| Some(h.parse::<f64>().ok()? * 3600.0 + m.parse::<f64>().ok()? * 60.0));
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_reads_as_itself() {
        let mut b = Backend::new();
        let me = std::process::id() as i32;
        let k = kinfo(&[1, 14, 1, me]);
        let p = b.read(&k);
        assert_eq!(p.pid, me);
        assert!(p.rss_kb > 0, "our resident memory");
        assert!(p.threads >= 1);
        assert!(p.start_s > 0.0);
        let (argv, _) = b.args(me).expect("our own arguments");
        assert!(!argv.is_empty());
        assert!(b.detail(me).unwrap().exe.contains("copal"));
    }

    #[test]
    fn two_ticks_read_the_machine() {
        let mut b = Backend::new();
        b.sample(0.0, 1.0, &[]);
        // Something to measure.
        let t = std::time::Instant::now();
        let mut x = 0u64;
        while t.elapsed().as_millis() < 200 {
            x = x.wrapping_mul(31).wrapping_add(7);
        }
        std::hint::black_box(x);
        let s = b.sample(0.2, 0.2, &[std::process::id() as i32]);
        assert_eq!(
            s.cpu.per_core.len(),
            sysctl_u64("hw.logicalcpu").unwrap() as usize
        );
        assert!(s.cpu.total > 0.0);
        assert!(s.mem.total_kb > 0 && s.mem.avail_kb <= s.mem.total_kb);
        assert!(s.uptime > 0.0);
        let me = s
            .procs
            .iter()
            .find(|p| p.pid == std::process::id() as i32)
            .unwrap();
        assert!(me.cpu > 0.1, "a busy test shows its CPU: {}", me.cpu);
        assert!(!me.cmdline.is_empty());
        assert!(s.procs.iter().any(|p| p.pid == 1 && p.name == "launchd"));
    }
}
