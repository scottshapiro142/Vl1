//! Throughput benchmark: how much faster than realtime the engine renders.

use std::time::Instant;
use vl1::{presets, Engine};

fn main() {
    let sr = 48_000.0f32;
    let seconds = 10.0f32;
    let frames = (sr * seconds) as usize;

    for name in ["Clarinet", "Trumpet", "Breath Pad"] {
        let patch = presets::by_name(name).unwrap();
        let elements = 1 + usize::from(patch.element2.is_some());
        let mut engine = Engine::with_patch(patch, sr);
        for i in 0..8 {
            engine.note_on(48 + i * 3, 100);
        }

        let mut buf = vec![0.0f32; 4096];
        let start = Instant::now();
        let mut done = 0;
        while done < frames {
            let n = (frames - done).min(2048);
            engine.render(&mut buf[..n * 2]);
            done += n;
        }
        let elapsed = start.elapsed().as_secs_f32();
        println!(
            "{name:<12} 8 voices x {elements} element(s): {:.0}x realtime ({:.1}% of one core)",
            seconds / elapsed,
            100.0 * elapsed / seconds
        );
    }
}
