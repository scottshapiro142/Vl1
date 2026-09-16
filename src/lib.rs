//! # vl1
//!
//! An eight-voice virtual acoustic (physical modelling) synthesizer in the
//! architecture of the Yamaha VL1.
//!
//! The VL1 did not store waveforms. It stored a description of an instrument —
//! a driver, a resonator and a chain of modifiers — and solved it in real time,
//! sample by sample, in response to how it was being played. That is what this
//! crate does:
//!
//! ```text
//!   breath / bow ──▶ Driver ──▶ Pipe / String ──▶ Modifiers ──▶ Amp ──▶ out
//!   (nonlinear, expressive)    (linear resonator)   (voicing)
//!                    ▲                  │
//!                    └──── reflection ◀─┘
//! ```
//!
//! The driver is the only nonlinear block and the only one the player touches
//! directly. The resonator stores one acoustic round trip and gives back what it
//! has not lost. Neither one produces a note by itself — the note is what the
//! feedback loop between them settles into, which is why these instruments
//! respond to playing the way real ones do.
//!
//! ## Quick start
//!
//! ```
//! use vl1::{Engine, presets};
//!
//! let mut engine = Engine::with_patch(presets::clarinet(), 48_000.0);
//! engine.note_on(60, 100);
//!
//! let mut buffer = vec![0.0f32; 48_000 * 2]; // 1 second, stereo interleaved
//! engine.render(&mut buffer);
//! ```
//!
//! ## Polyphony
//!
//! The hardware VL1 was two-voice: each voice cost a dedicated DSP. This engine
//! runs [`POLYPHONY`](engine::POLYPHONY) = 8 voices, allocated with last-note
//! priority and quietest-first stealing.

pub mod control;
pub mod driver;
pub mod dsp;
pub mod effects;
pub mod element;
pub mod engine;
pub mod midi;
pub mod modifier;
pub mod patch;
pub mod presets;
pub mod voice;
pub mod wav;
pub mod waveguide;

pub use control::Controls;
pub use driver::DriverKind;
pub use element::Element;
pub use engine::{Engine, POLYPHONY};
pub use patch::{ElementPatch, Patch, PortamentoMode};
pub use voice::Voice;
pub use waveguide::{PipeMode, Waveguide};
