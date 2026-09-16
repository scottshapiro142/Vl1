//! The polyphonic engine.
//!
//! The original VL1 managed two voices of virtual acoustic synthesis on its
//! dedicated DSP hardware; on a modern CPU eight is comfortable, so this engine
//! runs an eight-voice pool with last-note-priority stealing.

use crate::control::Controls;
use crate::effects::{Chorus, Reverb};
use crate::patch::Patch;
use crate::voice::Voice;

/// Number of simultaneously sounding notes.
pub const POLYPHONY: usize = 8;

pub struct Engine {
    voices: Vec<Voice>,
    patch: Patch,
    controls: Controls,
    chorus: Chorus,
    reverb: Reverb,
    sample_rate: f32,
    counter: u64,
    master: f32,
}

impl Engine {
    /// Create an engine with [`POLYPHONY`] voices.
    pub fn new(sample_rate: f32) -> Self {
        Self::with_patch(Patch::default(), sample_rate)
    }

    pub fn with_patch(patch: Patch, sample_rate: f32) -> Self {
        let voices = (0..POLYPHONY)
            .map(|i| Voice::new(&patch, sample_rate, i as u32 + 1))
            .collect();
        let mut engine = Self {
            voices,
            master: patch.performance.master_level,
            patch,
            controls: Controls::new(),
            chorus: Chorus::new(sample_rate),
            reverb: Reverb::new(sample_rate),
            sample_rate,
            counter: 0,
        };
        engine.chorus.configure(&engine.patch.chorus);
        engine.reverb.configure(&engine.patch.reverb);
        engine
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    pub fn polyphony(&self) -> usize {
        self.voices.len()
    }

    /// Number of voices currently sounding.
    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.is_active()).count()
    }

    pub fn patch(&self) -> &Patch {
        &self.patch
    }

    /// Load a patch. Notes already sounding keep playing with the new settings.
    pub fn set_patch(&mut self, patch: Patch) {
        self.patch = patch;
        for (i, voice) in self.voices.iter_mut().enumerate() {
            voice.apply_patch(&self.patch, i as u32 + 1);
        }
        self.master = self.patch.performance.master_level;
        self.chorus.configure(&self.patch.chorus);
        self.reverb.configure(&self.patch.reverb);
    }

    pub fn controls(&self) -> &Controls {
        &self.controls
    }

    pub fn controls_mut(&mut self) -> &mut Controls {
        &mut self.controls
    }

    /// Silence everything immediately.
    pub fn panic(&mut self) {
        for voice in &mut self.voices {
            voice.steal();
        }
        self.chorus.reset();
        self.reverb.reset();
    }

    pub fn note_on(&mut self, note: u8, velocity: u8) {
        if velocity == 0 {
            self.note_off(note);
            return;
        }
        self.counter += 1;
        let age = self.counter;
        let index = self.allocate(note);
        let patch = self.patch.clone();
        self.voices[index].note_on(note, velocity as f32 / 127.0, age, &patch);
    }

    pub fn note_off(&mut self, note: u8) {
        let sustain = self.controls.sustain;
        for voice in &mut self.voices {
            if voice.is_active() && voice.note() == note && voice.is_held() {
                voice.note_off(sustain);
            }
        }
    }

    /// Pick a voice for a new note: the same note retriggered, then any free
    /// voice, then the quietest released voice, then the oldest held one.
    fn allocate(&mut self, note: u8) -> usize {
        if let Some(i) = self
            .voices
            .iter()
            .position(|v| v.is_active() && v.note() == note && v.is_held())
        {
            return i;
        }
        if let Some(i) = self.voices.iter().position(|v| !v.is_active()) {
            return i;
        }

        let mut best = 0;
        let mut best_key = (true, f32::MAX, u64::MAX);
        for (i, v) in self.voices.iter().enumerate() {
            // Ordering: prefer not-held, then quietest, then oldest.
            let key = (v.is_held(), v.loudness(), v.age());
            if key < best_key {
                best_key = key;
                best = i;
            }
        }
        self.voices[best].steal();
        best
    }

    pub fn set_sustain(&mut self, on: bool) {
        self.controls.sustain = on;
        if !on {
            for voice in &mut self.voices {
                voice.pedal_up();
            }
        }
    }

    /// Pitch bend in the range -1..1, scaled by the patch's bend range.
    pub fn set_pitch_bend(&mut self, bend: f32) {
        self.controls.pitch_bend = bend.clamp(-1.0, 1.0);
    }

    /// Render one stereo sample pair.
    #[inline]
    pub fn tick(&mut self) -> [f32; 2] {
        let ctl = self.controls;
        let bend = ctl.pitch_bend * self.patch.performance.pitch_bend_range;

        let mut mix = [0.0f32; 2];
        for voice in &mut self.voices {
            voice.tick(&ctl, bend, &mut mix);
        }

        mix[0] *= self.master;
        mix[1] *= self.master;
        mix = self.chorus.tick(mix);
        mix = self.reverb.tick(mix);

        // Eight independent waveguides can sum well past full scale; a soft
        // limiter keeps that musical instead of letting it wrap or clip hard.
        [crate::dsp::soft_clip(mix[0]), crate::dsp::soft_clip(mix[1])]
    }

    /// Render interleaved stereo into `out` (length must be even).
    pub fn render(&mut self, out: &mut [f32]) {
        for frame in out.chunks_mut(2) {
            let s = self.tick();
            frame[0] = s[0];
            if frame.len() > 1 {
                frame[1] = s[1];
            }
        }
    }

    /// Render `frames` stereo frames into separate channel buffers.
    pub fn render_split(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        for i in 0..n {
            let s = self.tick();
            left[i] = s[0];
            right[i] = s[1];
        }
    }
}
