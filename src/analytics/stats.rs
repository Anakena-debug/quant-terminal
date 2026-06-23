//! Returns-based statistics — the math behind the risk, correlation, and
//! optimizer views. All functions are pure and operate on slices of returns or
//! prices, so they're trivially testable.

/// Periodic simple returns from a price series: `r_t = p_t / p_{t-1} - 1`.
pub fn returns(prices: &[f64]) -> Vec<f64> {
    prices
        .windows(2)
        .map(|w| {
            if w[0].abs() > f64::EPSILON {
                w[1] / w[0] - 1.0
            } else {
                0.0
            }
        })
        .collect()
}

pub fn mean(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        0.0
    } else {
        xs.iter().sum::<f64>() / xs.len() as f64
    }
}

/// Sample variance (divides by `n-1`).
pub fn variance(xs: &[f64]) -> f64 {
    if xs.len() < 2 {
        return 0.0;
    }
    let m = mean(xs);
    xs.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (xs.len() - 1) as f64
}

pub fn stddev(xs: &[f64]) -> f64 {
    variance(xs).sqrt()
}

/// Annualised volatility given `periods` per year (e.g. 252 for daily).
pub fn annualized_vol(rets: &[f64], periods: f64) -> f64 {
    stddev(rets) * periods.sqrt()
}

pub fn covariance(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len().min(b.len());
    if n < 2 {
        return 0.0;
    }
    let (ma, mb) = (mean(&a[..n]), mean(&b[..n]));
    (0..n).map(|i| (a[i] - ma) * (b[i] - mb)).sum::<f64>() / (n - 1) as f64
}

/// Pearson correlation, clamped to `[-1, 1]`.
pub fn correlation(a: &[f64], b: &[f64]) -> f64 {
    let (sa, sb) = (stddev(a), stddev(b));
    if sa < 1e-12 || sb < 1e-12 {
        0.0
    } else {
        (covariance(a, b) / (sa * sb)).clamp(-1.0, 1.0)
    }
}

/// Market beta of `asset` vs `market`.
pub fn beta(asset: &[f64], market: &[f64]) -> f64 {
    let mv = variance(market);
    if mv < 1e-12 {
        0.0
    } else {
        covariance(asset, market) / mv
    }
}

/// Annualised Sharpe ratio (risk-free = 0).
pub fn sharpe(rets: &[f64], periods: f64) -> f64 {
    let s = stddev(rets);
    if s < 1e-12 {
        0.0
    } else {
        mean(rets) / s * periods.sqrt()
    }
}

/// Annualised Sortino ratio (downside deviation only).
pub fn sortino(rets: &[f64], periods: f64) -> f64 {
    let downside: Vec<f64> = rets.iter().copied().filter(|&r| r < 0.0).collect();
    if downside.is_empty() {
        return 0.0;
    }
    let dd = (downside.iter().map(|r| r * r).sum::<f64>() / downside.len() as f64).sqrt();
    if dd < 1e-12 {
        0.0
    } else {
        mean(rets) / dd * periods.sqrt()
    }
}

/// Historical VaR at `confidence` (e.g. 0.95) as a positive loss fraction.
pub fn historical_var(rets: &[f64], confidence: f64) -> f64 {
    if rets.is_empty() {
        return 0.0;
    }
    let mut sorted = rets.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = ((1.0 - confidence) * sorted.len() as f64).floor() as usize;
    (-sorted[idx.min(sorted.len() - 1)]).max(0.0)
}

/// Parametric (normal) VaR at `confidence`, as a positive loss fraction.
pub fn parametric_var(rets: &[f64], confidence: f64) -> f64 {
    let z = if confidence >= 0.99 {
        2.326
    } else if confidence >= 0.975 {
        1.960
    } else {
        1.645
    };
    (z * stddev(rets) - mean(rets)).max(0.0)
}

pub fn skewness(xs: &[f64]) -> f64 {
    if xs.len() < 2 {
        return 0.0;
    }
    let m = mean(xs);
    let m2 = xs.iter().map(|x| (x - m).powi(2)).sum::<f64>() / xs.len() as f64;
    let m3 = xs.iter().map(|x| (x - m).powi(3)).sum::<f64>() / xs.len() as f64;
    if m2 < 1e-18 { 0.0 } else { m3 / m2.powf(1.5) }
}

/// Excess kurtosis (normal = 0).
pub fn excess_kurtosis(xs: &[f64]) -> f64 {
    if xs.len() < 2 {
        return 0.0;
    }
    let m = mean(xs);
    let m2 = xs.iter().map(|x| (x - m).powi(2)).sum::<f64>() / xs.len() as f64;
    let m4 = xs.iter().map(|x| (x - m).powi(4)).sum::<f64>() / xs.len() as f64;
    if m2 < 1e-18 {
        0.0
    } else {
        m4 / (m2 * m2) - 3.0
    }
}

/// Max drawdown (positive percent) of an equity/price curve.
pub fn max_drawdown(curve: &[f64]) -> f64 {
    let mut peak = f64::MIN;
    let mut mdd: f64 = 0.0;
    for &v in curve {
        peak = peak.max(v);
        if peak > 0.0 {
            mdd = mdd.max((peak - v) / peak);
        }
    }
    mdd * 100.0
}

/// Cumulative price index from returns, starting at 1.0.
pub fn equity_index(rets: &[f64]) -> Vec<f64> {
    let mut v = 1.0;
    let mut out = Vec::with_capacity(rets.len() + 1);
    out.push(v);
    for &r in rets {
        v *= 1.0 + r;
        out.push(v);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_and_moments() {
        let p = vec![100.0, 110.0, 99.0];
        let r = returns(&p);
        assert!((r[0] - 0.1).abs() < 1e-9);
        assert!((r[1] - (-0.1)).abs() < 1e-9);
        assert!(mean(&[1.0, 2.0, 3.0]) == 2.0);
        assert!(stddev(&[2.0, 2.0, 2.0]) < 1e-12);
    }

    #[test]
    fn correlation_bounds() {
        let a = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let b: Vec<f64> = a.iter().map(|x| x * 2.0 + 1.0).collect(); // perfectly correlated
        let c: Vec<f64> = a.iter().map(|x| -x).collect(); // perfectly anti-correlated
        assert!((correlation(&a, &b) - 1.0).abs() < 1e-9);
        assert!((correlation(&a, &c) + 1.0).abs() < 1e-9);
    }

    #[test]
    fn beta_of_self_is_one() {
        let m = vec![0.01, -0.02, 0.03, -0.01, 0.02];
        assert!((beta(&m, &m) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn var_is_positive_for_losses() {
        let r = vec![-0.05, -0.02, 0.01, 0.03, -0.10, 0.02, 0.04, -0.01];
        assert!(historical_var(&r, 0.95) >= 0.0);
        assert!(parametric_var(&r, 0.99) >= parametric_var(&r, 0.95));
    }

    #[test]
    fn drawdown_basic() {
        let eq = vec![100.0, 120.0, 90.0, 110.0];
        // peak 120 → trough 90 = 25%
        assert!((max_drawdown(&eq) - 25.0).abs() < 1e-9);
    }
}
