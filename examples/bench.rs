//! Throughput and worst-case block timing.
//!
//! An average well under realtime still crackles if individual blocks miss their
//! deadline, so this reports the worst block as well as the mean — including the
//! blocks where notes start, which are the expensive ones.

use std::time::Instant;
use vl1::{presets, Engine};

fn main() {
    let sr = 48_000.0f32;
    let block = std::env::args()
        .nth(1)
        .and_then(|a| a.parse::<usize>().ok())
        .unwrap_or(256);
    let budget = block as f64 / sr as f64;

    println!(
        "block {block} frames at {:.0} Hz = {:.2} ms of audio per callback\n",
        sr,
        budget * 1000.0
    );
    println!(
        "{:<13} {:>8} {:>9} {:>9} {:>8}",
        "patch", "mean", "worst", "worst-on", "over"
    );

    for name in presets::names() {
        let patch = presets::by_name(&name).unwrap();
        let mut engine = Engine::with_patch(patch, sr);
        let mut buf = vec![0.0f32; block * 2];

        let mut total = 0.0f64;
        let mut worst: f64 = 0.0;
        let mut worst_noteon: f64 = 0.0;
        let mut over = 0usize;
        let blocks = 1500;

        for i in 0..blocks {
            // Start all eight voices at once every so often: the worst case is a
            // block that has to retune eight waveguides as well as render.
            let note_block = i % 200 == 0;
            if note_block {
                engine.panic();
                for v in 0..8 {
                    engine.note_on(48 + v * 3, 100);
                }
            }

            let t = Instant::now();
            engine.render(&mut buf);
            let elapsed = t.elapsed().as_secs_f64();

            total += elapsed;
            worst = worst.max(elapsed);
            if note_block {
                worst_noteon = worst_noteon.max(elapsed);
            }
            if elapsed > budget {
                over += 1;
            }
        }

        let mean = total / blocks as f64;
        println!(
            "{name:<13} {:>7.1}% {:>8.1}% {:>8.1}% {:>8}",
            mean / budget * 100.0,
            worst / budget * 100.0,
            worst_noteon / budget * 100.0,
            over
        );
    }
}
