/// YIN fundamental-frequency estimator.
///
/// Plain autocorrelation is not usable here: breath noise and bow scratch make
/// the correlation decay monotonically, so the peak-picker just returns the
/// shortest lag it is allowed. YIN's cumulative mean normalized difference
/// function is insensitive to that, which is the whole reason it exists.
fn detect_pitch(x: &[f32], sample_rate: f32, min_hz: f32, max_hz: f32) -> f32 {
    let min_lag = (sample_rate / max_hz).floor().max(2.0) as usize;
    let max_lag = ((sample_rate / min_hz).ceil() as usize).min(x.len() / 2);
    if max_lag <= min_lag {
        return 0.0;
    }

    // Difference function.
    let mut d = vec![0.0f64; max_lag + 1];
    let n = x.len() - max_lag;
    for (lag, slot) in d.iter_mut().enumerate().skip(1) {
        let mut sum = 0.0f64;
        for i in 0..n {
            let diff = (x[i] - x[i + lag]) as f64;
            sum += diff * diff;
        }
        *slot = sum;
    }

    // Cumulative mean normalization.
    let mut cmnd = vec![1.0f64; max_lag + 1];
    let mut running = 0.0f64;
    for lag in 1..=max_lag {
        running += d[lag];
        cmnd[lag] = if running > 0.0 {
            d[lag] * lag as f64 / running
        } else {
            1.0
        };
    }

    // First local minimum below the threshold, else the global minimum.
    const THRESHOLD: f64 = 0.15;
    let mut best = min_lag;
    let mut found = false;
    for lag in min_lag..max_lag {
        if cmnd[lag] < THRESHOLD && cmnd[lag] <= cmnd[lag + 1] {
            best = lag;
            found = true;
            break;
        }
    }
    if !found {
        best = (min_lag..=max_lag)
            .min_by(|a, b| cmnd[*a].partial_cmp(&cmnd[*b]).unwrap())
            .unwrap_or(min_lag);
    }

    // Parabolic refinement.
    let mut lag = best as f64;
    if best > min_lag && best < max_lag {
        let (a, b, c) = (cmnd[best - 1], cmnd[best], cmnd[best + 1]);
        let denom = a - 2.0 * b + c;
        if denom.abs() > 1e-12 {
            lag += 0.5 * (a - c) / denom;
        }
    }
    (sample_rate as f64 / lag) as f32
}
