//! Bowed-string driver (violin, cello, bowed metal).
//!
//! Stick-slip friction: while the relative velocity between bow and string is
//! small the two are stuck and the string is dragged along; past a threshold the
//! string breaks free and slips back. The friction curve below is the standard
//! inverse-power approximation of that characteristic.

use super::{Driver, DriverInput};
use crate::dsp::sanitize;

#[derive(Clone, Copy, Debug)]
pub struct Bow {
    /// Bow force / hair grip. Steeper slope = more force = easier to stick.
    slope_min: f32,
    slope_max: f32,
}

impl Default for Bow {
    fn default() -> Self {
        Self::new()
    }
}

impl Bow {
    pub fn new() -> Self {
        Self {
            slope_min: 5.0,
            slope_max: 0.8,
        }
    }

    #[inline]
    fn friction(delta_v: f32, slope: f32) -> f32 {
        let x = (delta_v * slope + 0.75).abs();
        let f = x.powf(-4.0);
        if f.is_finite() {
            f.min(1.0)
        } else {
            1.0
        }
    }
}

impl Driver for Bow {
    fn configure(&mut self, patch: &crate::patch::DriverPatch) {
        self.slope_min = patch.bow_slope.0;
        self.slope_max = patch.bow_slope.1;
    }

    fn reset(&mut self) {}

    fn set_frequency(&mut self, _hz: f32) {}

    #[inline]
    fn tick(&mut self, input: &DriverInput) -> f32 {
        // For a bow, "breath" is bow velocity and "embouchure" is bow force.
        let bow_velocity = input.breath * (1.0 - input.tonguing.clamp(0.0, 1.0) * 0.9);
        let string = sanitize(input.bore);

        let emb = input.embouchure.clamp(0.0, 1.0);
        let slope = self.slope_min + (self.slope_max - self.slope_min) * emb;

        let delta_v = bow_velocity - string + input.noise;
        let mut f = Self::friction(delta_v, slope);

        // Scream = bow pressure past the point where the string can release
        // cleanly; the tone breaks up into the classic scratchy multiphonic.
        f = (f - input.scream.clamp(0.0, 1.0) * 0.5 * string.abs()).clamp(0.0, 1.0);

        sanitize(string + delta_v * f)
    }
}
