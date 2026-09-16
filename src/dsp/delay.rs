//! Fractional delay lines — the backbone of every digital waveguide.

/// A power-of-two circular buffer with linear-interpolated fractional taps.
///
/// Sample ordering is read-before-write, so a delay of 1.0 returns the sample
/// pushed on the previous `tick`.
#[derive(Clone, Debug)]
pub struct DelayLine {
    buf: Vec<f32>,
    mask: usize,
    write: usize,
    delay: f32,
}

impl DelayLine {
    pub fn new(max_delay: usize) -> Self {
        let cap = (max_delay + 4).next_power_of_two().max(8);
        Self {
            buf: vec![0.0; cap],
            mask: cap - 1,
            write: 0,
            delay: 1.0,
        }
    }

    pub fn clear(&mut self) {
        self.buf.iter_mut().for_each(|s| *s = 0.0);
        self.write = 0;
    }

    pub fn max_delay(&self) -> f32 {
        (self.buf.len() - 2) as f32
    }

    pub fn set_delay(&mut self, delay: f32) {
        self.delay = if delay.is_finite() {
            delay.clamp(1.0, self.max_delay())
        } else {
            1.0
        };
    }

    pub fn delay(&self) -> f32 {
        self.delay
    }

    /// Read an arbitrary tap without advancing the line.
    pub fn tap(&self, delay: f32) -> f32 {
        let d = delay.clamp(1.0, self.max_delay());
        let int = d.floor();
        let frac = d - int;
        let i0 = (self.write + self.buf.len() - int as usize) & self.mask;
        let i1 = (i0 + self.buf.len() - 1) & self.mask;
        self.buf[i0] + frac * (self.buf[i1] - self.buf[i0])
    }

    pub fn last_out(&self) -> f32 {
        self.tap(self.delay)
    }

    pub fn tick(&mut self, input: f32) -> f32 {
        let out = self.last_out();
        self.buf[self.write] = input;
        self.write = (self.write + 1) & self.mask;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_delay_is_exact() {
        let mut d = DelayLine::new(64);
        d.set_delay(4.0);
        let mut out = Vec::new();
        for i in 0..8 {
            out.push(d.tick(if i == 0 { 1.0 } else { 0.0 }));
        }
        assert_eq!(out[4], 1.0);
        assert_eq!(out[3], 0.0);
        assert_eq!(out[5], 0.0);
    }

    #[test]
    fn fractional_delay_interpolates() {
        let mut d = DelayLine::new(64);
        d.set_delay(4.5);
        let mut out = Vec::new();
        for i in 0..8 {
            out.push(d.tick(if i == 0 { 1.0 } else { 0.0 }));
        }
        assert!((out[4] - 0.5).abs() < 1e-6);
        assert!((out[5] - 0.5).abs() < 1e-6);
    }
}
