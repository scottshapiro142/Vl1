//! Low-frequency oscillators for vibrato, growl and tremolo.

use super::noise::Noise;
use std::f32::consts::TAU;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LfoShape {
    Sine,
    Triangle,
    Square,
    SampleHold,
}

#[derive(Clone, Copy, Debug)]
pub struct Lfo {
    phase: f32,
    inc: f32,
    shape: LfoShape,
    sample_rate: f32,
    hold: f32,
    noise: Noise,
    /// Fade-in ramp, in [0, 1]. Real players' vibrato does not start instantly.
    delay_samples: f32,
    fade_samples: f32,
    age: f32,
}

impl Lfo {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            phase: 0.0,
            inc: 5.0 / sample_rate,
            shape: LfoShape::Sine,
            sample_rate,
            hold: 0.0,
            noise: Noise::new(0x9E37_79B9),
            delay_samples: 0.0,
            fade_samples: 1.0,
            age: 0.0,
        }
    }

    pub fn set_shape(&mut self, shape: LfoShape) {
        self.shape = shape;
    }

    pub fn set_rate(&mut self, hz: f32) {
        self.inc = hz.max(0.0) / self.sample_rate;
    }

    pub fn set_delay(&mut self, delay_s: f32, fade_s: f32) {
        self.delay_samples = (delay_s.max(0.0) * self.sample_rate).max(0.0);
        self.fade_samples = (fade_s.max(1e-3) * self.sample_rate).max(1.0);
    }

    pub fn reset(&mut self, phase: f32) {
        self.phase = phase.rem_euclid(1.0);
        self.age = 0.0;
    }

    /// Output in [-1, 1], scaled by the delay/fade-in ramp.
    #[inline]
    pub fn tick(&mut self) -> f32 {
        let raw = match self.shape {
            LfoShape::Sine => (self.phase * TAU).sin(),
            LfoShape::Triangle => 4.0 * (self.phase - 0.5).abs() - 1.0,
            LfoShape::Square => {
                if self.phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            LfoShape::SampleHold => self.hold,
        };

        let prev = self.phase;
        self.phase += self.inc;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
            if self.shape == LfoShape::SampleHold {
                self.hold = self.noise.white();
            }
        }
        debug_assert!(prev >= 0.0);

        self.age += 1.0;
        let ramp = if self.age < self.delay_samples {
            0.0
        } else {
            ((self.age - self.delay_samples) / self.fade_samples).min(1.0)
        };
        raw * ramp
    }
}
