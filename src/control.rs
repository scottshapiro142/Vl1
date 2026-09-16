//! Real-time performance controls.
//!
//! On a VL1 these came from a breath controller, a foot pedal, the wheels and
//! the keyboard all at once. They are global rather than per-voice: one player,
//! one set of lungs.

/// Continuous controllers, normalised. All voices read the same instance.
#[derive(Clone, Copy, Debug)]
pub struct Controls {
    /// Breath pressure multiplier, 0..~1.3. 1.0 = the patch's own level.
    pub pressure: f32,
    /// Embouchure offset added to the patch value, -1..1.
    pub embouchure: f32,
    /// Extra chaotic drive, 0..1.
    pub scream: f32,
    /// Growl (throat flutter) depth, 0..1.
    pub growl: f32,
    /// Damping offset in octaves, -2..2. Positive = brighter.
    pub damping: f32,
    /// Absorption offset, -1..1. Positive = longer ring.
    pub absorption: f32,
    /// Vibrato depth scaler, 0..1 (mod wheel).
    pub modulation: f32,
    /// Output level, 0..1 (expression pedal).
    pub expression: f32,
    /// Pitch bend, -1..1.
    pub pitch_bend: f32,
    /// Channel aftertouch, 0..1.
    pub aftertouch: f32,
    /// Sustain pedal.
    pub sustain: bool,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            pressure: 1.0,
            embouchure: 0.0,
            scream: 0.0,
            growl: 0.0,
            damping: 0.0,
            absorption: 0.0,
            modulation: 0.0,
            expression: 1.0,
            pitch_bend: 0.0,
            aftertouch: 0.0,
            sustain: false,
        }
    }
}

impl Controls {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset everything to its neutral value.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}
