// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Paul Richeson
//! The simulated back end.
//!
//! The target is Alpine; the desk this is written at is a Mac, which has no
//! `/proc` at all.  Rather than develop the interface blind and carry it to a
//! virtual machine to look at it, the probe has a second back end that
//! produces plausible, seeded, slowly drifting readings and a synthetic
//! process tree.  `make demo` runs against it on either machine.
//!
//! The tree is not random.  It contains, deliberately, every case the halt
//! ladder of Section VIII has to reason about: a shell-wrapped child, a
//! supervised OpenRC daemon, a zombie waiting to be reaped, a process in
//! uninterruptible sleep, a stopped process, a process with a `SIGTERM`
//! handler installed, and one owned by root.  So the feature can be
//! rehearsed, and tested, on a machine that has none of them.
//!
//! It is never silent about being a simulation: `Snapshot::simulated` is set,
//! the status bar shows `SIMULATED`, and the first Transcript line says so.

use crate::signals::SigMask;
use crate::types::*;

/// A small linear congruential generator.  Numerical Recipes' constants; it
/// only has to look like noise, not resist anybody.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.0 >> 16) as u32
    }
    fn unit(&mut self) -> f32 {
        (self.next() % 100_000) as f32 / 100_000.0
    }
    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + self.unit() * (hi - lo)
    }
}

/// One quantity that wanders: a random walk pulled back toward a rest value,
/// with an occasional burst, so histories have both a texture and events.
struct Drift {
    v: f32,
    rest: f32,
    pull: f32,
    step: f32,
    burst: f32,
}

impl Drift {
    fn new(rest: f32, pull: f32, step: f32) -> Drift {
        Drift {
            v: rest,
            rest,
            pull,
            step,
            burst: 0.0,
        }
    }
    fn tick(&mut self, r: &mut Lcg) -> f32 {
        if self.burst > 0.0 {
            self.burst -= 1.0;
            self.v += self.step * 3.0;
        } else if r.unit() < 0.012 {
            self.burst = r.range(2.0, 7.0);
        }
        self.v += (self.rest - self.v) * self.pull + r.range(-self.step, self.step);
        self.v = self.v.clamp(0.0, 1.0);
        self.v
    }
}

pub struct Backend {
    r: Lcg,
    cpu: Drift,
    kernel: Drift,
    cores: Vec<Drift>,
    mem: Drift,
    net: Drift,
    disk: Drift,
    gpu: Drift,
    temp: Drift,
    charge: f32,
    table: Vec<Proc>,
    procs_cpu: Vec<Drift>,
}

impl Default for Backend {
    fn default() -> Self {
        Self::new()
    }
}

impl Backend {
    pub fn new() -> Backend {
        let mut r = Lcg(0xA5C1_771E);
        let cores = (0..10)
            .map(|_| Drift::new(r.range(0.05, 0.25), 0.08, 0.10))
            .collect();
        let table = synthetic_tree();
        let procs_cpu = table
            .iter()
            .map(|p| {
                // Most processes sit at nothing and stay there, which is what
                // a real table looks like; a few are busy. A simulation where
                // everything wanders equally would hide the rank-stability
                // rule rather than exercise it.
                if p.kernel_thread || p.state == 'Z' {
                    Drift::new(0.0, 1.0, 0.0)
                } else if r.unit() < 0.18 {
                    Drift::new(r.range(0.02, 0.30), 0.05, 0.012)
                } else {
                    Drift::new(r.range(0.0, 0.004), 0.20, 0.0015)
                }
            })
            .collect();
        Backend {
            cpu: Drift::new(0.11, 0.06, 0.05),
            kernel: Drift::new(0.03, 0.10, 0.02),
            mem: Drift::new(0.72, 0.03, 0.015),
            net: Drift::new(0.04, 0.05, 0.05),
            disk: Drift::new(0.024, 0.08, 0.03),
            gpu: Drift::new(0.08, 0.05, 0.04),
            temp: Drift::new(0.32, 0.04, 0.012),
            charge: 0.87,
            cores,
            table,
            procs_cpu,
            r,
        }
    }

    pub fn detail(&mut self, pid: i32) -> Option<Detail> {
        let p = self.table.iter().find(|p| p.pid == pid)?;
        Some(Detail {
            pid,
            cmdline: p.cmdline.clone(),
            exe: format!("/usr/bin/{}", p.name),
            exe_deleted: p.name == "nginx",
            cwd: if p.uid == 0 {
                "/".into()
            } else {
                "/home/copal".into()
            },
            cgroup: match &p.supervision {
                Supervision::OpenRc(n) => format!("/openrc.{n}"),
                Supervision::Systemd(n) => format!("/system.slice/{n}.service"),
                Supervision::None => "/".into(),
            },
            env_count: 34,
            fds: (7, 3, 4, 2),
            uid_real: p.uid,
            uid_eff: p.uid,
            gid_real: p.uid,
            gid_eff: p.uid,
            cap_eff: if p.uid == 0 { 0x1ff_ffff_ffff } else { 0 },
            seccomp: 0,
            ctx_vol: 40_000 + p.pid as u64 * 37,
            ctx_invol: 900 + p.pid as u64,
            sig: p.sig.unwrap_or_default(),
            sockets: synthetic_sockets(p),
        })
    }

    pub fn sample(&mut self, now: f64, dt: f64, _visible: &[i32]) -> Snapshot {
        let mut s = Snapshot {
            t: now,
            dt,
            simulated: true,
            ..Default::default()
        };

        let total = self.cpu.tick(&mut self.r);
        s.cpu.total = total;
        s.cpu.kernel = (self.kernel.tick(&mut self.r) * 0.6).min(total);
        s.cpu.iowait = s.cpu.kernel * 0.2;
        s.cpu.per_core = {
            let r = &mut self.r;
            self.cores
                .iter_mut()
                .map(|c| (c.tick(r) * 0.6 + total * 0.6).min(1.0))
                .collect()
        };
        s.cpu.freq_khz = s
            .cpu
            .per_core
            .iter()
            .map(|c| (1_800_000.0 + c * 2_200_000.0) as u64)
            .collect();
        s.cpu.model = "Simulated 10-core (6P + 4E)".into();
        s.cpu.governor = Some("schedutil".into());
        s.cpu.class_names = vec!["P".into(), "E".into()];
        s.cpu.classes = (0..10)
            .map(|i| CoreClass {
                class: if i < 6 { 0 } else { 1 },
                sibling: false,
            })
            .collect();

        let mf = self.mem.tick(&mut self.r);
        s.mem.total_kb = 32 * 1024 * 1024;
        s.mem.avail_kb = ((1.0 - mf) * s.mem.total_kb as f32) as u64;
        s.mem.cached_kb = 6_600_000;
        s.mem.swap_total_kb = 4 * 1024 * 1024;
        s.mem.swap_used_kb = 422_000;

        let n = self.net.tick(&mut self.r);
        s.net.rx_bps = (n * 90_000.0) as f64;
        s.net.tx_bps = (n * 24_000.0) as f64;
        s.net.link_mbps = Some(404);
        s.net.iface = "eth0".into();

        let d = self.disk.tick(&mut self.r);
        s.disk.busy = d;
        s.disk.read_bps = (d * 3_600_000.0) as f64;
        s.disk.write_bps = (d * 3_900_000.0) as f64;
        s.disk.name = "sda".into();
        s.disk.smart = Some("Healthy".into());

        s.gpu = Some(self.gpu.tick(&mut self.r));

        let t = self.temp.tick(&mut self.r) * 110.0;
        s.thermal.scale_max = 110.0;
        s.thermal.zones = vec![("soc".into(), t), ("nvme".into(), t * 0.82)];
        s.thermal.hottest_c = Some(t);
        s.thermal.hottest_name = "soc".into();

        s.psi = Psi {
            present: true,
            cpu10: total * 12.0,
            mem10: mf * 3.0,
            io10: d * 40.0,
        };
        s.load = [total * 10.0, total * 9.0, total * 8.0];
        s.uptime = 534_720.0 + now;

        // The battery drains slowly, so the SYS readout's lap time changes.
        self.charge = (self.charge - 0.000_02).max(0.05);
        let watts = 7.5 + total * 14.0;
        s.power = Power {
            present: true,
            on_battery: true,
            charge: Some(self.charge),
            watts: Some(watts),
            remaining_s: Some((self.charge * 60.0 * 3600.0 / watts) as f64),
            status: "Discharging".into(),
        };

        let r = &mut self.r;
        for (p, d) in self.table.iter_mut().zip(self.procs_cpu.iter_mut()) {
            p.cpu = d.tick(r);
            if p.state == 'Z' || p.kernel_thread {
                p.cpu = 0.0;
            }
            p.cpu_time += p.cpu as f64 * dt;
            p.rss_kb = (p.rss_kb as f32 * (1.0 + (r.unit() - 0.5) * 0.004)) as u64;
        }
        s.procs = self.table.clone();
        s
    }
}

/// The uid of whoever is running this. The simulated user processes belong to
/// them, so that the halt plan's privilege check behaves on the desk it is
/// being written at exactly as it will on the target.
fn my_uid() -> u32 {
    extern "C" {
        fn getuid() -> u32;
    }
    unsafe { getuid() }
}

/// Sockets for the simulated tree, so the Inspector's network section can be
/// developed on a machine whose `/proc/net` does not exist.
fn synthetic_sockets(p: &Proc) -> Vec<Socket> {
    let s = |proto: &'static str,
             local: &str,
             remote: &str,
             state: &'static str,
             tx: u64,
             rx: u64,
             inode: u64| Socket {
        proto,
        local: local.into(),
        remote: remote.into(),
        state,
        tx_queue: tx,
        rx_queue: rx,
        inode,
    };
    match p.name.as_str() {
        "nginx" => vec![
            s(
                "tcp",
                "0.0.0.0:80",
                "",
                "LISTEN",
                0,
                0,
                30_100 + p.pid as u64,
            ),
            s(
                "tcp",
                "0.0.0.0:443",
                "",
                "LISTEN",
                0,
                0,
                30_200 + p.pid as u64,
            ),
            s(
                "tcp",
                "10.0.0.7:443",
                "10.0.0.31:51820",
                "ESTABLISHED",
                0,
                1_448,
                30_300,
            ),
            s("unix", "/run/nginx/nginx.sock", "", "LISTEN", 0, 0, 30_400),
        ],
        "node" => vec![
            s("tcp", "0.0.0.0:8080", "", "LISTEN", 0, 0, 40_100),
            s(
                "tcp",
                "10.0.0.7:8080",
                "10.0.0.31:41002",
                "ESTABLISHED",
                8_192,
                0,
                40_101,
            ),
            s(
                "tcp",
                "10.0.0.7:44120",
                "151.101.1.140:443",
                "ESTABLISHED",
                0,
                0,
                40_102,
            ),
        ],
        "firefox" | "Web Content" => vec![
            s(
                "tcp",
                "10.0.0.7:44810",
                "151.101.1.140:443",
                "ESTABLISHED",
                0,
                0,
                50_100,
            ),
            s(
                "tcp",
                "10.0.0.7:44811",
                "140.82.121.4:443",
                "ESTABLISHED",
                0,
                2_896,
                50_101,
            ),
            s(
                "tcp6",
                "[fe80::1]:44812",
                "[2606:4700::1111]:443",
                "ESTABLISHED",
                0,
                0,
                50_102,
            ),
        ],
        "chronyd" => {
            vec![s("udp", "0.0.0.0:123", "", "UNCONNECTED", 0, 0, 60_100)]
        }
        "ytq" => vec![s(
            "tcp",
            "10.0.0.7:39114",
            "142.250.74.14:443",
            "ESTABLISHED",
            0,
            0,
            61_100,
        )],
        "Xorg" | "hyprland" => {
            vec![s(
                "unix",
                "/tmp/.X11-unix/X0",
                "",
                "LISTEN",
                0,
                0,
                70_100 + p.pid as u64,
            )]
        }
        _ => Vec::new(),
    }
}

/// The tree, chosen so that every branch of Section VIII is reachable here.
fn synthetic_tree() -> Vec<Proc> {
    let mut v = Vec::new();
    #[allow(non_snake_case)]
    let MINE = my_uid();
    let mut add = |pid: i32,
                   ppid: i32,
                   pgid: i32,
                   name: &str,
                   cmd: &str,
                   uid: u32,
                   user: &str,
                   rss_mb: u64,
                   state: char,
                   sig: SigMask,
                   sup: Supervision,
                   kern: bool| {
        v.push(Proc {
            pid,
            ppid,
            pgid,
            sid: 1,
            tty_nr: if kern { 0 } else { 34816 },
            state,
            name: name.into(),
            cmdline: if kern { String::new() } else { cmd.into() },
            uid,
            user: user.into(),
            nice: 0,
            prio: 20,
            threads: if kern { 1 } else { 4 },
            rss_kb: rss_mb * 1024,
            vsz_kb: rss_mb * 1024 * 6,
            cpu: 0.0,
            cpu_time: (pid as f64) * 0.7,
            start_s: 100.0 + pid as f64,
            kernel_thread: kern,
            supervision: sup,
            sig: Some(sig),
        });
    };

    // A shell that traps INT and TERM, which is what a shell does.
    let shell_mask = SigMask {
        ignored: 1 << 2,
        caught: (1 << 1) | (1 << 14),
        blocked: 0,
    };
    let none = SigMask::default();
    // A daemon that traps TERM (graceful shutdown) and HUP (reload).
    let daemon_mask = SigMask {
        ignored: 0,
        caught: (1 << 14) | 1,
        blocked: 0,
    };
    // Something that ignores TERM outright - the ladder must step past it.
    let stubborn = SigMask {
        ignored: 1 << 14,
        caught: 1 << 1,
        blocked: 0,
    };

    add(
        1,
        0,
        1,
        "init",
        "/sbin/init",
        0,
        "root",
        4,
        'S',
        none,
        Supervision::None,
        false,
    );
    add(
        2,
        0,
        0,
        "kthreadd",
        "",
        0,
        "root",
        0,
        'S',
        none,
        Supervision::None,
        true,
    );
    add(
        14,
        2,
        0,
        "kworker/0:1",
        "",
        0,
        "root",
        0,
        'I',
        none,
        Supervision::None,
        true,
    );
    add(
        21,
        2,
        0,
        "ksoftirqd/0",
        "",
        0,
        "root",
        0,
        'S',
        none,
        Supervision::None,
        true,
    );
    add(
        310,
        1,
        310,
        "nginx",
        "nginx: master process",
        0,
        "root",
        18,
        'S',
        daemon_mask,
        Supervision::OpenRc("nginx".into()),
        false,
    );
    add(
        311,
        310,
        310,
        "nginx",
        "nginx: worker process",
        100,
        "nginx",
        22,
        'S',
        daemon_mask,
        Supervision::OpenRc("nginx".into()),
        false,
    );
    add(
        402,
        1,
        402,
        "Xorg",
        "/usr/bin/Xorg :0 -seat seat0",
        0,
        "root",
        148,
        'S',
        none,
        Supervision::None,
        false,
    );
    add(
        610,
        1,
        610,
        "hyprland",
        "/usr/bin/Hyprland",
        MINE,
        "copal",
        312,
        'S',
        none,
        Supervision::None,
        false,
    );
    add(
        1127,
        610,
        1127,
        "foot",
        "foot -e /bin/ash",
        MINE,
        "copal",
        41,
        'S',
        none,
        Supervision::None,
        false,
    );
    add(
        4820,
        1127,
        4820,
        "sh",
        "/bin/sh -c node server.js",
        MINE,
        "copal",
        3,
        'S',
        shell_mask,
        Supervision::None,
        false,
    );
    add(
        4821,
        4820,
        4820,
        "node",
        "node server.js --port 8080",
        MINE,
        "copal",
        304,
        'S',
        daemon_mask,
        Supervision::None,
        false,
    );
    add(
        5140,
        610,
        5140,
        "firefox",
        "/usr/lib/firefox/firefox",
        MINE,
        "copal",
        511,
        'S',
        none,
        Supervision::None,
        false,
    );
    add(
        5166,
        5140,
        5140,
        "Web Content",
        "/usr/lib/firefox/firefox -contentproc -childID 3",
        MINE,
        "copal",
        459,
        'S',
        none,
        Supervision::None,
        false,
    );
    add(
        5188,
        5140,
        5140,
        "Web Content",
        "/usr/lib/firefox/firefox -contentproc -childID 4",
        MINE,
        "copal",
        152,
        'S',
        none,
        Supervision::None,
        false,
    );
    add(
        6002,
        1127,
        6002,
        "qemu-system-x8",
        "qemu-system-x86_64 -m 4096 -accel kvm",
        MINE,
        "copal",
        3_891,
        'S',
        none,
        Supervision::None,
        false,
    );
    add(
        6410,
        1127,
        6410,
        "cargo",
        "cargo build --release",
        MINE,
        "copal",
        88,
        'S',
        none,
        Supervision::None,
        false,
    );
    add(
        6411,
        6410,
        6410,
        "rustc",
        "rustc --edition 2021 --crate-name copal_tm",
        MINE,
        "copal",
        744,
        'R',
        none,
        Supervision::None,
        false,
    );
    add(
        6809,
        1127,
        6809,
        "ytq",
        "ytq --queue",
        MINE,
        "copal",
        36,
        'S',
        none,
        Supervision::None,
        false,
    );
    add(
        7001,
        1,
        7001,
        "chronyd",
        "/usr/sbin/chronyd -f /etc/chrony/chrony.conf",
        123,
        "chrony",
        6,
        'S',
        daemon_mask,
        Supervision::OpenRc("chronyd".into()),
        false,
    );
    add(
        7204,
        1127,
        7204,
        "dd",
        "dd if=/dev/sdb of=/dev/null",
        MINE,
        "copal",
        2,
        'D',
        none,
        Supervision::None,
        false,
    );
    add(
        7310,
        1127,
        7310,
        "vi",
        "vi notes.md",
        MINE,
        "copal",
        5,
        'T',
        none,
        Supervision::None,
        false,
    );
    add(
        7420,
        310,
        310,
        "cgi-worker",
        "",
        100,
        "nginx",
        0,
        'Z',
        none,
        Supervision::None,
        false,
    );
    add(
        7551,
        1127,
        7551,
        "stubborn",
        "./stubborn --no-really",
        MINE,
        "copal",
        12,
        'S',
        stubborn,
        Supervision::None,
        false,
    );
    add(
        7880,
        610,
        7880,
        "copal-tm",
        "copal-tm",
        MINE,
        "copal",
        9,
        'R',
        none,
        Supervision::None,
        false,
    );
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::halt;

    #[test]
    fn the_tree_contains_every_case_the_halt_plan_reasons_about() {
        let t = synthetic_tree();
        assert!(t.iter().any(|p| p.state == 'Z'), "a zombie");
        assert!(t.iter().any(|p| p.state == 'D'), "uninterruptible sleep");
        assert!(t.iter().any(|p| p.state == 'T'), "stopped");
        assert!(t.iter().any(|p| p.kernel_thread), "a kernel thread");
        assert!(
            t.iter()
                .any(|p| matches!(p.supervision, Supervision::OpenRc(_))),
            "a supervised service"
        );
        assert!(t.iter().any(|p| halt::is_wrapper(&p.name)), "a wrapper");
        assert!(
            t.iter().any(|p| p.sig.unwrap_or_default().has_trap()),
            "a trap"
        );
        assert!(t.iter().any(|p| p.uid == 0), "a root-owned process");
    }

    #[test]
    fn the_wrapped_node_server_is_addressed_by_its_group() {
        let t = synthetic_tree();
        let plan = halt::plan(&t, 4821, 2.0, 7880, my_uid(), false);
        assert_eq!(plan.recommended(), Some(&halt::Addressee::Group(4820)));
    }

    #[test]
    fn readings_stay_in_range_over_a_long_run() {
        let mut b = Backend::new();
        for i in 0..2000 {
            let s = b.sample(i as f64, 1.0, &[]);
            assert!((0.0..=1.0).contains(&s.cpu.total));
            assert!(s.cpu.kernel <= s.cpu.total + f32::EPSILON);
            assert!(s.mem.avail_kb <= s.mem.total_kb);
            for c in &s.cpu.per_core {
                assert!((0.0..=1.0).contains(c));
            }
            assert!(s.power.charge.unwrap() > 0.0);
        }
    }
}
