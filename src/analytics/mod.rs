//! Technical-analysis primitives.
//!
//! Implemented directly (rather than via a TA crate) so behaviour is fully
//! under our control and indicators align 1:1 with the candle slice — each
//! returns a `Vec<Option<f64>>` the same length as the input, `None` where the
//! indicator isn't defined yet (warm-up period).

pub mod options;
pub mod quant;
pub mod series;
pub mod stats;

pub use quant::{Portfolio, QuantData};
pub use series::ChartSeries;

/// One indicator output, aligned 1:1 with the input slice (`None` during warm-up).
pub type IndSeries = Vec<Option<f64>>;

/// Simple moving average.
pub fn sma(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; values.len()];
    if period == 0 {
        return out;
    }
    let mut sum = 0.0;
    for i in 0..values.len() {
        sum += values[i];
        if i >= period {
            sum -= values[i - period];
        }
        if i + 1 >= period {
            out[i] = Some(sum / period as f64);
        }
    }
    out
}

/// Exponential moving average, seeded with the SMA of the first `period`.
pub fn ema(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let n = values.len();
    let mut out = vec![None; n];
    if period == 0 || n < period {
        return out;
    }
    let k = 2.0 / (period as f64 + 1.0);
    let mut prev = values[..period].iter().sum::<f64>() / period as f64;
    out[period - 1] = Some(prev);
    for i in period..n {
        prev = values[i] * k + prev * (1.0 - k);
        out[i] = Some(prev);
    }
    out
}

/// Bollinger Bands: `(middle, upper, lower)` for an `n`-period SMA ± `k`·σ.
pub fn bollinger(values: &[f64], period: usize, k: f64) -> (IndSeries, IndSeries, IndSeries) {
    let n = values.len();
    let (mut mid, mut up, mut low) = (vec![None; n], vec![None; n], vec![None; n]);
    if period == 0 {
        return (mid, up, low);
    }
    for i in 0..n {
        if i + 1 >= period {
            let window = &values[i + 1 - period..=i];
            let mean = window.iter().sum::<f64>() / period as f64;
            let var = window.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / period as f64;
            let sd = var.sqrt();
            mid[i] = Some(mean);
            up[i] = Some(mean + k * sd);
            low[i] = Some(mean - k * sd);
        }
    }
    (mid, up, low)
}

/// Wilder's RSI.
pub fn rsi(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let n = values.len();
    let mut out = vec![None; n];
    if period == 0 || n <= period {
        return out;
    }
    let (mut gain, mut loss) = (0.0, 0.0);
    for i in 1..=period {
        let d = values[i] - values[i - 1];
        if d >= 0.0 {
            gain += d;
        } else {
            loss -= d;
        }
    }
    let mut avg_gain = gain / period as f64;
    let mut avg_loss = loss / period as f64;
    out[period] = Some(rsi_value(avg_gain, avg_loss));
    for i in period + 1..n {
        let d = values[i] - values[i - 1];
        let (g, l) = if d >= 0.0 { (d, 0.0) } else { (0.0, -d) };
        avg_gain = (avg_gain * (period as f64 - 1.0) + g) / period as f64;
        avg_loss = (avg_loss * (period as f64 - 1.0) + l) / period as f64;
        out[i] = Some(rsi_value(avg_gain, avg_loss));
    }
    out
}

fn rsi_value(avg_gain: f64, avg_loss: f64) -> f64 {
    if avg_loss == 0.0 {
        100.0
    } else {
        let rs = avg_gain / avg_loss;
        100.0 - 100.0 / (1.0 + rs)
    }
}

/// MACD: returns `(macd_line, signal, histogram)`.
pub fn macd(
    values: &[f64],
    fast: usize,
    slow: usize,
    signal: usize,
) -> (IndSeries, IndSeries, IndSeries) {
    let n = values.len();
    let ef = ema(values, fast);
    let es = ema(values, slow);

    let mut line = vec![None; n];
    let mut first = None;
    for i in 0..n {
        if let (Some(a), Some(b)) = (ef[i], es[i]) {
            line[i] = Some(a - b);
            if first.is_none() {
                first = Some(i);
            }
        }
    }

    let (mut sig, mut hist) = (vec![None; n], vec![None; n]);
    if let Some(start) = first {
        let contiguous: Vec<f64> = line[start..].iter().map(|x| x.unwrap_or(0.0)).collect();
        let signal_ema = ema(&contiguous, signal);
        for (j, s) in signal_ema.iter().enumerate() {
            if let Some(s) = s {
                let i = start + j;
                sig[i] = Some(*s);
                if let Some(m) = line[i] {
                    hist[i] = Some(m - *s);
                }
            }
        }
    }
    (line, sig, hist)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sma_of_constant_is_constant() {
        let v = vec![5.0; 10];
        let out = sma(&v, 3);
        assert_eq!(out[0], None);
        assert_eq!(out[1], None);
        assert_eq!(out[2], Some(5.0));
        assert_eq!(out[9], Some(5.0));
    }

    #[test]
    fn rsi_of_monotonic_rise_is_100() {
        let v: Vec<f64> = (0..30).map(|i| i as f64).collect();
        let out = rsi(&v, 14);
        assert_eq!(out[29], Some(100.0));
    }

    #[test]
    fn ema_tracks_then_converges() {
        let v = vec![10.0; 20];
        let out = ema(&v, 5);
        assert_eq!(out[3], None);
        assert_eq!(out[4], Some(10.0));
        assert_eq!(out[19], Some(10.0));
    }

    #[test]
    fn macd_lengths_align() {
        let v: Vec<f64> = (0..60)
            .map(|i| (i as f64 * 0.1).sin() * 5.0 + 100.0)
            .collect();
        let (line, sig, hist) = macd(&v, 12, 26, 9);
        assert_eq!(line.len(), v.len());
        assert_eq!(sig.len(), v.len());
        assert_eq!(hist.len(), v.len());
    }
}
