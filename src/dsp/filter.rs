//! Small, allocation-free filter primitives used throughout the model.

use std::f32::consts::PI;

/// One-pole lowpass: `y[n] = b0*x[n] + a1*y[n-1]`.
#[derive(Clone, Copy, Debug)]
pub struct OnePole {
    b0: f32,
    a1: f32,
    y1: f32,
}

impl Default for OnePole {
    fn default() -> Self {
        Self {
            b0: 1.0,
            a1: 0.0,
            y1: 0.0,
        }
    }
}

impl OnePole {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the feedback pole directly (`|pole| < 1`), unity DC gain.
    pub fn set_pole(&mut self, pole: f32) {
        let p = pole.clamp(-0.999, 0.999);
        self.a1 = p;
        self.b0 = 1.0 - p.abs();
    }

    pub fn set_lowpass(&mut self, cutoff_hz: f32, sample_rate: f32) {
        let fc = cutoff_hz.clamp(1.0, sample_rate * 0.49);
        let p = (-2.0 * PI * fc / sample_rate).exp();
        self.set_pole(p);
    }

    pub fn clear(&mut self) {
        self.y1 = 0.0;
    }

    pub fn last_out(&self) -> f32 {
        self.y1
    }

    /// Phase delay in samples at `freq`.
    ///
    /// Waveguide loops need this: the termination filter is part of the loop, so
    /// its delay has to come out of the delay line's length or the instrument
    /// plays flat — audibly so under heavy damping. Evaluated at the playing
    /// frequency rather than as the low-frequency limit `a1 / (1 - a1)`, which
    /// over-states the delay once the note approaches the cutoff.
    pub fn phase_delay(&self, freq: f32, sample_rate: f32) -> f32 {
        if self.a1 <= 0.0 {
            return 0.0;
        }
        let w = 2.0 * PI * freq.max(1e-3) / sample_rate;
        if w <= 1e-6 {
            return self.a1 / (1.0 - self.a1);
        }
        (self.a1 * w.sin()).atan2(1.0 - self.a1 * w.cos()) / w
    }

    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        self.y1 = self.b0 * x + self.a1 * self.y1;
        self.y1
    }
}

/// One-zero lowpass (`y = b0*x + b1*x[n-1]`), the classic waveguide loss filter.
#[derive(Clone, Copy, Debug, Default)]
pub struct OneZero {
    b0: f32,
    b1: f32,
    x1: f32,
}

impl OneZero {
    pub fn new() -> Self {
        let mut f = Self::default();
        f.set_coefficient(0.5);
        f
    }

    /// `b` in [0, 1]: 0 = bypass, 0.5 = averaging lowpass (brightest loss at Nyquist).
    pub fn set_coefficient(&mut self, b: f32) {
        let b = b.clamp(0.0, 0.999);
        self.b0 = 1.0 - b;
        self.b1 = b;
    }

    pub fn clear(&mut self) {
        self.x1 = 0.0;
    }

    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1;
        self.x1 = x;
        y
    }
}

/// DC blocker — essential: waveguide loops with asymmetric drivers accumulate offset.
#[derive(Clone, Copy, Debug)]
pub struct DcBlock {
    r: f32,
    x1: f32,
    y1: f32,
}

impl Default for DcBlock {
    fn default() -> Self {
        Self {
            r: 0.999,
            x1: 0.0,
            y1: 0.0,
        }
    }
}

impl DcBlock {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_cutoff(&mut self, cutoff_hz: f32, sample_rate: f32) {
        self.r = (1.0 - 2.0 * PI * cutoff_hz / sample_rate).clamp(0.9, 0.99999);
    }

    pub fn clear(&mut self) {
        self.x1 = 0.0;
        self.y1 = 0.0;
    }

    /// Phase delay in samples at `freq` — negative, because a DC blocker leads.
    ///
    /// In a waveguide loop that lead is a real tuning error: it shortens the
    /// round trip, and since the lead grows as the note approaches the blocker's
    /// corner, the bottom of the keyboard plays progressively sharp.
    pub fn phase_delay(&self, freq: f32, sample_rate: f32) -> f32 {
        let w = 2.0 * PI * freq.max(1e-3) / sample_rate;
        if w <= 1e-6 {
            return 0.0;
        }
        let (sw, cw) = (w.sin(), w.cos());
        // H(e^jw) = (1 - e^-jw) / (1 - r e^-jw)
        let num_phase = sw.atan2(1.0 - cw);
        let den_phase = (self.r * sw).atan2(1.0 - self.r * cw);
        -(num_phase - den_phase) / w
    }

    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let y = x - self.x1 + self.r * self.y1;
        self.x1 = x;
        self.y1 = y;
        y
    }
}

/// Direct-form-I biquad with RBJ cookbook coefficient setters.
#[derive(Clone, Copy, Debug)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Default for Biquad {
    fn default() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }
}

impl Biquad {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }

    fn normalize(&mut self, b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) {
        let inv = 1.0 / a0;
        self.b0 = b0 * inv;
        self.b1 = b1 * inv;
        self.b2 = b2 * inv;
        self.a1 = a1 * inv;
        self.a2 = a2 * inv;
    }

    fn omega(freq: f32, sample_rate: f32) -> (f32, f32, f32) {
        let f = freq.clamp(10.0, sample_rate * 0.48);
        let w = 2.0 * PI * f / sample_rate;
        (w, w.sin(), w.cos())
    }

    pub fn set_lowpass(&mut self, freq: f32, q: f32, sample_rate: f32) {
        let (_, sw, cw) = Self::omega(freq, sample_rate);
        let alpha = sw / (2.0 * q.max(0.05));
        let b1 = 1.0 - cw;
        self.normalize(b1 * 0.5, b1, b1 * 0.5, 1.0 + alpha, -2.0 * cw, 1.0 - alpha);
    }

    pub fn set_highpass(&mut self, freq: f32, q: f32, sample_rate: f32) {
        let (_, sw, cw) = Self::omega(freq, sample_rate);
        let alpha = sw / (2.0 * q.max(0.05));
        let b0 = (1.0 + cw) * 0.5;
        self.normalize(b0, -(1.0 + cw), b0, 1.0 + alpha, -2.0 * cw, 1.0 - alpha);
    }

    /// Constant-peak-gain bandpass.
    pub fn set_bandpass(&mut self, freq: f32, q: f32, sample_rate: f32) {
        let (_, sw, cw) = Self::omega(freq, sample_rate);
        let alpha = sw / (2.0 * q.max(0.05));
        self.normalize(alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cw, 1.0 - alpha);
    }

    pub fn set_notch(&mut self, freq: f32, q: f32, sample_rate: f32) {
        let (_, sw, cw) = Self::omega(freq, sample_rate);
        let alpha = sw / (2.0 * q.max(0.05));
        self.normalize(1.0, -2.0 * cw, 1.0, 1.0 + alpha, -2.0 * cw, 1.0 - alpha);
    }

    pub fn set_peaking(&mut self, freq: f32, q: f32, gain_db: f32, sample_rate: f32) {
        let a = 10f32.powf(gain_db / 40.0);
        let (_, sw, cw) = Self::omega(freq, sample_rate);
        let alpha = sw / (2.0 * q.max(0.05));
        self.normalize(
            1.0 + alpha * a,
            -2.0 * cw,
            1.0 - alpha * a,
            1.0 + alpha / a,
            -2.0 * cw,
            1.0 - alpha / a,
        );
    }

    pub fn set_low_shelf(&mut self, freq: f32, gain_db: f32, sample_rate: f32) {
        let a = 10f32.powf(gain_db / 40.0);
        let (_, sw, cw) = Self::omega(freq, sample_rate);
        let alpha = sw * 0.5 * (2.0f32).sqrt();
        let tsa = 2.0 * a.sqrt() * alpha;
        self.normalize(
            a * ((a + 1.0) - (a - 1.0) * cw + tsa),
            2.0 * a * ((a - 1.0) - (a + 1.0) * cw),
            a * ((a + 1.0) - (a - 1.0) * cw - tsa),
            (a + 1.0) + (a - 1.0) * cw + tsa,
            -2.0 * ((a - 1.0) + (a + 1.0) * cw),
            (a + 1.0) + (a - 1.0) * cw - tsa,
        );
    }

    pub fn set_high_shelf(&mut self, freq: f32, gain_db: f32, sample_rate: f32) {
        let a = 10f32.powf(gain_db / 40.0);
        let (_, sw, cw) = Self::omega(freq, sample_rate);
        let alpha = sw * 0.5 * (2.0f32).sqrt();
        let tsa = 2.0 * a.sqrt() * alpha;
        self.normalize(
            a * ((a + 1.0) + (a - 1.0) * cw + tsa),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * cw),
            a * ((a + 1.0) + (a - 1.0) * cw - tsa),
            (a + 1.0) - (a - 1.0) * cw + tsa,
            2.0 * ((a - 1.0) - (a + 1.0) * cw),
            (a + 1.0) - (a - 1.0) * cw - tsa,
        );
    }

    /// Resonant two-pole normalized for unity gain at DC.
    ///
    /// Unlike the bandpass forms, this one passes a static input — which is what
    /// you want for a mechanical resonator whose rest position is displaced by a
    /// constant force, such as a player's lips under mouth pressure.
    pub fn set_resonance_lowpass(&mut self, freq: f32, radius: f32, sample_rate: f32) {
        let r = radius.clamp(0.0, 0.9999);
        let (w, _, _) = Self::omega(freq, sample_rate);
        self.a1 = -2.0 * r * w.cos();
        self.a2 = r * r;
        self.b0 = 1.0 + self.a1 + self.a2;
        self.b1 = 0.0;
        self.b2 = 0.0;
    }

    /// Resonant two-pole with unity peak gain and zero gain at DC and Nyquist.
    pub fn set_resonance(&mut self, freq: f32, radius: f32, sample_rate: f32) {
        let r = radius.clamp(0.0, 0.9999);
        let (w, _, _) = Self::omega(freq, sample_rate);
        self.a1 = -2.0 * r * w.cos();
        self.a2 = r * r;
        // Normalize for unity gain at the resonant peak.
        self.b0 = 0.5 - 0.5 * r * r;
        self.b1 = 0.0;
        self.b2 = -self.b0;
    }

    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// Zero-delay-feedback-ish state variable filter (Chamberlin, oversampled coefficients).
/// Cheap, stable under fast cutoff modulation — ideal for the Dynamic Filter modifier.
#[derive(Clone, Copy, Debug, Default)]
pub struct Svf {
    f: f32,
    q: f32,
    low: f32,
    band: f32,
}

impl Svf {
    pub fn new() -> Self {
        Self {
            f: 0.1,
            q: 1.0,
            low: 0.0,
            band: 0.0,
        }
    }

    pub fn clear(&mut self) {
        self.low = 0.0;
        self.band = 0.0;
    }

    /// Tune the filter. Returns the cutoff actually achieved, in Hz.
    ///
    /// This topology is only conditionally stable: its poles leave the unit
    /// circle once `f * (f + 2q) >= 4`, so a high cutoff with little resonance
    /// will blow up. The coefficient is capped at that bound (with margin)
    /// rather than the cutoff being clamped to a fixed fraction of the sample
    /// rate, which would throw away the extra range that resonance allows.
    /// Callers that care use the returned value to blend toward dry.
    pub fn set(&mut self, cutoff_hz: f32, resonance: f32, sample_rate: f32) -> f32 {
        // resonance 0..1 -> damping 2..0.05
        let q = (2.0 - 1.95 * resonance.clamp(0.0, 1.0)).clamp(0.05, 2.0);
        let limit = ((q * q + 3.24).sqrt() - q).max(0.02);

        let fc = cutoff_hz.clamp(20.0, sample_rate * 0.49);
        let f = (2.0 * (PI * fc / sample_rate).sin()).min(limit);

        self.q = q;
        self.f = f;
        sample_rate / PI * (f * 0.5).clamp(0.0, 1.0).asin()
    }

    /// Returns `(lowpass, bandpass, highpass)`.
    #[inline]
    pub fn tick(&mut self, x: f32) -> (f32, f32, f32) {
        let high = x - self.low - self.q * self.band;
        self.band += self.f * high;
        self.low += self.f * self.band;
        if !self.low.is_finite() || !self.band.is_finite() {
            self.clear();
        }
        (self.low, self.band, high)
    }
}
