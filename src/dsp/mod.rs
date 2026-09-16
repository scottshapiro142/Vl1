//! Reusable DSP primitives. No allocations on the audio path beyond construction.

pub mod delay;
pub mod envelope;
pub mod filter;
pub mod lfo;
pub mod noise;

pub use delay::DelayLine;
pub use envelope::{Adsr, Smoother, Stage};
pub use filter::{Biquad, DcBlock, OnePole, OneZero, Svf};
pub use lfo::{Lfo, LfoShape};
pub use noise::Noise;

/// Convert a MIDI note number (with fractional cents) to Hz.
#[inline]
pub fn note_to_hz(note: f32) -> f32 {
    440.0 * ((note - 69.0) / 12.0).exp2()
}

/// Decibels to a linear gain.
#[inline]
pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Odd-symmetric soft clipper. Bounded output for any finite input, unity slope at 0.
#[inline]
pub fn soft_clip(x: f32) -> f32 {
    x.tanh()
}

/// Cheap tanh approximation — accurate to ~1e-3 over [-3, 3], much faster.
#[inline]
pub fn fast_tanh(x: f32) -> f32 {
    let x = x.clamp(-4.0, 4.0);
    let x2 = x * x;
    x * (27.0 + x2) / (27.0 + 9.0 * x2)
}

/// Replace NaN/inf with zero. Waveguide loops are conditionally stable; a single
/// bad sample must not be able to poison the delay line forever.
#[inline]
pub fn sanitize(x: f32) -> f32 {
    if x.is_finite() {
        x
    } else {
        0.0
    }
}

/// Linear interpolation.
#[inline]
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
