//! Deterministic noise sources for breath turbulence and bow scratch.

/// xorshift32 — fast, deterministic, good enough for audio-rate turbulence.
#[derive(Clone, Copy, Debug)]
pub struct Noise {
    state: u32,
    pink: [f32; 3],
}

impl Default for Noise {
    fn default() -> Self {
        Self::new(0x1234_5678)
    }
}

impl Noise {
    pub fn new(seed: u32) -> Self {
        Self {
            state: if seed == 0 { 1 } else { seed },
            pink: [0.0; 3],
        }
    }

    #[inline]
    fn next_u32(&mut self) -> u32 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        x
    }

    /// Uniform white noise in [-1, 1].
    #[inline]
    pub fn white(&mut self) -> f32 {
        (self.next_u32() as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    /// Uniform in [0, 1).
    #[inline]
    pub fn uniform(&mut self) -> f32 {
        self.next_u32() as f32 / u32::MAX as f32
    }

    /// Pink-ish noise (Robert Bristow-Johnson's economy filter). Breath noise in a
    /// real instrument is far from white; the low-frequency tilt matters a lot.
    #[inline]
    pub fn pink(&mut self) -> f32 {
        let w = self.white();
        self.pink[0] = 0.99886 * self.pink[0] + w * 0.0555179;
        self.pink[1] = 0.99332 * self.pink[1] + w * 0.0750759;
        self.pink[2] = 0.96900 * self.pink[2] + w * 0.153852;
        (self.pink[0] + self.pink[1] + self.pink[2] + w * 0.1848) * 0.5
    }
}
