# vl1

An **8-voice virtual acoustic synthesizer** in Rust, built in the architecture of the
1994 Yamaha VL1.

The VL1 stored no waveforms. It stored a *description of an instrument* — a driver, a
resonator, a chain of modifiers — and solved it in real time, sample by sample, in
response to how it was being played. Blowing harder did not turn up a volume knob; it
changed the pressure across a reed, which changed how the reed beat against the
mouthpiece, which changed the spectrum, the pitch, and whether the thing spoke at all.
That is what this crate does.

No dependencies. The DSP, the WAV writer, and the CLI are all here.

## Architecture

```
  breath / bow ──▶  Driver  ──▶  Pipe / String  ──▶  Modifiers  ──▶  Amp  ──▶ out
                 (nonlinear)      (linear resonator)   (voicing)
                      ▲                    │
                      └──── reflection ◀───┘
```

The **driver** is the only nonlinear block and the only one the player touches directly.
The **resonator** stores one acoustic round trip and returns whatever it has not lost.
Neither produces a note on its own: the note is whatever the feedback loop between them
settles into. That is why these instruments respond the way real ones do — and also why
they can refuse to speak, overblow, or break register, exactly as real ones do.

| Module | Role |
| --- | --- |
| `driver` | Reed, lip, air-jet and bow excitation models |
| `waveguide` | Lossy delay loop with termination filtering |
| `modifier` | Impulse expander, resonator, harmonic enhancer, dynamic filter, 5-band EQ |
| `element` | One complete instrument: driver + resonator + modifiers + amp |
| `voice` | One or two elements, plus pitch, glide and panning |
| `engine` | The 8-voice pool, controls, and master effects |
| `patch` | The serialisable description of a sound |

### Drivers

- **Reed** — a nonlinear reflection coefficient driven by the pressure difference across
  the reed, plus reed inertia. Clarinet, saxophone.
- **Lip** — the same junction, but the valve is driven through a damped lip resonance
  that selects which partial of the bore the player is locked onto. Trumpet, trombone.
- **Jet** — an air ribbon crossing an edge, with its own delay that is a fraction of the
  bore's. Flute, shakuhachi.
- **Bow** — stick-slip friction against the string. Violin, cello.

### Polyphony

The hardware VL1 was **two-voice**: each voice cost a dedicated DSP chip. This engine
runs **eight**, allocated with last-note priority and quietest-first stealing, with a
sustain pedal and a voice pool that never exceeds its cap.

Eight voices cost roughly **10% of one core** at 48 kHz — about 10× faster than realtime
(`cargo run --release --example bench`).

## Usage

```rust
use vl1::{Engine, presets};

let mut engine = Engine::with_patch(presets::tenor_sax(), 48_000.0);
engine.note_on(60, 100);

// Lean on the breath controller mid-note.
engine.controls_mut().pressure = 1.15;

let mut buffer = vec![0.0f32; 48_000 * 2]; // 1 second, stereo interleaved
engine.render(&mut buffer);
```

Raw MIDI works too, including a wind-controller CC map (breath on CC2, embouchure on
CC3, scream on CC16, growl on CC17, damping on CC18, absorption on CC19):

```rust
vl1::midi::handle(&mut engine, &[0x90, 60, 100]);   // note on
vl1::midi::handle(&mut engine, &[0xB0, 2, 127]);    // breath controller
```

### Command line

```console
$ cargo run --release --bin vl1-render -- --list
$ cargo run --release --bin vl1-render -- --preset "Tenor Sax" --mode phrase --out sax.wav
$ cargo run --release --bin vl1-render -- --preset Flute --mode chord --out chord.wav
$ cargo run --release --bin vl1-render -- --all demos/
```

`--mode` is `note`, `chord` (eight notes, one per voice), `phrase` (a legato line), or
`steal` (nine overlapping notes against eight voices). The renderer applies a breath
swell across the take, because a physical model played at constant pressure throws away
most of what it is for.

## Factory patches

Each patch declares the key range over which it has been *verified* to speak and stay in
tune, and `tests/engine.rs` checks that claim every three semitones across the whole
range. Physical models have practical ranges just as real instruments do; nothing stops
you playing outside one, but the model may break register or fail to start.

| Patch | Driver | Bore | Range | Worst tuning error |
| --- | --- | --- | --- | --- |
| Clarinet | Reed | odd harmonics | C2–C6 | +30 c |
| Tenor Sax | Reed | all harmonics | C2–C6 | −13 c |
| Trumpet | Lip | all harmonics | C2–D5 | −15 c |
| Trombone | Lip | all harmonics | C2–E4 | −14 c |
| Flute | Jet | all harmonics | C2–G5 | −13 c |
| Shakuhachi | Jet | all harmonics | C2–C5 | −13 c |
| Violin | Bow | all harmonics | C2–C5 | −6 c |
| Cello | Bow | all harmonics | C2–C5 | +5 c |
| Scream Lead | Reed | all harmonics | C2–C6 | +18 c |
| Breath Pad | Jet + Bow | all harmonics | C2–C6 | −8 c |

The clarinet is the only patch using an odd-harmonic (quarter-wave) bore, which is why
it is hollow, why it overblows a twelfth rather than an octave, and why it is the patch
most sensitive to loop-length error — its loop is half as long, so any given error in
samples costs twice the cents.

## Expressive controls

All global, as on a real instrument with one player:

| Control | What it does |
| --- | --- |
| `pressure` | Mouth pressure / bow speed. The main expression control. |
| `embouchure` | Reed and lip tension, bow force. On brass it selects the partial. |
| `scream` | Drives the excitation into its chaotic regime. |
| `growl` | Throat flutter. |
| `damping` | How much high frequency the resonator loses per round trip. |
| `absorption` | Broadband loss: how long the resonator rings. |
| `modulation` | Vibrato depth. |
| `expression` | Output level. |

Each patch also carries tonguing, throat formant, breath noise, and per-element vibrato.

## Notes on the model

A few things the implementation gets right that are easy to get wrong, all of which
showed up as measurable defects during development:

- **Loop length is not just the delay line.** The termination filter's phase delay and
  the one-sample latency of reading the reflection are both part of the round trip. Left
  uncompensated, every patch plays flat.
- **A mouthpiece must not throttle the bore's reflection.** Scaling the returning wave by
  how far the lips are open damps the tube's modes until the lip resonance, not the tube,
  decides the pitch.
- **Nothing radiates DC.** An instrument radiates through an opening, which cannot couple
  a steady pressure to the air. Without a high-pass at the element output, most of what
  comes out is the breath offset rather than the tone.
- **A lip reed is heavily damped.** Real lips have a Q in the single digits. A high-Q lip
  stops selecting a partial and starts dictating the pitch.
- **Reeds have mass.** Without that lag, the upper modes regenerate as strongly as the
  fundamental and notes jump register on their own.
- **Drivers have a pressure window.** Blow past it and the valve simply stays shut — the
  instrument goes silent rather than louder. Patch levels are calibrated inside that
  window; loudness is set afterwards, in the output stage.

## Tests

```console
$ cargo test --release
```

Covers: every patch speaks, stays bounded, and holds pitch across its declared range;
notes release to silence; the pool stays at eight voices under a ninth note; the sustain
pedal holds and releases; MIDI drives the engine; and every control pushed to its limit
at once on every patch never produces a non-finite or out-of-range sample.

Pitch is measured with YIN rather than plain autocorrelation — breath noise and bow
scratch make a correlation peak-picker return whatever the shortest allowed lag is,
which quietly turns the tuning test into a no-op.

`examples/sweep.rs` is the development harness used to calibrate the patches; it renders
exactly the way the tests do, so its numbers and theirs agree.

```console
$ cargo run --release --example sweep -- table
$ cargo run --release --example sweep -- notes Trumpet 60 79
$ cargo run --release --example sweep -- param Clarinet reed_cutoff 1500 3000 6000
```

## License

MIT
