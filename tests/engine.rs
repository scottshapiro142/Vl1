//! Integration tests: every factory patch must sound, stay bounded, play in
//! tune, and go silent when released.

use vl1::engine::POLYPHONY;
use vl1::{presets, Engine};

const SR: f32 = 48_000.0;

/// Render a single note: `hold` seconds sounding, then `tail` seconds after
/// release. Returns the mono sum.
fn render_note(patch: vl1::Patch, note: u8, hold: f32, tail: f32) -> Vec<f32> {
    let mut engine = Engine::with_patch(patch, SR);
    let hold_n = (hold * SR) as usize;
    let tail_n = (tail * SR) as usize;
    let mut out = Vec::with_capacity(hold_n + tail_n);

    engine.note_on(note, 100);
    for i in 0..(hold_n + tail_n) {
        if i == hold_n {
            engine.note_off(note);
        }
        let s = engine.tick();
        out.push((s[0] + s[1]) * 0.5);
    }
    out
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

fn rms(x: &[f32]) -> f32 {
    if x.is_empty() {
        return 0.0;
    }
    (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt()
}

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

#[test]
fn every_preset_sounds_and_stays_bounded() {
    for patch in presets::all() {
        let name = patch.name.clone();
        let samples = render_note(patch, 60, 2.0, 1.0);

        assert!(
            samples.iter().all(|s| s.is_finite()),
            "{name}: produced a non-finite sample"
        );
        let p = peak(&samples);
        assert!(p <= 1.0, "{name}: peak {p} exceeds full scale");
        assert!(
            p > 0.02,
            "{name}: peak {p} is inaudibly quiet — it never spoke"
        );

        // Sustain portion should be well established by one second in.
        let sustain = &samples[(1.0 * SR) as usize..(1.9 * SR) as usize];
        assert!(
            rms(sustain) > 0.004,
            "{name}: sustain died out (rms {})",
            rms(sustain)
        );
    }
}

#[test]
fn every_preset_plays_in_tune_across_its_range() {
    // A waveguide whose loop is a sample or two off length plays measurably
    // flat, and a driver that loses its grip on the right mode jumps register
    // entirely. Both show up here, which is why this sweeps the whole declared
    // range rather than checking one comfortable note in the middle.
    for patch in presets::all() {
        let name = patch.name.clone();
        let (low, high) = patch.performance.key_range;

        let mut note = low;
        loop {
            let target = 440.0 * ((note as f32 - 69.0) / 12.0).exp2();
            let samples = render_note(patch.clone(), note, 2.5, 0.0);
            let sustain = &samples[(1.2 * SR) as usize..(2.4 * SR) as usize];

            // Well above the noise floor of a patch that is only hissing, but
            // below the quietest genuine limit cycle in the factory set. Master
            // gain is staged for eight voices, so a single note is modest.
            assert!(
                rms(sustain) > 0.006,
                "{name}: note {note} never spoke (rms {:.4})",
                rms(sustain)
            );

            let hz = detect_pitch(sustain, SR, 40.0, 2000.0);
            let cents = 1200.0 * (hz / target).log2();
            assert!(
                cents.abs() < 35.0,
                "{name}: note {note} played {hz:.2} Hz, {cents:+.1} cents off"
            );

            if note >= high {
                break;
            }
            note = (note + 3).min(high);
        }
    }
}

#[test]
fn every_preset_speaks_promptly() {
    // A waveguide near its oscillation threshold can take the better part of a
    // second to grow out of the noise floor, which is heard as latency however
    // small the audio buffer is. The articulation transient at note-on is what
    // prevents that, and this is the test that would catch it going missing.
    for patch in presets::all() {
        let name = patch.name.clone();
        // A patch is allowed to be slow only as far as its own envelope asks:
        // the pad swells deliberately, the sax does not.
        let budget_ms = patch.element.breath.attack * 2000.0 + 30.0;

        let samples = render_note(patch, 60, 1.5, 0.0);
        let steady = samples[(1.0 * SR) as usize..]
            .iter()
            .fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(steady > 0.0, "{name}: never sounded at all");

        let mut follower = 0.0f32;
        let mut spoke = None;
        for (i, s) in samples.iter().enumerate() {
            follower = s.abs().max(follower * 0.9995);
            if follower >= steady * 0.5 {
                spoke = Some(i as f32 / SR * 1000.0);
                break;
            }
        }

        let spoke = spoke.expect("envelope never reached half of its own steady level");
        assert!(
            spoke <= budget_ms,
            "{name}: took {spoke:.0} ms to speak, budget {budget_ms:.0} ms"
        );
    }
}

#[test]
fn notes_release_to_silence() {
    for patch in presets::all() {
        let name = patch.name.clone();
        let samples = render_note(patch, 60, 1.0, 4.0);
        let tail = &samples[samples.len() - (0.5 * SR) as usize..];
        assert!(
            rms(tail) < 1e-4,
            "{name}: still ringing 3.5s after release (rms {})",
            rms(tail)
        );
    }
}

#[test]
fn engine_is_eight_voice() {
    let mut engine = Engine::with_patch(presets::clarinet(), SR);
    assert_eq!(POLYPHONY, 8);
    assert_eq!(engine.polyphony(), 8);

    for i in 0..8 {
        engine.note_on(60 + i * 2, 100);
    }
    let mut buf = vec![0.0f32; 4096];
    engine.render(&mut buf);
    assert_eq!(engine.active_voices(), 8, "all eight voices should sound");
    assert!(buf.iter().all(|s| s.is_finite()));
    assert!(peak(&buf) > 0.0);
}

#[test]
fn ninth_note_steals_a_voice() {
    let mut engine = Engine::with_patch(presets::clarinet(), SR);
    let mut buf = vec![0.0f32; 2048];

    for i in 0..8 {
        engine.note_on(60 + i, 100);
        engine.render(&mut buf);
    }
    assert_eq!(engine.active_voices(), 8);

    engine.note_on(72, 100);
    engine.render(&mut buf);
    assert_eq!(
        engine.active_voices(),
        8,
        "polyphony must stay capped at eight"
    );
    assert!(buf.iter().all(|s| s.is_finite()));
}

#[test]
fn sustain_pedal_holds_released_notes() {
    let mut engine = Engine::with_patch(presets::cello(), SR);
    let mut buf = vec![0.0f32; 4096];

    engine.set_sustain(true);
    engine.note_on(60, 100);
    engine.render(&mut buf);
    engine.note_off(60);
    engine.render(&mut buf);
    assert_eq!(engine.active_voices(), 1, "pedal should hold the note");

    engine.set_sustain(false);
    for _ in 0..80 {
        engine.render(&mut buf);
    }
    assert_eq!(engine.active_voices(), 0, "note should release once lifted");
}

#[test]
fn midi_messages_drive_the_engine() {
    let mut engine = Engine::with_patch(presets::trumpet(), SR);
    let mut buf = vec![0.0f32; 2048];

    vl1::midi::handle(&mut engine, &[0x90, 64, 100]);
    engine.render(&mut buf);
    assert_eq!(engine.active_voices(), 1);

    vl1::midi::handle(&mut engine, &[0xB0, vl1::midi::cc::BREATH, 127]);
    assert!(engine.controls().pressure > 1.0);

    // Note-on with velocity 0 is a note-off.
    vl1::midi::handle(&mut engine, &[0x90, 64, 0]);
    for _ in 0..60 {
        engine.render(&mut buf);
    }
    assert_eq!(engine.active_voices(), 0);
}

#[test]
fn extreme_controls_stay_stable() {
    // Push every control to its limit at once and confirm nothing blows up.
    for patch in presets::all() {
        let name = patch.name.clone();
        let mut engine = Engine::with_patch(patch, SR);
        {
            let c = engine.controls_mut();
            c.pressure = 1.3;
            c.scream = 1.0;
            c.growl = 1.0;
            c.embouchure = 1.0;
            c.damping = 2.0;
            c.absorption = 1.0;
            c.modulation = 1.0;
        }
        for note in [36, 48, 60, 72, 84, 96] {
            engine.note_on(note, 127);
        }
        let mut buf = vec![0.0f32; (SR as usize) * 2];
        engine.render(&mut buf);
        assert!(
            buf.iter().all(|s| s.is_finite()),
            "{name}: non-finite output under extreme controls"
        );
        assert!(peak(&buf) <= 1.0, "{name}: output exceeded full scale");
    }
}
