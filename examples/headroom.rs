//! How hard the output limiter is working on a full eight-voice chord.
use vl1::{presets, Engine};
fn main() {
    let sr = 48_000.0f32;
    for name in presets::names() {
        let mut e = Engine::with_patch(presets::by_name(&name).unwrap(), sr);
        for (i, iv) in [0, 7, 12, 16, 19, 24, 26, 31].iter().enumerate() {
            let _ = i;
            e.note_on((60 + iv) as u8, 100);
        }
        let n = (sr * 3.0) as usize;
        let (mut peak, mut over) = (0.0f32, 0usize);
        for _ in 0..n {
            let s = e.tick();
            let a = s[0].abs().max(s[1].abs());
            peak = peak.max(a);
            // tanh(1.0) = 0.7616: above this the limiter is compressing > 1 dB.
            if a > 0.7616 {
                over += 1;
            }
        }
        println!(
            "{name:<13} peak {peak:.3}  limiting on {:.2}% of samples",
            100.0 * over as f32 / n as f32
        );
    }
}
