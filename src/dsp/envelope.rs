//! Envelope generators.
//!
//! The VL1's breath envelope is what makes a wind patch feel played rather than
//! triggered: pressure ramps in, overshoots slightly, settles, then decays away.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// ADSR with a linear attack and exponential decay/release segments.
#[derive(Clone, Copy, Debug)]
pub struct Adsr {
    stage: Stage,
    value: f32,
    target: f32,
    attack_inc: f32,
    decay_coef: f32,
    release_coef: f32,
    sustain: f32,
    sample_rate: f32,
}

impl Adsr {
    pub fn new(sample_rate: f32) -> Self {
        let mut env = Self {
            stage: Stage::Idle,
            value: 0.0,
            target: 0.0,
            attack_inc: 1.0,
            decay_coef: 0.999,
            release_coef: 0.999,
            sustain: 0.8,
            sample_rate,
        };
        env.set(0.01, 0.1, 0.8, 0.2);
        env
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }

    /// Times in seconds; sustain is a level in [0, 1].
    pub fn set(&mut self, attack: f32, decay: f32, sustain: f32, release: f32) {
        self.attack_inc = 1.0 / (attack.max(1e-4) * self.sample_rate);
        self.decay_coef = time_to_coef(decay, self.sample_rate);
        self.release_coef = time_to_coef(release, self.sample_rate);
        self.sustain = sustain.clamp(0.0, 1.0);
    }

    pub fn set_release(&mut self, release: f32) {
        self.release_coef = time_to_coef(release, self.sample_rate);
    }

    pub fn stage(&self) -> Stage {
        self.stage
    }

    pub fn value(&self) -> f32 {
        self.value
    }

    pub fn is_active(&self) -> bool {
        self.stage != Stage::Idle
    }

    pub fn gate_on(&mut self) {
        self.stage = Stage::Attack;
        self.target = 1.0;
    }

    /// Re-trigger without returning to zero (legato / slurred articulation).
    pub fn gate_on_legato(&mut self) {
        if self.stage == Stage::Idle || self.stage == Stage::Release {
            self.stage = Stage::Attack;
        }
        self.target = 1.0;
    }

    pub fn gate_off(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
            self.target = 0.0;
        }
    }

    pub fn reset(&mut self) {
        self.stage = Stage::Idle;
        self.value = 0.0;
        self.target = 0.0;
    }

    #[inline]
    pub fn tick(&mut self) -> f32 {
        match self.stage {
            Stage::Idle => self.value = 0.0,
            Stage::Attack => {
                self.value += self.attack_inc;
                if self.value >= 1.0 {
                    self.value = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                self.value = self.sustain + (self.value - self.sustain) * self.decay_coef;
                if (self.value - self.sustain).abs() < 1e-4 {
                    self.value = self.sustain;
                    self.stage = Stage::Sustain;
                }
            }
            Stage::Sustain => self.value = self.sustain,
            Stage::Release => {
                self.value *= self.release_coef;
                if self.value < 1e-4 {
                    self.value = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }
        self.value
    }
}

fn time_to_coef(seconds: f32, sample_rate: f32) -> f32 {
    let n = (seconds.max(1e-4) * sample_rate).max(1.0);
    // Reach ~1/1000 of the distance to target over `seconds`.
    (-6.9078 / n).exp()
}

/// A one-pole smoother for control signals (breath CC, embouchure, pitch glide).
#[derive(Clone, Copy, Debug)]
pub struct Smoother {
    coef: f32,
    value: f32,
}

impl Smoother {
    pub fn new(time_s: f32, sample_rate: f32) -> Self {
        Self {
            coef: time_to_coef(time_s, sample_rate),
            value: 0.0,
        }
    }

    pub fn set_time(&mut self, time_s: f32, sample_rate: f32) {
        self.coef = time_to_coef(time_s, sample_rate);
    }

    pub fn set_immediate(&mut self, v: f32) {
        self.value = v;
    }

    pub fn value(&self) -> f32 {
        self.value
    }

    #[inline]
    pub fn tick(&mut self, target: f32) -> f32 {
        self.value = target + (self.value - target) * self.coef;
        self.value
    }
}
