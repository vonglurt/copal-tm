// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Paul Richeson
//! Signals, and what a process has already decided to do about them.
//!
//! Every Linux process publishes three hexadecimal words in
//! `/proc/[pid]/status`: `SigIgn`, `SigCgt` and `SigBlk` - the signals it
//! ignores, the ones it has installed a handler for, and the ones it has
//! blocked. No common task manager shows them, and they are the difference
//! between "terminate did not work" and "terminate was never going to work".
//!
//! Bit *n−1* of each word is signal *n*, so `SIGTERM` (15) is bit 14.

/// A signal this program can send, with the label the Services menu uses.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Signal {
    pub num: i32,
    pub name: &'static str,
    pub label: &'static str,
    pub note: &'static str,
}

pub const SIGHUP: Signal = Signal {
    num: 1,
    name: "HUP",
    label: "Hangup",
    note: "for a daemon this usually means reload",
};
pub const SIGINT: Signal = Signal {
    num: 2,
    name: "INT",
    label: "Interrupt",
    note: "what Ctrl-C sends",
};
pub const SIGQUIT: Signal = Signal {
    num: 3,
    name: "QUIT",
    label: "Quit",
    note: "terminates AND dumps core",
};
pub const SIGKILL: Signal = Signal {
    num: 9,
    name: "KILL",
    label: "Kill",
    note: "cannot be caught, blocked or ignored; no chance to clean up",
};
pub const SIGUSR1: Signal = Signal {
    num: 10,
    name: "USR1",
    label: "User 1",
    note: "whatever the program made of it",
};
pub const SIGUSR2: Signal = Signal {
    num: 12,
    name: "USR2",
    label: "User 2",
    note: "whatever the program made of it",
};
pub const SIGTERM: Signal = Signal {
    num: 15,
    name: "TERM",
    label: "Terminate",
    note: "the polite one",
};
pub const SIGCONT: Signal = Signal {
    num: 18,
    name: "CONT",
    label: "Continue",
    note: "resume a stopped process",
};
pub const SIGSTOP: Signal = Signal {
    num: 19,
    name: "STOP",
    label: "Stop",
    note: "pause it; cannot be caught",
};

/// The Services menu, in the order it is shown.
pub const MENU: &[Signal] = &[
    SIGTERM, SIGINT, SIGHUP, SIGQUIT, SIGSTOP, SIGCONT, SIGUSR1, SIGUSR2, SIGKILL,
];

/// The signals the Inspector's table lists.
pub const TABLE: &[Signal] = &[SIGTERM, SIGINT, SIGHUP, SIGQUIT, SIGUSR1, SIGUSR2, SIGKILL];

/// What will happen when a signal arrives.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Disposition {
    /// The kernel's default action - for the terminating signals, death.
    Default,
    /// A handler is installed.  It may be cleaning up; it may be swallowing.
    /// Either way, sending it is not the same as ending the process.
    Trapped,
    /// Explicitly ignored.  Sending it does nothing at all.
    Ignored,
    /// Blocked for now; it will be delivered when the process unblocks it,
    /// which may be never.
    Blocked,
    /// `KILL` and `STOP`, which cannot be any of the above.
    Uncatchable,
}

impl Disposition {
    pub fn word(self) -> &'static str {
        match self {
            Disposition::Default => "default",
            Disposition::Trapped => "trapped",
            Disposition::Ignored => "ignored",
            Disposition::Blocked => "blocked",
            Disposition::Uncatchable => "default",
        }
    }

    /// Whether sending this signal can be expected to do anything.
    pub fn effective(self) -> bool {
        !matches!(self, Disposition::Ignored)
    }
}

/// The three masks, as read.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SigMask {
    pub ignored: u64,
    pub caught: u64,
    pub blocked: u64,
}

impl SigMask {
    /// Parse the three hexadecimal words, in `SigBlk SigIgn SigCgt` order as
    /// `/proc/[pid]/status` prints them.
    pub fn from_hex(blk: &str, ign: &str, cgt: &str) -> SigMask {
        let p = |s: &str| u64::from_str_radix(s.trim(), 16).unwrap_or(0);
        SigMask {
            blocked: p(blk),
            ignored: p(ign),
            caught: p(cgt),
        }
    }

    fn bit(sig: i32) -> u64 {
        if !(1..=64).contains(&sig) {
            0
        } else {
            1u64 << (sig - 1)
        }
    }

    pub fn disposition(&self, sig: i32) -> Disposition {
        if sig == SIGKILL.num || sig == SIGSTOP.num {
            // POSIX: neither can be caught, blocked or ignored.  A kernel
            // that reported otherwise would be lying, so we do not ask.
            return Disposition::Uncatchable;
        }
        let b = SigMask::bit(sig);
        if self.ignored & b != 0 {
            Disposition::Ignored
        } else if self.caught & b != 0 {
            Disposition::Trapped
        } else if self.blocked & b != 0 {
            Disposition::Blocked
        } else {
            Disposition::Default
        }
    }

    /// Does this process hold a handler for any of the terminating signals?
    /// The short answer to "will a polite stop be honoured".
    pub fn has_trap(&self) -> bool {
        TABLE
            .iter()
            .any(|s| self.disposition(s.num) == Disposition::Trapped)
    }
}

/// Send a signal.  The one syscall this program makes that changes anything.
///
/// A negative `pid` addresses a process *group*, which is deliberate and is
/// how the halt ladder gets past a trap held by one member of a job: it is
/// exactly what a terminal does when it sends `Ctrl-C`.
pub fn send(pid: i32, sig: i32) -> Result<(), std::io::Error> {
    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    // Refuse the two that can never be right, here as well as in the plan,
    // because this function is the only door and the check belongs on it.
    if pid == 1 || pid == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "refusing to signal pid 1 or the whole session",
        ));
    }
    let rc = unsafe { kill(pid, sig) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Is this process still there?  `kill(pid, 0)` checks without sending.
pub fn alive(pid: i32) -> bool {
    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    if pid <= 0 {
        return false;
    }
    let rc = unsafe { kill(pid, 0) };
    if rc == 0 {
        return true;
    }
    // EPERM means it exists and is not ours, which is still "there".
    std::io::Error::last_os_error().kind() == std::io::ErrorKind::PermissionDenied
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_known_mask_decodes() {
        // A shell: it catches INT (2) and TERM (15) and ignores QUIT (3).
        // bit 1 = INT, bit 14 = TERM  -> 0x4002
        // bit 2 = QUIT                -> 0x0004
        let m = SigMask::from_hex("0000000000000000", "0000000000000004", "0000000000004002");
        assert_eq!(m.disposition(SIGINT.num), Disposition::Trapped);
        assert_eq!(m.disposition(SIGTERM.num), Disposition::Trapped);
        assert_eq!(m.disposition(SIGQUIT.num), Disposition::Ignored);
        assert_eq!(m.disposition(SIGHUP.num), Disposition::Default);
        assert!(m.has_trap());
    }

    #[test]
    fn kill_and_stop_are_never_catchable() {
        let m = SigMask {
            ignored: u64::MAX,
            caught: u64::MAX,
            blocked: u64::MAX,
        };
        assert_eq!(m.disposition(SIGKILL.num), Disposition::Uncatchable);
        assert_eq!(m.disposition(SIGSTOP.num), Disposition::Uncatchable);
        assert!(m.disposition(SIGKILL.num).effective());
    }

    #[test]
    fn blocked_outranks_nothing_but_default() {
        let m = SigMask {
            ignored: 0,
            caught: 0,
            blocked: 1 << 14,
        };
        assert_eq!(m.disposition(SIGTERM.num), Disposition::Blocked);
    }

    #[test]
    fn signalling_init_is_refused_before_the_syscall() {
        assert!(send(1, SIGTERM.num).is_err());
        assert!(send(0, SIGTERM.num).is_err());
    }

    #[test]
    fn we_are_alive() {
        assert!(alive(std::process::id() as i32));
        assert!(!alive(-1));
    }
}
