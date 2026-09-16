//! Master effects: chorus and reverb.
//!
//! The VL1's models are dry and directional by nature — a waveguide has no room
//! around it. A little space is not decoration here, it is most of what sells
//! the illusion of an instrument in front of you.

use crate::dsp::{sanitize, DelayLine, Lfo, OnePole};
use crate::patch::{ChorusPatch, ReverbPatch};

/// Two-tap modulated delay, opposed LFO phases for a wide stereo image.
pub struct Chorus {
    lines: [DelayLine; 2],
    lfos: [Lfo; 2],
    base_samples: f32,
    depth_samples: f32,
    mix: f32,
    sample_rate: f32,
}

impl Chorus {
    pub fn new(sample_rate: f32) -> Self {
        let max = (sample_rate * 0.05) as usize;
        let mut lfos = [Lfo::new(sample_rate), Lfo::new(sample_rate)];
        lfos[1].reset(0.5);
        Self {
            lines: [DelayLine::new(max), DelayLine::new(max)],
            lfos,
            base_samples: sample_rate * 0.012,
            depth_samples: sample_rate * 0.0025,
            mix: 0.0,
            sample_rate,
        }
    }

    pub fn configure(&mut self, p: &ChorusPatch) {
        for lfo in &mut self.lfos {
            lfo.set_rate(p.rate.max(0.01));
            lfo.set_delay(0.0, 0.01);
        }
        self.lfos[1].reset(0.5 * p.spread.clamp(0.0, 1.0));
        self.depth_samples = self.sample_rate * (p.depth_ms.max(0.0) * 0.001);
        self.mix = p.mix.clamp(0.0, 1.0);
    }

    pub fn reset(&mut self) {
        self.lines.iter_mut().for_each(|l| l.clear());
    }

    #[inline]
    pub fn tick(&mut self, input: [f32; 2]) -> [f32; 2] {
        if self.mix <= 0.0 {
            return input;
        }
        let mut out = [0.0f32; 2];
        for ch in 0..2 {
            let m = self.lfos[ch].tick();
            self.lines[ch].set_delay((self.base_samples + m * self.depth_samples).max(2.0));
            let wet = self.lines[ch].tick(input[ch]);
            out[ch] = input[ch] * (1.0 - self.mix * 0.5) + wet * self.mix;
        }
        out
    }
}

struct Comb {
    line: DelayLine,
    damp: OnePole,
    feedback: f32,
}

impl Comb {
    fn new(len: usize) -> Self {
        let mut line = DelayLine::new(len + 2);
        line.set_delay(len as f32);
        Self {
            line,
            damp: OnePole::new(),
            feedback: 0.84,
        }
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let y = self.line.last_out();
        let filtered = self.damp.tick(y);
        self.line.tick(sanitize(x + filtered * self.feedback));
        y
    }
}

struct Allpass {
    line: DelayLine,
}

impl Allpass {
    fn new(len: usize) -> Self {
        let mut line = DelayLine::new(len + 2);
        line.set_delay(len as f32);
        Self { line }
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        const G: f32 = 0.5;
        let delayed = self.line.last_out();
        let v = x + delayed * G;
        self.line.tick(sanitize(v));
        delayed - v * G
    }
}

/// Schroeder/Freeverb-style reverb: damped combs in parallel into allpasses.
pub struct Reverb {
    combs: [[Comb; 4]; 2],
    allpasses: [[Allpass; 2]; 2],
    mix: f32,
    /// Scales the signal entering the comb bank.
    ///
    /// Each comb has a steady-state gain of 1/(1-feedback) — around 7.6 at
    /// typical settings — and four of them run in parallel. Fed raw, the wet
    /// path comes back roughly thirty times louder than the dry one, which
    /// swamps the instrument instead of placing it in a room.
    input_gain: f32,
    sample_rate: f32,
}

const COMB_TUNING: [usize; 4] = [1116, 1188, 1277, 1356];
const ALLPASS_TUNING: [usize; 2] = [556, 441];
const STEREO_SPREAD: usize = 23;

impl Reverb {
    pub fn new(sample_rate: f32) -> Self {
        let scale = sample_rate / 44100.0;
        let len = |n: usize, ch: usize| -> usize {
            ((n + ch * STEREO_SPREAD) as f32 * scale).round().max(4.0) as usize
        };
        let combs = [
            [
                Comb::new(len(COMB_TUNING[0], 0)),
                Comb::new(len(COMB_TUNING[1], 0)),
                Comb::new(len(COMB_TUNING[2], 0)),
                Comb::new(len(COMB_TUNING[3], 0)),
            ],
            [
                Comb::new(len(COMB_TUNING[0], 1)),
                Comb::new(len(COMB_TUNING[1], 1)),
                Comb::new(len(COMB_TUNING[2], 1)),
                Comb::new(len(COMB_TUNING[3], 1)),
            ],
        ];
        let allpasses = [
            [
                Allpass::new(len(ALLPASS_TUNING[0], 0)),
                Allpass::new(len(ALLPASS_TUNING[1], 0)),
            ],
            [
                Allpass::new(len(ALLPASS_TUNING[0], 1)),
                Allpass::new(len(ALLPASS_TUNING[1], 1)),
            ],
        ];
        let mut rv = Self {
            combs,
            allpasses,
            mix: 0.0,
            input_gain: 0.03,
            sample_rate,
        };
        rv.configure(&ReverbPatch::default());
        rv
    }

    pub fn configure(&mut self, p: &ReverbPatch) {
        let feedback = 0.70 + 0.28 * p.size.clamp(0.0, 1.0);
        let damp_hz = 12000.0 * (1.0 - p.damping.clamp(0.0, 0.99)).max(0.05);
        for ch in &mut self.combs {
            for comb in ch.iter_mut() {
                comb.feedback = feedback;
                comb.damp.set_lowpass(damp_hz, self.sample_rate);
            }
        }
        self.input_gain = (1.0 - feedback) / COMB_TUNING.len() as f32;
        self.mix = p.mix.clamp(0.0, 1.0);
    }

    pub fn reset(&mut self) {
        for ch in &mut self.combs {
            for comb in ch.iter_mut() {
                comb.line.clear();
                comb.damp.clear();
            }
        }
        for ch in &mut self.allpasses {
            for ap in ch.iter_mut() {
                ap.line.clear();
            }
        }
    }

    #[inline]
    pub fn tick(&mut self, input: [f32; 2]) -> [f32; 2] {
        if self.mix <= 0.0 {
            return input;
        }
        let mono = (input[0] + input[1]) * 0.5 * self.input_gain;
        let mut out = [0.0f32; 2];
        for ch in 0..2 {
            let mut wet: f32 = self.combs[ch].iter_mut().map(|c| c.tick(mono)).sum();
            for ap in &mut self.allpasses[ch] {
                wet = ap.tick(wet);
            }
            out[ch] = input[ch] * (1.0 - self.mix) + sanitize(wet) * self.mix;
        }
        out
    }
}
