//! Note-on to speaking time, and how breath pressure changes it.
//!
//! A waveguide's amplitude grows at a rate set by how far the driver's gain
//! exceeds the loop's losses. Sitting just above that threshold oscillates, but
//! builds up slowly — which is felt as latency. Further in, it speaks fast; too
//! far and the valve stays shut and it never speaks at all.

use vl1::{presets, Engine, Patch};

/// Returns (ms to half of steady level, steady level, attack peak / steady).
fn speak_detail(patch: Patch, note: u8, sr: f32) -> (f32, f32, f32) {
    let mut engine = Engine::with_patch(patch, sr);
    engine.note_on(note, 100);
    let n = (sr * 1.5) as usize;
    let mut raw = Vec::with_capacity(n);
    for _ in 0..n {
        let s = engine.tick();
        raw.push(s[0].abs().max(s[1].abs()));
    }
    let attack_peak = raw[..(sr * 0.05) as usize]
        .iter()
        .fold(0.0f32, |m, &v| m.max(v));
    let steady = raw[(sr * 1.0) as usize..]
        .iter()
        .fold(0.0f32, |m, &v| m.max(v));

    let mut follower = 0.0f32;
    let mut half = f32::NAN;
    for (i, &a) in raw.iter().enumerate() {
        follower = a.max(follower * 0.9995);
        if half.is_nan() && follower >= steady * 0.5 {
            half = i as f32 / sr * 1000.0;
        }
    }
    (half, steady, attack_peak / steady.max(1e-6))
}

fn speak_time(patch: Patch, note: u8, sr: f32) -> (f32, f32) {
    let mut engine = Engine::with_patch(patch, sr);
    engine.note_on(note, 100);

    let n = (sr * 1.5) as usize;
    let mut env = Vec::with_capacity(n);
    let mut follower = 0.0f32;
    for _ in 0..n {
        let s = engine.tick();
        let a = s[0].abs().max(s[1].abs());
        follower = a.max(follower * 0.9995);
        env.push(follower);
    }

    let steady = env[env.len() - 1];
    let half = env
        .iter()
        .position(|&v| v >= steady * 0.5)
        .map(|i| i as f32 / sr * 1000.0)
        .unwrap_or(f32::NAN);
    (half, steady)
}

fn main() {
    let sr = 48_000.0f32;
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() {
        println!("{:<13} {:>10} {:>9}", "patch", "to half", "level");
        for name in presets::names() {
            let (t, level) = speak_time(presets::by_name(&name).unwrap(), 60, sr);
            println!("{name:<13} {t:>9.1}ms {level:>9.3}");
        }
        return;
    }

    // Sweep the articulation impulse for one patch.
    let name = &args[0];
    let base = presets::by_name(name).unwrap();
    println!("{name}: impulse -> time/level/attack-overshoot (note 48 / 60 / 72)");
    for imp in [0.0f32, 2.0, 3.0, 4.0, 6.0, 8.0] {
        print!("  impulse {imp:5.1} ");
        for note in [48u8, 60, 72] {
            let mut p = base.clone();
            p.element.driver.attack_impulse = imp;
            if let Some(e2) = p.element2.as_mut() {
                e2.driver.attack_impulse = imp;
            }
            let (t, lv, over) = speak_detail(p, note, sr);
            print!("  {t:>6.1}ms/{lv:.2}/x{over:.1}");
        }
        println!();
    }
}
