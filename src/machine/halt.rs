// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Paul Richeson
//! The halt plan: finding the right thing to stop, and the right order to
//! ask in.
//!
//! This module decides; it does not act.  It is given a process table and a
//! pid and it returns a plan - addressees, warnings, an escalation ladder and
//! the exact equivalent command line for each step.  The binary shows that
//! plan, waits for a confirmation, and only then calls `signals::send`.
//!
//! Keeping the reasoning here, with no syscalls in it, is what makes it
//! testable: every case below is a unit test over a synthetic process table,
//! and none of them ends anything.

use crate::machine::signals::{
    Disposition, SigMask, Signal, SIGCONT, SIGHUP, SIGINT, SIGKILL, SIGTERM,
};
use crate::machine::types::{Proc, Supervision};

/// The programs that stand in front of other programs.  When one of these is
/// the parent *and* has a single child, signalling the child alone tends to
/// leave the wrapper to respawn it, or leaves a shell reporting a failure
/// that is really the stop we asked for.
pub const WRAPPERS: &[&str] = &[
    "sh",
    "ash",
    "bash",
    "dash",
    "zsh",
    "ksh",
    "busybox",
    "env",
    "sudo",
    "doas",
    "su",
    "setsid",
    "nohup",
    "timeout",
    "stdbuf",
    "tini",
    "dumb-init",
    "s6-supervise",
    "runsv",
    "supervise-daemon",
    "start-stop-daemon",
    "openrc-run",
];

pub fn is_wrapper(name: &str) -> bool {
    WRAPPERS.contains(&name)
}

/// Who to send it to.  Ordered by how often it is the right answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Addressee {
    /// `kill -SIG -<pgid>`.  What Ctrl-C does: it reaches every member, so
    /// one member's trap cannot hold the others.
    Group(i32),
    /// The wrapper standing in front of the real program.
    Wrapper(i32, String),
    /// The process the cursor is on.
    Process(i32),
    /// `kill -SIG -<sid>`, for a job that has escaped its group.
    Session(i32),
    /// Not a signal at all: ask the service manager.
    Service(String, String),
    /// The parent, when the target is a zombie and only the parent can reap.
    Parent(i32, String),
}

impl Addressee {
    /// The pid argument to `kill`, negative for a group or session.
    pub fn target(&self) -> Option<i32> {
        match self {
            Addressee::Group(p) => Some(-p),
            Addressee::Session(s) => Some(-s),
            Addressee::Wrapper(p, _) | Addressee::Process(p) | Addressee::Parent(p, _) => Some(*p),
            Addressee::Service(..) => None,
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Addressee::Group(p) => format!("process group {p}"),
            Addressee::Wrapper(p, n) => format!("the wrapper {n} ({p})"),
            Addressee::Process(p) => format!("process {p}"),
            Addressee::Session(s) => format!("session {s}"),
            Addressee::Service(kind, n) => format!("{kind} service {n}"),
            Addressee::Parent(p, n) => format!("the parent {n} ({p})"),
        }
    }

    /// The command line this is equivalent to, shown before anything is sent.
    pub fn command(&self, sig: &Signal) -> String {
        match self {
            Addressee::Service(kind, n) if kind == "OpenRC" => format!("rc-service {n} stop"),
            Addressee::Service(_, n) => format!("systemctl stop {n}"),
            other => match other.target() {
                Some(t) => format!("kill -{} {}", sig.name, t),
                None => String::new(),
            },
        }
    }
}

/// Something the user should know before pressing the key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    pub severity: Severity,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Worth knowing.
    Note,
    /// This will probably not do what you expect.
    Caution,
    /// This will not work, or will do harm.
    Refuse,
}

/// One rung.
#[derive(Clone, Debug, PartialEq)]
pub struct Rung {
    pub sig: Signal,
    /// Also send this immediately after - `CONT` after `TERM` for a stopped
    /// process, which will not run its own handler until it is continued.
    pub also: Option<Signal>,
    pub disposition: Disposition,
    /// Seconds to wait before the next rung.
    pub grace: f64,
    /// Set when the mask says this one would do nothing; struck out, skipped.
    pub skipped: bool,
    pub why: String,
}

/// The whole answer.
#[derive(Clone, Debug)]
pub struct Plan {
    pub pid: i32,
    pub name: String,
    /// The addressee the plan recommends, first in `choices`.
    pub choices: Vec<Addressee>,
    pub warnings: Vec<Warning>,
    pub ladder: Vec<Rung>,
    /// True when nothing should be sent at all.
    pub refused: bool,
}

impl Plan {
    pub fn recommended(&self) -> Option<&Addressee> {
        self.choices.first()
    }

    pub fn worst(&self) -> Option<Severity> {
        self.warnings.iter().map(|w| w.severity).max()
    }
}

/// Build the plan.  `table` is the process list; `pid` is the selection;
/// `grace` is `halt.grace` from the configuration; `me` is our own pid, and
/// `root` says whether we can signal processes we do not own.
pub fn plan(table: &[Proc], pid: i32, grace: f64, me: i32, uid: u32, root: bool) -> Plan {
    let mut p = Plan {
        pid,
        name: String::new(),
        choices: Vec::new(),
        warnings: Vec::new(),
        ladder: Vec::new(),
        refused: false,
    };
    let Some(proc) = table.iter().find(|x| x.pid == pid) else {
        p.refused = true;
        p.warnings.push(Warning {
            severity: Severity::Refuse,
            text: "that process is gone".into(),
        });
        return p;
    };
    p.name = proc.name.clone();

    // 1. The refusals.  Section VIII-C-1.
    if proc.pid == 1 {
        p.refused = true;
        p.warnings.push(Warning {
            severity: Severity::Refuse,
            text: "pid 1 is init; signalling it would take the machine down".into(),
        });
        return p;
    }
    if proc.kernel_thread {
        p.refused = true;
        p.warnings.push(Warning {
            severity: Severity::Refuse,
            text: format!(
                "[{}] is a kernel thread; it has no user-space to signal",
                proc.name
            ),
        });
        return p;
    }
    // The service check runs before every other test but the absolute
    // refusals: a supervised daemon should name `rc-service ... stop` even
    // when it belongs to another user and we could not signal it anyway.
    match &proc.supervision {
        Supervision::OpenRc(name) => {
            p.choices
                .push(Addressee::Service("OpenRC".into(), name.clone()));
            p.warnings.push(Warning {
                severity: Severity::Caution,
                text: format!(
                    "{name} is a supervised OpenRC service; signalling it usually gets it \
                     restarted. The stop is `rc-service {name} stop`."
                ),
            });
        }
        Supervision::Systemd(name) => {
            p.choices
                .push(Addressee::Service("systemd".into(), name.clone()));
            p.warnings.push(Warning {
                severity: Severity::Caution,
                text: format!("{name} is a systemd unit; stop the unit, not the process."),
            });
        }
        Supervision::None => {}
    }

    if proc.uid != uid && !root {
        p.refused = true;
        p.warnings.push(Warning {
            severity: Severity::Refuse,
            text: format!(
                "owned by {} and you are not root; this needs doas or sudo{}",
                if proc.user.is_empty() {
                    proc.uid.to_string()
                } else {
                    proc.user.clone()
                },
                match p.choices.first() {
                    Some(Addressee::Service(..)) => " in front of the command above",
                    _ => "",
                }
            ),
        });
        return p;
    }
    if proc.pid == me {
        p.warnings.push(Warning {
            severity: Severity::Caution,
            text: "this is the task manager itself".into(),
        });
    }

    // 2. States that change the answer.  Section VIII-C-2.
    match proc.state {
        'Z' => {
            let parent = table.iter().find(|x| x.pid == proc.ppid);
            p.warnings.push(Warning {
                severity: Severity::Caution,
                text: "already dead: a zombie is waiting to be reaped, and no signal can \
                       affect it. Its parent has to collect it."
                    .into(),
            });
            if let Some(par) = parent {
                p.choices.push(Addressee::Parent(par.pid, par.name.clone()));
            }
            // A zombie has no ladder of its own; what is offered is the parent.
            p.ladder = ladder_for(&SigMask::default(), grace, false);
            return p;
        }
        'D' => p.warnings.push(Warning {
            severity: Severity::Caution,
            text: "in uninterruptible sleep: the signal is queued and delivered only when it \
                   leaves the kernel. On dead storage or a hung mount that may be never - and \
                   KILL is no different."
                .into(),
        }),
        'T' | 't' => p.warnings.push(Warning {
            severity: Severity::Note,
            text: "stopped: it will not run its own handler until it is continued, so each \
                   rung sends CONT straight after."
                .into(),
        }),
        _ => {}
    }

    // (the service check has already run; see above)
    // 4. The wrapper walk.  Section VIII-C-3.
    let mut wrapper: Option<&Proc> = None;
    let mut cur = proc;
    for _ in 0..8 {
        let Some(parent) = table.iter().find(|x| x.pid == cur.ppid) else {
            break;
        };
        if parent.pid <= 1 || !is_wrapper(&parent.name) {
            break;
        }
        let children = table.iter().filter(|x| x.ppid == parent.pid).count();
        if children != 1 {
            break;
        }
        wrapper = Some(parent);
        cur = parent;
    }

    if let Some(w) = wrapper {
        p.warnings.push(Warning {
            severity: Severity::Note,
            text: format!(
                "the parent {} ({}) is a wrapper with one child; the group reaches both",
                w.name, w.pid
            ),
        });
        if proc.pgid > 0 {
            p.choices.push(Addressee::Group(proc.pgid));
        }
        p.choices.push(Addressee::Wrapper(w.pid, w.name.clone()));
        p.choices.push(Addressee::Process(proc.pid));
        if proc.sid > 0 && proc.sid == w.pid {
            p.choices.push(Addressee::Session(proc.sid));
        }
    } else {
        // No wrapper: the process itself first, its group second - the group
        // is still worth offering, because a job that forked children of its
        // own is stopped by the group and only partly by the process.
        p.choices.push(Addressee::Process(proc.pid));
        let group_members = table.iter().filter(|x| x.pgid == proc.pgid).count();
        if proc.pgid > 0 && proc.pgid != proc.pid && group_members > 1 {
            p.choices.insert(0, Addressee::Group(proc.pgid));
        } else if proc.pgid > 0 && group_members > 1 {
            p.choices.push(Addressee::Group(proc.pgid));
        }
    }

    // 5. The ladder.
    let mask = proc.sig.unwrap_or_default();
    let stopped = matches!(proc.state, 'T' | 't');
    p.ladder = ladder_for(&mask, grace, stopped);
    if mask.has_trap() {
        p.warnings.push(Warning {
            severity: Severity::Note,
            text: "it has handlers installed; a polite stop may be a clean shutdown, or it may \
                   be swallowed. The ladder finds out."
                .into(),
        });
    }
    p
}

/// TERM, INT, HUP, KILL - skipping what is ignored, doubling the grace on
/// what is trapped, and always ending on KILL.
fn ladder_for(mask: &SigMask, grace: f64, stopped: bool) -> Vec<Rung> {
    let mut out = Vec::new();
    for sig in [SIGTERM, SIGINT, SIGHUP] {
        let d = mask.disposition(sig.num);
        let (skipped, why, g) = match d {
            Disposition::Ignored => (
                true,
                format!("{} is ignored; it would do nothing", sig.name),
                0.0,
            ),
            Disposition::Trapped => (
                false,
                format!("{} is trapped; the handler gets twice the grace", sig.name),
                grace * 2.0,
            ),
            Disposition::Blocked => (
                false,
                format!(
                    "{} is blocked; it will be delivered when unblocked",
                    sig.name
                ),
                grace,
            ),
            _ => (false, String::new(), grace),
        };
        out.push(Rung {
            sig,
            also: if stopped { Some(SIGCONT) } else { None },
            disposition: d,
            grace: g,
            skipped,
            why,
        });
    }
    out.push(Rung {
        sig: SIGKILL,
        also: None,
        disposition: Disposition::Uncatchable,
        grace: 0.0,
        skipped: false,
        why: "cannot be caught; the last rung, always".into(),
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: i32, ppid: i32, name: &str) -> Proc {
        Proc {
            pid,
            ppid,
            pgid: if ppid <= 1 { pid } else { ppid },
            sid: 1000,
            state: 'S',
            name: name.into(),
            uid: 1000,
            sig: Some(SigMask::default()),
            ..Default::default()
        }
    }

    /// The case the whole feature exists for: `sh -c 'node server.js'`.
    #[test]
    fn a_shell_wrapped_child_is_addressed_by_its_group() {
        let table = vec![
            p(100, 1, "login"),
            p(4820, 100, "sh"),
            p(4821, 4820, "node"),
        ];
        let plan = plan(&table, 4821, 2.0, 1, 1000, false);
        assert!(!plan.refused);
        assert_eq!(plan.recommended(), Some(&Addressee::Group(4820)));
        assert_eq!(
            plan.recommended().unwrap().command(&SIGTERM),
            "kill -TERM -4820",
            "a negative pid is the group, which is what Ctrl-C sends"
        );
        assert!(plan
            .choices
            .contains(&Addressee::Wrapper(4820, "sh".into())));
        assert!(plan.choices.contains(&Addressee::Process(4821)));
    }

    #[test]
    fn a_wrapper_with_two_children_is_not_a_wrapper() {
        let mut table = vec![
            p(100, 1, "login"),
            p(4820, 100, "sh"),
            p(4821, 4820, "node"),
        ];
        table.push(p(4822, 4820, "tail"));
        let plan = plan(&table, 4821, 2.0, 1, 1000, false);
        assert_eq!(plan.recommended(), Some(&Addressee::Group(4820)));
        assert!(!plan
            .choices
            .iter()
            .any(|c| matches!(c, Addressee::Wrapper(..))));
    }

    #[test]
    fn a_zombie_offers_its_parent_and_says_why() {
        let mut table = vec![p(200, 1, "supervisor"), p(201, 200, "worker")];
        table[1].state = 'Z';
        let plan = plan(&table, 201, 2.0, 1, 1000, false);
        assert_eq!(
            plan.recommended(),
            Some(&Addressee::Parent(200, "supervisor".into()))
        );
        assert!(plan.warnings.iter().any(|w| w.text.contains("reaped")));
    }

    #[test]
    fn a_supervised_service_is_stopped_by_its_manager() {
        let mut table = vec![p(300, 1, "nginx")];
        table[0].supervision = Supervision::OpenRc("nginx".into());
        let plan = plan(&table, 300, 2.0, 1, 1000, false);
        assert_eq!(
            plan.recommended().unwrap().command(&SIGTERM),
            "rc-service nginx stop",
            "signalling a supervised daemon just gets it restarted"
        );
    }

    #[test]
    fn an_ignored_signal_is_struck_out_and_kill_always_remains() {
        let mut table = vec![p(400, 1, "stubborn")];
        // Ignores TERM (bit 14) and HUP (bit 0).
        table[0].sig = Some(SigMask {
            ignored: (1 << 14) | 1,
            caught: 1 << 1,
            blocked: 0,
        });
        let plan = plan(&table, 400, 2.0, 1, 1000, false);
        let by = |n: &str| plan.ladder.iter().find(|r| r.sig.name == n).unwrap();
        assert!(by("TERM").skipped);
        assert!(by("HUP").skipped);
        assert!(!by("INT").skipped);
        assert_eq!(
            by("INT").grace,
            4.0,
            "a trapped signal gets twice the grace"
        );
        assert_eq!(plan.ladder.last().unwrap().sig.name, "KILL");
        assert!(!plan.ladder.last().unwrap().skipped);
    }

    #[test]
    fn a_stopped_process_gets_cont_with_every_rung() {
        let mut table = vec![p(500, 1, "paused")];
        table[0].state = 'T';
        let plan = plan(&table, 500, 2.0, 1, 1000, false);
        assert_eq!(plan.ladder[0].also, Some(SIGCONT));
        assert!(plan.warnings.iter().any(|w| w.text.contains("continued")));
    }

    #[test]
    fn uninterruptible_sleep_is_named_and_not_promised_away() {
        let mut table = vec![p(600, 1, "stuck")];
        table[0].state = 'D';
        let plan = plan(&table, 600, 2.0, 1, 1000, false);
        assert!(!plan.refused);
        assert!(plan
            .warnings
            .iter()
            .any(|w| w.text.contains("KILL is no different")));
    }

    #[test]
    fn init_and_kernel_threads_are_refused() {
        let table = vec![p(1, 0, "init"), {
            let mut k = p(2, 0, "kthreadd");
            k.kernel_thread = true;
            k
        }];
        assert!(plan(&table, 1, 2.0, 1, 1000, true).refused);
        assert!(plan(&table, 2, 2.0, 1, 1000, true).refused);
    }

    #[test]
    fn another_users_process_says_what_privilege_is_missing() {
        let mut table = vec![p(700, 1, "theirs")];
        table[0].uid = 0;
        table[0].user = "root".into();
        let theirs = plan(&table, 700, 2.0, 1, 1000, false);
        assert!(theirs.refused);
        assert!(theirs.warnings[0].text.contains("doas"));
        // As root, the same process is fine.
        assert!(!plan(&table, 700, 2.0, 1, 0, true).refused);
    }
}
