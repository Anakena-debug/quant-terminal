//! A long-only backtester with a few classic strategies and an SMA
//! parameter-sweep. Each strategy produces an in-market boolean per bar; the
//! simulator turns that into an equity curve + trades.

use crate::analytics::{bollinger, rsi, sma};

#[derive(Clone, Copy, Debug)]
pub struct Trade {
    pub entry: f64,
    pub exit: f64,
}

impl Trade {
    pub fn is_win(&self) -> bool {
        self.exit > self.entry
    }
}

#[derive(Clone, Debug)]
pub struct BacktestResult {
    pub initial: f64,
    pub equity: Vec<f64>,
    pub buy_hold: Vec<f64>,
    pub trades: Vec<Trade>,
}

impl BacktestResult {
    pub fn final_equity(&self) -> f64 {
        *self.equity.last().unwrap_or(&self.initial)
    }
    pub fn total_return_pct(&self) -> f64 {
        (self.final_equity() / self.initial - 1.0) * 100.0
    }
    pub fn buy_hold_return_pct(&self) -> f64 {
        match (self.buy_hold.first(), self.buy_hold.last()) {
            (Some(&a), Some(&b)) if a.abs() > f64::EPSILON => (b / a - 1.0) * 100.0,
            _ => 0.0,
        }
    }
    pub fn max_drawdown_pct(&self) -> f64 {
        let mut peak = f64::MIN;
        let mut mdd: f64 = 0.0;
        for &e in &self.equity {
            peak = peak.max(e);
            if peak > 0.0 {
                mdd = mdd.max((peak - e) / peak);
            }
        }
        mdd * 100.0
    }
    pub fn sharpe(&self) -> f64 {
        if self.equity.len() < 3 {
            return 0.0;
        }
        let r: Vec<f64> = self
            .equity
            .windows(2)
            .map(|w| if w[0] > 0.0 { w[1] / w[0] - 1.0 } else { 0.0 })
            .collect();
        let n = r.len() as f64;
        let mean = r.iter().sum::<f64>() / n;
        let var = r.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n;
        let sd = var.sqrt();
        if sd < 1e-12 {
            0.0
        } else {
            mean / sd * (252.0_f64).sqrt()
        }
    }
    pub fn num_trades(&self) -> usize {
        self.trades.len()
    }
    pub fn win_rate_pct(&self) -> f64 {
        if self.trades.is_empty() {
            0.0
        } else {
            let wins = self.trades.iter().filter(|t| t.is_win()).count();
            wins as f64 / self.trades.len() as f64 * 100.0
        }
    }
}

/// The available strategies (preset parameters).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strategy {
    SmaCross,
    MeanReversion,
    Momentum,
    Bollinger,
    Rsi,
}

impl Strategy {
    pub const ALL: [Strategy; 5] = [
        Strategy::SmaCross,
        Strategy::MeanReversion,
        Strategy::Momentum,
        Strategy::Bollinger,
        Strategy::Rsi,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Strategy::SmaCross => "SMA 20/50 crossover",
            Strategy::MeanReversion => "Mean-Reversion z(20)",
            Strategy::Momentum => "Momentum 60d",
            Strategy::Bollinger => "Bollinger 20/2",
            Strategy::Rsi => "RSI 14 (30/70)",
        }
    }

    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|&s| s == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    /// In-market (long) flag per bar.
    pub fn signals(self, closes: &[f64]) -> Vec<bool> {
        let n = closes.len();
        let mut out = vec![false; n];
        match self {
            Strategy::SmaCross => out = sma_cross(closes, 20, 50),
            Strategy::Momentum => {
                let lb = 60;
                for i in lb..n {
                    out[i] = closes[i] > closes[i - lb];
                }
            }
            Strategy::MeanReversion => {
                let (mid, up, _lo) = bollinger(closes, 20, 1.0);
                let mut long = false;
                for i in 0..n {
                    if let (Some(m), Some(u)) = (mid[i], up[i]) {
                        let z = (closes[i] - m) / (u - m).max(1e-9);
                        if !long && z < -1.0 {
                            long = true;
                        } else if long && z > 0.0 {
                            long = false;
                        }
                    }
                    out[i] = long;
                }
            }
            Strategy::Bollinger => {
                let (mid, _up, lo) = bollinger(closes, 20, 2.0);
                let mut long = false;
                for i in 0..n {
                    if let (Some(m), Some(l)) = (mid[i], lo[i]) {
                        if !long && closes[i] < l {
                            long = true;
                        } else if long && closes[i] > m {
                            long = false;
                        }
                    }
                    out[i] = long;
                }
            }
            Strategy::Rsi => {
                let r = rsi(closes, 14);
                let mut long = false;
                for i in 0..n {
                    if let Some(v) = r[i] {
                        if !long && v < 30.0 {
                            long = true;
                        } else if long && v > 70.0 {
                            long = false;
                        }
                    }
                    out[i] = long;
                }
            }
        }
        out
    }
}

fn sma_cross(closes: &[f64], fast: usize, slow: usize) -> Vec<bool> {
    let f = sma(closes, fast);
    let s = sma(closes, slow);
    (0..closes.len())
        .map(|i| matches!((f[i], s[i]), (Some(a), Some(b)) if a > b))
        .collect()
}

/// Simulate an in-market signal into an equity curve.
pub fn run(closes: &[f64], in_market: &[bool], initial: f64) -> BacktestResult {
    let mut equity = Vec::with_capacity(closes.len());
    let mut buy_hold = Vec::with_capacity(closes.len());
    let mut trades = Vec::new();
    let bh_shares = closes.first().map(|&p| initial / p).unwrap_or(0.0);

    let mut cash = initial;
    let mut shares = 0.0;
    let mut long = false;
    let mut entry = 0.0;

    for (i, &price) in closes.iter().enumerate() {
        buy_hold.push(bh_shares * price);
        let want = in_market.get(i).copied().unwrap_or(false);
        if want && !long {
            shares = cash / price;
            entry = price;
            long = true;
        } else if !want && long {
            cash = shares * price;
            trades.push(Trade { entry, exit: price });
            shares = 0.0;
            long = false;
        }
        equity.push(if long { shares * price } else { cash });
    }
    if long && let Some(&price) = closes.last() {
        trades.push(Trade { entry, exit: price });
    }

    BacktestResult {
        initial,
        equity,
        buy_hold,
        trades,
    }
}

pub fn run_strategy(closes: &[f64], strategy: Strategy, initial: f64) -> BacktestResult {
    run(closes, &strategy.signals(closes), initial)
}

/// Grid of total-return% for SMA crossover over `fasts × slows`
/// (`NaN` where fast ≥ slow).
pub fn sma_sweep(closes: &[f64], fasts: &[usize], slows: &[usize], initial: f64) -> Vec<Vec<f64>> {
    fasts
        .iter()
        .map(|&f| {
            slows
                .iter()
                .map(|&s| {
                    if f >= s {
                        f64::NAN
                    } else {
                        run(closes, &sma_cross(closes, f, s), initial).total_return_pct()
                    }
                })
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_cross_enters_and_profits() {
        let closes: Vec<f64> = (0..220)
            .map(|i| {
                if i < 60 {
                    200.0 - i as f64 * 1.5
                } else {
                    110.0 + (i as f64 - 60.0) * 1.4
                }
            })
            .collect();
        let r = run_strategy(&closes, Strategy::SmaCross, 10_000.0);
        assert_eq!(r.equity.len(), closes.len());
        assert!(r.num_trades() >= 1, "a golden cross should trade");
        assert!(
            r.final_equity() > r.initial,
            "should profit riding the rise"
        );
        assert!((0.0..=100.0).contains(&r.win_rate_pct()));
    }

    #[test]
    fn flat_series_no_trades() {
        let closes = vec![100.0; 100];
        let r = run_strategy(&closes, Strategy::SmaCross, 1_000.0);
        assert_eq!(r.num_trades(), 0);
        assert!((r.final_equity() - 1_000.0).abs() < 1e-6);
    }

    #[test]
    fn every_strategy_runs() {
        let closes: Vec<f64> = (0..200)
            .map(|i| 100.0 + (i as f64 * 0.15).sin() * 12.0 + i as f64 * 0.1)
            .collect();
        for s in Strategy::ALL {
            let r = run_strategy(&closes, s, 10_000.0);
            assert_eq!(r.equity.len(), closes.len());
            assert!(r.final_equity().is_finite());
        }
    }

    #[test]
    fn sweep_shape() {
        let closes: Vec<f64> = (0..200)
            .map(|i| 100.0 + (i as f64 * 0.1).sin() * 8.0)
            .collect();
        let grid = sma_sweep(&closes, &[5, 10, 20], &[30, 50, 80], 10_000.0);
        assert_eq!(grid.len(), 3);
        assert_eq!(grid[0].len(), 3);
    }
}
