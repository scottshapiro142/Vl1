//! Play the synth live, from a MIDI controller and/or the computer keyboard.
//!
//! ```console
//! $ cargo run --release --features live --bin vl1-play
//! $ cargo run --release --features live --bin vl1-play -- --list
//! $ cargo run --release --features live --bin vl1-play -- --preset "Tenor Sax"
//! ```
//!
//! Three threads: MIDI callbacks and the terminal keyboard both feed lock-free
//! queues, and the audio callback drains them and renders. Nothing on the audio
//! side locks, allocates or frees.

use std::error::Error;
use std::io::{stdout, Write};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SizedSample};
use crossterm::event::{
    self, Event as TermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use crossterm::{execute, style::Print};

use vl1::queue::{self, Consumer, Producer};
use vl1::{keymap, presets, Engine};

/// What the keyboard and MIDI threads send to the audio thread.
#[derive(Clone, Copy, Debug)]
enum Command {
    /// A raw MIDI message, zero-padded. `len` says how many bytes are real.
    Midi { bytes: [u8; 3], len: u8 },
    /// Switch to the factory patch at this index.
    Preset(usize),
    /// Silence everything.
    Panic,
}

impl Command {
    fn midi(bytes: &[u8]) -> Option<Self> {
        if bytes.is_empty() || bytes.len() > 3 {
            return None;
        }
        let mut buf = [0u8; 3];
        buf[..bytes.len()].copy_from_slice(bytes);
        Some(Command::Midi {
            bytes: buf,
            len: bytes.len() as u8,
        })
    }
}

// ---------------------------------------------------------------------------
// Arguments
// ---------------------------------------------------------------------------

struct Args {
    preset: String,
    list: bool,
    selftest: bool,
    midi_port: Option<usize>,
    no_midi: bool,
    buffer: Option<u32>,
}

fn usage() -> &'static str {
    "vl1-play — play the 8-voice VL1-style engine live

USAGE:
    vl1-play [OPTIONS]

OPTIONS:
    --preset <NAME>   Factory patch to start on (default: Tenor Sax)
    --midi-port <N>   Connect only to this MIDI input (default: all of them)
    --no-midi         Computer keyboard only
    --buffer <FRAMES> Audio buffer size; smaller is tighter but riskier
    --list            List audio devices, MIDI inputs and patches, then exit
    --selftest        Exercise the control path without an audio device
    -h, --help        Show this help
"
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        preset: "Tenor Sax".into(),
        list: false,
        selftest: false,
        midi_port: None,
        no_midi: false,
        buffer: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = || {
            it.next()
                .ok_or_else(|| format!("option `{arg}` needs a value"))
        };
        match arg.as_str() {
            "--preset" => args.preset = value()?,
            "--midi-port" => {
                args.midi_port = Some(value()?.parse::<usize>().map_err(|e| e.to_string())?)
            }
            "--no-midi" => args.no_midi = true,
            "--buffer" => args.buffer = Some(value()?.parse::<u32>().map_err(|e| e.to_string())?),
            "--list" => args.list = true,
            "--selftest" => args.selftest = true,
            "-h" | "--help" => {
                print!("{}", usage());
                std::process::exit(0);
            }
            other => return Err(format!("unknown option `{other}`")),
        }
    }
    Ok(args)
}

// ---------------------------------------------------------------------------
// Shared state the UI reads back out of the audio thread
// ---------------------------------------------------------------------------

struct Shared {
    active_voices: AtomicU8,
}

// ---------------------------------------------------------------------------
// Audio
// ---------------------------------------------------------------------------

/// Everything the audio callback owns.
///
/// One engine per factory patch, so switching patches costs an index change
/// rather than an allocation on the audio thread. `--selftest` drives this same
/// type without an audio device, which is what makes the realtime path testable
/// on a machine with no sound card.
struct Mixer {
    engines: Vec<Engine>,
    active: usize,
    from_keys: Consumer<Command>,
    from_midi: Consumer<Command>,
}

impl Mixer {
    /// Apply every queued command. Never blocks, allocates or frees.
    fn drain(&mut self) {
        for source in [&mut self.from_keys, &mut self.from_midi] {
            while let Some(cmd) = source.pop() {
                match cmd {
                    Command::Midi { bytes, len } => {
                        vl1::midi::handle(&mut self.engines[self.active], &bytes[..len as usize]);
                    }
                    Command::Preset(i) => {
                        if i < self.engines.len() && i != self.active {
                            // Release the outgoing patch's notes rather than
                            // leaving them hanging on an engine nobody renders.
                            self.engines[self.active].panic();
                            self.active = i;
                        }
                    }
                    Command::Panic => self.engines[self.active].panic(),
                }
            }
        }
    }

    /// Render one block, mapping the stereo pair onto the device's channels.
    fn fill<T: SizedSample + cpal::FromSample<f32>>(&mut self, out: &mut [T], channels: usize) {
        let engine = &mut self.engines[self.active];
        for frame in out.chunks_mut(channels) {
            let [l, r] = engine.tick();
            for (i, slot) in frame.iter_mut().enumerate() {
                // A mono device gets the sum; anything past stereo is fed the
                // stereo pair and then silence.
                let v = match (channels, i) {
                    (1, _) => (l + r) * 0.5,
                    (_, 0) => l,
                    (_, 1) => r,
                    _ => 0.0,
                };
                *slot = T::from_sample(v);
            }
        }
    }

    fn active_voices(&self) -> usize {
        self.engines[self.active].active_voices()
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    mut mixer: Mixer,
    shared: Arc<Shared>,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: SizedSample + cpal::FromSample<f32>,
{
    device.build_output_stream(
        config,
        move |out: &mut [T], _: &cpal::OutputCallbackInfo| {
            // Drain control input first, so this block reflects it.
            mixer.drain();
            mixer.fill(out, channels);
            shared
                .active_voices
                .store(mixer.active_voices() as u8, Ordering::Relaxed);
        },
        move |err| eprintln!("audio stream error: {err}"),
        None,
    )
}

/// Exercise the full control path — queues, command handling, patch switching
/// and rendering — with no audio device involved.
fn selftest() -> Result<(), Box<dyn Error>> {
    let sample_rate = 48_000.0;
    let patches = presets::all();
    let (mut key_tx, key_rx) = queue::channel::<Command>(256);
    let (_midi_tx, midi_rx) = queue::channel::<Command>(256);

    let mut mixer = Mixer {
        engines: patches
            .iter()
            .map(|p| Engine::with_patch(p.clone(), sample_rate))
            .collect(),
        active: 0,
        from_keys: key_rx,
        from_midi: midi_rx,
    };

    let mut out = vec![0.0f32; 1024];
    let mut failures = 0;

    for (index, patch) in patches.iter().enumerate() {
        key_tx.push(Command::Preset(index)).unwrap();

        // Play the z row, as the computer keyboard would.
        let mut notes = Vec::new();
        for key in "zxcvbnm".chars() {
            let note = keymap::note(key, 4).unwrap();
            notes.push(note);
            key_tx
                .push(Command::midi(&[0x90, note, 100]).unwrap())
                .unwrap();
        }

        let (mut peak, mut voices) = (0.0f32, 0usize);
        for _ in 0..40 {
            mixer.drain();
            mixer.fill(&mut out, 2);
            voices = voices.max(mixer.active_voices());
            peak = peak.max(out.iter().fold(0.0f32, |m, s| m.max(s.abs())));
        }

        for note in notes {
            key_tx
                .push(Command::midi(&[0x80, note, 0]).unwrap())
                .unwrap();
        }

        let ok = peak > 0.001 && peak <= 1.0 && voices == 7 && out.iter().all(|s| s.is_finite());
        if !ok {
            failures += 1;
        }
        println!(
            "  {:<13} {} peak {peak:.3}  {voices} voices",
            patch.name,
            if ok { "ok  " } else { "FAIL" },
        );
    }

    if failures > 0 {
        return Err(format!("{failures} patch(es) failed the self test").into());
    }
    println!(
        "
Control path OK: queues, patch switching and rendering all work."
    );
    println!("This does not test the audio device or MIDI input, only what runs behind them.");
    Ok(())
}

// ---------------------------------------------------------------------------
// Listing
// ---------------------------------------------------------------------------

fn list_everything() {
    println!("Audio output devices:");
    match cpal::default_host().output_devices() {
        Ok(devices) => {
            let mut any = false;
            let default = cpal::default_host()
                .default_output_device()
                .and_then(|d| d.name().ok());
            for device in devices {
                let name = device.name().unwrap_or_else(|_| "<unnamed>".into());
                let marker = if Some(&name) == default.as_ref() {
                    " (default)"
                } else {
                    ""
                };
                println!("  {name}{marker}");
                any = true;
            }
            if !any {
                println!("  (none found)");
            }
        }
        Err(e) => println!("  (unavailable: {e})"),
    }

    println!("\nMIDI inputs:");
    match midir::MidiInput::new("vl1") {
        Ok(midi) => {
            let ports = midi.ports();
            if ports.is_empty() {
                println!("  (none found)");
            }
            for (i, port) in ports.iter().enumerate() {
                let name = midi.port_name(port).unwrap_or_else(|_| "<unnamed>".into());
                println!("  {i}: {name}");
            }
        }
        Err(e) => println!("  (unavailable: {e})"),
    }

    println!("\nFactory patches:");
    for (i, name) in presets::names().iter().enumerate() {
        println!("  {i}: {name}");
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    if let Err(e) = run() {
        // Make sure a failure never leaves the terminal in raw mode.
        let _ = disable_raw_mode();
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args = parse_args()?;

    if args.list {
        list_everything();
        return Ok(());
    }
    if args.selftest {
        return selftest();
    }

    let patches = presets::all();
    let start = presets::names()
        .iter()
        .position(|n| n.eq_ignore_ascii_case(&args.preset))
        .ok_or_else(|| format!("no factory patch named `{}` (try --list)", args.preset))?;

    // --- Audio device ------------------------------------------------------
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or(
        "no audio output device available. \
         Run `vl1-play --list` to see what this machine reports.",
    )?;
    // A "default device" can exist and still be unusable — a container with no
    // sound card reports one. Say so in terms the player can act on.
    let supported = device.default_output_config().map_err(|e| {
        format!(
            "the default audio device (`{}`) could not be opened ({e}).\n\
             Run `vl1-play --list` to see what this machine reports; on a \
             headless box there may be no sound card at all.",
            device.name().unwrap_or_else(|_| "<unnamed>".into())
        )
    })?;
    let sample_rate = supported.sample_rate().0 as f32;
    let channels = supported.channels() as usize;

    let mut config: cpal::StreamConfig = supported.clone().into();
    if let Some(frames) = args.buffer {
        config.buffer_size = cpal::BufferSize::Fixed(frames);
    }

    let (key_tx, key_rx) = queue::channel::<Command>(256);
    let (midi_tx, midi_rx) = queue::channel::<Command>(256);

    let mixer = Mixer {
        engines: patches
            .iter()
            .map(|p| Engine::with_patch(p.clone(), sample_rate))
            .collect(),
        active: start,
        from_keys: key_rx,
        from_midi: midi_rx,
    };

    let shared = Arc::new(Shared {
        active_voices: AtomicU8::new(0),
    });

    let stream = match supported.sample_format() {
        SampleFormat::F32 => build_stream::<f32>(&device, &config, channels, mixer, shared.clone()),
        SampleFormat::I16 => build_stream::<i16>(&device, &config, channels, mixer, shared.clone()),
        SampleFormat::U16 => build_stream::<u16>(&device, &config, channels, mixer, shared.clone()),
        other => return Err(format!("unsupported sample format {other:?}").into()),
    }
    .map_err(|e| {
        format!(
            "could not open an audio stream on `{}` ({e}).\n\
             Run `vl1-play --list` to see what this machine reports.",
            device.name().unwrap_or_else(|_| "<unnamed>".into())
        )
    })?;
    stream
        .play()
        .map_err(|e| format!("audio device accepted the stream but would not start it ({e})"))?;

    // --- MIDI inputs -------------------------------------------------------
    // Connections must outlive the session, so they are held until the end.
    let mut _midi_connections = Vec::new();
    let mut midi_names = Vec::new();
    if !args.no_midi {
        match connect_midi(args.midi_port, midi_tx) {
            Ok((connections, names)) => {
                _midi_connections = connections;
                midi_names = names;
            }
            Err(e) => eprintln!("MIDI unavailable ({e}); computer keyboard only"),
        }
    }

    // --- Terminal ----------------------------------------------------------
    println!(
        "vl1 — {} | {:.0} Hz, {} ch | {} voices",
        patches[start].name,
        sample_rate,
        channels,
        vl1::POLYPHONY
    );
    if midi_names.is_empty() {
        println!("MIDI: none connected");
    } else {
        for name in &midi_names {
            println!("MIDI: {name}");
        }
    }
    println!("\n{}\n", keymap::layout_help());
    println!(
        "  [ ]  patch      - =  octave      up/down  breath      left/right  embouchure\n\
       \x20 space  sustain    1  scream      4  growl           8  panic\n\
       \x20 esc / ctrl-c  quit\n"
    );

    keyboard_loop(key_tx, shared, &patches, start)?;
    Ok(())
}

type MidiConnections = Vec<midir::MidiInputConnection<()>>;

fn connect_midi(
    only: Option<usize>,
    tx: Producer<Command>,
) -> Result<(MidiConnections, Vec<String>), Box<dyn Error>> {
    let input = midir::MidiInput::new("vl1")?;
    let ports = input.ports();
    if ports.is_empty() {
        return Err("no MIDI input ports".into());
    }

    // The queue is single-producer but `midir` gives every port its own
    // callback thread, so they share one producer behind a mutex. Only MIDI
    // threads ever take that lock — the audio callback reads the other end of
    // the queue and never touches it — so it cannot stall the audio thread.
    let tx = Arc::new(Mutex::new(tx));
    let mut connections = Vec::new();
    let mut names = Vec::new();

    for (i, port) in ports.iter().enumerate() {
        if only.is_some_and(|want| want != i) {
            continue;
        }
        let input = midir::MidiInput::new("vl1")?;
        let name = input.port_name(port).unwrap_or_else(|_| "<unnamed>".into());
        let tx = tx.clone();

        let connection = input.connect(
            port,
            "vl1-in",
            move |_timestamp, message, _| {
                if let Some(cmd) = Command::midi(message) {
                    if let Ok(mut tx) = tx.lock() {
                        let _ = tx.push(cmd);
                    }
                }
            },
            (),
        )?;
        connections.push(connection);
        names.push(format!("{i}: {name}"));
    }

    if connections.is_empty() {
        return Err("requested MIDI port does not exist".into());
    }
    Ok((connections, names))
}

/// How long a held key is assumed to still be held, when the terminal cannot
/// report key releases. Auto-repeat refreshes it well inside this window.
const KEY_HOLD: Duration = Duration::from_millis(220);

fn keyboard_loop(
    mut tx: Producer<Command>,
    shared: Arc<Shared>,
    patches: &[vl1::Patch],
    start: usize,
) -> Result<(), Box<dyn Error>> {
    enable_raw_mode()?;

    // Where supported (kitty, foot, WezTerm, recent xterm) this gets us real
    // key-release events, which means true polyphonic playing and proper
    // legato. Everywhere else we fall back to releasing on a timeout, which
    // plays fine but cannot sustain a chord indefinitely.
    let precise = crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false);
    if precise {
        let _ = execute!(
            stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::REPORT_EVENT_TYPES)
        );
    } else {
        // Worth saying out loud: a terminal without release reporting only
        // auto-repeats the most recent key, so a held chord decays to its last
        // note. Single lines play fine.
        println!(
            "  note: this terminal does not report key releases, so computer-keyboard\r\n  \
             chords will not hold. Melodies are fine; use MIDI for chords.\r\n"
        );
    }

    let result = keyboard_loop_inner(&mut tx, &shared, patches, start, precise);

    if precise {
        let _ = execute!(stdout(), PopKeyboardEnhancementFlags);
    }
    disable_raw_mode()?;
    let _ = execute!(stdout(), Print("\r\n"));
    result
}

fn keyboard_loop_inner(
    tx: &mut Producer<Command>,
    shared: &Arc<Shared>,
    patches: &[vl1::Patch],
    start: usize,
    precise: bool,
) -> Result<(), Box<dyn Error>> {
    let mut octave = 4i32;
    let mut preset = start;
    let mut breath = 100u8;
    let mut embouchure = 64u8;
    let mut scream = 0u8;
    let mut growl = 0u8;
    let mut sustain = false;

    // Notes started from the computer keyboard: (key, midi note, last seen).
    let mut held: Vec<(char, u8, Instant)> = Vec::new();
    let mut last_status = Instant::now() - Duration::from_secs(1);

    let mut send = |tx: &mut Producer<Command>, msg: &[u8]| {
        if let Some(cmd) = Command::midi(msg) {
            let _ = tx.push(cmd);
        }
    };

    loop {
        if event::poll(Duration::from_millis(15))? {
            match event::read()? {
                TermEvent::Key(key) => {
                    if matches!(key.kind, KeyEventKind::Release) {
                        if let KeyCode::Char(c) = key.code {
                            if let Some(pos) = held.iter().position(|(k, _, _)| *k == c) {
                                let (_, note, _) = held.remove(pos);
                                send(tx, &[0x80, note, 0]);
                            }
                        }
                        continue;
                    }

                    // Quit
                    if key.code == KeyCode::Esc
                        || (key.code == KeyCode::Char('c')
                            && key.modifiers.contains(KeyModifiers::CONTROL))
                    {
                        for (_, note, _) in held.drain(..) {
                            send(tx, &[0x80, note, 0]);
                        }
                        let _ = tx.push(Command::Panic);
                        return Ok(());
                    }

                    handle_key(
                        key,
                        tx,
                        &mut send,
                        &mut held,
                        &mut octave,
                        &mut preset,
                        &mut breath,
                        &mut embouchure,
                        &mut scream,
                        &mut growl,
                        &mut sustain,
                        patches.len(),
                    );
                }
                TermEvent::Resize(_, _) => {}
                _ => {}
            }
        }

        // Without key-release reporting, a key that has stopped repeating has
        // been let go.
        if !precise {
            let now = Instant::now();
            held.retain(|(_, note, seen)| {
                if now.duration_since(*seen) > KEY_HOLD {
                    if let Some(cmd) = Command::midi(&[0x80, *note, 0]) {
                        let _ = tx.push(cmd);
                    }
                    false
                } else {
                    true
                }
            });
        }

        if last_status.elapsed() > Duration::from_millis(80) {
            last_status = Instant::now();
            let voices = shared.active_voices.load(Ordering::Relaxed);
            print!(
                "\r  {:<13} oct {:<2} breath {:>3} emb {:>3} scream {:>3} growl {:>3} {} voices {}   ",
                patches[preset].name,
                octave,
                breath,
                embouchure,
                scream,
                growl,
                if sustain { "sus" } else { "   " },
                voices
            );
            let _ = stdout().flush();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_key(
    key: KeyEvent,
    tx: &mut Producer<Command>,
    send: &mut impl FnMut(&mut Producer<Command>, &[u8]),
    held: &mut Vec<(char, u8, Instant)>,
    octave: &mut i32,
    preset: &mut usize,
    breath: &mut u8,
    embouchure: &mut u8,
    scream: &mut u8,
    growl: &mut u8,
    sustain: &mut bool,
    preset_count: usize,
) {
    use vl1::midi::cc;

    let bump = |v: &mut u8, delta: i32| {
        *v = (*v as i32 + delta).clamp(0, 127) as u8;
        *v
    };

    match key.code {
        KeyCode::Char(c) if keymap::is_note_key(c) => {
            if let Some(note) = keymap::note(c, *octave) {
                if let Some(entry) = held.iter_mut().find(|(k, _, _)| *k == c) {
                    // Auto-repeat of a key that is still down: refresh, do not
                    // retrigger, or holding a note would machine-gun it.
                    entry.2 = Instant::now();
                } else {
                    held.push((c, note, Instant::now()));
                    send(tx, &[0x90, note, 100]);
                }
            }
        }
        KeyCode::Char('[') => {
            *preset = (*preset + preset_count - 1) % preset_count;
            let _ = tx.push(Command::Preset(*preset));
            held.clear();
        }
        KeyCode::Char(']') => {
            *preset = (*preset + 1) % preset_count;
            let _ = tx.push(Command::Preset(*preset));
            held.clear();
        }
        KeyCode::Char('-') => *octave = (*octave - 1).max(keymap::MIN_OCTAVE),
        KeyCode::Char('=') | KeyCode::Char('+') => *octave = (*octave + 1).min(keymap::MAX_OCTAVE),
        KeyCode::Up => {
            let v = bump(breath, 6);
            send(tx, &[0xB0, cc::BREATH, v]);
        }
        KeyCode::Down => {
            let v = bump(breath, -6);
            send(tx, &[0xB0, cc::BREATH, v]);
        }
        KeyCode::Right => {
            let v = bump(embouchure, 4);
            send(tx, &[0xB0, cc::EMBOUCHURE, v]);
        }
        KeyCode::Left => {
            let v = bump(embouchure, -4);
            send(tx, &[0xB0, cc::EMBOUCHURE, v]);
        }
        KeyCode::Char('1') => {
            let v = if *scream > 0 { 0 } else { 90 };
            *scream = v;
            send(tx, &[0xB0, cc::SCREAM, v]);
        }
        KeyCode::Char('4') => {
            let v = if *growl > 0 { 0 } else { 90 };
            *growl = v;
            send(tx, &[0xB0, cc::GROWL, v]);
        }
        KeyCode::Char('8') => {
            held.clear();
            let _ = tx.push(Command::Panic);
        }
        KeyCode::Char(' ') => {
            *sustain = !*sustain;
            send(tx, &[0xB0, cc::SUSTAIN, if *sustain { 127 } else { 0 }]);
        }
        _ => {}
    }
}
