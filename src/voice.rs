//! A single voice: one or two elements, plus pitch handling.

use crate::control::Controls;
use crate::dsp::Smoother;
use crate::element::Element;
use crate::patch::{Patch, PerformancePatch, PortamentoMode};

/// Per-voice state. The engine owns a fixed pool of these.
pub struct Voice {
    elements: Vec<Element>,
    pan: Vec<f32>,
    /// MIDI note currently assigned, if any.
    note: u8,
    velocity: f32,
    /// True between note-on and note-off (ignoring sustain pedal).
    held: bool,
    /// True while the sustain pedal is holding a released note.
    sustained: bool,
    /// Monotonic counter, for oldest-first stealing.
    age: u64,
    active: bool,
    glide: Smoother,
    target_note: f32,
    sample_rate: f32,
}

impl Voice {
    pub fn new(patch: &Patch, sample_rate: f32, seed: u32) -> Self {
        let mut voice = Self {
            elements: Vec::new(),
            pan: Vec::new(),
            note: 60,
            velocity: 0.0,
            held: false,
            sustained: false,
            age: 0,
            active: false,
            glide: Smoother::new(0.05, sample_rate),
            target_note: 60.0,
            sample_rate,
        };
        voice.apply_patch(patch, seed);
        voice
    }

    /// Rebuild this voice's elements from a patch.
    pub fn apply_patch(&mut self, patch: &Patch, seed: u32) {
        let specs: Vec<_> = patch.elements().copied().collect();

        // Reuse existing elements where we can so that changing a parameter
        // mid-note does not silence what is currently sounding.
        self.elements.truncate(specs.len());
        self.pan.clear();
        for (i, spec) in specs.iter().enumerate() {
            if let Some(el) = self.elements.get_mut(i) {
                el.apply_patch(spec);
            } else {
                self.elements.push(Element::new(
                    spec,
                    self.sample_rate,
                    seed.wrapping_mul(2_654_435_761).wrapping_add(i as u32 + 1),
                ));
            }
            self.pan.push(spec.pan.clamp(-1.0, 1.0));
        }

        self.glide
            .set_time(patch.performance.portamento_time, self.sample_rate);
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn is_held(&self) -> bool {
        self.held || self.sustained
    }

    pub fn note(&self) -> u8 {
        self.note
    }

    pub fn age(&self) -> u64 {
        self.age
    }

    /// Current output envelope — the engine steals the quietest voice first.
    pub fn loudness(&self) -> f32 {
        self.elements
            .iter()
            .map(|e| e.loudness())
            .fold(0.0, f32::max)
    }

    /// Start a note.
    ///
    /// Takes the performance settings by value rather than the whole patch: this
    /// runs on the audio thread for every note-on, and `Patch` owns a `String`,
    /// so passing it by reference to a caller that has to clone it first means a
    /// malloc and a free per note.
    pub fn note_on(&mut self, note: u8, velocity: f32, age: u64, performance: PerformancePatch) {
        let legato = self.active && self.is_held();
        let glide = match performance.portamento {
            PortamentoMode::Off => false,
            PortamentoMode::Always => self.active,
            PortamentoMode::Legato => legato,
        };

        self.target_note = note as f32;
        if !glide {
            self.glide.set_immediate(self.target_note);
        }

        if !self.active {
            for el in &mut self.elements {
                el.reset();
            }
        }

        self.note = note;
        self.velocity = velocity.clamp(0.0, 1.0);
        self.held = true;
        self.sustained = false;
        self.active = true;
        self.age = age;

        for el in &mut self.elements {
            el.note_on(legato && glide);
        }
    }

    pub fn note_off(&mut self, sustain_pedal: bool) {
        self.held = false;
        if sustain_pedal {
            self.sustained = true;
        } else {
            self.release();
        }
    }

    /// Sustain pedal lifted.
    pub fn pedal_up(&mut self) {
        if self.sustained {
            self.sustained = false;
            self.release();
        }
    }

    fn release(&mut self) {
        for el in &mut self.elements {
            el.note_off();
        }
    }

    /// Cut the voice short — used when stealing.
    pub fn steal(&mut self) {
        self.held = false;
        self.sustained = false;
        self.release();
    }

    /// Render one stereo sample into `out`, accumulating.
    #[inline]
    pub fn tick(&mut self, ctl: &Controls, bend_semitones: f32, out: &mut [f32; 2]) {
        if !self.active {
            return;
        }

        let note = self.glide.tick(self.target_note) + bend_semitones;
        let mut any_active = false;

        for (i, el) in self.elements.iter_mut().enumerate() {
            if !el.is_active() {
                continue;
            }
            any_active = true;
            let s = el.tick(note, self.velocity, ctl);
            // Constant-power pan.
            let p = (self.pan[i] + 1.0) * 0.25 * std::f32::consts::PI;
            out[0] += s * p.cos();
            out[1] += s * p.sin();
        }

        if !any_active {
            self.active = false;
            self.held = false;
            self.sustained = false;
        }
    }
}
