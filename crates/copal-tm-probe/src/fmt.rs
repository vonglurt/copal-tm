// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Paul Richeson
//! Formatting readings for display.
//!
//! Every number in this program is fixed-width and fixed-decimal, because a
//! figure that changes width jitters in place and a jittering figure cannot be
//! read at a glance.  `00.0%` is the image's format and it is this one's.

/// `10.6%`, always one decimal, always at least four characters.
pub fn pct(v: f32) -> String {
    format!("{:>4.1}%", (v * 100.0).clamp(0.0, 999.9))
}

/// `3.8 GB`.  Binary multiples, decimal notation, as every task manager
/// since the first one has quietly done.
pub fn bytes(b: u64) -> String {
    const U: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i + 1 < U.len() {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{} B", b)
    } else if v >= 100.0 {
        format!("{:.0} {}", v, U[i])
    } else {
        format!("{:.1} {}", v, U[i])
    }
}

pub fn kb(k: u64) -> String {
    bytes(k.saturating_mul(1024))
}

/// `5.97 KB/s`, matching the tile's headline in the reference image.
pub fn rate(bps: f64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bps.max(0.0);
    let mut i = 0;
    while v >= 1024.0 && i + 1 < U.len() {
        v /= 1024.0;
        i += 1;
    }
    format!("{:.2} {}/s", v, U[i])
}

/// `2:41` - hours and minutes, the lap-time format.
pub fn hm(secs: f64) -> String {
    if !secs.is_finite() || secs < 0.0 {
        return "--:--".into();
    }
    let t = secs as u64;
    format!("{}:{:02}", t / 3600, (t % 3600) / 60)
}

/// `6d 04:12`, for uptime.
pub fn uptime(secs: f64) -> String {
    let t = secs.max(0.0) as u64;
    let d = t / 86400;
    let h = (t % 86400) / 3600;
    let m = (t % 3600) / 60;
    if d > 0 {
        format!("{d}d {h:02}:{m:02}")
    } else {
        format!("{h:02}:{m:02}:{:02}", t % 60)
    }
}

/// The TIME+ column: cumulative CPU time. Three forms, each at most nine
/// characters, because a figure that outgrows its column is truncated from the
/// left and then reads as a smaller number than it is.
///
/// `12:34.56` under an hour, `19h26:30` under a hundred, `4d 09h` beyond.
pub fn cputime(secs: f64) -> String {
    let t = secs.max(0.0);
    let total = t as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = t % 60.0;
    if h == 0 {
        format!("{m}:{s:05.2}")
    } else if h < 100 {
        format!("{h}h{m:02}:{:02}", total % 60)
    } else {
        format!("{}d {:02}h", h / 24, h % 24)
    }
}

/// `35.1 °C`.
pub fn celsius(c: f32) -> String {
    format!("{:.1} \u{b0}C", c)
}

/// `9.4 W`.
pub fn watts(w: f32) -> String {
    format!("{:.1} W", w)
}

/// Shorten a long string in the middle, keeping both ends, because the
/// interesting part of a command line is at both ends and never the middle.
pub fn elide(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max || max < 4 {
        return s.chars().take(max).collect();
    }
    let keep = max - 1;
    let head = keep / 2 + keep % 2;
    let tail = keep - head;
    let h: String = s.chars().take(head).collect();
    let t: String = s.chars().skip(n - tail).collect();
    format!("{h}\u{2026}{t}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentages_hold_their_width() {
        assert_eq!(pct(0.106), "10.6%");
        assert_eq!(pct(0.08), " 8.0%");
        assert_eq!(pct(1.0), "100.0%");
        assert_eq!(pct(0.0), " 0.0%");
    }

    #[test]
    fn byte_sizes_read_like_the_image() {
        assert_eq!(bytes(3_951_369_912), "3.7 GB");
        assert_eq!(bytes(511 * 1024 * 1024), "511 MB");
        assert_eq!(bytes(900), "900 B");
        assert_eq!(rate(6113.0), "5.97 KB/s");
    }

    #[test]
    fn times() {
        assert_eq!(hm(9660.0), "2:41");
        assert_eq!(uptime(534_720.0), "6d 04:32");
        assert_eq!(cputime(754.56), "12:34.56");
        assert_eq!(cputime(69990.0), "19h26:30");
        assert_eq!(cputime(378_000.0), "4d 09h");
        for t in [0.0, 1.0, 3599.0, 3600.0, 359_999.0, 900_000.0] {
            assert!(cputime(t).chars().count() <= 9, "{t}: {}", cputime(t));
        }
    }

    #[test]
    fn elision_keeps_both_ends() {
        assert_eq!(elide("abcdefghij", 10), "abcdefghij");
        assert_eq!(elide("/usr/lib/firefox/firefox", 12), "/usr/l\u{2026}refox");
        assert_eq!(elide("short", 20), "short");
    }
}
