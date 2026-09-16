// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Paul Richeson
//! `~/.config/copal/taskman.conf` — Section IX of the design report.
//!
//! `key = value`, `#` comments, the same shape and the same directory as the
//! rest of Copal's user configuration. Every key is also a command-line flag
//! of the same name, and `--dump-config` writes the effective configuration
//! with its defaults commented, so the file never has to be written from
//! documentation.

use copal_tm::tty::Charset;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Split,
    Dashboard,
    Browser,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Readout {
    Power,
    Remaining,
    Uptime,
    Load,
    Temp,
    Pressure,
}

impl Readout {
    pub const ALL: [Readout; 6] = [
        Readout::Remaining,
        Readout::Power,
        Readout::Uptime,
        Readout::Load,
        Readout::Temp,
        Readout::Pressure,
    ];
    pub fn next(self) -> Readout {
        let i = Readout::ALL.iter().position(|r| *r == self).unwrap_or(0);
        Readout::ALL[(i + 1) % Readout::ALL.len()]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Meter {
    Cpu,
    Battery,
    Temp,
    Gpu,
    Mem,
    Swap,
    Io,
    Net,
    Load,
    Pressure,
}

impl Meter {
    fn parse(s: &str) -> Option<Meter> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "cpu" => Meter::Cpu,
            "battery" | "bat" => Meter::Battery,
            "temp" => Meter::Temp,
            "gpu" => Meter::Gpu,
            "mem" | "memory" => Meter::Mem,
            "swap" => Meter::Swap,
            "io" | "disk" => Meter::Io,
            "net" | "network" => Meter::Net,
            "load" => Meter::Load,
            "pressure" | "psi" => Meter::Pressure,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sort {
    Cpu,
    Mem,
    Pid,
    Time,
    Name,
    User,
}

impl Sort {
    pub const ALL: [Sort; 6] = [
        Sort::Cpu,
        Sort::Mem,
        Sort::Pid,
        Sort::Time,
        Sort::Name,
        Sort::User,
    ];
    pub fn next(self) -> Sort {
        let i = Sort::ALL.iter().position(|s| *s == self).unwrap_or(0);
        Sort::ALL[(i + 1) % Sort::ALL.len()]
    }
    pub fn label(self) -> &'static str {
        match self {
            Sort::Cpu => "cpu",
            Sort::Mem => "mem",
            Sort::Pid => "pid",
            Sort::Time => "time",
            Sort::Name => "name",
            Sort::User => "user",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub refresh: f64,
    pub view: View,
    pub charset: Charset,
    pub truecolor: bool,
    pub meters: Vec<Meter>,
    pub readout: Readout,
    pub slot: String,
    pub history_span: usize,
    /// `true` when 100 % means the whole machine; `false` when it means one
    /// core, as `top` reports it.
    pub cpu_total_scale: bool,
    pub tree: bool,
    pub kernel: bool,
    pub sort: Sort,
    pub grace: f64,
    pub confirm: bool,
    pub theme: String,
    pub mouse: bool,
    /// Force the simulation, whatever the machine. `make demo`.
    pub simulate: bool,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            refresh: 1.0,
            view: View::Split,
            charset: Charset::detect(),
            truecolor: truecolor_detect(),
            meters: vec![Meter::Cpu, Meter::Battery, Meter::Temp, Meter::Gpu],
            readout: Readout::Remaining,
            slot: "gpu".into(),
            history_span: 300,
            cpu_total_scale: true,
            tree: true,
            kernel: false,
            sort: Sort::Cpu,
            grace: 2.0,
            confirm: true,
            theme: "copal".into(),
            mouse: true,
            simulate: false,
        }
    }
}

fn truecolor_detect() -> bool {
    let ct = std::env::var("COLORTERM")
        .unwrap_or_default()
        .to_ascii_lowercase();
    if ct.contains("truecolor") || ct.contains("24bit") {
        return true;
    }
    // A terminal that says nothing is asked to prove nothing: most do speak
    // it, and the 16-colour fallback is one configuration key away.
    !matches!(
        std::env::var("TERM").unwrap_or_default().as_str(),
        "linux" | "dumb" | "vt100"
    )
}

pub fn path() -> std::path::PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("{}/.config", std::env::var("HOME").unwrap_or_default()));
    std::path::PathBuf::from(base)
        .join("copal")
        .join("taskman.conf")
}

impl Config {
    /// The file, then the command line, so a flag always wins.
    pub fn load(args: &[String]) -> (Config, Vec<String>) {
        let mut cfg = Config::default();
        let mut notes = Vec::new();
        let p = path();
        if let Ok(text) = std::fs::read_to_string(&p) {
            for (n, line) in text.lines().enumerate() {
                let line = line.split('#').next().unwrap_or("").trim();
                if line.is_empty() {
                    continue;
                }
                let Some((k, v)) = line.split_once('=') else {
                    notes.push(format!("{}:{}: not a key = value line", p.display(), n + 1));
                    continue;
                };
                if let Err(e) = cfg.set(k.trim(), v.trim()) {
                    notes.push(format!("{}:{}: {e}", p.display(), n + 1));
                }
            }
        }
        let mut i = 0;
        while i < args.len() {
            let a = &args[i];
            if let Some(rest) = a.strip_prefix("--") {
                let (k, v) = match rest.split_once('=') {
                    Some((k, v)) => (k.to_string(), v.to_string()),
                    None => {
                        // A bare flag is a boolean; anything else takes the
                        // next argument.
                        let k = rest.to_string();
                        let next = args.get(i + 1).cloned();
                        match next {
                            Some(v) if !v.starts_with("--") && needs_value(&k) => {
                                i += 1;
                                (k, v)
                            }
                            _ => (k, "true".into()),
                        }
                    }
                };
                if let Err(e) = cfg.set(&k, &v) {
                    notes.push(format!("--{k}: {e}"));
                }
            }
            i += 1;
        }
        (cfg, notes)
    }

    pub fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
        match key {
            "refresh" => {
                self.refresh = value
                    .parse::<f64>()
                    .map_err(|_| "not a number".to_string())?
                    .clamp(0.25, 10.0)
            }
            "view" => {
                self.view = match value {
                    "split" => View::Split,
                    "dashboard" => View::Dashboard,
                    "browser" => View::Browser,
                    _ => return Err("expected split, dashboard or browser".into()),
                }
            }
            "charset" => {
                self.charset =
                    Charset::parse(value).ok_or("expected auto, full, blocks or ascii")?
            }
            "color" => match value {
                "auto" => self.truecolor = truecolor_detect(),
                "truecolor" => self.truecolor = true,
                "16" => self.truecolor = false,
                _ => return Err("expected auto, truecolor or 16".into()),
            },
            "sys.meters" => {
                let v: Vec<Meter> = value.split(',').filter_map(Meter::parse).collect();
                if v.is_empty() {
                    return Err("no meter names recognised".into());
                }
                self.meters = v.into_iter().take(6).collect();
            }
            "sys.readout" => {
                self.readout = match value {
                    "power" => Readout::Power,
                    "remaining" => Readout::Remaining,
                    "uptime" => Readout::Uptime,
                    "load" => Readout::Load,
                    "temp" => Readout::Temp,
                    "pressure" => Readout::Pressure,
                    _ => return Err("unknown readout".into()),
                }
            }
            "slot.tile" => self.slot = value.to_string(),
            "history.span" => {
                self.history_span = value
                    .parse::<usize>()
                    .map_err(|_| "not a number")?
                    .clamp(30, 1024)
            }
            "cpu.scale" => match value {
                "total" => self.cpu_total_scale = true,
                "core" => self.cpu_total_scale = false,
                _ => return Err("expected total or core".into()),
            },
            "browser.tree" => self.tree = truthy(value),
            "browser.kernel" => self.kernel = truthy(value),
            "browser.sort" => {
                self.sort = Sort::ALL
                    .iter()
                    .copied()
                    .find(|s| s.label() == value)
                    .ok_or("unknown sort key")?
            }
            "halt.grace" => {
                self.grace = value
                    .parse::<f64>()
                    .map_err(|_| "not a number")?
                    .clamp(0.0, 60.0)
            }
            "halt.confirm" => self.confirm = truthy(value),
            "theme" => self.theme = value.to_string(),
            "mouse" => self.mouse = truthy(value),
            "simulate" | "demo" => self.simulate = truthy(value),
            "help" | "version" | "dump-config" | "frame" => {}
            _ => return Err("unknown key".into()),
        }
        Ok(())
    }

    /// The effective configuration, as a file that can be saved as-is.
    pub fn dump(&self) -> String {
        format!(
            "# copal-tm, written by --dump-config\n\
             # {}\n\n\
             refresh        = {}\n\
             view           = {}\n\
             charset        = {}\n\
             color          = {}\n\
             sys.meters     = {}\n\
             sys.readout    = {}\n\
             slot.tile      = {}\n\
             history.span   = {}\n\
             cpu.scale      = {}\n\
             browser.tree   = {}\n\
             browser.kernel = {}\n\
             browser.sort   = {}\n\
             halt.grace     = {}\n\
             halt.confirm   = {}\n\
             theme          = {}\n\
             mouse          = {}\n",
            path().display(),
            self.refresh,
            match self.view {
                View::Split => "split",
                View::Dashboard => "dashboard",
                View::Browser => "browser",
            },
            match self.charset {
                Charset::Full => "full",
                Charset::Blocks => "blocks",
                Charset::Ascii => "ascii",
            },
            if self.truecolor { "truecolor" } else { "16" },
            self.meters
                .iter()
                .map(|m| format!("{m:?}").to_ascii_lowercase())
                .collect::<Vec<_>>()
                .join(","),
            format!("{:?}", self.readout).to_ascii_lowercase(),
            self.slot,
            self.history_span,
            if self.cpu_total_scale {
                "total"
            } else {
                "core"
            },
            self.tree,
            self.kernel,
            self.sort.label(),
            self.grace,
            self.confirm,
            self.theme,
            self.mouse,
        )
    }
}

fn truthy(v: &str) -> bool {
    matches!(
        v.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn needs_value(k: &str) -> bool {
    !matches!(k, "help" | "version" | "dump-config" | "simulate" | "demo") || k == "frame"
}

pub const USAGE: &str = "\
copal-tm - the Copal task manager: an instrument panel and a process browser

usage: copal-tm [options]

  --refresh SECONDS      tick period, 0.25 .. 10          (default 1.0)
  --view MODE            split | dashboard | browser      (default split)
  --charset SET          auto | full | blocks | ascii
  --color MODE           auto | truecolor | 16
  --sys.meters LIST      cpu,battery,temp,gpu,mem,swap,io,net,load,pressure
  --sys.readout WHICH    power | remaining | uptime | load | temp | pressure
  --slot.tile WHICH      gpu | swap-rate | load | processes | throttle
  --cpu.scale MODE       total | core
  --browser.sort KEY     cpu | mem | pid | time | name | user
  --browser.tree BOOL    nest the process tree           (default true)
  --browser.kernel BOOL  show kernel threads             (default false)
  --halt.grace SECONDS   between rungs of the halt ladder (default 2.0)
  --theme NAME           copal | mono
  --simulate             run against the simulation, whatever the machine
  --frame WxH            render one frame to stdout and exit (for screenshots)
  --dump-config          write the effective configuration and exit
  --help, --version

Configuration file: ~/.config/copal/taskman.conf
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flag_overrides_the_default() {
        let args: Vec<String> = ["--refresh", "2.5", "--view", "browser", "--simulate"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let (cfg, notes) = Config::load(&args);
        assert_eq!(cfg.refresh, 2.5);
        assert_eq!(cfg.view, View::Browser);
        assert!(cfg.simulate);
        assert!(notes.is_empty(), "{notes:?}");
    }

    #[test]
    fn out_of_range_values_are_clamped_not_rejected() {
        let mut c = Config::default();
        c.set("refresh", "900").unwrap();
        assert_eq!(c.refresh, 10.0);
        c.set("refresh", "0.001").unwrap();
        assert_eq!(c.refresh, 0.25);
    }

    #[test]
    fn a_bad_key_is_reported_and_does_not_stop_the_rest() {
        let args: Vec<String> = ["--nonsense=1", "--refresh=3"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let (cfg, notes) = Config::load(&args);
        assert_eq!(cfg.refresh, 3.0);
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("nonsense"));
    }

    #[test]
    fn the_dump_round_trips_through_the_parser() {
        let mut a = Config::default();
        a.set("sys.meters", "cpu,mem,io,net").unwrap();
        a.set("browser.sort", "mem").unwrap();
        a.set("theme", "mono").unwrap();
        let text = a.dump();
        let mut b = Config::default();
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            if let Some((k, v)) = line.split_once('=') {
                b.set(k.trim(), v.trim())
                    .unwrap_or_else(|e| panic!("{k}: {e}"));
            }
        }
        assert_eq!(b.meters, a.meters);
        assert_eq!(b.sort, a.sort);
        assert_eq!(b.theme, a.theme);
        assert_eq!(b.refresh, a.refresh);
    }

    #[test]
    fn readouts_and_sorts_cycle_back_to_where_they_started() {
        let mut r = Readout::Power;
        for _ in 0..Readout::ALL.len() {
            r = r.next();
        }
        assert_eq!(r, Readout::Power);
        let mut s = Sort::Cpu;
        for _ in 0..Sort::ALL.len() {
            s = s.next();
        }
        assert_eq!(s, Sort::Cpu);
    }
}
