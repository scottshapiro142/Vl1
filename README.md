# vl1

An **8-voice virtual acoustic synthesizer** in Rust, built in the architecture of the
1994 Yamaha VL1.

The VL1 stored no waveforms. It stored a *description of an instrument* — a driver, a
resonator, a chain of modifiers — and solved it in real time, sample by sample, in
response to how it was being played. Blowing harder did not turn up a volume knob; it
changed the pressure across a reed, which changed how the reed beat against the
mouthpiece, which changed the spectrum, the pitch, and whether the thing spoke at all.
That is what this crate does.

The synthesis library has **no dependencies** — the DSP, the WAV writer and the offline
renderer are all here. Playing it live needs an audio device, MIDI input and raw terminal
keys, so that lives behind an optional `live` feature and nothing else has to build it.

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

## Playing it live

```console
$ cargo run --release --features live --bin vl1-play
$ cargo run --release --features live --bin vl1-play -- --preset Shakuhachi
$ cargo run --release --features live --bin vl1-play -- --list
```

`vl1-play` opens the default audio device, connects to every MIDI input it can
find, and also plays from the computer keyboard. The synthesis library itself
stays dependency-free — audio, MIDI and terminal handling live behind the `live`
feature, so nothing that only renders offline has to build them.

On Linux you will need ALSA's development headers (`libasound2-dev` on Debian and
Ubuntu, `alsa-lib-devel` on Fedora). macOS and Windows need nothing extra.

### Computer keyboard

```
  black   2 3   5 6 7   9 0        s d   g h j
  white  q w e r t y u i o p     z x c v b n m , . /
         (octave above)          (current octave)
```

| Key | |
| --- | --- |
| `[` `]` | previous / next patch |
| `-` `=` | octave down / up |
| ↑ ↓ | breath pressure |
| ← → | embouchure |
| `1` | scream on/off |
| `4` | growl on/off |
| `space` | sustain pedal |
| `8` | panic |
| `esc` | quit |

One caveat worth knowing before you try to play a chord: terminals traditionally
report only key *presses*, and auto-repeat applies to the most recent key alone,
so a held chord decays to its last note. `vl1-play` requests the kitty keyboard
protocol, and where the terminal supports it (kitty, foot, WezTerm, Ghostty,
recent xterm) you get real key-release events and proper polyphonic playing.
Elsewhere it falls back to releasing a note shortly after its auto-repeat stops,
which plays melodies correctly but not sustained chords. It says which mode it
got on startup. For chords, use MIDI.

### MIDI

Every input port is connected by default (`--midi-port N` picks one,
`--no-midi` ignores them all). Notes, velocity, pitch bend and channel
aftertouch all work, and the controller map is the wind-controller layout the
VL1 was built around:

| CC | Control |
| --- | --- |
| 1 | vibrato depth |
| 2 | breath pressure |
| 3 | embouchure |
| 7 / 11 | volume / expression |
| 16 | scream |
| 17 | growl |
| 18 | damping |
| 19 | absorption |
| 64 | sustain |

A breath controller on CC2 is what this engine is really for: pressure is not a
volume knob here, it is the thing the whole model is solved around.

### If it feels laggy

Two unrelated things add delay, and they are worth separating:

**The audio buffer.** Shown at startup and in the status line. `vl1-play` asks
for 256 frames (about 5 ms at 48 kHz) rather than accepting the host default,
which on PulseAudio or PipeWire can be thousands of frames. `--buffer` changes
it; smaller is tighter until it starts costing you `xrun`s.

**How long the instrument takes to speak.** This one is peculiar to physical
modelling. A waveguide does not start at full amplitude — the oscillation grows
at a rate set by how far the driver's gain exceeds the loop's losses, and a patch
voiced close to its threshold can take *half a second* to become audible. No
audio setting will fix that, because it is the instrument, not the software.

What fixes it is the same thing that fixes it on a real instrument: an
articulation. `attack_impulse` injects a single half-cycle pulse at the played
pitch into the resonator at note-on — the tongue releasing, the bow biting — so
the loop starts with a body of energy instead of amplifying its own noise floor.
Every playable patch here speaks within about 4 ms, and `tests/engine.rs` holds
them to it.

The pulse is pitched rather than broadband on purpose. A click excites every
mode of the tube at once, and a cylindrical bore answers by jumping to its third
mode — which is a clarinet overblowing to the twelfth. Correct behaviour for a
clarinet, wrong note for a synthesiser, and with a noise burst which way it went
depended on the noise. Too much `attack_impulse` still overblows: the value in
each patch sits below where its own register breaks, measured across its range.

### If it sounds bad

Choppy and distorted are different faults with opposite fixes, and they are hard
to tell apart by ear, so the status line names them:

```
  Tenor Sax    oct 4  br 100 emb  64 ... | 3 voices  cpu   7% (max  21%)  buf 256   peak  412
```

- **`xrun`** — the audio callback missed its deadline. That is a dropout: clicks,
  gaps, stuttering. Raise `--buffer` (try 512 or 1024). Check `cpu`: the engine
  uses under 10% of a 256-frame budget on a modern core, so a high reading means
  something else is wrong — almost always a **debug build**. Use `--release`;
  debug is roughly twenty times slower and cannot keep up.
- **`lim`** — the output limiter is saturating. That is distortion, not a
  dropout, and a bigger buffer will not help. Pull the level down with
  `--gain -6`, or play fewer notes at once.
- Neither flag, but everything feels **late**: see the section above — audio
  buffer and speaking time are different problems.
- Neither flag, but notes **stutter or double-strike**: something is retriggering
  them. From the computer keyboard in a terminal without key-release reporting,
  a held note is kept alive by auto-repeat, which is inherently uneven. Over
  MIDI, check the startup banner for more than one connected port — a controller
  exposed twice delivers every note twice. `--midi-port N` picks one.

`--selftest` renders the whole control path with no audio device, so if it
reports every patch `ok` the synthesis is fine and the problem is in the audio
configuration.

### How the realtime side is put together

The audio callback owns one `Engine` per factory patch, so switching patches is
an index change rather than an allocation. The keyboard thread and each MIDI
callback hand it commands through `vl1::queue`, a lock-free SPSC ring of `Copy`
values. Nothing on the audio thread locks, allocates or frees.

If you have no sound card — a container, a CI box — `--selftest` drives that
whole path with the device removed, so you can tell a broken engine apart from a
broken audio setup:

```console
$ cargo run --release --features live --bin vl1-play -- --selftest
```

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
| Flute | Jet | all harmonics | C2–D5 | −13 c |
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
at once on every patch never produces a non-finite or out-of-range sample. The keyboard
map and the lock-free queue are tested too — the queue against a live concurrent
producer.

The parts that need real hardware (the audio device, MIDI ports, terminal key handling)
are not unit-testable; `vl1-play --selftest` covers everything behind them.

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
