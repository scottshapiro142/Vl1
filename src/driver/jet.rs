//! Air-jet driver (flute, recorder, shakuhachi, organ flue pipe).
//!
//! A ribbon of air crosses the embouchure hole and flaps above and below the
//! labium. The delay between leaving the player's lips and reaching the edge is
//! a fixed fraction of the bore length, and blowing harder shortens it — which
//! is why a flute overblows to the octave.

use super::{Driver, DriverInput};
use crate::dsp::{sanitize, DelayLine};

#[derive(Clone, Debug)]
pub struct Jet {
    delay: DelayLine,
    /// Jet delay as a fraction of the bore delay.
    ratio: f32,
    bore_delay: f32,
    jet_reflection: f32,
    end_reflection: f32,
}

impl Jet {
    pub fn new(sample_rate: f32) -> Self {
        // Enough room for the lowest note's jet delay.
        let max = (sample_rate / 20.0) as usize;
        Self {
            delay: DelayLine::new(max),
            ratio: 0.32,
            bore_delay: 100.0,
            jet_reflection: 0.5,
            end_reflection: 0.5,
        }
    }

    pub fn set_ratio(&mut self, ratio: f32) {
        self.ratio = ratio.clamp(0.05, 0.9);
        self.update();
    }

    pub fn set_reflections(&mut self, jet: f32, end: f32) {
        self.jet_reflection = jet.clamp(0.0, 1.0);
        self.end_reflection = end.clamp(0.0, 1.0);
    }

    fn update(&mut self) {
        self.delay.set_delay(self.bore_delay * self.ratio);
    }

    /// The jet's nonlinear pressure characteristic. Cubic with a stable region
    /// around zero and saturation at the extremes.
    #[inline]
    fn table(x: f32) -> f32 {
        let x = x.clamp(-2.0, 2.0);
        (x * (x * x - 1.0)).clamp(-1.0, 1.0)
    }
}

impl Driver for Jet {
    fn configure(&mut self, patch: &crate::patch::DriverPatch) {
        self.set_ratio(patch.jet_ratio);
    }

    fn reset(&mut self) {
        self.delay.clear();
    }

    fn set_frequency(&mut self, _hz: f32) {}

    fn set_bore_delay(&mut self, samples: f32) {
        self.bore_delay = samples.max(2.0);
        self.update();
    }

    #[inline]
    fn tick(&mut self, input: &DriverInput) -> f32 {
        let breath = input.breath * (1.0 - input.tonguing.clamp(0.0, 1.0)) + input.noise;
        let bore = sanitize(input.bore);

        // Embouchure here is the player's lip-to-edge distance: it shortens the
        // jet, raising the frequency at which the jet wants to flap.
        let emb = input.embouchure.clamp(0.0, 1.0);
        self.delay
            .set_delay(self.bore_delay * self.ratio * (1.3 - 0.6 * emb));

        let drive = breath - self.jet_reflection * bore + input.scream * bore * 0.6;
        let jet_out = Self::table(self.delay.tick(drive));

        sanitize(jet_out + self.end_reflection * bore)
    }
}
