//! The linear resonator: a bidirectional waveguide collapsed into a single
//! loop delay plus termination filtering.
//!
//! This is the VL1's "Pipe/String" block. It contributes no energy of its own —
//! it stores the driver's output for one acoustic round trip, loses a
//! frequency-dependent fraction of it, and hands what is left back to the
//! driver with the right sign.

use crate::dsp::DelayLine;
use crate::dsp::{sanitize, DcBlock, OnePole};

/// How the resonator terminates, which decides its harmonic series.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipeMode {
    /// Closed at the driver, open at the far end: one sign inversion per round
    /// trip, so only odd harmonics survive. A cylinder driven by a reed — the
    /// clarinet's hollow, octave-shy voice.
    OddHarmonics,
    /// Effectively open at both ends (or a cone): the full harmonic series.
    /// Flutes, brass, saxophones, strings.
    AllHarmonics,
}

/// A lossy acoustic tube or string.
#[derive(Clone, Debug)]
pub struct Waveguide {
    delay: DelayLine,
    damping: OnePole,
    dc: DcBlock,
    sample_rate: f32,
    /// Broadband loss per round trip, |absorption| < 1.
    absorption: f32,
    /// Termination lowpass cutoff.
    damping_hz: f32,
    mode: PipeMode,
    freq: f32,
    tuning_ratio: f32,
    /// The reflected wave produced by the most recent tick.
    reflected: f32,
    loop_delay: f32,
}

impl Waveguide {
    pub fn new(sample_rate: f32) -> Self {
        // Room for the round trip of the lowest useful note.
        let max = (sample_rate / 15.0) as usize;
        let mut wg = Self {
            delay: DelayLine::new(max),
            damping: OnePole::new(),
            dc: DcBlock::new(),
            sample_rate,
            absorption: 0.97,
            damping_hz: 4000.0,
            mode: PipeMode::AllHarmonics,
            freq: 220.0,
            tuning_ratio: 1.0,
            reflected: 0.0,
            loop_delay: 100.0,
        };
        wg.dc.set_cutoff(8.0, sample_rate);
        wg.damping.set_lowpass(4000.0, sample_rate);
        wg.retune();
        wg
    }

    pub fn reset(&mut self) {
        self.delay.clear();
        self.damping.clear();
        self.dc.clear();
        self.reflected = 0.0;
    }

    pub fn set_mode(&mut self, mode: PipeMode) {
        self.mode = mode;
        self.retune();
    }

    /// Fine length scaling — an alternate fingering, a slide position, or simply
    /// a patch that wants to sit slightly sharp of the tube it nominally models.
    pub fn set_tuning_ratio(&mut self, ratio: f32) {
        self.tuning_ratio = ratio.clamp(0.25, 4.0);
        self.retune();
    }

    /// Frequency-dependent loss at the terminations. Low cutoff = dull, heavily
    /// damped, short-sounding; high cutoff = bright and ringing.
    pub fn set_damping(&mut self, cutoff_hz: f32) {
        let hz = cutoff_hz.clamp(200.0, self.sample_rate * 0.48);
        // The engine pushes a new cutoff every control tick as the envelope
        // moves. Retuning involves several transcendentals, and a fraction of a
        // percent of cutoff change is not worth them.
        if (hz - self.damping_hz).abs() < self.damping_hz * 0.005 {
            return;
        }
        self.damping_hz = hz;
        self.damping.set_lowpass(self.damping_hz, self.sample_rate);
        self.retune();
    }

    /// Broadband round-trip loss in [0, 1]; 1 is lossless (and will ring forever).
    pub fn set_absorption(&mut self, gain: f32) {
        self.absorption = gain.clamp(0.0, 0.9999);
    }

    pub fn set_frequency(&mut self, hz: f32) {
        self.freq = hz.clamp(8.0, self.sample_rate * 0.45);
        self.retune();
    }

    /// The loop delay in samples, as currently tuned.
    pub fn loop_delay(&self) -> f32 {
        self.loop_delay
    }

    fn retune(&mut self) {
        // Target round-trip length. An inverting loop resonates at SR/(2D), a
        // non-inverting one at SR/D.
        let period = self.sample_rate / self.freq * self.tuning_ratio;
        let target = match self.mode {
            PipeMode::OddHarmonics => period * 0.5,
            PipeMode::AllHarmonics => period,
        };

        // The loop's DC blocker is a high-pass inside the feedback path: its
        // phase lead shortens the round trip and sharpens the pitch, worth some
        // 50 cents on a low clarinet note. Only part of that lead is worth
        // taking back. Compensating for all of it over-corrects every model into
        // flatness — the oscillation is set by a nonlinear driver, not by the
        // loop's linear phase alone — and this fraction is what measures best
        // across all four driver types and the whole key range.
        const DC_COMPENSATION: f32 = 0.4;
        let d = target
            - 1.0
            - self.damping.phase_delay(self.freq, self.sample_rate)
            - DC_COMPENSATION * self.dc.phase_delay(self.freq, self.sample_rate);
        self.loop_delay = d.max(1.0);
        self.delay.set_delay(self.loop_delay);
    }

    /// The reflected wave the driver should see on the next sample.
    #[inline]
    pub fn reflected(&self) -> f32 {
        self.reflected
    }

    /// Push the driver's output into the tube; returns the pressure at the tap
    /// point, which is the element's raw acoustic output.
    #[inline]
    pub fn tick(&mut self, input: f32) -> f32 {
        let out = self.delay.tick(sanitize(input));
        let sign = match self.mode {
            PipeMode::OddHarmonics => -1.0,
            PipeMode::AllHarmonics => 1.0,
        };
        let damped = self.dc.tick(self.damping.tick(out));
        self.reflected = sanitize(damped * self.absorption * sign);
        out
    }

    /// Tap the standing wave part-way along the tube. Mixing a tap in adds the
    /// comb colouration of a real bell/soundhole position.
    pub fn tap(&self, fraction: f32) -> f32 {
        self.delay
            .tap((self.loop_delay * fraction.clamp(0.0, 1.0)).max(1.0))
    }
}
