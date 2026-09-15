// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Paul Richeson
//! The history buffers.
//!
//! One per series, 1024 samples deep - about seventeen minutes at the default
//! one-second tick, and more than any plot can show.  Narrower plots are fed
//! by `bucket_max`, which takes the **maximum** of each bucket rather than the
//! mean: a spike that is averaged away is exactly the event a history exists
//! to show.

#[derive(Clone, Debug)]
pub struct Ring {
    data: Vec<f32>,
    head: usize,
    len: usize,
}

impl Ring {
    pub fn new(cap: usize) -> Ring {
        Ring {
            data: vec![0.0; cap.max(1)],
            head: 0,
            len: 0,
        }
    }

    pub fn cap(&self) -> usize {
        self.data.len()
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn push(&mut self, v: f32) {
        self.data[self.head] = v;
        self.head = (self.head + 1) % self.data.len();
        if self.len < self.data.len() {
            self.len += 1;
        }
    }

    /// Oldest first.
    pub fn iter(&self) -> impl Iterator<Item = f32> + '_ {
        let cap = self.data.len();
        let start = (self.head + cap - self.len) % cap;
        (0..self.len).map(move |i| self.data[(start + i) % cap])
    }

    /// The `n` most recent, oldest first.  Fewer if there are fewer.
    pub fn last_n(&self, n: usize) -> Vec<f32> {
        let n = n.min(self.len);
        self.iter().skip(self.len - n).collect()
    }

    pub fn last(&self) -> f32 {
        if self.len == 0 {
            0.0
        } else {
            self.data[(self.head + self.data.len() - 1) % self.data.len()]
        }
    }

    /// The extent of the `n` most recent samples: `(min, max)`.  The Memory
    /// panel's in-plot corner labels are exactly this.
    pub fn extent(&self, n: usize) -> (f32, f32) {
        let v = self.last_n(n);
        if v.is_empty() {
            return (0.0, 0.0);
        }
        v.iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), &x| (lo.min(x), hi.max(x)))
    }

    pub fn mean(&self, n: usize) -> f32 {
        let v = self.last_n(n);
        if v.is_empty() {
            0.0
        } else {
            v.iter().sum::<f32>() / v.len() as f32
        }
    }

    /// Resample the `span` most recent samples down to `cols` values, taking
    /// the maximum of each bucket.  Returned oldest first, `cols` long, and
    /// padded at the front with the oldest value when there is not yet enough
    /// history - so a chart fills from the right as time passes rather than
    /// stretching a short history across the whole plot.
    pub fn bucket_max(&self, span: usize, cols: usize) -> Vec<f32> {
        if cols == 0 {
            return Vec::new();
        }
        let src = self.last_n(span);
        if src.is_empty() {
            return vec![0.0; cols];
        }
        if src.len() < cols {
            let mut out = vec![src[0]; cols - src.len()];
            out.extend_from_slice(&src);
            return out;
        }
        let mut out = Vec::with_capacity(cols);
        for c in 0..cols {
            let a = c * src.len() / cols;
            let b = ((c + 1) * src.len() / cols).max(a + 1).min(src.len());
            out.push(src[a..b].iter().fold(f32::MIN, |m, &x| m.max(x)));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_wraps_and_keeps_the_newest() {
        let mut r = Ring::new(4);
        for i in 0..6 {
            r.push(i as f32);
        }
        assert_eq!(r.len(), 4);
        assert_eq!(r.iter().collect::<Vec<_>>(), vec![2.0, 3.0, 4.0, 5.0]);
        assert_eq!(r.last(), 5.0);
    }

    #[test]
    fn buckets_take_the_peak_not_the_mean() {
        let mut r = Ring::new(16);
        for i in 0..8 {
            r.push(if i == 3 { 100.0 } else { 1.0 });
        }
        let b = r.bucket_max(8, 4);
        assert_eq!(b.len(), 4);
        assert_eq!(b[1], 100.0, "the spike survives the downsample");
    }

    #[test]
    fn a_short_history_fills_from_the_right() {
        let mut r = Ring::new(64);
        r.push(5.0);
        r.push(7.0);
        let b = r.bucket_max(60, 6);
        assert_eq!(b, vec![5.0, 5.0, 5.0, 5.0, 5.0, 7.0]);
    }

    #[test]
    fn extent_is_min_then_max() {
        let mut r = Ring::new(8);
        for v in [3.0, 9.0, 1.0, 4.0] {
            r.push(v);
        }
        assert_eq!(r.extent(4), (1.0, 9.0));
    }
}
