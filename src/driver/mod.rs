//! Drivers — the nonlinear excitation element of a virtual acoustic instrument.
//!
//! In the VL1's terms this is the "Instrument/Driver" block: the reed, the
//! player's lips, an air jet across an edge, or a bow. It is the only nonlinear
//! part of the model, and it is where all of the expressive control lands. The
//! waveguide that follows is linear and, on its own, silent.

mod bow;
mod jet;
mod lip;
mod reed;

pub use bow::Bow;
pub use jet::Jet;
pub use lip::Lip;
pub use reed::Reed;

/// Which excitation model an element uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriverKind {
    /// Single reed against a mouthpiece: clarinet, saxophone.
    Reed,
    /// Lip-valve: trumpet, trombone, horn.
    Lip,
    /// Air jet striking an edge: flute, recorder, shakuhachi, organ pipe.
    Jet,
    /// Stick-slip friction: violin, cello, bowed metal.
    Bow,
}

/// Per-sample control inputs to a driver. These are the VL1's real-time
/// performance parameters, evaluated once per sample.
#[derive(Clone, Copy, Debug, Default)]
pub struct DriverInput {
    /// Mouth pressure, or bow velocity for [`DriverKind::Bow`].
    pub breath: f32,
    /// The wave returning from the resonator (already reflected and damped).
    pub bore: f32,
    /// Lip/reed tension, bow force. 0 = slack, 1 = tight.
    pub embouchure: f32,
    /// Tongue against the reed / breath interruption. 1 = fully stopped.
    pub tonguing: f32,
    /// Drives the driver into its chaotic regime (the VL1's "Scream").
    pub scream: f32,
    /// Turbulence injected at the excitation point.
    pub noise: f32,
}

/// A nonlinear excitation element.
pub trait Driver: Send {
    /// Apply the patch's driver settings. Each model reads the fields that
    /// apply to it and ignores the rest.
    fn configure(&mut self, _patch: &crate::patch::DriverPatch) {}

    /// Clear all internal state.
    fn reset(&mut self);

    /// Retune any pitch-dependent internal resonance.
    fn set_frequency(&mut self, hz: f32);

    /// Inform the driver of the resonator's loop length, in samples. Only the
    /// jet model needs this (its jet delay is a fixed fraction of the bore).
    fn set_bore_delay(&mut self, _samples: f32) {}

    /// Produce the wave injected into the resonator for this sample.
    fn tick(&mut self, input: &DriverInput) -> f32;
}

/// Construct a driver of the given kind.
pub fn make(kind: DriverKind, sample_rate: f32) -> Box<dyn Driver> {
    match kind {
        DriverKind::Reed => Box::new(Reed::new(sample_rate)),
        DriverKind::Lip => Box::new(Lip::new(sample_rate)),
        DriverKind::Jet => Box::new(Jet::new(sample_rate)),
        DriverKind::Bow => Box::new(Bow::new()),
    }
}
