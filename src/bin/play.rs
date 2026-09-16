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
use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};
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
    sample_rate: Option<u32>,
    gain_db: f32,
}

fn usage() -> &'static str {
    "vl1-play — play the 8-voice VL1-style engine live

USAGE:
    vl1-play [OPTIONS]

OPTIONS:
    --preset <NAME>   Factory patch to start on (default: Tenor Sax)
    --midi-port <N>   Connect only to this MIDI input (default: all of them)
    --no-midi         Computer keyboard only
    --buffer <FRAMES> Audio buffer size; raise it if the audio breaks up
    --sr <HZ>         Ask the device for this sample rate
    --gain <DB>       Output trim, e.g. -6 to stay clear of the limiter
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
        sample_rate: None,
        gain_db: 0.0,
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
            "--sr" => args.sample_rate = Some(value()?.parse::<u32>().map_err(|e| e.to_string())?),
            "--gain" => args.gain_db = value()?.parse::<f32>().map_err(|e| e.to_string())?,
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
    /// Callback time as a fraction of the block's wall-clock budget, in
    /// thousandths. Over 1000 means the callback took longer than the audio it
    /// produced, which is a dropout.
    load: AtomicU32,
    peak_load: AtomicU32,
    /// Blocks that missed their deadline.
    xruns: AtomicU32,
    /// Peak level before the limiter, in thousandths. Over 1000 is saturation.
    peak: AtomicU32,
    /// Frames per callback, as the device actually delivers them.
    block: AtomicU32,
}

impl Shared {
    fn new() -> Self {
        Self {
            active_voices: AtomicU8::new(0),
            load: AtomicU32::new(0),
            peak_load: AtomicU32::new(0),
            xruns: AtomicU32::new(0),
            peak: AtomicU32::new(0),
            block: AtomicU32::new(0),
        }
    }
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
    /// Output trim, so a player can back off the limiter without editing patches.
    gain: f32,
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
        let gain = self.gain;
        let engine = &mut self.engines[self.active];
        for frame in out.chunks_mut(channels) {
            let [l, r] = engine.tick();
            let (l, r) = (l * gain, r * gain);
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

    fn take_peak(&mut self) -> f32 {
        self.engines[self.active].take_peak()
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
    let sample_rate = config.sample_rate.0 as f64;

    device.build_output_stream(
        config,
        move |out: &mut [T], _: &cpal::OutputCallbackInfo| {
            let started = Instant::now();

            // Drain control input first, so this block reflects it.
            mixer.drain();
            mixer.fill(out, channels);

            // How much of this block's real-time budget the work consumed.
            // Anything near 100% will crackle, and the player deserves to be
            // told that rather than left guessing at their audio settings.
            let frames = (out.len() / channels.max(1)) as f64;
            shared.block.store(frames as u32, Ordering::Relaxed);
            let budget = frames / sample_rate;
            let load = if budget > 0.0 {
                (started.elapsed().as_secs_f64() / budget * 1000.0) as u32
            } else {
                0
            };
            shared.load.store(load, Ordering::Relaxed);
            shared.peak_load.fetch_max(load, Ordering::Relaxed);
            if load >= 1000 {
                shared.xruns.fetch_add(1, Ordering::Relaxed);
            }
            shared
                .peak
                .store((mixer.take_peak() * 1000.0) as u32, Ordering::Relaxed);
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
        gain: 1.0,
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
    let mut supported = device.default_output_config().map_err(|e| {
        format!(
            "the default audio device (`{}`) could not be opened ({e}).\n\
             Run `vl1-play --list` to see what this machine reports; on a \
             headless box there may be no sound card at all.",
            device.name().unwrap_or_else(|_| "<unnamed>".into())
        )
    })?;
    // A device's default rate can be far higher than anything this needs; at
    // 192 kHz the engine does four times the work per second of audio for no
    // musical gain, so allow asking for something saner.
    if let Some(want) = args.sample_rate {
        let rate = cpal::SampleRate(want);
        let found = device
            .supported_output_configs()
            .map(|configs| {
                configs
                    .filter(|c| c.min_sample_rate() <= rate && rate <= c.max_sample_rate())
                    .map(|c| c.with_sample_rate(rate))
                    .next()
            })
            .unwrap_or(None);
        match found {
            Some(config) => supported = config,
            None => eprintln!(
                "note: this device does not offer {want} Hz; using {} Hz",
                supported.sample_rate().0
            ),
        }
    }

    let sample_rate = supported.sample_rate().0 as f32;
    let channels = supported.channels() as usize;

    // Left to itself, a host can hand out a buffer of several thousand frames —
    // PulseAudio and PipeWire routinely do — which is a tenth of a second of
    // latency before a single sample of synthesis happens. Ask for something
    // playable and fall back if the device refuses.
    const DEFAULT_BUFFER: u32 = 256;
    let mut config: cpal::StreamConfig = supported.clone().into();
    let wanted = args.buffer.unwrap_or(DEFAULT_BUFFER);
    config.buffer_size = match supported.buffer_size() {
        cpal::SupportedBufferSize::Range { min, max } => {
            cpal::BufferSize::Fixed(wanted.clamp(*min, *max))
        }
        cpal::SupportedBufferSize::Unknown => cpal::BufferSize::Fixed(wanted),
    };

    let shared = Arc::new(Shared::new());
    let gain = vl1::dsp::db_to_gain(args.gain_db);

    // Building the stream consumes the queue's reading ends, so a failed
    // attempt cannot be retried with the same ones. Each attempt gets a fresh
    // pair; nothing has sent to them yet, since MIDI and the keyboard start
    // further down.
    let open = |config: &cpal::StreamConfig| -> Result<
        (cpal::Stream, Producer<Command>, Producer<Command>),
        cpal::BuildStreamError,
    > {
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
            gain,
        };
        let stream = match supported.sample_format() {
            SampleFormat::F32 => {
                build_stream::<f32>(&device, config, channels, mixer, shared.clone())
            }
            SampleFormat::I16 => {
                build_stream::<i16>(&device, config, channels, mixer, shared.clone())
            }
            SampleFormat::U16 => {
                build_stream::<u16>(&device, config, channels, mixer, shared.clone())
            }
            _ => Err(cpal::BuildStreamError::StreamConfigNotSupported),
        }?;
        Ok((stream, key_tx, midi_tx))
    };

    if !matches!(
        supported.sample_format(),
        SampleFormat::F32 | SampleFormat::I16 | SampleFormat::U16
    ) {
        return Err(format!("unsupported sample format {:?}", supported.sample_format()).into());
    }

    let (stream, key_tx, midi_tx) = match open(&config) {
        Ok(opened) => opened,
        Err(first) => {
            // Some devices refuse a fixed buffer outright. Playing with whatever
            // the host wants to give beats not playing at all.
            eprintln!(
                "note: this device would not take a {wanted}-frame buffer ({first}); \
                 using its own, which may add latency"
            );
            config.buffer_size = cpal::BufferSize::Default;
            open(&config).map_err(|e| {
                format!(
                    "could not open an audio stream on `{}` ({e}).\n\
                     Run `vl1-play --list` to see what this machine reports.",
                    device.name().unwrap_or_else(|_| "<unnamed>".into())
                )
            })?
        }
    };

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
    if cfg!(debug_assertions) {
        eprintln!(
            "WARNING: this is a debug build. The synthesis is roughly twenty times\n\
             slower than a release build and the audio will break up badly.\n\
             Rebuild with:  cargo run --release --features live --bin vl1-play\n"
        );
    }

    let buffered_ms = match config.buffer_size {
        cpal::BufferSize::Fixed(frames) => frames as f32 / sample_rate * 1000.0,
        cpal::BufferSize::Default => f32::NAN,
    };
    println!(
        "vl1 — {} | {:.0} Hz, {} ch | {} voices",
        patches[start].name,
        sample_rate,
        channels,
        vl1::POLYPHONY
    );
    if buffered_ms.is_finite() {
        println!("audio buffer: {:.1} ms (--buffer to change)", buffered_ms);
    }
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
    let mut skipped = Vec::new();

    for (i, port) in ports.iter().enumerate() {
        if only.is_some_and(|want| want != i) {
            continue;
        }
        let input = midir::MidiInput::new("vl1")?;
        let name = input.port_name(port).unwrap_or_else(|_| "<unnamed>".into());

        // Skip echo ports unless one was asked for by number. A "Through" port
        // re-emits whatever arrives on it, so connecting to both it and the
        // real port delivers every note twice — which retriggers each note a
        // few milliseconds after it starts and sounds like a stutter.
        if only.is_none() && name.to_lowercase().contains("through") {
            skipped.push(format!("{i}: {name} (echo port)"));
            continue;
        }

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
        return Err(if skipped.is_empty() {
            "requested MIDI port does not exist".into()
        } else {
            "the only MIDI ports available are echo ports; \
             pass --midi-port N to use one anyway"
                .to_string()
        }
        .into());
    }
    for note in skipped {
        names.push(format!("{note} — skipped"));
    }
    Ok((connections, names))
}

/// A key the computer keyboard is holding down.
struct HeldKey {
    key: char,
    note: u8,
    /// When this key was last seen, either pressed or auto-repeated.
    seen: Instant,
    /// Auto-repeats observed so far.
    repeats: u32,
}

impl HeldKey {
    /// How long to keep sounding after the last event for this key, when the
    /// terminal cannot report releases.
    ///
    /// Auto-repeat has two phases and they are an order of magnitude apart: the
    /// first repeat comes after the system's initial delay, typically 250-600 ms,
    /// and subsequent ones every 30-50 ms. One fixed window cannot serve both —
    /// short enough to release promptly and it cuts every note off before its
    /// first repeat even arrives, which is heard as the note stuttering.
    fn hold(&self) -> Duration {
        if self.repeats == 0 {
            // Still waiting for the first repeat: outlast any sane initial delay.
            Duration::from_millis(900)
        } else {
            // Repeating steadily now, so silence means the key really is up.
            Duration::from_millis(180)
        }
    }

    /// Whether this key should be released, given the time now.
    fn expired(&self, now: Instant) -> bool {
        now.duration_since(self.seen) > self.hold()
    }
}

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

    // Notes started from the computer keyboard.
    let mut held: Vec<HeldKey> = Vec::new();
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
                            if let Some(pos) = held.iter().position(|h| h.key == c) {
                                let note = held.remove(pos).note;
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
                        for h in held.drain(..) {
                            send(tx, &[0x80, h.note, 0]);
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
            held.retain(|h| {
                if h.expired(now) {
                    if let Some(cmd) = Command::midi(&[0x80, h.note, 0]) {
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
            let load = shared.load.load(Ordering::Relaxed);
            let peak_load = shared.peak_load.load(Ordering::Relaxed);
            let xruns = shared.xruns.load(Ordering::Relaxed);
            let peak = shared.peak.load(Ordering::Relaxed);
            let block = shared.block.load(Ordering::Relaxed);

            // `lim` means the output limiter is saturating: distortion, not a
            // dropout. `xrun` means the callback missed its deadline: a
            // dropout, not distortion. Telling those apart by ear is hard, and
            // the fixes are opposite.
            let flags = match (peak > 1000, xruns > 0) {
                (true, true) => "lim xrun",
                (true, false) => "lim     ",
                (false, true) => "    xrun",
                (false, false) => "        ",
            };
            print!(
                "\r  {:<12} oct {:<2} br {:>3} emb {:>3} scr {:>3} grw {:>3} {} | {} voices  \
                 cpu {:>3}% (max {:>3}%)  buf {:<5} peak {:>4}  {}   ",
                patches[preset].name,
                octave,
                breath,
                embouchure,
                scream,
                growl,
                if sustain { "sus" } else { "   " },
                voices,
                load / 10,
                peak_load / 10,
                block,
                peak,
                flags,
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
    held: &mut Vec<HeldKey>,
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
                if let Some(entry) = held.iter_mut().find(|h| h.key == c) {
                    // Auto-repeat of a key that is still down: refresh, do not
                    // retrigger, or holding a note would machine-gun it.
                    entry.seen = Instant::now();
                    entry.repeats += 1;
                } else {
                    held.push(HeldKey {
                        key: c,
                        note,
                        seen: Instant::now(),
                        repeats: 0,
                    });
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

#[cfg(test)]
mod tests {
    use super::*;

    fn held(repeats: u32, age_ms: u64) -> HeldKey {
        HeldKey {
            key: 'z',
            note: 60,
            seen: Instant::now() - Duration::from_millis(age_ms),
            repeats,
        }
    }

    #[test]
    fn a_key_survives_the_initial_auto_repeat_delay() {
        // The bug this guards against: a hold window shorter than the system's
        // initial key-repeat delay releases every note before its first repeat
        // arrives, so a held note stutters instead of sustaining.
        let now = Instant::now();
        assert!(!held(0, 300).expired(now));
        assert!(!held(0, 600).expired(now));
        assert!(!held(0, 850).expired(now));
    }

    #[test]
    fn a_key_released_before_any_repeat_still_stops() {
        assert!(held(0, 1000).expired(Instant::now()));
    }

    #[test]
    fn a_repeating_key_releases_promptly() {
        // Once repeats are arriving every 30-50 ms, a long gap means key-up.
        let now = Instant::now();
        assert!(!held(5, 60).expired(now));
        assert!(!held(5, 150).expired(now));
        assert!(held(5, 250).expired(now));
    }

    #[test]
    fn midi_commands_carry_their_length() {
        match Command::midi(&[0x90, 60, 100]).unwrap() {
            Command::Midi { bytes, len } => {
                assert_eq!(len, 3);
                assert_eq!(&bytes[..3], &[0x90, 60, 100]);
            }
            other => panic!("expected a MIDI command, got {other:?}"),
        }
        // Two-byte messages (program change, channel pressure) round-trip too.
        match Command::midi(&[0xD0, 64]).unwrap() {
            Command::Midi { bytes, len } => {
                assert_eq!(len, 2);
                assert_eq!(&bytes[..2], &[0xD0, 64]);
            }
            other => panic!("expected a MIDI command, got {other:?}"),
        }
        assert!(Command::midi(&[]).is_none());
        assert!(Command::midi(&[0xF0, 1, 2, 3]).is_none());
    }
}
