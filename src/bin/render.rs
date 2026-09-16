//! Offline renderer for the vl1 engine.
//!
//! Examples:
//! ```text
//!   vl1-render --list
//!   vl1-render --preset "Tenor Sax" --mode phrase --out sax.wav
//!   vl1-render --preset Flute --mode chord --out chord.wav
//!   vl1-render --all demos/
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

use vl1::engine::POLYPHONY;
use vl1::{presets, wav, Engine};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// A single sustained note with a breath swell.
    Note,
    /// An eight-note chord — one note per voice.
    Chord,
    /// A short legato melodic line.
    Phrase,
    /// Nine overlapping notes, so the eight-voice pool has to steal one.
    Steal,
}

impl Mode {
    fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "note" => Some(Mode::Note),
            "chord" => Some(Mode::Chord),
            "phrase" => Some(Mode::Phrase),
            "steal" => Some(Mode::Steal),
            _ => None,
        }
    }
}

struct Args {
    preset: String,
    out: PathBuf,
    sample_rate: f32,
    mode: Mode,
    note: u8,
    velocity: u8,
    duration: f32,
    tail: f32,
    list: bool,
    all: Option<PathBuf>,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            preset: "Clarinet".into(),
            out: PathBuf::from("out.wav"),
            sample_rate: 48_000.0,
            mode: Mode::Note,
            note: 60,
            velocity: 100,
            duration: 3.0,
            tail: 1.5,
            list: false,
            all: None,
        }
    }
}

fn usage() -> &'static str {
    "vl1-render — offline renderer for the 8-voice VL1-style engine

USAGE:
    vl1-render [OPTIONS]

OPTIONS:
    --preset <NAME>   Factory patch to play (default: Clarinet)
    --mode <MODE>     note | chord | phrase | steal (default: note)
    --out <FILE>      Output WAV path (default: out.wav)
    --note <0-127>    Root MIDI note (default: 60)
    --vel <1-127>     Velocity (default: 100)
    --dur <SECONDS>   Note/phrase length (default: 3.0)
    --tail <SECONDS>  Extra time rendered after release (default: 1.5)
    --sr <HZ>         Sample rate (default: 48000)
    --all <DIR>       Render every factory patch into DIR and exit
    --list            List factory patches and exit
    -h, --help        Show this help
"
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args::default();
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = || {
            it.next()
                .ok_or_else(|| format!("option `{arg}` needs a value"))
        };
        match arg.as_str() {
            "--preset" => args.preset = value()?,
            "--out" => args.out = PathBuf::from(value()?),
            "--all" => args.all = Some(PathBuf::from(value()?)),
            "--list" => args.list = true,
            "--mode" => {
                let v = value()?;
                args.mode = Mode::parse(&v).ok_or_else(|| format!("unknown mode `{v}`"))?;
            }
            "--note" => args.note = value()?.parse::<u8>().map_err(|e| e.to_string())?,
            "--vel" => args.velocity = value()?.parse::<u8>().map_err(|e| e.to_string())?,
            "--dur" => args.duration = value()?.parse::<f32>().map_err(|e| e.to_string())?,
            "--tail" => args.tail = value()?.parse::<f32>().map_err(|e| e.to_string())?,
            "--sr" => args.sample_rate = value()?.parse::<f32>().map_err(|e| e.to_string())?,
            "-h" | "--help" => {
                print!("{}", usage());
                std::process::exit(0);
            }
            other => return Err(format!("unknown option `{other}`")),
        }
    }
    Ok(args)
}

enum Action {
    NoteOn(u8, u8),
    NoteOff(u8),
}

struct Event {
    frame: usize,
    action: Action,
}

/// Build the note events for a mode, plus the total render length in frames.
fn build_score(args: &Args, mode: Mode) -> (Vec<Event>, usize) {
    let sr = args.sample_rate;
    let at = |s: f32| (s * sr) as usize;
    let mut events = Vec::new();
    let end;

    match mode {
        Mode::Note => {
            events.push(Event {
                frame: 0,
                action: Action::NoteOn(args.note, args.velocity),
            });
            events.push(Event {
                frame: at(args.duration),
                action: Action::NoteOff(args.note),
            });
            end = at(args.duration + args.tail);
        }
        Mode::Chord => {
            // A four-octave stack of a major ninth: eight notes, eight voices.
            let intervals = [0, 7, 12, 16, 19, 24, 26, 31];
            for (i, iv) in intervals.iter().enumerate() {
                let note = (args.note as i32 + iv).clamp(0, 127) as u8;
                // Roll the chord in slightly — all eight at once is inhumanly tidy.
                let onset = at(i as f32 * 0.045);
                events.push(Event {
                    frame: onset,
                    action: Action::NoteOn(note, args.velocity),
                });
                events.push(Event {
                    frame: at(args.duration),
                    action: Action::NoteOff(note),
                });
            }
            end = at(args.duration + args.tail);
        }
        Mode::Phrase => {
            let steps: [(f32, i32, f32); 8] = [
                (0.00, 0, 0.55),
                (0.50, 3, 0.30),
                (0.80, 5, 0.30),
                (1.10, 7, 0.75),
                (1.85, 10, 0.30),
                (2.15, 12, 0.90),
                (3.05, 7, 0.40),
                (3.45, 3, 1.30),
            ];
            let scale = args.duration / 4.75;
            let mut last = 0.0f32;
            for (start, iv, len) in steps {
                let note = (args.note as i32 + iv).clamp(0, 127) as u8;
                let s = start * scale;
                let e = (start + len) * scale;
                events.push(Event {
                    frame: at(s),
                    action: Action::NoteOn(note, args.velocity),
                });
                events.push(Event {
                    frame: at(e),
                    action: Action::NoteOff(note),
                });
                last = last.max(e);
            }
            end = at(last + args.tail);
        }
        Mode::Steal => {
            // Nine sustained notes against an eight-voice pool.
            for i in 0..(POLYPHONY + 1) {
                let note = (args.note as i32 + (i as i32) * 4).clamp(0, 127) as u8;
                events.push(Event {
                    frame: at(i as f32 * 0.25),
                    action: Action::NoteOn(note, args.velocity),
                });
                events.push(Event {
                    frame: at(args.duration + i as f32 * 0.1),
                    action: Action::NoteOff(note),
                });
            }
            end = at(args.duration + POLYPHONY as f32 * 0.1 + args.tail);
        }
    }

    events.sort_by_key(|e| e.frame);
    (events, end)
}

/// A breath-controller gesture: lean into the note, then ease off. Without
/// something like this every note starts and ends identically, which is exactly
/// what a physical model is supposed to save you from.
fn breath_at(t: f32, total: f32) -> f32 {
    let x = (t / total.max(1e-3)).clamp(0.0, 1.0);
    let swell = (x * std::f32::consts::PI).sin().powf(0.45);
    0.72 + 0.33 * swell
}

fn render(patch_name: &str, args: &Args, mode: Mode) -> Result<Vec<f32>, String> {
    let patch = presets::by_name(patch_name)
        .ok_or_else(|| format!("no factory patch named `{patch_name}`"))?;
    let mut engine = Engine::with_patch(patch, args.sample_rate);

    let (events, total_frames) = build_score(args, mode);
    let mut out = vec![0.0f32; total_frames * 2];
    let mut next = 0usize;
    let total_s = total_frames as f32 / args.sample_rate;

    for frame in 0..total_frames {
        while next < events.len() && events[next].frame <= frame {
            match events[next].action {
                Action::NoteOn(n, v) => engine.note_on(n, v),
                Action::NoteOff(n) => engine.note_off(n),
            }
            next += 1;
        }

        let t = frame as f32 / args.sample_rate;
        engine.controls_mut().pressure = breath_at(t, total_s);

        let s = engine.tick();
        out[frame * 2] = s[0];
        out[frame * 2 + 1] = s[1];
    }

    Ok(out)
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

fn run() -> Result<(), String> {
    let args = parse_args()?;

    if args.list {
        println!("Factory patches:");
        for name in presets::names() {
            println!("  {name}");
        }
        return Ok(());
    }

    if let Some(dir) = &args.all {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        for name in presets::names() {
            let samples = render(&name, &args, args.mode)?;
            let file = dir.join(format!("{}.wav", name.to_lowercase().replace(' ', "-")));
            wav::write(&file, &samples, args.sample_rate as u32, 2).map_err(|e| e.to_string())?;
            println!(
                "{:<14} peak {:.3}  -> {}",
                name,
                peak(&samples),
                file.display()
            );
        }
        return Ok(());
    }

    let samples = render(&args.preset, &args, args.mode)?;
    wav::write(&args.out, &samples, args.sample_rate as u32, 2).map_err(|e| e.to_string())?;
    println!(
        "{} | {:.2}s | peak {:.3} -> {}",
        args.preset,
        samples.len() as f32 / 2.0 / args.sample_rate,
        peak(&samples),
        args.out.display()
    );
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}\n\n{}", usage());
            ExitCode::FAILURE
        }
    }
}
