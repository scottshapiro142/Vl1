//! Single-reed driver (clarinet, saxophone).
//!
//! The reed is modelled as a memoryless nonlinear reflection coefficient driven
//! by the pressure difference across it. When mouth pressure exceeds bore
//! pressure the reed is pushed closed, flow is throttled, and the resulting
//! hysteresis-free "beating reed" characteristic sustains the oscillation.

use super::{Driver, DriverInput};
use crate::dsp::{sanitize, OnePole};

#[derive(Clone, Copy, Debug)]
pub struct Reed {
    /// The reed has mass: it cannot follow arbitrarily fast pressure changes.
    /// That lag is what keeps the tube in its low register — without it the
    /// upper modes get just as much regeneration as the fundamental and the
    /// note jumps register on its own.
    inertia: OnePole,
    cutoff: f32,
    sample_rate: f32,
    /// Reed table offset: the reflection coefficient at zero pressure difference.
    offset_min: f32,
    offset_max: f32,
    /// Reed table slope magnitude: how quickly the reed closes.
    slope_min: f32,
    slope_max: f32,
}

impl Reed {
    pub fn new(sample_rate: f32) -> Self {
        let mut inertia = OnePole::new();
        inertia.set_lowpass(3000.0, sample_rate);
        Self {
            inertia,
            cutoff: 3000.0,
            sample_rate,
            offset_min: 0.55,
            offset_max: 0.80,
            slope_min: -0.22,
            slope_max: -0.62,
        }
    }

    /// Reed resonance in Hz: how fast the reed can respond.
    pub fn set_cutoff(&mut self, hz: f32) {
        self.cutoff = hz.clamp(200.0, 16000.0);
        self.inertia.set_lowpass(self.cutoff, self.sample_rate);
    }

    /// Override the reed table endpoints that embouchure interpolates between.
    pub fn set_table(&mut self, offset: (f32, f32), slope: (f32, f32)) {
        self.offset_min = offset.0;
        self.offset_max = offset.1;
        self.slope_min = slope.0;
        self.slope_max = slope.1;
    }
}

impl Driver for Reed {
    fn configure(&mut self, patch: &crate::patch::DriverPatch) {
        self.set_table(patch.reed_offset, patch.reed_slope);
        self.set_cutoff(patch.reed_cutoff);
    }

    fn reset(&mut self) {
        self.inertia.clear();
    }

    fn set_frequency(&mut self, _hz: f32) {}

    #[inline]
    fn tick(&mut self, input: &DriverInput) -> f32 {
        // Tonguing chokes the air column at the mouthpiece.
        let breath = input.breath * (1.0 - input.tonguing.clamp(0.0, 1.0)) + input.noise;
        let bore = sanitize(input.bore);

        // Pressure difference across the reed. `bore` already carries the
        // inverting reflection from the open end of the pipe.
        let pd = bore - breath;

        let emb = input.embouchure.clamp(0.0, 1.0);
        let offset = self.offset_min + (self.offset_max - self.offset_min) * emb;
        let slope = self.slope_min + (self.slope_max - self.slope_min) * emb;

        // "Scream": let the bore modulate the reed's operating point. Above a
        // threshold the loop period-doubles and then breaks into chaos, which is
        // exactly the multiphonic squeal the VL1 named this control after.
        let bias = input.scream.clamp(0.0, 1.0) * bore * 1.6;

        // Reed reflection coefficient, clipped where the reed beats closed.
        // The table is driven through the reed's own inertia, but the pressure
        // difference that the flow is computed from is not.
        let refl = (offset + slope * self.inertia.tick(pd) + bias).clamp(-1.0, 1.0);

        sanitize(breath + pd * refl)
    }
}
