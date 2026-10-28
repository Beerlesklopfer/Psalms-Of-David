// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Time stretching that keeps the pitch: WSOLA (waveform similarity overlap-add).
//!
//! The output is built from windows of the input that overlap by half. The windows are
//! taken `hop * speed` frames apart, so the audio runs faster (speed > 1) or slower
//! (speed < 1). Each window is nudged by a few milliseconds to the place where it
//! fits the end of the previous one best, which keeps voices free of the warbling that
//! plain overlap-add produces.
//!
//! The stretcher is fed in blocks (`push`) and gives output in blocks of one hop
//! (`pull`). It runs on the decoder thread, never on the real-time callback.

use std::f32::consts::PI;

/// Length of one hop, and so half a window.
const HOP_MS: usize = 14;
/// How far a window may be moved to find the best fit, in each direction.
const TOLERANCE_MS: usize = 10;

pub struct Stretcher {
    channels: usize,
    hop: usize,
    window: usize,
    tolerance: usize,
    speed: f64,
    hann: Vec<f32>,
    /// Interleaved input not yet dropped; its first frame has the absolute index `base`.
    input: Vec<f32>,
    /// The same input mixed down to one channel, for the search.
    mono: Vec<f32>,
    base: u64,
    /// Where the next window nominally starts, in absolute input frames.
    position: f64,
    /// Overlap-add buffer: the second half of the last window waits here.
    acc: Vec<f32>,
    /// What the input does after the last window, mono: the next window should resemble it.
    target: Vec<f32>,
    primed: bool,
    pushed: u64,
}

impl Stretcher {
    pub fn new(rate: u32, channels: usize) -> Stretcher {
        let hop = (rate as usize * HOP_MS / 1000).max(8);
        let window = hop * 2;
        let hann = (0..window).map(|n| 0.5 - 0.5 * (2.0 * PI * n as f32 / window as f32).cos()).collect();
        Stretcher {
            channels,
            hop,
            window,
            tolerance: rate as usize * TOLERANCE_MS / 1000,
            speed: 1.0,
            hann,
            input: Vec::new(),
            mono: Vec::new(),
            base: 0,
            position: 0.0,
            acc: vec![0.0; window * channels],
            target: Vec::new(),
            primed: false,
            pushed: 0,
        }
    }

    pub fn set_speed(&mut self, speed: f64) {
        self.speed = speed.clamp(0.25, 4.0);
    }

    /// Appends interleaved input.
    pub fn push(&mut self, samples: &[f32]) {
        let frames = samples.len() / self.channels;
        let samples = &samples[..frames * self.channels];
        self.input.extend_from_slice(samples);
        self.mono.extend(samples.chunks_exact(self.channels).map(|f| f.iter().sum::<f32>() / self.channels as f32));
        self.pushed += frames as u64;
    }

    /// Where the analysis is in the input, in frames since the first `push`.
    pub fn position(&self) -> u64 {
        (self.position.round().max(0.0) as u64).min(self.pushed)
    }

    /// The input after the analysis position, unprocessed, and forgets everything.
    /// Used when the speed returns to 1.0, so that no audio is lost.
    pub fn take_rest(&mut self) -> Vec<f32> {
        let skip = (self.position() - self.base.min(self.position())) as usize * self.channels;
        let rest = self.input.get(skip..).unwrap_or(&[]).to_vec();
        self.input.clear();
        self.mono.clear();
        self.base = self.pushed;
        self.acc.fill(0.0);
        self.primed = false;
        self.position = self.pushed as f64;
        rest
    }

    /// Pads the input with silence so that the last real frames come out too.
    pub fn finish(&mut self) {
        let zeros = vec![0.0; (self.window + self.tolerance + self.hop) * self.channels];
        self.push(&zeros);
    }

    /// The next hop of output (interleaved), or `None` if more input is needed.
    pub fn pull(&mut self) -> Option<Vec<f32>> {
        let centre = self.position.round().max(0.0) as u64;
        let available = self.base + (self.mono.len()) as u64;
        if available < centre + (self.tolerance + self.window) as u64 {
            return None;
        }
        let start = if self.primed { self.best_start(centre) } else { centre.max(self.base) };
        let at = (start - self.base) as usize;

        // Add the window to the waiting second half of the last one.
        for n in 0..self.window {
            let mut gain = self.hann[n];
            if !self.primed && n < self.hop {
                gain = 1.0; // the very start fades in from nothing otherwise
            }
            for c in 0..self.channels {
                self.acc[n * self.channels + c] += self.input[(at + n) * self.channels + c] * gain;
            }
        }
        let out: Vec<f32> = self.acc[..self.hop * self.channels].to_vec();
        self.acc.copy_within(self.hop * self.channels.., 0);
        let tail = self.acc.len() - self.hop * self.channels;
        self.acc[tail..].fill(0.0);

        self.target.clear();
        self.target.extend_from_slice(&self.mono[at + self.hop..at + 2 * self.hop]);
        self.primed = true;
        self.position += self.hop as f64 * self.speed;

        // Frames before the lowest place the next search can reach are no longer needed.
        let keep_from = (self.position.round().max(0.0) as u64).saturating_sub(self.tolerance as u64).max(self.base);
        let drop = (keep_from - self.base) as usize;
        if drop > 0 {
            self.input.drain(..drop * self.channels);
            self.mono.drain(..drop);
            self.base = keep_from;
        }
        Some(out)
    }

    /// The window start in `centre ± tolerance` whose first hop is most like `target`.
    fn best_start(&self, centre: u64) -> u64 {
        let low = centre.saturating_sub(self.tolerance as u64).max(self.base);
        let high = centre + self.tolerance as u64;
        let score = |start: u64| -> f32 {
            let at = (start - self.base) as usize;
            let candidate = &self.mono[at..at + self.hop];
            let (mut dot, mut energy) = (0.0f32, 1e-6f32);
            for (a, b) in candidate.iter().zip(&self.target) {
                dot += a * b;
                energy += a * a;
            }
            dot / energy.sqrt()
        };
        // Coarse search in steps of two frames, then the neighbours of the best.
        let (mut best, mut best_score) = (low, f32::MIN);
        let mut s = low;
        while s <= high {
            let v = score(s);
            if v > best_score {
                (best, best_score) = (s, v);
            }
            s += 2;
        }
        for s in [best.saturating_sub(1).max(low), (best + 1).min(high)] {
            let v = score(s);
            if v > best_score {
                (best, best_score) = (s, v);
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 44_100;

    fn sine(frames: usize, channels: usize, hz: f32) -> Vec<f32> {
        (0..frames)
            .flat_map(|n| {
                let v = 0.5 * (2.0 * PI * hz * n as f32 / RATE as f32).sin();
                std::iter::repeat_n(v, channels)
            })
            .collect()
    }

    fn stretch(input: &[f32], channels: usize, speed: f64) -> Vec<f32> {
        let mut st = Stretcher::new(RATE, channels);
        st.set_speed(speed);
        let mut out = Vec::new();
        for block in input.chunks(4096 * channels) {
            st.push(block);
            while let Some(o) = st.pull() {
                out.extend(o);
            }
        }
        out
    }

    /// Frequency by counting rising zero crossings in the middle of the signal.
    fn frequency(mono: &[f32]) -> f32 {
        let part = &mono[mono.len() / 4..mono.len() * 3 / 4];
        let crossings = part.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        crossings as f32 * RATE as f32 / part.len() as f32
    }

    #[test]
    fn keeps_the_pitch_and_scales_the_length() {
        let input = sine(RATE as usize * 3, 1, 440.0);
        for speed in [0.5, 0.75, 1.25, 1.5, 2.0] {
            let out = stretch(&input, 1, speed);
            let expected = input.len() as f64 / speed;
            assert!((out.len() as f64 - expected).abs() < expected * 0.04, "speed {speed}: {} frames, expected {expected}", out.len());
            let hz = frequency(&out);
            assert!((hz - 440.0).abs() < 440.0 * 0.01, "speed {speed}: {hz} Hz");
            let rms = (out.iter().map(|v| v * v).sum::<f32>() / out.len() as f32).sqrt();
            assert!((rms - 0.3535).abs() < 0.04, "speed {speed}: rms {rms}");
        }
    }

    #[test]
    fn channels_stay_together() {
        let input: Vec<f32> = sine(RATE as usize, 2, 300.0);
        let out = stretch(&input, 2, 1.5);
        assert!(out.as_chunks::<2>().0.iter().all(|f| (f[0] - f[1]).abs() < 1e-6));
        assert!(out.len() > 2 * RATE as usize / 2);
    }

    #[test]
    fn position_follows_the_input() {
        let mut st = Stretcher::new(RATE, 1);
        st.set_speed(2.0);
        st.push(&sine(RATE as usize * 2, 1, 200.0));
        let mut hops = 0;
        while st.pull().is_some() {
            hops += 1;
        }
        let expected = hops * st.hop * 2;
        assert!((st.position() as i64 - expected as i64).abs() <= 1, "{} vs {expected}", st.position());
    }

    #[test]
    fn take_rest_gives_back_the_unprocessed_input() {
        let mut st = Stretcher::new(RATE, 1);
        st.set_speed(1.5);
        let input = sine(RATE as usize, 1, 200.0);
        st.push(&input);
        while st.pull().is_some() {}
        let at = st.position() as usize;
        let rest = st.take_rest();
        assert_eq!(rest.len(), input.len() - at);
        assert_eq!(rest[..], input[at..]);
        assert!(st.pull().is_none());
    }

    #[test]
    fn finish_flushes_the_tail() {
        let mut st = Stretcher::new(RATE, 1);
        st.set_speed(1.5);
        st.push(&sine(RATE as usize / 2, 1, 200.0));
        st.finish();
        let mut frames = 0;
        while let Some(o) = st.pull() {
            frames += o.len();
        }
        assert!(frames as f64 > RATE as f64 / 2.0 / 1.5 * 0.95);
    }
}
