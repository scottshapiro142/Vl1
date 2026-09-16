//! The Modifier section.
//!
//! On the VL1 the driver and pipe produce a physically plausible but rather raw
//! tone; the modifiers are what turn it into a finished instrument. Everything
//! here is linear or memoryless — none of it feeds back into the waveguide.

use crate::dsp::{db_to_gain, fast_tanh, sanitize, Biquad, DcBlock, OnePole, Svf};

/// Per-sample context the modifiers use to track the performance.
#[derive(Clone, Copy, Debug, Default)]
pub struct ModifierContext {
    /// Current breath/bow drive, 0..~1.
    pub drive: f32,
    /// Note frequency in Hz, for key tracking.
    pub freq: f32,
}

/// Emphasises the attack transient — the "chiff" of a flute, the buzz at the
/// start of a brass note. Physically the model under-produces these because the
/// excitation ramps in smoothly; the expander puts the edge back.
#[derive(Clone, Debug)]
pub struct ImpulseExpander {
    amount: f32,
    band: Biquad,
    follower: OnePole,
    prev: f32,
    sample_rate: f32,
}

impl ImpulseExpander {
    pub fn new(sample_rate: f32) -> Self {
        let mut band = Biquad::new();
        band.set_bandpass(2000.0, 1.2, sample_rate);
        let mut follower = OnePole::new();
        follower.set_lowpass(30.0, sample_rate);
        Self {
            amount: 0.0,
            band,
            follower,
            prev: 0.0,
            sample_rate,
        }
    }

    pub fn set(&mut self, amount: f32, freq: f32) {
        self.amount = amount.clamp(0.0, 4.0);
        self.band
            .set_bandpass(freq.clamp(200.0, 12000.0), 1.2, self.sample_rate);
    }

    pub fn reset(&mut self) {
        self.band.clear();
        self.follower.clear();
        self.prev = 0.0;
    }

    #[inline]
    pub fn tick(&mut self, x: f32, ctx: &ModifierContext) -> f32 {
        if self.amount <= 0.0 {
            return x;
        }
        // Rising drive only: a positive-going rate of change means an attack.
        let rise = (ctx.drive - self.prev).max(0.0);
        self.prev = ctx.drive;
        let burst = self.follower.tick(rise * self.sample_rate * 0.02).min(1.0);
        x + self.band.tick(x) * burst * self.amount
    }
}

/// A fixed body/bell resonance in parallel with the dry signal. This is the
/// formant that stays put when the pitch moves — the thing that makes an
/// instrument sound like one object rather than a transposed sample.
#[derive(Clone, Debug)]
pub struct Resonator {
    filters: [Biquad; 2],
    mix: [f32; 2],
    enabled: bool,
}

impl Resonator {
    pub fn new(sample_rate: f32) -> Self {
        let mut filters = [Biquad::new(), Biquad::new()];
        filters[0].set_bandpass(500.0, 4.0, sample_rate);
        filters[1].set_bandpass(1500.0, 6.0, sample_rate);
        Self {
            filters,
            mix: [0.0, 0.0],
            enabled: false,
        }
    }

    pub fn set_band(&mut self, index: usize, freq: f32, q: f32, mix: f32, sample_rate: f32) {
        if index >= 2 {
            return;
        }
        self.filters[index].set_bandpass(freq.clamp(40.0, 16000.0), q.max(0.3), sample_rate);
        self.mix[index] = mix;
        self.enabled = self.mix.iter().any(|m| m.abs() > 1e-4);
    }

    pub fn reset(&mut self) {
        self.filters.iter_mut().for_each(|f| f.clear());
    }

    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        if !self.enabled {
            return x;
        }
        x + self.filters[0].tick(x) * self.mix[0] + self.filters[1].tick(x) * self.mix[1]
    }
}

/// Waveshaping harmonic generator. `bias` sweeps from even harmonics (a warm,
/// hollow doubling) through to odd (brass-like edge).
#[derive(Clone, Copy, Debug)]
pub struct HarmonicEnhancer {
    amount: f32,
    bias: f32,
    drive: f32,
    /// How strongly playing louder brings in more harmonics.
    drive_tracking: f32,
    /// The even-order term is a rectifier: it has a DC component that grows
    /// with level, and a non-zero value even at silence. Both have to go.
    dc: DcBlock,
}

impl Default for HarmonicEnhancer {
    fn default() -> Self {
        Self {
            amount: 0.0,
            bias: 0.5,
            drive: 1.0,
            drive_tracking: 0.0,
            dc: DcBlock::new(),
        }
    }
}

impl HarmonicEnhancer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, amount: f32, bias: f32, drive: f32, drive_tracking: f32) {
        self.amount = amount.clamp(0.0, 1.0);
        self.bias = bias.clamp(0.0, 1.0);
        self.drive = drive.clamp(0.1, 8.0);
        self.drive_tracking = drive_tracking.clamp(0.0, 4.0);
    }

    #[inline]
    pub fn tick(&mut self, x: f32, ctx: &ModifierContext) -> f32 {
        if self.amount <= 0.0 {
            return x;
        }
        let drive = self.drive * (1.0 + self.drive_tracking * ctx.drive);
        let d = x * drive;
        // Chebyshev-ish second and third order terms.
        let even = 2.0 * d * d - 1.0;
        let odd = 4.0 * d * d * d - 3.0 * d;
        let shaped = fast_tanh(even * (1.0 - self.bias) + odd * self.bias) * 0.5;
        x + self.dc.tick(shaped) * self.amount
    }

    pub fn reset(&mut self) {
        self.dc.clear();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterKind {
    LowPass,
    HighPass,
    BandPass,
}

/// A filter whose cutoff follows how hard the instrument is being played. On a
/// real instrument spectral content and dynamics are inseparable; this restores
/// that link for the parts of the tone the waveguide does not already provide.
#[derive(Clone, Copy, Debug)]
pub struct DynamicFilter {
    svf: Svf,
    kind: FilterKind,
    base_hz: f32,
    /// Octaves of cutoff sweep at full drive.
    env_depth: f32,
    resonance: f32,
    key_track: f32,
    enabled: bool,
    sample_rate: f32,
}

impl DynamicFilter {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            svf: Svf::new(),
            kind: FilterKind::LowPass,
            base_hz: 20000.0,
            env_depth: 0.0,
            resonance: 0.0,
            key_track: 0.0,
            enabled: false,
            sample_rate,
        }
    }

    pub fn set(
        &mut self,
        kind: FilterKind,
        base_hz: f32,
        env_depth: f32,
        resonance: f32,
        key_track: f32,
    ) {
        self.kind = kind;
        self.base_hz = base_hz.clamp(30.0, 20000.0);
        self.env_depth = env_depth.clamp(-8.0, 8.0);
        self.resonance = resonance.clamp(0.0, 0.98);
        self.key_track = key_track.clamp(0.0, 2.0);
        self.enabled = !(kind == FilterKind::LowPass
            && self.base_hz >= 18000.0
            && self.env_depth.abs() < 1e-3);
    }

    pub fn reset(&mut self) {
        self.svf.clear();
    }

    #[inline]
    pub fn tick(&mut self, x: f32, ctx: &ModifierContext) -> f32 {
        if !self.enabled {
            return x;
        }
        let key_oct = if self.key_track > 0.0 && ctx.freq > 0.0 {
            (ctx.freq / 261.63).log2() * self.key_track
        } else {
            0.0
        };
        let cutoff = self.base_hz * (self.env_depth * ctx.drive + key_oct).exp2();
        let achieved = self.svf.set(cutoff, self.resonance, self.sample_rate);
        let (lp, bp, hp) = self.svf.tick(x);

        match self.kind {
            FilterKind::LowPass => {
                // Past the filter's stable range a lowpass is doing nothing
                // anyway, so fade to dry instead of parking the cutoff at the
                // limit — otherwise a wide-open envelope sweep sounds capped.
                let blend = ((cutoff / achieved.max(1.0) - 1.0) * 0.5).clamp(0.0, 1.0);
                lp + (x - lp) * blend
            }
            FilterKind::HighPass => hp,
            FilterKind::BandPass => bp,
        }
    }
}

/// Five-band output EQ: low shelf, three peaks, high shelf.
#[derive(Clone, Debug)]
pub struct Equalizer {
    bands: [Biquad; 5],
    active: [bool; 5],
}

/// One EQ band's settings. `q` is ignored for the two shelves.
#[derive(Clone, Copy, Debug)]
pub struct EqBand {
    pub freq: f32,
    pub gain_db: f32,
    pub q: f32,
}

impl EqBand {
    pub const fn new(freq: f32, gain_db: f32, q: f32) -> Self {
        Self { freq, gain_db, q }
    }
}

impl Equalizer {
    pub fn new() -> Self {
        Self {
            bands: [Biquad::new(); 5],
            active: [false; 5],
        }
    }

    pub fn set(&mut self, bands: &[EqBand; 5], sample_rate: f32) {
        for (i, b) in bands.iter().enumerate() {
            self.active[i] = b.gain_db.abs() > 0.01;
            if !self.active[i] {
                continue;
            }
            match i {
                0 => self.bands[0].set_low_shelf(b.freq, b.gain_db, sample_rate),
                4 => self.bands[4].set_high_shelf(b.freq, b.gain_db, sample_rate),
                _ => self.bands[i].set_peaking(b.freq, b.q, b.gain_db, sample_rate),
            }
        }
    }

    pub fn reset(&mut self) {
        self.bands.iter_mut().for_each(|b| b.clear());
    }

    #[inline]
    pub fn tick(&mut self, mut x: f32) -> f32 {
        for (i, band) in self.bands.iter_mut().enumerate() {
            if self.active[i] {
                x = band.tick(x);
            }
        }
        x
    }
}

impl Default for Equalizer {
    fn default() -> Self {
        Self::new()
    }
}

/// The complete modifier chain for one element, in signal order.
#[derive(Clone, Debug)]
pub struct ModifierChain {
    pub impulse: ImpulseExpander,
    pub resonator: Resonator,
    pub enhancer: HarmonicEnhancer,
    pub filter: DynamicFilter,
    pub eq: Equalizer,
    output_gain: f32,
}

impl ModifierChain {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            impulse: ImpulseExpander::new(sample_rate),
            resonator: Resonator::new(sample_rate),
            enhancer: HarmonicEnhancer::new(),
            filter: DynamicFilter::new(sample_rate),
            eq: Equalizer::new(),
            output_gain: 1.0,
        }
    }

    pub fn set_output_gain_db(&mut self, db: f32) {
        self.output_gain = db_to_gain(db);
    }

    pub fn reset(&mut self) {
        self.impulse.reset();
        self.resonator.reset();
        self.enhancer.reset();
        self.filter.reset();
        self.eq.reset();
    }

    #[inline]
    pub fn tick(&mut self, x: f32, ctx: &ModifierContext) -> f32 {
        let mut y = self.impulse.tick(x, ctx);
        y = self.resonator.tick(y);
        y = self.enhancer.tick(y, ctx);
        y = self.filter.tick(y, ctx);
        y = self.eq.tick(y);
        sanitize(y * self.output_gain)
    }
}
