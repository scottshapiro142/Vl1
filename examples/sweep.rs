//! Parameter sweep / range verification harness (development tool).
//!
//! Renders exactly the way tests/engine.rs does, so its numbers and the test's
//! agree. Usage:
//!   cargo run --release --example sweep -- table
//!   cargo run --release --example sweep -- param "Tenor Sax" reed_cutoff 1500 2200 3000 4500

use vl1::{presets, Engine, Patch};

const SR: f32 = 48_000.0;

/// YIN fundamental-frequency estimator.
///
/// Plain autocorrelation is not usable here: breath noise and bow scratch make
/// the correlation decay monotonically, so the peak-picker just returns the
/// shortest lag it is allowed. YIN's cumulative mean normalized difference
/// function is insensitive to that, which is the whole reason it exists.
fn detect_pitch(x: &[f32], sample_rate: f32, min_hz: f32, max_hz: f32) -> f32 {
    let min_lag = (sample_rate / max_hz).floor().max(2.0) as usize;
    let max_lag = ((sample_rate / min_hz).ceil() as usize).min(x.len() / 2);
    if max_lag <= min_lag {
        return 0.0;
    }

    // Difference function.
    let mut d = vec![0.0f64; max_lag + 1];
    let n = x.len() - max_lag;
    for (lag, slot) in d.iter_mut().enumerate().skip(1) {
        let mut sum = 0.0f64;
        for i in 0..n {
            let diff = (x[i] - x[i + lag]) as f64;
            sum += diff * diff;
        }
        *slot = sum;
    }

    // Cumulative mean normalization.
    let mut cmnd = vec![1.0f64; max_lag + 1];
    let mut running = 0.0f64;
    for lag in 1..=max_lag {
        running += d[lag];
        cmnd[lag] = if running > 0.0 {
            d[lag] * lag as f64 / running
        } else {
            1.0
        };
    }

    // First local minimum below the threshold, else the global minimum.
    const THRESHOLD: f64 = 0.15;
    let mut best = min_lag;
    let mut found = false;
    for lag in min_lag..max_lag {
        if cmnd[lag] < THRESHOLD && cmnd[lag] <= cmnd[lag + 1] {
            best = lag;
            found = true;
            break;
        }
    }
    if !found {
        best = (min_lag..=max_lag)
            .min_by(|a, b| cmnd[*a].partial_cmp(&cmnd[*b]).unwrap())
            .unwrap_or(min_lag);
    }

    // Parabolic refinement.
    let mut lag = best as f64;
    if best > min_lag && best < max_lag {
        let (a, b, c) = (cmnd[best - 1], cmnd[best], cmnd[best + 1]);
        let denom = a - 2.0 * b + c;
        if denom.abs() > 1e-12 {
            lag += 0.5 * (a - c) / denom;
        }
    }
    (sample_rate as f64 / lag) as f32
}

fn render_note(patch: Patch, note: u8) -> Vec<f32> {
    let mut engine = Engine::with_patch(patch, SR);
    engine.note_on(note, 100);
    let n = (2.5 * SR) as usize;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let s = engine.tick();
        out.push((s[0] + s[1]) * 0.5);
    }
    out
}

fn probe(patch: Patch, note: u8) -> (f32, f32) {
    let target = 440.0 * ((note as f32 - 69.0) / 12.0).exp2();
    let samples = render_note(patch, note);
    let sustain = &samples[(1.2 * SR) as usize..(2.4 * SR) as usize];
    let rms = (sustain.iter().map(|s| s * s).sum::<f32>() / sustain.len() as f32).sqrt();
    let hz = detect_pitch(sustain, SR, 40.0, 2000.0);
    (rms, 1200.0 * (hz / target).log2())
}

/// Worst |cents| and lowest rms over a note range, stepping by 3 semitones.
fn scan(patch: &Patch, low: u8, high: u8) -> (f32, f32, u8) {
    let (mut worst, mut min_rms, mut worst_note) = (0.0f32, f32::MAX, low);
    let mut note = low;
    loop {
        let (rms, cents) = probe(patch.clone(), note);
        if cents.abs() > worst.abs() {
            worst = cents;
            worst_note = note;
        }
        min_rms = min_rms.min(rms);
        if note >= high {
            break;
        }
        note = (note + 3).min(high);
    }
    (worst, min_rms, worst_note)
}

fn set_param(p: &mut Patch, name: &str, v: f32) {
    if name == "breath_noise" {
        p.element.driver.breath_noise = v;
        return;
    }
    if name == "breath_level" {
        p.element.breath.level = v;
        return;
    }
    let d = &mut p.element.driver;
    let pipe = &mut p.element.pipe;
    match name {
        "reed_cutoff" => d.reed_cutoff = v,
        "embouchure" => d.embouchure = v,
        "embouchure_key_track" => d.embouchure_key_track = v,
        "jet_ratio" => d.jet_ratio = v,
        "lip_q" => d.lip_q = v,
        "lip_offset" => d.lip_offset = v,
        "damping_hz" => pipe.damping_hz = v,
        "damping_key_track" => pipe.damping_key_track = v,
        "damping_breath" => pipe.damping_breath = v,
        "absorption" => pipe.absorption = v,
        other => panic!("unknown parameter `{other}`"),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args.first().map(String::as_str).unwrap_or("table");

    match mode {
        "table" => {
            for name in presets::names() {
                let p = presets::by_name(&name).unwrap();
                let (lo, hi) = p.performance.key_range;
                let (worst, rms, note) = scan(&p, lo, hi);
                println!(
                    "{name:<13} range {lo}-{hi}  worst {worst:6.1}c at note {note:<3}  min rms {rms:.3}"
                );
            }
        }
        "grid" => {
            let p0 = presets::by_name(&args[1]).unwrap();
            let (lo, hi) = p0.performance.key_range;
            for emb in [0.3f32, 0.4, 0.5, 0.6] {
                for lvl in [0.55f32, 0.65, 0.75, 0.9, 1.1] {
                    let mut p = p0.clone();
                    p.element.driver.embouchure = emb;
                    p.element.breath.level = lvl;
                    let (worst, rms, note) = scan(&p, lo, hi);
                    println!("  emb={emb:3.1} lvl={lvl:4.2}  worst {worst:8.1}c at {note:<3} min rms {rms:.3}");
                }
            }
        }
        "calib" => {
            for name in presets::names() {
                let p0 = presets::by_name(&name).unwrap();
                let (lo, hi) = p0.performance.key_range;
                let base_level = p0.element.breath.level;
                println!("{name}  (breath.level {base_level:.2}, range {lo}-{hi})");
                for mult in [1.0f32, 1.2, 1.4, 1.6, 1.9, 2.3] {
                    let mut p = p0.clone();
                    p.element.breath.level = base_level * mult;
                    let (worst, rms, note) = scan(&p, lo, hi);
                    println!("    x{mult:.1} -> level {:.2}  worst {worst:8.1}c at {note:<3} min rms {rms:.3}",
                             base_level * mult);
                }
            }
        }
        "levels" => {
            for name in presets::names() {
                let p = presets::by_name(&name).unwrap();
                let mut line = format!("{name:<13}");
                for note in [41u8, 53, 60, 67, 79] {
                    let (rms, _) = probe(p.clone(), note);
                    line.push_str(&format!("  n{note}:{rms:.4}"));
                }
                println!("{line}");
            }
        }
        "param" => {
            let preset = &args[1];
            let param = &args[2];
            let p0 = presets::by_name(preset).unwrap();
            let (lo, hi) = p0.performance.key_range;
            for value in &args[3..] {
                let v: f32 = value.parse().unwrap();
                let mut p = p0.clone();
                set_param(&mut p, param, v);
                let (worst, rms, note) = scan(&p, lo, hi);
                println!("{param}={v:<9} worst {worst:7.1}c at note {note:<3} min rms {rms:.3}");
            }
        }
        "notes" => {
            let mut p0 = presets::by_name(&args[1]).unwrap();
            if args.len() > 5 {
                let v: f32 = args[5].parse().unwrap();
                set_param(&mut p0, &args[4], v);
            }
            let lo: u8 = args[2].parse().unwrap();
            let hi: u8 = args[3].parse().unwrap();
            for note in lo..=hi {
                let (rms, cents) = probe(p0.clone(), note);
                println!("  note {note:<3} rms {rms:.3}  {cents:+8.1}c");
            }
        }
        other => eprintln!("unknown mode `{other}`"),
    }
}
