// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Paul Richeson
//! The program's state, and everything that changes it.
//!
//! `App` holds the configuration, the probe, the last snapshot, the histories
//! and the selection. `tick` samples; `key` handles input; `draw` (in `draw`)
//! reads this and writes cells. Nothing here draws and nothing there decides.

use std::collections::{HashMap, HashSet};

use copal_tm_probe::halt::{Addressee, Plan, Rung};
use copal_tm_probe::signals::{self, Signal, MENU};
use copal_tm_probe::{Detail, Probe, Proc, Ring, Snapshot, Source};
use copal_tm_tty::Key;
use copal_tm_ui::Theme;

use crate::config::{Config, Meter, Readout, Sort, View};
use crate::transcript::Transcript;

/// Which panel has the keyboard.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Focus {
    Dashboard,
    Browser,
}

/// What is on top of everything else, if anything.
#[derive(Clone, PartialEq, Debug)]
pub enum Overlay {
    None,
    Inspector,
    Services,
    Halt,
    Transcript,
    Help,
    Filter,
}

/// One line of the Browser, after the tree has been flattened.
#[derive(Clone, Debug)]
pub struct Row {
    pub pid: i32,
    pub idx: usize,
    pub depth: usize,
    pub ancestors_last: Vec<bool>,
    pub is_last: bool,
    pub expandable: bool,
    pub open: bool,
    /// Descendants rolled up into this row because it is collapsed.
    pub hidden: usize,
    /// CPU and RSS, including the rolled-up subtree when collapsed.
    pub cpu: f32,
    pub rss_kb: u64,
}

/// The histories, one ring per series.
pub struct History {
    pub util: Ring,
    pub kernel: Ring,
    pub temp: Ring,
    pub mem: Ring,
    pub net: Ring,
    pub disk: Ring,
    pub gpu: Ring,
    pub cores: Vec<Ring>,
}

impl Default for History {
    fn default() -> Self {
        History {
            util: Ring::new(1024),
            kernel: Ring::new(1024),
            temp: Ring::new(1024),
            mem: Ring::new(1024),
            net: Ring::new(1024),
            disk: Ring::new(1024),
            gpu: Ring::new(1024),
            cores: Vec::new(),
        }
    }
}

impl History {
    fn push(&mut self, s: &Snapshot) {
        self.util.push(s.cpu.total);
        self.kernel.push(s.cpu.kernel);
        self.temp.push(s.thermal.hottest_c.unwrap_or(0.0));
        self.mem.push(s.mem.frac());
        self.net.push(s.net.total_bps() as f32);
        self.disk.push(s.disk.busy);
        self.gpu.push(s.gpu.unwrap_or(0.0));
        if self.cores.len() != s.cpu.per_core.len() {
            self.cores = (0..s.cpu.per_core.len()).map(|_| Ring::new(256)).collect();
        }
        for (r, v) in self.cores.iter_mut().zip(s.cpu.per_core.iter()) {
            r.push(*v);
        }
    }
}

/// A halt ladder in progress: which rung, and when the next one is due.
pub struct LadderRun {
    pub plan: Plan,
    pub addressee: Addressee,
    pub step: usize,
    pub next_at: f64,
    pub stepwise: bool,
}

pub struct App {
    pub cfg: Config,
    pub theme: Theme,
    pub probe: Probe,
    pub snap: Snapshot,
    pub hist: History,
    pub log: Transcript,

    pub focus: Focus,
    pub overlay: Overlay,
    pub inspector_tab: u8,
    pub detail: Option<Detail>,
    pub detail_for: i32,

    pub rows: Vec<Row>,
    pub sel: usize,
    pub scroll: usize,
    pub marked: HashSet<i32>,
    pub collapsed: HashSet<i32>,
    pub filter: String,
    pub series_on: [bool; 3],
    pub desc: bool,

    pub paused: bool,
    pub quit: bool,
    pub run: Option<LadderRun>,
    pub service_sel: usize,
    pub choice_sel: usize,
    /// The auto-scaling full scale of the Network tile's meter.
    pub net_scale: f64,
    /// Rank stability for the Roll: last tick's order.
    prev_rank: HashMap<i32, usize>,
    pub roll: Vec<usize>,
    /// The tick period the throttle has settled on, which may be slower than
    /// the configured one.
    pub period: f64,
    throttle_run: u32,
    pub rows_visible: usize,
}

impl App {
    pub fn new(cfg: Config, notes: Vec<String>) -> App {
        let theme = Theme::named(&cfg.theme, cfg.charset);
        let probe = if cfg.simulate {
            Probe::simulated()
        } else {
            Probe::new()
        };
        let mut log = Transcript::new();
        if probe.source == Source::Simulated {
            log.note("SIMULATED: this machine has no /proc, so every reading below is made up");
        }
        for n in notes {
            log.note(n);
        }
        let period = cfg.refresh;
        App {
            focus: if cfg.view == View::Dashboard {
                Focus::Dashboard
            } else {
                Focus::Browser
            },
            cfg,
            theme,
            probe,
            snap: Snapshot::default(),
            hist: History::default(),
            log,
            overlay: Overlay::None,
            inspector_tab: 1,
            detail: None,
            detail_for: -1,
            rows: Vec::new(),
            sel: 0,
            scroll: 0,
            marked: HashSet::new(),
            collapsed: HashSet::new(),
            filter: String::new(),
            series_on: [true, true, true],
            desc: true,
            paused: false,
            quit: false,
            run: None,
            service_sel: 0,
            choice_sel: 0,
            net_scale: 65536.0,
            prev_rank: HashMap::new(),
            roll: Vec::new(),
            period,
            throttle_run: 0,
            rows_visible: 20,
        }
    }

    /// Put up one overlay by name, selecting a process that shows it at its
    /// most interesting. Used by `--frame --open NAME`, so the pictures in the
    /// documentation are rendered by the program and cannot drift from it.
    pub fn open_overlay(&mut self, name: &str) {
        if name == "collapsed" {
            // Every parent folded, which is the state the roll-up is worth
            // showing in.
            let parents: Vec<i32> = self
                .rows
                .iter()
                .filter(|r| r.expandable && r.depth > 1)
                .map(|r| r.pid)
                .collect();
            self.collapsed.extend(parents);
            self.rebuild_rows();
            return;
        }
        let (want, overlay) = match name {
            "halt" => ("node", Overlay::Halt),
            "services" => ("stubborn", Overlay::Services),
            "inspector" | "network" => ("nginx", Overlay::Inspector),
            "transcript" => ("", Overlay::Transcript),
            "help" | "keys" => ("", Overlay::Help),
            _ => return,
        };
        if !want.is_empty() {
            if let Some(pid) = self
                .snap
                .procs
                .iter()
                .find(|p| p.name == want)
                .map(|p| p.pid)
            {
                if let Some(n) = self.rows.iter().position(|r| r.pid == pid) {
                    self.sel = n;
                    self.clamp_scroll();
                }
                self.load_detail(pid);
            }
        }
        if name == "network" {
            self.inspector_tab = 3;
        }
        if overlay == Overlay::Halt {
            self.open_halt();
        } else {
            self.overlay = overlay;
        }
    }

    pub fn selected_pid(&self) -> Option<i32> {
        self.rows.get(self.sel).map(|r| r.pid)
    }

    pub fn proc_of(&self, pid: i32) -> Option<&Proc> {
        self.snap.procs.iter().find(|p| p.pid == pid)
    }

    /// One sample, and everything that follows from it.
    pub fn tick(&mut self) {
        if self.paused {
            return;
        }
        // Row 8 of the schedule: the expensive per-process files, for the
        // rows actually on screen and the selection, and nothing else.
        let mut visible: Vec<i32> = self
            .rows
            .iter()
            .skip(self.scroll)
            .take(self.rows_visible + 2)
            .map(|r| r.pid)
            .collect();
        if let Some(p) = self.selected_pid() {
            visible.push(p);
        }
        visible.extend(
            self.roll
                .iter()
                .filter_map(|i| self.snap.procs.get(*i))
                .map(|p| p.pid),
        );
        self.snap = self.probe.tick(&visible);
        self.hist.push(&self.snap);
        self.rebuild_rows();
        self.rank_roll();
        self.auto_scale_net();
        self.throttle();
        self.step_ladder();
        if self.overlay == Overlay::Inspector {
            if let Some(pid) = self.selected_pid() {
                if pid != self.detail_for {
                    self.load_detail(pid);
                }
            }
        }
    }

    /// Rule 4 of Section VII-B: a task manager that is in the top five of its
    /// own table has failed at its job.
    fn throttle(&mut self) {
        let mine = self.proc_of(self.probe.me).map(|p| p.cpu).unwrap_or(0.0);
        let low_battery =
            self.snap.power.on_battery && self.snap.power.charge.map(|c| c < 0.15).unwrap_or(false);
        if mine > 0.02 || low_battery {
            self.throttle_run += 1;
        } else {
            self.throttle_run = 0;
        }
        let want = if self.throttle_run >= 10 {
            (self.cfg.refresh * 2.0).min(4.0)
        } else {
            self.cfg.refresh
        };
        if (want - self.period).abs() > 1e-6 {
            self.period = want;
            if want > self.cfg.refresh {
                self.log.note(format!(
                    "slowing to {want:.1}s: {}",
                    if low_battery {
                        "battery below 15%"
                    } else {
                        "our own CPU share"
                    }
                ));
            } else {
                self.log.note(format!("back to {want:.1}s"));
            }
        }
    }

    /// The Network tile's scale rises at once and decays slowly, so a burst is
    /// visible immediately and the meter does not stay pinned to it.
    fn auto_scale_net(&mut self) {
        let v = self.snap.net.total_bps();
        if v > self.net_scale {
            self.net_scale = v * 1.25;
        } else {
            self.net_scale = (self.net_scale * 0.97).max(v.max(4096.0));
        }
    }

    pub fn load_detail(&mut self, pid: i32) {
        self.detail = self.probe.detail(pid);
        self.detail_for = pid;
    }

    // ---- the Browser's rows -------------------------------------------

    fn passes(&self, p: &Proc) -> bool {
        if p.kernel_thread && !self.cfg.kernel {
            return false;
        }
        if self.filter.is_empty() {
            return true;
        }
        let f = self.filter.to_ascii_lowercase();
        p.name.to_ascii_lowercase().contains(&f)
            || p.cmdline.to_ascii_lowercase().contains(&f)
            || p.user.to_ascii_lowercase().contains(&f)
            || p.pid.to_string() == f
    }

    fn sort_key(&self, p: &Proc) -> (i64, String) {
        match self.cfg.sort {
            Sort::Cpu => ((p.cpu * 100_000.0) as i64, String::new()),
            Sort::Mem => (p.rss_kb as i64, String::new()),
            Sort::Pid => (p.pid as i64, String::new()),
            Sort::Time => ((p.cpu_time * 1000.0) as i64, String::new()),
            Sort::Name => (0, p.name.to_ascii_lowercase()),
            Sort::User => (0, format!("{}\u{1}{}", p.user, p.name)),
        }
    }

    pub fn rebuild_rows(&mut self) {
        let keep: Vec<usize> = (0..self.snap.procs.len())
            .filter(|i| self.passes(&self.snap.procs[*i]))
            .collect();
        let sel_pid = self.selected_pid();

        let mut rows = Vec::with_capacity(keep.len());
        if !self.cfg.tree || !self.filter.is_empty() {
            // A filtered tree hides the parents of what matched, which reads
            // as "not found". Filtering therefore flattens.
            let mut v = keep;
            self.sort_indices(&mut v);
            for i in v {
                let p = &self.snap.procs[i];
                rows.push(Row {
                    pid: p.pid,
                    idx: i,
                    depth: 0,
                    ancestors_last: Vec::new(),
                    is_last: false,
                    expandable: false,
                    open: false,
                    hidden: 0,
                    cpu: p.cpu,
                    rss_kb: p.rss_kb,
                });
            }
        } else {
            let present: HashSet<i32> = keep.iter().map(|i| self.snap.procs[*i].pid).collect();
            let mut children: HashMap<i32, Vec<usize>> = HashMap::new();
            let mut roots: Vec<usize> = Vec::new();
            for &i in &keep {
                let p = &self.snap.procs[i];
                if p.ppid > 0 && present.contains(&p.ppid) && p.ppid != p.pid {
                    children.entry(p.ppid).or_default().push(i);
                } else {
                    roots.push(i);
                }
            }
            for v in children.values_mut() {
                self.sort_indices(v);
            }
            self.sort_indices(&mut roots);
            let mut stack: Vec<(usize, Vec<bool>, bool)> = Vec::new();
            for (n, &r) in roots.iter().enumerate().rev() {
                stack.push((r, Vec::new(), n + 1 == roots.len()));
            }
            while let Some((i, anc, last)) = stack.pop() {
                let p = &self.snap.procs[i];
                let kids = children.get(&p.pid).cloned().unwrap_or_default();
                let open = !self.collapsed.contains(&p.pid);
                let (cpu, rss, hidden) = if open || kids.is_empty() {
                    (p.cpu, p.rss_kb, 0)
                } else {
                    let (c, r, n) = subtree_totals(&self.snap.procs, &children, p.pid);
                    (p.cpu + c, p.rss_kb + r, n)
                };
                rows.push(Row {
                    pid: p.pid,
                    idx: i,
                    depth: anc.len(),
                    ancestors_last: anc.clone(),
                    is_last: last,
                    expandable: !kids.is_empty(),
                    open,
                    hidden,
                    cpu,
                    rss_kb: rss,
                });
                if open {
                    let mut a = anc.clone();
                    a.push(last);
                    for (n, &k) in kids.iter().enumerate().rev() {
                        stack.push((k, a.clone(), n + 1 == kids.len()));
                    }
                }
            }
        }
        self.rows = rows;
        // Keep the cursor on the same process across a rebuild; a selection
        // that wanders while the table re-sorts is how the wrong thing gets
        // signalled.
        if let Some(pid) = sel_pid {
            if let Some(n) = self.rows.iter().position(|r| r.pid == pid) {
                self.sel = n;
            }
        }
        self.sel = self.sel.min(self.rows.len().saturating_sub(1));
        self.clamp_scroll();
    }

    fn sort_indices(&self, v: &mut [usize]) {
        let desc = self.desc;
        v.sort_by(|&a, &b| {
            let ka = self.sort_key(&self.snap.procs[a]);
            let kb = self.sort_key(&self.snap.procs[b]);
            let o = ka.cmp(&kb);
            let o = if desc { o.reverse() } else { o };
            o.then_with(|| self.snap.procs[a].pid.cmp(&self.snap.procs[b].pid))
        });
    }

    /// Group C's stability rule: a row only changes rank when it crosses
    /// another by more than 0.3 %. An unstable table cannot be read, and
    /// reading it is the only thing it is for.
    fn rank_roll(&mut self) {
        let mut v: Vec<usize> = (0..self.snap.procs.len())
            .filter(|i| !self.snap.procs[*i].kernel_thread)
            .collect();
        // Sort strictly first - a comparator that consults last tick's order
        // is not a total order, and Rust's sort will say so - then settle the
        // near-ties back toward the order they were already in.
        v.sort_by(|&a, &b| {
            let (pa, pb) = (&self.snap.procs[a], &self.snap.procs[b]);
            pb.cpu
                .partial_cmp(&pa.cpu)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(pa.pid.cmp(&pb.pid))
        });
        const HYSTERESIS: f32 = 0.003;
        let rank = |pid: i32| self.prev_rank.get(&pid).copied().unwrap_or(usize::MAX);
        for _ in 0..3 {
            let mut settled = true;
            for i in 1..v.len() {
                let (a, b) = (&self.snap.procs[v[i - 1]], &self.snap.procs[v[i]]);
                if (a.cpu - b.cpu).abs() < HYSTERESIS && rank(b.pid) < rank(a.pid) {
                    v.swap(i - 1, i);
                    settled = false;
                }
            }
            if settled {
                break;
            }
        }
        v.truncate(64);
        self.prev_rank = v
            .iter()
            .enumerate()
            .map(|(n, &i)| (self.snap.procs[i].pid, n))
            .collect();
        self.roll = v;
    }

    fn clamp_scroll(&mut self) {
        let h = self.rows_visible.max(1);
        if self.sel < self.scroll {
            self.scroll = self.sel;
        } else if self.sel >= self.scroll + h {
            self.scroll = self.sel + 1 - h;
        }
        let max = self.rows.len().saturating_sub(h);
        self.scroll = self.scroll.min(max);
    }

    pub fn move_sel(&mut self, by: i64) {
        if self.rows.is_empty() {
            return;
        }
        let n = self.rows.len() as i64;
        self.sel = (self.sel as i64 + by).clamp(0, n - 1) as usize;
        self.clamp_scroll();
        if self.overlay == Overlay::Inspector {
            if let Some(pid) = self.selected_pid() {
                self.load_detail(pid);
            }
        }
    }

    // ---- acting -------------------------------------------------------

    pub fn services(&self) -> &'static [Signal] {
        MENU
    }

    /// Build a plan for the selection, and open it.
    pub fn open_halt(&mut self) {
        let Some(pid) = self.selected_pid() else {
            return;
        };
        let plan = self.probe.plan(&self.snap, pid, self.cfg.grace);
        let name = plan.name.clone();
        if plan.refused {
            let why = plan
                .warnings
                .first()
                .map(|w| w.text.clone())
                .unwrap_or_else(|| "refused".into());
            self.log.refusal(format!("{name} ({pid}): {why}"));
        }
        self.choice_sel = 0;
        let addressee = plan
            .recommended()
            .cloned()
            .unwrap_or(copal_tm_probe::halt::Addressee::Process(pid));
        self.run = Some(LadderRun {
            plan,
            addressee,
            step: 0,
            next_at: f64::MAX,
            stepwise: true,
        });
        self.overlay = Overlay::Halt;
    }

    /// Send one signal to one addressee, and say so.
    pub fn send(&mut self, addressee: &Addressee, sig: &Signal, also: Option<&Signal>) {
        let cmd = addressee.command(sig);
        let Some(target) = addressee.target() else {
            self.log
                .refusal(format!("{cmd} - run this yourself; it is not a signal"));
            return;
        };
        match signals::send(target, sig.num) {
            Ok(()) => {
                self.log.signal(cmd);
                if let Some(a) = also {
                    if signals::send(target, a.num).is_ok() {
                        self.log.signal(addressee.command(a));
                    }
                }
            }
            Err(e) => self.log.refusal(format!("{cmd}: {e}")),
        }
    }

    /// Advance a running ladder if its grace has expired.
    fn step_ladder(&mut self) {
        let now = self.probe.now();
        let Some(run) = &self.run else { return };
        if run.stepwise || now < run.next_at {
            return;
        }
        let pid = run.plan.pid;
        if !signals::alive(pid) {
            let rung = run.step.saturating_sub(1);
            let name = run.plan.name.clone();
            let sig = run.plan.ladder.get(rung).map(|r| r.sig.name).unwrap_or("?");
            self.log.death(format!(
                "{name} ({pid}) is gone; {sig} was the rung that worked"
            ));
            self.run = None;
            self.overlay = Overlay::None;
            return;
        }
        self.advance_ladder();
    }

    /// Send the next rung that is not struck out.
    pub fn advance_ladder(&mut self) {
        // Walk past the struck-out rungs first, collecting what to say about
        // them, so the Transcript is not borrowed while the plan is.
        let mut skipped = Vec::new();
        {
            let Some(run) = &mut self.run else { return };
            while run.step < run.plan.ladder.len() && run.plan.ladder[run.step].skipped {
                let r: &Rung = &run.plan.ladder[run.step];
                skipped.push(format!("skipping {}: {}", r.sig.name, r.why));
                run.step += 1;
            }
        }
        for s in skipped {
            self.log.note(s);
        }

        let (rung, addressee) = {
            let Some(run) = &mut self.run else { return };
            if run.step >= run.plan.ladder.len() {
                let pid = run.plan.pid;
                let name = run.plan.name.clone();
                self.run = None;
                self.overlay = Overlay::None;
                if signals::alive(pid) {
                    self.log.refusal(format!(
                        "{name} ({pid}) survived every rung including KILL - it is in \
                         uninterruptible sleep, or it is not ours"
                    ));
                } else {
                    self.log.death(format!("{name} ({pid}) is gone"));
                }
                return;
            }
            let rung = run.plan.ladder[run.step].clone();
            run.step += 1;
            run.next_at = self.probe.now() + rung.grace.max(0.2);
            (rung, run.addressee.clone())
        };
        self.send(&addressee, &rung.sig, rung.also.as_ref());
    }

    // ---- input --------------------------------------------------------

    pub fn key(&mut self, k: Key) {
        if self.overlay == Overlay::Filter {
            self.filter_key(k);
            return;
        }
        match (&self.overlay, k) {
            (Overlay::None, _) => self.key_main(k),
            (_, Key::Esc) | (_, Key::Char('q')) => {
                if self.run.is_some() {
                    let step = self.run.as_ref().map(|r| r.step).unwrap_or(0);
                    if step > 0 {
                        self.log
                            .note("ladder stopped part-way; what was sent has been sent");
                    }
                    self.run = None;
                }
                self.overlay = Overlay::None;
            }
            (Overlay::Inspector, Key::Char(c @ '1'..='4')) => {
                self.inspector_tab = c as u8 - b'0';
            }
            (Overlay::Inspector, _) => self.key_main(k),
            (Overlay::Services, Key::Up) => self.service_sel = self.service_sel.saturating_sub(1),
            (Overlay::Services, Key::Down) => {
                self.service_sel = (self.service_sel + 1).min(MENU.len() - 1)
            }
            (Overlay::Services, Key::Enter) => {
                let sig = MENU[self.service_sel];
                if let Some(pid) = self.selected_pid() {
                    let plan = self.probe.plan(&self.snap, pid, self.cfg.grace);
                    if plan.refused {
                        let why = plan
                            .warnings
                            .first()
                            .map(|w| w.text.clone())
                            .unwrap_or_else(|| "refused".into());
                        self.log.refusal(format!("{} ({pid}): {why}", plan.name));
                    } else {
                        let a = plan
                            .recommended()
                            .cloned()
                            .unwrap_or(copal_tm_probe::halt::Addressee::Process(pid));
                        self.send(&a, &sig, None);
                    }
                }
                self.overlay = Overlay::None;
            }
            (Overlay::Halt, Key::Up) => self.choice_sel = self.choice_sel.saturating_sub(1),
            (Overlay::Halt, Key::Down) => {
                let n = self.run.as_ref().map(|r| r.plan.choices.len()).unwrap_or(1);
                self.choice_sel = (self.choice_sel + 1).min(n.saturating_sub(1));
                if let Some(run) = &mut self.run {
                    if let Some(a) = run.plan.choices.get(self.choice_sel) {
                        run.addressee = a.clone();
                    }
                }
            }
            (Overlay::Halt, Key::Enter) => {
                if let Some(run) = &mut self.run {
                    if let Some(a) = run.plan.choices.get(self.choice_sel) {
                        run.addressee = a.clone();
                    }
                    if run.plan.refused {
                        self.overlay = Overlay::None;
                        self.run = None;
                        return;
                    }
                    run.stepwise = false;
                }
                self.advance_ladder();
            }
            (Overlay::Halt, Key::Char('e')) => {
                if let Some(run) = &mut self.run {
                    run.stepwise = true;
                }
                self.advance_ladder();
            }
            (Overlay::Transcript, _) | (Overlay::Help, _) => self.overlay = Overlay::None,
            _ => {}
        }
    }

    fn filter_key(&mut self, k: Key) {
        match k {
            Key::Esc => {
                self.filter.clear();
                self.overlay = Overlay::None;
                self.rebuild_rows();
            }
            Key::Enter => self.overlay = Overlay::None,
            Key::Backspace => {
                self.filter.pop();
                self.rebuild_rows();
            }
            Key::Char(c) => {
                self.filter.push(c);
                self.rebuild_rows();
            }
            _ => {}
        }
    }

    fn key_main(&mut self, k: Key) {
        match k {
            Key::Char('q') | Key::Ctrl('c') => self.quit = true,
            Key::F(1) => self.set_view(View::Dashboard),
            Key::F(2) => self.set_view(View::Split),
            Key::F(3) => self.set_view(View::Browser),
            Key::Tab => {
                self.focus = if self.focus == Focus::Browser {
                    Focus::Dashboard
                } else {
                    Focus::Browser
                }
            }
            Key::Up => self.move_sel(-1),
            Key::Down => self.move_sel(1),
            Key::PageUp => self.move_sel(-(self.rows_visible as i64)),
            Key::PageDown => self.move_sel(self.rows_visible as i64),
            Key::Home => self.move_sel(-(self.rows.len() as i64)),
            Key::End => self.move_sel(self.rows.len() as i64),
            Key::Left => {
                if let Some(pid) = self.selected_pid() {
                    self.collapsed.insert(pid);
                    self.rebuild_rows();
                }
            }
            Key::Right => {
                if let Some(pid) = self.selected_pid() {
                    self.collapsed.remove(&pid);
                    self.rebuild_rows();
                }
            }
            Key::Char(' ') => {
                if let Some(pid) = self.selected_pid() {
                    let name = self
                        .proc_of(pid)
                        .map(|p| p.name.clone())
                        .unwrap_or_default();
                    if self.marked.insert(pid) {
                        self.log.action(format!("marked {name} ({pid})"));
                    } else {
                        self.marked.remove(&pid);
                        self.log.action(format!("unmarked {name} ({pid})"));
                    }
                }
            }
            Key::Enter | Key::Char('i') => {
                if let Some(pid) = self.selected_pid() {
                    self.load_detail(pid);
                    self.overlay = Overlay::Inspector;
                }
            }
            Key::Char('t') => {
                self.cfg.tree = !self.cfg.tree;
                self.log
                    .note(format!("tree {}", if self.cfg.tree { "on" } else { "off" }));
                self.rebuild_rows();
            }
            Key::Char('s') => {
                self.cfg.sort = self.cfg.sort.next();
                self.rebuild_rows();
            }
            Key::Char('r') => {
                self.desc = !self.desc;
                self.rebuild_rows();
            }
            Key::Char('K') => {
                self.cfg.kernel = !self.cfg.kernel;
                self.rebuild_rows();
            }
            Key::Char('/') => self.overlay = Overlay::Filter,
            Key::Char('k') => {
                if self.selected_pid().is_some() {
                    self.service_sel = 0;
                    self.overlay = Overlay::Services;
                }
            }
            Key::Char('x') => self.open_halt(),
            Key::Char('T') => self.overlay = Overlay::Transcript,
            Key::Char('?') => self.overlay = Overlay::Help,
            Key::Char('p') => {
                self.paused = !self.paused;
                self.log
                    .note(if self.paused { "paused" } else { "resumed" });
            }
            Key::Char('w') => {
                self.cfg.readout = self.cfg.readout.next();
            }
            Key::Char(c @ '1'..='3') => {
                let i = c as usize - '1' as usize;
                self.series_on[i] = !self.series_on[i];
            }
            Key::Char('+') | Key::Char('=') => {
                self.cfg.refresh = (self.cfg.refresh / 2.0).max(0.25);
                self.period = self.cfg.refresh;
                self.log.note(format!("refresh {:.2}s", self.cfg.refresh));
            }
            Key::Char('-') => {
                self.cfg.refresh = (self.cfg.refresh * 2.0).min(10.0);
                self.period = self.cfg.refresh;
                self.log.note(format!("refresh {:.2}s", self.cfg.refresh));
            }
            _ => {}
        }
    }

    fn set_view(&mut self, v: View) {
        self.cfg.view = v;
        if v == View::Dashboard {
            self.focus = Focus::Dashboard;
        }
        if v == View::Browser {
            self.focus = Focus::Browser;
        }
    }

    /// The SYS panel's right-hand readout, per `sys.readout`.
    pub fn sys_readout(&self) -> String {
        use copal_tm_probe::fmt;
        match self.cfg.readout {
            Readout::Power => self
                .snap
                .power
                .watts
                .map(fmt::watts)
                .unwrap_or("-- W".into()),
            Readout::Remaining => match self.snap.power.remaining_s {
                Some(s) => format!("{} left", fmt::hm(s)),
                None if self.snap.power.present => self
                    .snap
                    .power
                    .charge
                    .map(|c| format!("{:.0}% {}", c * 100.0, self.snap.power.status))
                    .unwrap_or("on power".into()),
                None => fmt::uptime(self.snap.uptime),
            },
            Readout::Uptime => fmt::uptime(self.snap.uptime),
            Readout::Load => format!("{:.2}", self.snap.load[0]),
            Readout::Temp => self
                .snap
                .thermal
                .hottest_c
                .map(fmt::celsius)
                .unwrap_or("-- \u{b0}C".into()),
            Readout::Pressure => self.snap.psi.verdict().to_string(),
        }
    }

    /// The value and readout of one SYS column.
    pub fn meter(&self, m: Meter) -> (f32, String, &'static str) {
        use copal_tm_probe::fmt;
        match m {
            Meter::Cpu => (self.snap.cpu.total, fmt::pct(self.snap.cpu.total), "CPU"),
            Meter::Battery => match self.snap.power.charge {
                Some(c) => (
                    c,
                    self.snap
                        .power
                        .watts
                        .map(fmt::watts)
                        .unwrap_or_else(|| format!("{:.0}%", c * 100.0)),
                    "Batt",
                ),
                None => (self.snap.mem.frac(), fmt::pct(self.snap.mem.frac()), "Mem"),
            },
            Meter::Temp => {
                let c = self.snap.thermal.hottest_c.unwrap_or(0.0);
                let max = self.snap.thermal.scale_max.max(1.0);
                (c / max, fmt::celsius(c), "Temp")
            }
            Meter::Gpu => match self.snap.gpu {
                Some(g) => (g, fmt::pct(g), "GPU"),
                None => (self.snap.disk.busy, fmt::pct(self.snap.disk.busy), "I/O"),
            },
            Meter::Mem => (self.snap.mem.frac(), fmt::pct(self.snap.mem.frac()), "Mem"),
            Meter::Swap => {
                let f = if self.snap.mem.swap_total_kb == 0 {
                    0.0
                } else {
                    self.snap.mem.swap_used_kb as f32 / self.snap.mem.swap_total_kb as f32
                };
                (f, fmt::pct(f), "Swap")
            }
            Meter::Io => (self.snap.disk.busy, fmt::pct(self.snap.disk.busy), "I/O"),
            Meter::Net => {
                let f = (self.snap.net.total_bps() / self.net_scale.max(1.0)) as f32;
                (
                    f.clamp(0.0, 1.0),
                    fmt::rate(self.snap.net.total_bps()),
                    "Net",
                )
            }
            Meter::Load => {
                let n = self.snap.cpu.logical().max(1) as f32;
                (
                    self.snap.load[0] / n,
                    format!("{:.2}", self.snap.load[0]),
                    "Load",
                )
            }
            Meter::Pressure => {
                let w = self.snap.psi.worst() / 100.0;
                (w, self.snap.psi.verdict().to_string(), "PSI")
            }
        }
    }
}

fn subtree_totals(
    procs: &[Proc],
    children: &HashMap<i32, Vec<usize>>,
    pid: i32,
) -> (f32, u64, usize) {
    let mut cpu = 0.0;
    let mut rss = 0;
    let mut n = 0;
    let mut stack = vec![pid];
    while let Some(p) = stack.pop() {
        for &i in children.get(&p).map(|v| v.as_slice()).unwrap_or(&[]) {
            cpu += procs[i].cpu;
            rss += procs[i].rss_kb;
            n += 1;
            stack.push(procs[i].pid);
        }
    }
    (cpu, rss, n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::Kind;

    fn app() -> App {
        let cfg = Config {
            simulate: true,
            ..Config::default()
        };
        let mut a = App::new(cfg, Vec::new());
        a.tick();
        a
    }

    #[test]
    fn the_tree_nests_and_the_wrapped_child_is_under_its_shell() {
        let a = app();
        let sh = a
            .rows
            .iter()
            .find(|r| a.proc_of(r.pid).unwrap().name == "sh")
            .unwrap();
        let node = a
            .rows
            .iter()
            .find(|r| a.proc_of(r.pid).unwrap().name == "node")
            .unwrap();
        assert_eq!(node.depth, sh.depth + 1);
        assert!(sh.expandable);
    }

    #[test]
    fn a_collapsed_parent_rolls_its_subtree_up_into_itself() {
        let mut a = app();
        let ff = a
            .rows
            .iter()
            .find(|r| a.proc_of(r.pid).unwrap().name == "firefox")
            .unwrap();
        let pid = ff.pid;
        let kids: Vec<i32> = a
            .snap
            .procs
            .iter()
            .filter(|p| p.ppid == pid)
            .map(|p| p.pid)
            .collect();
        assert_eq!(
            kids.len(),
            2,
            "the simulated firefox has two content processes"
        );
        let want: u64 = a.proc_of(pid).unwrap().rss_kb
            + kids
                .iter()
                .map(|k| a.proc_of(*k).unwrap().rss_kb)
                .sum::<u64>();
        a.collapsed.insert(pid);
        a.rebuild_rows();
        let row = a.rows.iter().find(|r| r.pid == pid).unwrap();
        assert_eq!(row.hidden, 2);
        assert_eq!(
            row.rss_kb, want,
            "a collapsed parent carries its subtree's total"
        );
        assert!(
            !a.rows.iter().any(|r| kids.contains(&r.pid)),
            "the children are hidden"
        );
    }

    #[test]
    fn kernel_threads_are_hidden_until_asked_for() {
        let mut a = app();
        assert!(!a
            .rows
            .iter()
            .any(|r| a.proc_of(r.pid).unwrap().kernel_thread));
        a.key(Key::Char('K'));
        assert!(a
            .rows
            .iter()
            .any(|r| a.proc_of(r.pid).unwrap().kernel_thread));
    }

    #[test]
    fn filtering_flattens_so_a_match_is_never_hidden_under_a_parent() {
        let mut a = app();
        a.filter = "node".into();
        a.rebuild_rows();
        assert!(!a.rows.is_empty());
        assert!(a.rows.iter().all(|r| r.depth == 0));
        assert!(a.rows.iter().all(|r| {
            let p = a.proc_of(r.pid).unwrap();
            p.name.contains("node") || p.cmdline.contains("node")
        }));
    }

    #[test]
    fn the_selection_stays_on_the_same_process_across_a_rebuild() {
        let mut a = app();
        a.move_sel(5);
        let pid = a.selected_pid().unwrap();
        a.cfg.sort = Sort::Mem;
        a.rebuild_rows();
        assert_eq!(a.selected_pid(), Some(pid));
    }

    #[test]
    fn the_roll_does_not_reshuffle_for_noise() {
        let mut a = app();
        for _ in 0..5 {
            a.tick();
        }
        let before: Vec<i32> = a.roll.iter().map(|i| a.snap.procs[*i].pid).collect();
        a.tick();
        let after: Vec<i32> = a.roll.iter().map(|i| a.snap.procs[*i].pid).collect();
        let moved = before
            .iter()
            .zip(after.iter())
            .filter(|(x, y)| x != y)
            .count();
        assert!(
            moved < before.len() / 2,
            "{moved} of {} rows moved",
            before.len()
        );
    }

    #[test]
    fn the_network_scale_rises_at_once_and_decays_slowly() {
        let mut a = app();
        a.net_scale = 1000.0;
        a.snap.net.rx_bps = 100_000.0;
        a.auto_scale_net();
        assert!(a.net_scale >= 100_000.0);
        a.snap.net.rx_bps = 0.0;
        let high = a.net_scale;
        a.auto_scale_net();
        assert!(
            a.net_scale < high && a.net_scale > high * 0.9,
            "it decays, it does not drop"
        );
    }

    #[test]
    fn opening_a_halt_plan_sends_nothing() {
        let mut a = app();
        let pid = a.snap.procs.iter().find(|p| p.name == "node").unwrap().pid;
        a.sel = a.rows.iter().position(|r| r.pid == pid).unwrap();
        a.open_halt();
        assert_eq!(a.overlay, Overlay::Halt);
        assert!(a.run.is_some());
        assert!(
            !a.log.entries().iter().any(|e| e.kind == Kind::Signal),
            "a plan is shown, not run"
        );
    }

    #[test]
    fn a_refused_plan_is_logged_and_nothing_is_sent() {
        let mut a = app();
        a.cfg.kernel = true;
        a.rebuild_rows();
        let pid = a
            .snap
            .procs
            .iter()
            .find(|p| p.kernel_thread && p.pid != 2)
            .unwrap()
            .pid;
        a.sel = a.rows.iter().position(|r| r.pid == pid).unwrap();
        a.open_halt();
        assert!(a.log.entries().iter().any(|e| e.kind == Kind::Refusal));
        assert!(!a.log.entries().iter().any(|e| e.kind == Kind::Signal));
    }

    #[test]
    fn the_sys_readout_cycles_through_every_stat_without_panicking() {
        let mut a = app();
        for _ in 0..8 {
            assert!(!a.sys_readout().is_empty());
            a.key(Key::Char('w'));
        }
    }

    #[test]
    fn every_meter_reports_a_value_in_range() {
        let a = app();
        for m in [
            Meter::Cpu,
            Meter::Battery,
            Meter::Temp,
            Meter::Gpu,
            Meter::Mem,
            Meter::Swap,
            Meter::Io,
            Meter::Net,
            Meter::Load,
            Meter::Pressure,
        ] {
            let (v, text, label) = a.meter(m);
            assert!((0.0..=1.0).contains(&v), "{label} gave {v}");
            assert!(!text.is_empty());
        }
    }
}
