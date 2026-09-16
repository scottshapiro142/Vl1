//! Factory patches.
//!
//! Each of these is a starting point rather than an imitation: the parameters
//! describe a physical setup (how long the tube is, how it terminates, how hard
//! the reed is) and the sound follows from that.
//!
//! Every patch is verified by `tests/engine.rs` to speak and hold pitch across
//! the key range it declares.

// Patches read as "start from the init patch, then say what differs", which is
// far clearer for a parameter set this size than a full struct literal.
#![allow(clippy::field_reassign_with_default)]

use crate::driver::DriverKind;
use crate::modifier::{EqBand, FilterKind};
use crate::patch::*;
use crate::waveguide::PipeMode;

/// Every factory patch, in order.
pub fn all() -> Vec<Patch> {
    vec![
        clarinet(),
        tenor_sax(),
        trumpet(),
        trombone(),
        flute(),
        shakuhachi(),
        violin(),
        cello(),
        scream_lead(),
        breath_pad(),
    ]
}

/// Look a patch up by name, case-insensitively, ignoring spaces and hyphens.
pub fn by_name(name: &str) -> Option<Patch> {
    let key = normalize(name);
    all().into_iter().find(|p| normalize(&p.name) == key)
}

/// The names of all factory patches.
pub fn names() -> Vec<String> {
    all().into_iter().map(|p| p.name).collect()
}

fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Cylindrical bore, single reed: only odd harmonics, and it overblows a
/// twelfth rather than an octave. That is the whole character of the clarinet.
pub fn clarinet() -> Patch {
    let mut p = Patch::default();
    p.name = "Clarinet".into();
    p.performance.key_range = (36, 84);
    let e = &mut p.element;

    e.driver.kind = DriverKind::Reed;
    e.driver.embouchure = 0.40;
    e.driver.breath_noise = 0.015;
    e.driver.tonguing = 0.55;
    e.driver.tongue_time = 0.018;
    e.driver.throat_freq = 1100.0;
    e.driver.throat_amount = 0.15;

    e.pipe.mode = PipeMode::OddHarmonics;
    e.pipe.damping_hz = 2600.0;
    e.pipe.damping_key_track = 0.5;
    e.pipe.damping_breath = 1.2;
    e.pipe.absorption = 0.982;

    e.breath.attack = 0.035;
    e.breath.decay = 0.12;
    e.breath.sustain = 0.9;
    e.breath.release = 0.09;
    e.breath.level = 0.75;
    e.breath.vibrato_depth = 4.0;
    e.breath.vibrato_rate = 4.8;

    e.modifiers.impulse.amount = 0.5;
    e.modifiers.impulse.freq = 2200.0;
    e.modifiers.eq[1] = EqBand::new(450.0, 2.0, 0.9);
    e.modifiers.eq[4] = EqBand::new(6000.0, -3.0, 0.7);

    p.reverb.mix = 0.18;
    p
}

/// A conical bore behaves as if open at both ends: the full harmonic series,
/// and a much brighter, reedier spectrum than the clarinet's.
pub fn tenor_sax() -> Patch {
    let mut p = Patch::default();
    p.name = "Tenor Sax".into();
    p.performance.key_range = (36, 84);
    let e = &mut p.element;

    e.driver.kind = DriverKind::Reed;
    e.driver.embouchure = 0.52;
    e.driver.embouchure_env = 0.1;
    e.driver.breath_noise = 0.05;
    e.driver.tonguing = 0.45;
    e.driver.growl_rate = 28.0;
    e.driver.growl_depth = 0.35;
    e.driver.throat_freq = 900.0;
    e.driver.throat_q = 1.2;
    e.driver.throat_amount = 0.3;

    e.pipe.mode = PipeMode::AllHarmonics;
    e.pipe.damping_hz = 3200.0;
    e.pipe.damping_key_track = 0.45;
    e.pipe.damping_breath = 1.6;
    e.pipe.absorption = 0.972;

    e.breath.attack = 0.03;
    e.breath.decay = 0.2;
    e.breath.sustain = 0.88;
    e.breath.release = 0.12;
    e.breath.level = 0.5;
    e.breath.vibrato_depth = 9.0;
    e.breath.vibrato_rate = 5.4;

    e.modifiers.impulse.amount = 0.8;
    e.modifiers.resonator[0] = ResonatorBand {
        freq: 700.0,
        q: 2.5,
        mix: 0.25,
    };
    e.modifiers.enhancer = EnhancerPatch {
        amount: 0.22,
        bias: 0.65,
        drive: 1.4,
        drive_tracking: 1.2,
    };
    e.modifiers.filter = DynamicFilterPatch {
        kind: FilterKind::LowPass,
        base_hz: 1400.0,
        env_depth: 2.4,
        resonance: 0.25,
        key_track: 0.5,
    };
    e.modifiers.eq[2] = EqBand::new(1600.0, 2.5, 1.1);

    p.reverb.mix = 0.2;
    p
}

pub fn trumpet() -> Patch {
    let mut p = Patch::default();
    p.name = "Trumpet".into();
    p.performance.key_range = (36, 74);
    let e = &mut p.element;

    e.driver.kind = DriverKind::Lip;
    // Embouchure selects the partial: 0.12 sits on the fundamental.
    e.driver.embouchure = 0.12;
    e.driver.embouchure_env = 0.04;
    // Players tighten up as they ascend; without this the top of the range
    // loses its grip on the partial and the pitch collapses.
    e.driver.embouchure_key_track = 0.2;
    e.driver.lip_q = 4.0;
    e.driver.breath_noise = 0.02;
    e.driver.tonguing = 0.6;
    e.driver.tongue_time = 0.012;

    e.pipe.mode = PipeMode::AllHarmonics;
    e.pipe.damping_hz = 3800.0;
    e.pipe.damping_key_track = 0.4;
    e.pipe.damping_breath = 2.0;
    e.pipe.absorption = 0.968;

    e.breath.attack = 0.02;
    e.breath.decay = 0.15;
    e.breath.sustain = 0.9;
    e.breath.release = 0.1;
    e.breath.level = 0.6;
    e.breath.velocity_depth = 0.7;
    e.breath.vibrato_depth = 5.0;
    e.breath.vibrato_rate = 5.6;

    e.modifiers.impulse.amount = 1.2;
    e.modifiers.impulse.freq = 3000.0;
    // The bell's flare is a broad resonance that stays put as the pitch moves.
    e.modifiers.resonator[0] = ResonatorBand {
        freq: 1200.0,
        q: 2.0,
        mix: 0.3,
    };
    e.modifiers.enhancer = EnhancerPatch {
        amount: 0.35,
        bias: 0.8,
        drive: 1.8,
        drive_tracking: 2.0,
    };
    e.modifiers.filter = DynamicFilterPatch {
        kind: FilterKind::LowPass,
        base_hz: 1100.0,
        env_depth: 3.2,
        resonance: 0.2,
        key_track: 0.6,
    };
    e.modifiers.eq[3] = EqBand::new(3000.0, 3.0, 1.2);

    p.reverb.mix = 0.22;
    p
}

pub fn trombone() -> Patch {
    let mut p = trumpet();
    p.name = "Trombone".into();
    p.performance.key_range = (36, 64);
    let e = &mut p.element;

    e.driver.embouchure = 0.08;
    e.driver.embouchure_key_track = 0.25;
    e.driver.lip_q = 4.5;
    e.pipe.damping_hz = 2800.0;
    e.pipe.absorption = 0.976;
    e.breath.level = 0.55;
    e.modifiers.resonator[0].freq = 600.0;
    e.modifiers.enhancer.amount = 0.28;
    e.modifiers.filter.base_hz = 800.0;
    e.modifiers.eq[0] = EqBand::new(200.0, 3.0, 0.7);
    e.modifiers.eq[3] = EqBand::new(2400.0, 2.0, 1.2);

    // Slide, not valves: legato means glide.
    p.performance.portamento = PortamentoMode::Legato;
    p.performance.portamento_time = 0.08;
    p
}

pub fn flute() -> Patch {
    let mut p = Patch::default();
    p.name = "Flute".into();
    p.performance.key_range = (36, 79);
    let e = &mut p.element;

    e.driver.kind = DriverKind::Jet;
    e.driver.embouchure = 0.5;
    e.driver.jet_ratio = 0.32;
    e.driver.breath_noise = 0.18;
    e.driver.noise_tracking = 0.8;
    e.driver.tonguing = 0.35;
    e.driver.throat_freq = 2000.0;
    e.driver.throat_amount = 0.25;

    e.pipe.mode = PipeMode::AllHarmonics;
    e.pipe.damping_hz = 5000.0;
    e.pipe.damping_key_track = 0.5;
    e.pipe.damping_breath = 0.8;
    e.pipe.absorption = 0.978;

    e.breath.attack = 0.05;
    e.breath.decay = 0.1;
    e.breath.sustain = 0.9;
    e.breath.release = 0.1;
    e.breath.level = 0.66;
    e.breath.vibrato_depth = 12.0;
    e.breath.vibrato_rate = 5.2;
    e.breath.vibrato_pressure = 0.18;

    // The jet's own output is quiet, so the element carries a lot of make-up
    // gain — which means the attack chiff has to be kept in proportion or it
    // alone drives the output into the limiter on a full chord.
    e.modifiers.impulse.amount = 0.55;
    e.modifiers.impulse.freq = 4000.0;
    e.modifiers.eq[4] = EqBand::new(7000.0, 2.0, 0.7);
    e.modifiers.output_db = 18.5;

    p.reverb.mix = 0.25;
    p
}

pub fn shakuhachi() -> Patch {
    let mut p = flute();
    p.name = "Shakuhachi".into();
    p.performance.key_range = (36, 72);
    let e = &mut p.element;

    e.driver.jet_ratio = 0.42;
    e.driver.breath_noise = 0.42;
    e.driver.noise_tracking = 0.6;
    e.driver.embouchure = 0.4;
    e.driver.throat_freq = 1200.0;
    e.driver.throat_q = 2.5;
    e.driver.throat_amount = 0.5;
    e.driver.scream = 0.08;

    e.pipe.damping_hz = 3000.0;
    e.pipe.absorption = 0.968;

    e.breath.attack = 0.09;
    e.breath.level = 0.60;
    e.breath.vibrato_rate = 4.2;
    e.breath.vibrato_depth = 18.0;
    e.breath.vibrato_delay = 0.5;

    e.modifiers.resonator[0] = ResonatorBand {
        freq: 900.0,
        q: 3.0,
        mix: 0.3,
    };
    e.modifiers.output_db = 14.0;
    p.reverb.mix = 0.32;
    p.reverb.size = 0.72;
    p
}

pub fn violin() -> Patch {
    let mut p = Patch::default();
    p.name = "Violin".into();
    p.performance.key_range = (36, 72);
    let e = &mut p.element;

    e.driver.kind = DriverKind::Bow;
    e.driver.embouchure = 0.45; // bow force
    e.driver.breath_noise = 0.012;
    e.driver.noise_tracking = 1.0;
    e.driver.tonguing = 0.0;

    e.pipe.mode = PipeMode::AllHarmonics;
    e.pipe.damping_hz = 4200.0;
    e.pipe.damping_key_track = 0.7;
    e.pipe.damping_breath = 0.6;
    e.pipe.absorption = 0.988;

    e.breath.attack = 0.07;
    e.breath.decay = 0.2;
    e.breath.sustain = 0.85;
    e.breath.release = 0.15;
    e.breath.level = 0.62;
    e.breath.velocity_depth = 0.5;
    e.breath.vibrato_rate = 6.0;
    e.breath.vibrato_depth = 14.0;
    e.breath.vibrato_delay = 0.25;

    e.amp.release = 0.4;

    // Body resonances: the air mode and the main wood mode of a violin corpus.
    e.modifiers.resonator[0] = ResonatorBand {
        freq: 275.0,
        q: 3.5,
        mix: 0.35,
    };
    e.modifiers.resonator[1] = ResonatorBand {
        freq: 460.0,
        q: 5.0,
        mix: 0.25,
    };
    e.modifiers.impulse.amount = 0.6;
    e.modifiers.eq[2] = EqBand::new(1800.0, 2.0, 1.0);
    e.modifiers.eq[4] = EqBand::new(6500.0, -2.0, 0.7);
    e.modifiers.output_db = -4.0;

    p.performance.portamento = PortamentoMode::Legato;
    p.performance.portamento_time = 0.05;
    p.reverb.mix = 0.26;
    p
}

pub fn cello() -> Patch {
    let mut p = violin();
    p.name = "Cello".into();
    p.performance.key_range = (36, 72);
    let e = &mut p.element;

    e.driver.embouchure = 0.5;
    e.pipe.damping_hz = 2800.0;
    e.pipe.absorption = 0.99;
    e.breath.attack = 0.09;
    e.breath.level = 0.7;
    e.breath.vibrato_rate = 5.2;
    e.modifiers.resonator[0] = ResonatorBand {
        freq: 105.0,
        q: 3.0,
        mix: 0.4,
    };
    e.modifiers.resonator[1] = ResonatorBand {
        freq: 190.0,
        q: 4.5,
        mix: 0.3,
    };
    e.modifiers.eq[0] = EqBand::new(150.0, 3.0, 0.7);
    e.modifiers.output_db = -3.5;
    p
}

/// What the VL1 was really bought for: a reed pushed past the point where it
/// behaves, with the Scream control doing the pushing.
pub fn scream_lead() -> Patch {
    let mut p = Patch::default();
    p.name = "Scream Lead".into();
    p.performance.key_range = (36, 84);
    let e = &mut p.element;

    e.driver.kind = DriverKind::Reed;
    e.driver.embouchure = 0.62;
    e.driver.embouchure_env = 0.15;
    e.driver.scream = 0.22;
    e.driver.breath_noise = 0.03;
    e.driver.growl_rate = 22.0;
    e.driver.growl_depth = 0.5;
    e.driver.tonguing = 0.5;

    e.pipe.mode = PipeMode::AllHarmonics;
    e.pipe.damping_hz = 4200.0;
    e.pipe.damping_breath = 2.2;
    e.pipe.absorption = 0.974;

    e.breath.attack = 0.025;
    e.breath.decay = 0.18;
    e.breath.sustain = 0.92;
    e.breath.release = 0.1;
    e.breath.level = 0.6;
    e.breath.vibrato_rate = 6.2;
    e.breath.vibrato_depth = 16.0;

    e.modifiers.impulse.amount = 1.0;
    e.modifiers.enhancer = EnhancerPatch {
        amount: 0.5,
        bias: 0.75,
        drive: 2.2,
        drive_tracking: 2.5,
    };
    e.modifiers.filter = DynamicFilterPatch {
        kind: FilterKind::LowPass,
        base_hz: 900.0,
        env_depth: 3.6,
        resonance: 0.35,
        key_track: 0.7,
    };
    e.modifiers.eq[3] = EqBand::new(2600.0, 3.5, 1.2);
    e.modifiers.output_db = 5.0;

    p.performance.portamento = PortamentoMode::Legato;
    p.performance.portamento_time = 0.07;
    p.chorus.mix = 0.25;
    p.reverb.mix = 0.25;
    p
}

/// Two elements detuned against each other — a breathy jet over a slow bowed
/// drone. This is the kind of thing the VL1's dual-element architecture was for.
pub fn breath_pad() -> Patch {
    let mut p = Patch::default();
    p.name = "Breath Pad".into();
    p.performance.key_range = (36, 84);

    let e = &mut p.element;
    e.driver.kind = DriverKind::Jet;
    e.driver.jet_ratio = 0.38;
    e.driver.breath_noise = 0.3;
    e.driver.embouchure = 0.45;
    e.driver.throat_freq = 1600.0;
    e.driver.throat_amount = 0.35;
    e.pipe.mode = PipeMode::AllHarmonics;
    e.pipe.damping_hz = 3400.0;
    e.pipe.absorption = 0.982;
    e.breath.attack = 0.5;
    e.breath.decay = 0.4;
    e.breath.sustain = 0.9;
    e.breath.release = 0.7;
    e.breath.level = 0.3;
    e.breath.vibrato_rate = 3.4;
    e.breath.vibrato_depth = 7.0;
    e.amp.release = 0.9;
    e.modifiers.output_db = 9.0;
    e.pan = -0.35;
    e.detune_cents = -6.0;
    e.level = 0.8;

    let mut second = *e;
    second.driver.kind = DriverKind::Bow;
    second.driver.embouchure = 0.5;
    second.driver.breath_noise = 0.01;
    second.pipe.damping_hz = 2200.0;
    second.pipe.absorption = 0.99;
    second.breath.attack = 0.8;
    second.breath.level = 0.26;
    second.pan = 0.35;
    second.detune_cents = 6.0;
    second.transpose = 0;
    second.level = 0.7;
    second.modifiers.output_db = 9.0;
    second.modifiers.resonator[0] = ResonatorBand {
        freq: 320.0,
        q: 3.0,
        mix: 0.3,
    };
    p.element2 = Some(second);

    p.performance.master_level = 0.26;
    p.chorus.mix = 0.35;
    p.chorus.rate = 0.35;
    p.reverb.mix = 0.4;
    p.reverb.size = 0.8;
    p
}
