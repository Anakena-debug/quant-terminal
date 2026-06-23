//! Cross-sectional quant analytics: correlation matrix, per-symbol risk stats,
//! and a Markowitz mean-variance optimizer (efficient frontier + optimal
//! weights). Built from aligned price histories of the watchlist plus a
//! benchmark (for beta).

use super::stats;

#[derive(Clone, Debug)]
pub struct RiskStats {
    pub symbol: String,
    pub ret_annual: f64, // %
    pub vol_annual: f64, // %
    pub beta: f64,
    pub sharpe: f64,
    pub sortino: f64,
    pub var95: f64, // % per-period loss
    pub var99: f64,
    pub max_dd: f64, // %
    pub skew: f64,
    pub kurt: f64,
}

#[derive(Clone, Debug)]
pub struct Portfolio {
    pub weights: Vec<f64>,
    pub ret: f64, // annual %
    pub vol: f64, // annual %
    pub sharpe: f64,
}

#[derive(Clone, Debug)]
pub struct QuantData {
    pub symbols: Vec<String>,
    /// n×n Pearson correlation matrix of returns.
    pub corr: Vec<Vec<f64>>,
    pub stats: Vec<RiskStats>,
    /// Monte-Carlo long-only portfolios `(vol%, ret%, sharpe)` — the efficient
    /// frontier is the upper-left edge of this cloud.
    pub cloud: Vec<(f64, f64, f64)>,
    /// Per-asset `(vol%, ret%)` for the scatter.
    pub assets: Vec<(f64, f64)>,
    pub max_sharpe: Portfolio,
    pub min_var: Portfolio,
    /// Number of return observations used.
    pub observations: usize,
}

impl QuantData {
    /// Build from `(symbol, closes)` series + a benchmark close series.
    /// Returns `None` if there isn't enough overlapping history.
    pub fn build(series: &[(String, Vec<f64>)], benchmark: &[f64], periods: f64) -> Option<Self> {
        let n = series.len();
        if n == 0 {
            return None;
        }
        let min_len = series
            .iter()
            .map(|(_, c)| c.len())
            .chain(std::iter::once(benchmark.len()))
            .min()
            .unwrap_or(0);
        if min_len < 10 {
            return None;
        }
        let tail = |c: &[f64]| c[c.len() - min_len..].to_vec();

        let rets: Vec<Vec<f64>> = series
            .iter()
            .map(|(_, c)| stats::returns(&tail(c)))
            .collect();
        let bench_rets = stats::returns(&tail(benchmark));
        let symbols: Vec<String> = series.iter().map(|(s, _)| s.clone()).collect();
        let observations = rets.first().map(|r| r.len()).unwrap_or(0);

        let corr: Vec<Vec<f64>> = (0..n)
            .map(|i| {
                (0..n)
                    .map(|j| stats::correlation(&rets[i], &rets[j]))
                    .collect()
            })
            .collect();

        let stats_vec: Vec<RiskStats> = (0..n)
            .map(|i| {
                let r = &rets[i];
                RiskStats {
                    symbol: symbols[i].clone(),
                    ret_annual: stats::mean(r) * periods * 100.0,
                    vol_annual: stats::annualized_vol(r, periods) * 100.0,
                    beta: stats::beta(r, &bench_rets),
                    sharpe: stats::sharpe(r, periods),
                    sortino: stats::sortino(r, periods),
                    var95: stats::historical_var(r, 0.95) * 100.0, // empirical
                    var99: stats::parametric_var(r, 0.99) * 100.0, // normal-tail
                    max_dd: stats::max_drawdown(&stats::equity_index(r)),
                    skew: stats::skewness(r),
                    kurt: stats::excess_kurtosis(r),
                }
            })
            .collect();

        let mean_ann: Vec<f64> = (0..n).map(|i| stats::mean(&rets[i]) * periods).collect();
        let mut cov_ann: Vec<Vec<f64>> = (0..n)
            .map(|i| {
                (0..n)
                    .map(|j| stats::covariance(&rets[i], &rets[j]) * periods)
                    .collect()
            })
            .collect();
        for (i, row) in cov_ann.iter_mut().enumerate() {
            row[i] += 1e-6; // ridge for numerical stability
        }

        let assets: Vec<(f64, f64)> = stats_vec
            .iter()
            .map(|s| (s.vol_annual, s.ret_annual))
            .collect();

        let port = |w: &[f64]| {
            let ret = dot(w, &mean_ann) * 100.0;
            let vol = quad_form(w, &cov_ann).max(0.0).sqrt() * 100.0;
            Portfolio {
                weights: w.to_vec(),
                ret,
                vol,
                sharpe: if vol > 1e-9 { ret / vol } else { 0.0 },
            }
        };

        // Monte-Carlo long-only portfolios (weights ≥ 0, sum to 1): bounded and
        // interpretable, unlike the analytical unconstrained tangency portfolio.
        let mut seed = 0x9E37_79B9_7F4A_7C15_u64;
        let mut rand01 = || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) as f64) / ((1u64 << 31) as f64)
        };
        let mut cloud = Vec::with_capacity(4000);
        let mut best_sharpe: Option<(f64, Vec<f64>)> = None;
        let mut best_minvar: Option<(f64, Vec<f64>)> = None;
        for _ in 0..4000 {
            // Dirichlet(1,…,1) via normalized exponentials → uniform on the simplex.
            let mut w: Vec<f64> = (0..n).map(|_| -rand01().max(1e-9).ln()).collect();
            let sum: f64 = w.iter().sum();
            if sum <= 0.0 {
                continue;
            }
            for x in w.iter_mut() {
                *x /= sum;
            }
            let ret = dot(&w, &mean_ann) * 100.0;
            let vol = quad_form(&w, &cov_ann).max(0.0).sqrt() * 100.0;
            let sh = if vol > 1e-9 { ret / vol } else { 0.0 };
            cloud.push((vol, ret, sh));
            if best_sharpe.as_ref().is_none_or(|b| sh > b.0) {
                best_sharpe = Some((sh, w.clone()));
            }
            if best_minvar.as_ref().is_none_or(|b| vol < b.0) {
                best_minvar = Some((vol, w));
            }
        }
        let equal_weight = vec![1.0 / n as f64; n];
        let max_sharpe = best_sharpe
            .map(|(_, w)| port(&w))
            .unwrap_or_else(|| port(&equal_weight));
        let min_var = best_minvar
            .map(|(_, w)| port(&w))
            .unwrap_or_else(|| port(&equal_weight));

        Some(QuantData {
            symbols,
            corr,
            stats: stats_vec,
            cloud,
            assets,
            max_sharpe,
            min_var,
            observations,
        })
    }
}

// ── small dense linear algebra (n is the watchlist size, ~10) ───────────────

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn matvec(m: &[Vec<f64>], v: &[f64]) -> Vec<f64> {
    m.iter().map(|row| dot(row, v)).collect()
}

fn quad_form(w: &[f64], m: &[Vec<f64>]) -> f64 {
    dot(w, &matvec(m, w))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_produces_sane_matrices() {
        // two anti-correlated-ish synthetic series
        let a: Vec<f64> = (0..120)
            .map(|i| 100.0 + (i as f64 * 0.2).sin() * 10.0)
            .collect();
        let b: Vec<f64> = (0..120)
            .map(|i| 100.0 - (i as f64 * 0.2).sin() * 8.0)
            .collect();
        let bench: Vec<f64> = (0..120).map(|i| 100.0 + i as f64 * 0.1).collect();
        let q = QuantData::build(&[("A".into(), a), ("B".into(), b)], &bench, 252.0).unwrap();
        assert_eq!(q.corr.len(), 2);
        assert!((q.corr[0][0] - 1.0).abs() < 1e-9); // diagonal is 1
        assert!(q.corr[0][1] < 0.0); // anti-correlated
        assert_eq!(q.stats.len(), 2);
        assert!(!q.cloud.is_empty());
        assert_eq!(q.max_sharpe.weights.len(), 2);
        // long-only weights sum to ~1
        let wsum: f64 = q.max_sharpe.weights.iter().sum();
        assert!((wsum - 1.0).abs() < 1e-6);
        assert!(q.max_sharpe.weights.iter().all(|&w| w >= -1e-9));
    }
}
