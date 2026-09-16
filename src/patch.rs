//! Patch data model — the complete, serialisable description of a sound.
//!
//! Every field here is a plain value with a sane default, so a patch can be
//! built by overriding only what matters:
//!
//! ```
//! use vl1::patch::Patch;
//! let mut p = Patch::default();
//! p.name = "My Clarinet".into();
//! p.element.driver.embouchure = 0.4;
//! ```

use crate::driver::DriverKind;
use crate::modifier::{EqBand, FilterKind};
use crate::waveguide::PipeMode;

/// Excitation settings: everything that happens at the player's end.
#[derive(Clone, Copy, Debug)]
pub struct DriverPatch {
    pub kind: DriverKind,
    /// Reed/lip tension, bow force. 0..1.
    pub embouchure: f32,
    /// How far the breath envelope pushes embouchure as the note swells.
    pub embouchure_env: f32,
    /// Tongue stroke depth at the start of a note, 0..1.
    pub tonguing: f32,
    /// Duration of that tongue stroke, seconds.
    pub tongue_time: f32,
    /// Chaotic-regime drive, 0..1.
    pub scream: f32,
    /// Turbulence level at the excitation point.
    pub breath_noise: f32,
    /// How strongly breath pressure gates the turbulence.
    pub noise_tracking: f32,
    /// Growl (throat flutter) rate in Hz and depth.
    pub growl_rate: f32,
    pub growl_depth: f32,
    /// Vocal tract formant imposed on the breath signal.
    pub throat_freq: f32,
    pub throat_q: f32,
    pub throat_amount: f32,
    /// Players tighten up as they go higher. Embouchure added per octave above
    /// C4 — small values here keep the top of the range from breaking register.
    pub embouchure_key_track: f32,
    /// Jet-driver only: jet length as a fraction of bore length.
    pub jet_ratio: f32,
    /// Lip-driver only: Q of the lip resonance (real lips are heavily damped).
    pub lip_q: f32,
    /// Lip-driver only: where the lip resonance sits relative to the bore mode
    /// it is playing. A lip reed oscillates just below its own resonance, so
    /// values slightly above 1.0 are what actually speak.
    pub lip_offset: f32,
    /// Reed-driver only: reed table offset at slack and tight embouchure.
    pub reed_offset: (f32, f32),
    /// Reed-driver only: reed table slope at slack and tight embouchure.
    pub reed_slope: (f32, f32),
    /// Reed-driver only: the reed's own resonance in Hz. Lower keeps the tube
    /// firmly in its bottom register; higher lets the upper registers speak.
    pub reed_cutoff: f32,
    /// Bow-driver only: friction slope at minimum and maximum bow force.
    pub bow_slope: (f32, f32),
}

impl Default for DriverPatch {
    fn default() -> Self {
        Self {
            kind: DriverKind::Reed,
            embouchure: 0.5,
            embouchure_env: 0.0,
            tonguing: 0.0,
            tongue_time: 0.02,
            scream: 0.0,
            breath_noise: 0.02,
            noise_tracking: 1.0,
            growl_rate: 30.0,
            growl_depth: 0.0,
            throat_freq: 800.0,
            throat_q: 1.5,
            throat_amount: 0.0,
            embouchure_key_track: 0.0,
            jet_ratio: 0.32,
            lip_q: 4.0,
            lip_offset: 1.0,
            reed_offset: (0.55, 0.80),
            reed_slope: (-0.22, -0.62),
            reed_cutoff: 3000.0,
            bow_slope: (5.0, 0.8),
        }
    }
}

/// Resonator settings: the tube or string the driver excites.
#[derive(Clone, Copy, Debug)]
pub struct PipePatch {
    pub mode: PipeMode,
    /// Length scaling; 1.0 = the nominal tube for the played pitch.
    pub tuning_ratio: f32,
    /// Termination lowpass cutoff in Hz at C4.
    pub damping_hz: f32,
    /// Octaves the damping cutoff moves per octave of pitch.
    pub damping_key_track: f32,
    /// How far blowing harder opens the damping filter, in octaves.
    pub damping_breath: f32,
    /// Broadband round-trip gain, < 1.
    pub absorption: f32,
    /// Where along the tube the output is taken, 0..1.
    pub tap_position: f32,
    /// How much of that tap is mixed with the main output.
    pub tap_mix: f32,
}

impl Default for PipePatch {
    fn default() -> Self {
        Self {
            mode: PipeMode::OddHarmonics,
            tuning_ratio: 1.0,
            damping_hz: 3500.0,
            damping_key_track: 0.6,
            damping_breath: 1.0,
            absorption: 0.975,
            tap_position: 0.0,
            tap_mix: 0.0,
        }
    }
}

/// The breath (or bow) envelope, and the vibrato riding on top of it.
#[derive(Clone, Copy, Debug)]
pub struct BreathPatch {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    /// Peak pressure at full velocity.
    pub level: f32,
    /// How much velocity scales pressure, 0..1.
    pub velocity_depth: f32,
    pub vibrato_rate: f32,
    /// Vibrato depth in cents.
    pub vibrato_depth: f32,
    pub vibrato_delay: f32,
    /// Tremolo: vibrato LFO applied to pressure instead of pitch.
    pub vibrato_pressure: f32,
}

impl Default for BreathPatch {
    fn default() -> Self {
        Self {
            attack: 0.04,
            decay: 0.15,
            sustain: 0.85,
            release: 0.12,
            level: 0.5,
            velocity_depth: 0.6,
            vibrato_rate: 5.0,
            vibrato_depth: 8.0,
            vibrato_delay: 0.35,
            vibrato_pressure: 0.1,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ImpulsePatch {
    pub amount: f32,
    pub freq: f32,
}

impl Default for ImpulsePatch {
    fn default() -> Self {
        Self {
            amount: 0.0,
            freq: 2500.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ResonatorBand {
    pub freq: f32,
    pub q: f32,
    pub mix: f32,
}

impl Default for ResonatorBand {
    fn default() -> Self {
        Self {
            freq: 500.0,
            q: 4.0,
            mix: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct EnhancerPatch {
    pub amount: f32,
    pub bias: f32,
    pub drive: f32,
    pub drive_tracking: f32,
}

impl Default for EnhancerPatch {
    fn default() -> Self {
        Self {
            amount: 0.0,
            bias: 0.5,
            drive: 1.0,
            drive_tracking: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct DynamicFilterPatch {
    pub kind: FilterKind,
    pub base_hz: f32,
    /// Octaves of cutoff sweep at full drive.
    pub env_depth: f32,
    pub resonance: f32,
    pub key_track: f32,
}

impl Default for DynamicFilterPatch {
    fn default() -> Self {
        Self {
            kind: FilterKind::LowPass,
            base_hz: 20000.0,
            env_depth: 0.0,
            resonance: 0.0,
            key_track: 0.0,
        }
    }
}

/// The whole modifier section for one element.
#[derive(Clone, Copy, Debug)]
pub struct ModifierPatch {
    pub impulse: ImpulsePatch,
    pub resonator: [ResonatorBand; 2],
    pub enhancer: EnhancerPatch,
    pub filter: DynamicFilterPatch,
    pub eq: [EqBand; 5],
    pub output_db: f32,
}

impl Default for ModifierPatch {
    fn default() -> Self {
        Self {
            impulse: ImpulsePatch::default(),
            resonator: [ResonatorBand::default(); 2],
            enhancer: EnhancerPatch::default(),
            filter: DynamicFilterPatch::default(),
            eq: [
                EqBand::new(120.0, 0.0, 0.7),
                EqBand::new(400.0, 0.0, 1.0),
                EqBand::new(1200.0, 0.0, 1.0),
                EqBand::new(3000.0, 0.0, 1.0),
                EqBand::new(8000.0, 0.0, 0.7),
            ],
            output_db: 0.0,
        }
    }
}

/// Output amplitude shaping for one element.
#[derive(Clone, Copy, Debug)]
pub struct AmpPatch {
    pub level: f32,
    pub velocity_sense: f32,
    /// How long the element keeps sounding after the note is released, letting
    /// the resonator ring out naturally.
    pub release: f32,
}

impl Default for AmpPatch {
    fn default() -> Self {
        Self {
            level: 1.0,
            velocity_sense: 0.3,
            release: 0.25,
        }
    }
}

/// One complete instrument element. The VL1 gave each voice two of these.
#[derive(Clone, Copy, Debug)]
pub struct ElementPatch {
    pub enabled: bool,
    pub level: f32,
    pub pan: f32,
    pub detune_cents: f32,
    pub transpose: i32,
    pub driver: DriverPatch,
    pub pipe: PipePatch,
    pub breath: BreathPatch,
    pub modifiers: ModifierPatch,
    pub amp: AmpPatch,
}

impl Default for ElementPatch {
    fn default() -> Self {
        Self {
            enabled: true,
            level: 1.0,
            pan: 0.0,
            detune_cents: 0.0,
            transpose: 0,
            driver: DriverPatch::default(),
            pipe: PipePatch::default(),
            breath: BreathPatch::default(),
            modifiers: ModifierPatch::default(),
            amp: AmpPatch::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortamentoMode {
    Off,
    /// Glide on every note.
    Always,
    /// Glide only when a note is already sounding (fingered legato).
    Legato,
}

#[derive(Clone, Copy, Debug)]
pub struct ChorusPatch {
    pub rate: f32,
    pub depth_ms: f32,
    pub mix: f32,
    pub spread: f32,
}

impl Default for ChorusPatch {
    fn default() -> Self {
        Self {
            rate: 0.6,
            depth_ms: 2.5,
            mix: 0.0,
            spread: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ReverbPatch {
    /// 0..1, maps to decay time.
    pub size: f32,
    /// High-frequency absorption, 0..1.
    pub damping: f32,
    pub mix: f32,
}

impl Default for ReverbPatch {
    fn default() -> Self {
        Self {
            size: 0.6,
            damping: 0.4,
            mix: 0.15,
        }
    }
}

/// Voice assignment and global performance behaviour.
#[derive(Clone, Copy, Debug)]
pub struct PerformancePatch {
    /// The MIDI note range over which this patch has been verified to speak and
    /// stay in tune. Physical models have practical ranges just as the
    /// instruments they model do; nothing stops you playing outside it, but the
    /// model may break register or fail to start, exactly as a real one would.
    pub key_range: (u8, u8),
    pub pitch_bend_range: f32,
    pub portamento: PortamentoMode,
    pub portamento_time: f32,
    /// Master output gain.
    ///
    /// Staged so that a full eight-voice chord lands below full scale rather
    /// than leaning on the output limiter; a single note is correspondingly
    /// quieter, as on any polyphonic instrument.
    pub master_level: f32,
}

impl Default for PerformancePatch {
    fn default() -> Self {
        Self {
            key_range: (36, 84),
            pitch_bend_range: 2.0,
            portamento: PortamentoMode::Off,
            portamento_time: 0.06,
            master_level: 0.32,
        }
    }
}

/// A complete VL1-style patch: up to two elements plus global settings.
#[derive(Clone, Debug)]
pub struct Patch {
    pub name: String,
    pub element: ElementPatch,
    /// Optional second element, layered with the first.
    pub element2: Option<ElementPatch>,
    pub performance: PerformancePatch,
    pub chorus: ChorusPatch,
    pub reverb: ReverbPatch,
}

impl Default for Patch {
    fn default() -> Self {
        Self {
            name: "Init".to_string(),
            element: ElementPatch::default(),
            element2: None,
            performance: PerformancePatch::default(),
            chorus: ChorusPatch::default(),
            reverb: ReverbPatch::default(),
        }
    }
}

impl Patch {
    /// Iterate the enabled elements of this patch.
    pub fn elements(&self) -> impl Iterator<Item = &ElementPatch> {
        std::iter::once(&self.element)
            .chain(self.element2.iter())
            .filter(|e| e.enabled)
    }
}
