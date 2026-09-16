//! Lip-valve driver (trumpet, trombone, horn).
//!
//! The lips are a mechanical resonator sitting between the mouth and the bore.
//! Embouchure tunes that resonance, which is what selects which partial of the
//! tube the player locks onto — slide/valve position alone does not.

use super::{Driver, DriverInput};
use crate::dsp::{sanitize, Biquad};

/// Valve table endpoints, chosen by sweeping the model for the combination that
/// stays within ~15 cents across three octaves while still speaking strongly.
const TABLE_OFFSET: f32 = 0.65;
const TABLE_SLOPE: f32 = -0.8;

#[derive(Clone, Debug)]
pub struct Lip {
    filter: Biquad,
    sample_rate: f32,
    freq: f32,
    radius: f32,
    /// Lip resonance relative to the selected partial.
    offset: f32,
    /// Embouchure at the time the resonance was last computed.
    emb: f32,
}

impl Lip {
    pub fn new(sample_rate: f32) -> Self {
        let mut lip = Self {
            filter: Biquad::new(),
            sample_rate,
            freq: 220.0,
            radius: 0.875,
            offset: 1.0,
            emb: 0.5,
        };
        lip.update();
        lip
    }

    /// Q of the lip resonance. Higher = more "locked in", harder to bend.
    ///
    /// Real lips are heavily damped — Q in the single digits. A high-Q lip
    /// dominates the coupled system instead of merely selecting a partial of
    /// the bore, and the instrument stops playing the note you asked for.
    pub fn set_q(&mut self, q: f32) {
        let q = q.clamp(0.5, 40.0);
        self.radius = (1.0 - 1.0 / (2.0 * q)).clamp(0.1, 0.995);
        self.update();
    }

    /// Which partial of the bore the current embouchure selects (1 = fundamental).
    pub fn partial(&self) -> f32 {
        (0.8 + 2.6 * self.emb.clamp(0.0, 1.0)).round().max(1.0)
    }

    fn update(&mut self) {
        // Lip tension picks which mode of the tube the player locks onto. It is
        // snapped to an integer partial rather than swept continuously: a lip
        // resonance sitting between two modes drags the pitch with it, which is
        // real enough on a real instrument but reads as simply out of tune here.
        // Sweeping embouchure therefore jumps partials the way overblowing does.
        //
        let f = (self.freq * self.partial() * self.offset).clamp(20.0, self.sample_rate * 0.45);
        // DC-passing, so that steady mouth pressure holds the lips open and the
        // valve has an operating point to oscillate around. A bandpass here
        // leaves the model stuck shut: it would never speak at all.
        self.filter
            .set_resonance_lowpass(f, self.radius, self.sample_rate);
    }
}

impl Driver for Lip {
    fn configure(&mut self, patch: &crate::patch::DriverPatch) {
        self.offset = patch.lip_offset.clamp(0.5, 2.0);
        self.set_q(patch.lip_q);
    }

    fn reset(&mut self) {
        self.filter.clear();
    }

    fn set_frequency(&mut self, hz: f32) {
        self.freq = hz.max(1.0);
        self.update();
    }

    #[inline]
    fn tick(&mut self, input: &DriverInput) -> f32 {
        let emb = input.embouchure.clamp(0.0, 1.0);
        if (emb - self.emb).abs() > 0.002 {
            self.emb = emb;
            self.update();
        }

        let breath = input.breath * (1.0 - input.tonguing.clamp(0.0, 1.0)) + input.noise;
        let bore = sanitize(input.bore);

        // Same nonlinear junction as the reed, but the valve's reflection is
        // driven through the lip resonance rather than directly by the pressure
        // difference. That resonance is what selects the partial; the bore,
        // whose reflection is left intact, is what fixes the pitch.
        let pd = bore - breath;
        let x = self.filter.tick(pd);
        let bias = input.scream.clamp(0.0, 1.0) * bore * 1.2;
        let refl = (TABLE_OFFSET + TABLE_SLOPE * x + bias).clamp(-1.0, 1.0);

        sanitize(breath + pd * refl)
    }
}
