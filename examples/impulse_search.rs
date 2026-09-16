//! For each patch, find the strongest articulation impulse that still starts
//! the note in the right register across its whole key range.
//!
//! A bigger impulse starts the note faster, but the driver is nonlinear: past
//! some point the transient lands the loop in a different stable regime and the
//! note comes out an octave or a twelfth from where it should.

use vl1::{presets, Engine, Patch};

const SR: f32 = 48_000.0;

include!("shared/pitch.rs");

fn render(patch: Patch, note: u8, seconds: f32) -> Vec<f32> {
    let mut engine = Engine::with_patch(patch, SR);
    engine.note_on(note, 100);
    let n = (seconds * SR) as usize;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let s = engine.tick();
        out.push((s[0] + s[1]) * 0.5);
    }
    out
}

/// Worst tuning error across the patch's range, in cents.
fn worst_cents(patch: &Patch) -> f32 {
    let (lo, hi) = patch.performance.key_range;
    let mut worst = 0.0f32;
    let mut note = lo;
    loop {
        let target = 440.0 * ((note as f32 - 69.0) / 12.0).exp2();
        let samples = render(patch.clone(), note, 2.5);
        let sustain = &samples[(1.2 * SR) as usize..(2.4 * SR) as usize];
        let hz = detect_pitch(sustain, SR, 40.0, 2000.0);
        let cents = 1200.0 * (hz / target).log2();
        if cents.abs() > worst.abs() {
            worst = cents;
        }
        if note >= hi {
            break;
        }
        note = (note + 3).min(hi);
    }
    worst
}

/// Milliseconds until the note reaches half its steady level.
fn speak_ms(patch: &Patch, note: u8) -> f32 {
    let raw: Vec<f32> = render(patch.clone(), note, 1.5)
        .iter()
        .map(|s| s.abs())
        .collect();
    let steady = raw[(SR as usize)..].iter().fold(0.0f32, |m, &v| m.max(v));
    let mut follower = 0.0f32;
    for (i, &a) in raw.iter().enumerate() {
        follower = a.max(follower * 0.9995);
        if follower >= steady * 0.5 {
            return i as f32 / SR * 1000.0;
        }
    }
    f32::NAN
}

fn main() {
    let only = std::env::args().nth(1);
    for name in presets::names() {
        if only.as_ref().is_some_and(|o| !name.eq_ignore_ascii_case(o)) {
            continue;
        }
        let base = presets::by_name(&name).unwrap();
        println!("{name}:");
        for imp in [0.0f32, 0.5, 1.0, 1.5, 2.0, 3.0, 4.0, 6.0] {
            let mut p = base.clone();
            p.element.driver.attack_impulse = imp;
            if let Some(e2) = p.element2.as_mut() {
                e2.driver.attack_impulse = imp;
            }
            let cents = worst_cents(&p);
            let t = (speak_ms(&p, 48) + speak_ms(&p, 60) + speak_ms(&p, 72)) / 3.0;
            println!("   impulse {imp:>4.1}  worst {cents:>8.1}c  speaks {t:>7.1}ms");
        }
    }
}
