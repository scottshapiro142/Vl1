//! An element: one complete virtual instrument.
//!
//! Driver → Pipe/String → Modifiers → Amplitude. A voice plays one or two of
//! these; the engine plays eight voices.

use crate::control::Controls;
use crate::driver::{self, Driver, DriverInput};
use crate::dsp::{note_to_hz, sanitize, Adsr, Biquad, DcBlock, Lfo, Noise, Smoother};
use crate::modifier::{ModifierChain, ModifierContext};
use crate::patch::ElementPatch;
use crate::waveguide::Waveguide;

/// Control-rate subdivision. Retuning the waveguide and its termination filter
/// every sample is wasted work; every 8 samples is well above any rate a player
/// or an LFO can move at, and saves a lot of transcendental math.
const CONTROL_INTERVAL: u32 = 8;

pub struct Element {
    patch: ElementPatch,
    sample_rate: f32,

    driver: Box<dyn Driver>,
    guide: Waveguide,
    /// Radiation from the bell or sound hole. An instrument does not radiate
    /// the standing wave itself, and it certainly does not radiate the steady
    /// mouth pressure riding underneath it — an opening cannot couple DC to
    /// the air at all. Without this the element's output sits on a large
    /// offset that swamps the tone.
    radiation: DcBlock,
    mods: ModifierChain,

    breath_env: Adsr,
    amp_env: Adsr,
    vibrato: Lfo,
    growl: Lfo,
    throat: Biquad,
    noise: Noise,

    pressure: Smoother,
    embouchure: Smoother,

    tongue: f32,
    tongue_coef: f32,

    /// Articulation transient: position within the pulse, in samples, and its
    /// length. Negative position means no pulse is in flight.
    ///
    /// The pulse is a single half-cycle of a sine at the played pitch, which is
    /// what the resonator is being asked to sustain anyway. Filtered noise was
    /// the obvious thing to inject here and it is the wrong thing: broadband
    /// energy excites every mode of the tube at once, and a cylindrical bore
    /// answers by jumping to its third mode — a clarinet overblowing to the
    /// twelfth. Faithful, but not the note anyone asked for, and which way it
    /// went depended on the noise. A single smooth pulse has almost no energy
    /// up there, so the register is decided by the patch rather than by luck.
    strike_pos: f32,
    strike_len: f32,

    /// Base pitch in Hz before vibrato and bend, set by the voice.
    freq: f32,
    tuned_freq: f32,
    counter: u32,
    drive: f32,
}

impl Element {
    pub fn new(patch: &ElementPatch, sample_rate: f32, seed: u32) -> Self {
        let mut el = Self {
            patch: *patch,
            sample_rate,
            driver: driver::make(patch.driver.kind, sample_rate),
            guide: Waveguide::new(sample_rate),
            radiation: DcBlock::new(),
            mods: ModifierChain::new(sample_rate),
            breath_env: Adsr::new(sample_rate),
            amp_env: Adsr::new(sample_rate),
            vibrato: Lfo::new(sample_rate),
            growl: Lfo::new(sample_rate),
            throat: Biquad::new(),
            noise: Noise::new(seed),
            pressure: Smoother::new(0.002, sample_rate),
            embouchure: Smoother::new(0.01, sample_rate),
            tongue: 0.0,
            tongue_coef: 0.99,
            strike_pos: -1.0,
            strike_len: 1.0,
            freq: 261.63,
            tuned_freq: 0.0,
            counter: 0,
            drive: 0.0,
        };
        el.radiation.set_cutoff(20.0, sample_rate);
        el.apply_patch(patch);
        el
    }

    /// Reconfigure from a patch. Allocates only if the driver kind changed.
    pub fn apply_patch(&mut self, patch: &ElementPatch) {
        if patch.driver.kind != self.patch.driver.kind {
            self.driver = driver::make(patch.driver.kind, self.sample_rate);
        }
        self.patch = *patch;
        self.driver.configure(&patch.driver);

        let d = &patch.driver;
        let b = &patch.breath;
        let p = &patch.pipe;

        self.guide.set_mode(p.mode);
        self.guide.set_tuning_ratio(p.tuning_ratio);
        self.guide.set_absorption(p.absorption);
        self.guide.set_damping(p.damping_hz);

        self.breath_env.set(b.attack, b.decay, b.sustain, b.release);
        self.amp_env.set(0.002, 0.001, 1.0, patch.amp.release);

        self.vibrato.set_rate(b.vibrato_rate);
        self.vibrato.set_delay(b.vibrato_delay, 0.35);
        self.growl.set_rate(d.growl_rate);
        self.growl.set_delay(0.0, 0.01);

        self.throat
            .set_bandpass(d.throat_freq, d.throat_q, self.sample_rate);

        self.tongue_coef = (-1.0 / (d.tongue_time.max(1e-3) * self.sample_rate)).exp();

        let m = &patch.modifiers;
        self.mods.impulse.set(m.impulse.amount, m.impulse.freq);
        for (i, band) in m.resonator.iter().enumerate() {
            self.mods
                .resonator
                .set_band(i, band.freq, band.q, band.mix, self.sample_rate);
        }
        self.mods.enhancer.set(
            m.enhancer.amount,
            m.enhancer.bias,
            m.enhancer.drive,
            m.enhancer.drive_tracking,
        );
        self.mods.filter.set(
            m.filter.kind,
            m.filter.base_hz,
            m.filter.env_depth,
            m.filter.resonance,
            m.filter.key_track,
        );
        self.mods.eq.set(&m.eq, self.sample_rate);
        self.mods.set_output_gain_db(m.output_db);

        self.tuned_freq = 0.0;
    }

    pub fn patch(&self) -> &ElementPatch {
        &self.patch
    }

    pub fn reset(&mut self) {
        self.driver.reset();
        self.guide.reset();
        self.radiation.clear();
        self.mods.reset();
        self.breath_env.reset();
        self.amp_env.reset();
        self.throat.clear();
        self.pressure.set_immediate(0.0);
        self.embouchure.set_immediate(self.patch.driver.embouchure);
        self.tongue = 0.0;
        self.strike_pos = -1.0;
        self.drive = 0.0;
        self.tuned_freq = 0.0;
    }

    /// The element's own pitch, including its transpose and detune.
    pub fn pitch_offset(&self) -> f32 {
        self.patch.transpose as f32 + self.patch.detune_cents / 100.0
    }

    pub fn note_on(&mut self, legato: bool) {
        if legato {
            self.breath_env.gate_on_legato();
            self.amp_env.gate_on_legato();
        } else {
            self.breath_env.gate_on();
            self.amp_env.gate_on();
            self.vibrato.reset(0.0);
            self.growl.reset(0.0);
        }
        self.tongue = self.patch.driver.tonguing.clamp(0.0, 1.0);
        if !legato && self.patch.driver.attack_impulse > 0.0 {
            self.strike_pos = 0.0;
        }
    }

    pub fn note_off(&mut self) {
        self.breath_env.gate_off();
        self.amp_env.gate_off();
    }

    pub fn is_active(&self) -> bool {
        self.breath_env.is_active() || self.amp_env.is_active()
    }

    /// Current output envelope, used by the engine to pick a voice to steal.
    pub fn loudness(&self) -> f32 {
        self.amp_env.value() * self.patch.level
    }

    /// Render one sample at the given pitch (MIDI note, fractional).
    pub fn tick(&mut self, note: f32, velocity: f32, ctl: &Controls) -> f32 {
        let patch = self.patch;
        let b = &patch.breath;
        let d = &patch.driver;

        let env = self.breath_env.tick();
        let amp = self.amp_env.tick();
        let vib = self.vibrato.tick();
        let growl = self.growl.tick();

        // --- Pitch -----------------------------------------------------------
        let vib_depth = b.vibrato_depth * (0.25 + 0.75 * ctl.modulation);
        let cents = vib * vib_depth;
        self.freq = note_to_hz(note + self.pitch_offset() + cents / 100.0);

        // --- Control-rate updates -------------------------------------------
        if self.counter == 0 {
            if (self.freq - self.tuned_freq).abs() > self.tuned_freq * 1e-4 {
                self.guide.set_frequency(self.freq);
                self.driver.set_frequency(self.freq);
                self.driver.set_bore_delay(self.guide.loop_delay());
                self.tuned_freq = self.freq;
            }

            let p = &patch.pipe;
            let key_oct = (self.freq / 261.63).log2() * p.damping_key_track;
            let cutoff = p.damping_hz * (key_oct + p.damping_breath * env + ctl.damping).exp2();
            self.guide.set_damping(cutoff);
            self.guide
                .set_absorption((p.absorption + ctl.absorption * 0.02).clamp(0.0, 0.9995));
        }
        self.counter = (self.counter + 1) % CONTROL_INTERVAL;

        // --- Breath pressure -------------------------------------------------
        let vel_scale = 1.0 - b.velocity_depth + b.velocity_depth * velocity;
        let tremolo = 1.0 + vib * b.vibrato_pressure;
        let growl_mod = 1.0 + growl * d.growl_depth * ctl.growl;
        let target =
            b.level * vel_scale * env * ctl.pressure * ctl.expression * tremolo * growl_mod;
        let pressure = self.pressure.tick(target.max(0.0));
        self.drive = env;

        // --- Turbulence ------------------------------------------------------
        let gate = 1.0 - d.noise_tracking + d.noise_tracking * env;
        let mut noise = self.noise.pink() * d.breath_noise * gate;
        if d.throat_amount > 0.0 {
            // The player's vocal tract colours the air before it ever reaches
            // the reed; this is what "throat formant" is buying you.
            let formant = self.throat.tick(noise + self.guide.reflected() * 0.1);
            noise = noise * (1.0 - d.throat_amount) + formant * d.throat_amount * 3.0;
        }

        // --- Excitation ------------------------------------------------------
        let key_track = if d.embouchure_key_track != 0.0 {
            (self.freq / 261.63).log2() * d.embouchure_key_track
        } else {
            0.0
        };
        let emb_target =
            (d.embouchure + d.embouchure_env * env + key_track + ctl.embouchure).clamp(0.0, 1.0);
        let embouchure = self.embouchure.tick(emb_target);

        self.tongue *= self.tongue_coef;
        let input = DriverInput {
            breath: pressure,
            bore: self.guide.reflected(),
            embouchure,
            tonguing: self.tongue,
            scream: (d.scream + ctl.scream).clamp(0.0, 1.0),
            noise,
        };
        let injected = self.driver.tick(&input);

        // --- Resonator -------------------------------------------------------
        // The articulation transient goes straight into the tube, where it
        // excites every mode at once and the loop immediately has something to
        // work with instead of amplifying its own noise floor for half a second.
        let excite = if self.strike_pos >= 0.0 {
            if self.strike_pos == 0.0 {
                // Half a period of the note being played.
                self.strike_len = (0.5 * self.sample_rate / self.freq.max(1.0)).max(2.0);
            }
            let phase = self.strike_pos / self.strike_len;
            self.strike_pos += 1.0;
            if self.strike_pos > self.strike_len {
                self.strike_pos = -1.0;
            }
            (phase * std::f32::consts::PI).sin() * d.attack_impulse * b.level * velocity
        } else {
            0.0
        };

        let mut sig = self.guide.tick(injected + excite);
        if patch.pipe.tap_mix.abs() > 1e-4 {
            sig += self.guide.tap(patch.pipe.tap_position) * patch.pipe.tap_mix;
        }
        sig = self.radiation.tick(sig);

        // --- Modifiers and amplitude ----------------------------------------
        let ctx = ModifierContext {
            drive: self.drive,
            freq: self.freq,
        };
        let out = self.mods.tick(sig, &ctx);
        let vel_amp = 1.0 - patch.amp.velocity_sense + patch.amp.velocity_sense * velocity;

        sanitize(out * amp * vel_amp * patch.level)
    }
}
