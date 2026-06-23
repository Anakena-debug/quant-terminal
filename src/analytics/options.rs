//! Black-Scholes option pricing, Greeks, an implied-vol solver, and a synthetic
//! option chain. Pure math — no I/O — so it's trivially testable and works on
//! any symbol/spot.
//!
//! Greeks are in "raw" units (vega per 1.00 vol, theta per year); the view
//! scales them to trader conventions (vega per 1%, theta per day).

use std::f64::consts::{PI, SQRT_2};

/// Abramowitz–Stegun 7.1.26 error-function approximation (|err| < 1.5e-7).
fn erf(x: f64) -> f64 {
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs();
    let t = 1.0 / (1.0 + 0.327_591_1 * x);
    let y = 1.0
        - (((((1.061_405_429 * t - 1.453_152_027) * t) + 1.421_413_741) * t - 0.284_496_736) * t
            + 0.254_829_592)
            * t
            * (-x * x).exp();
    sign * y
}

pub fn norm_cdf(x: f64) -> f64 {
    0.5 * (1.0 + erf(x / SQRT_2))
}

pub fn norm_pdf(x: f64) -> f64 {
    (-0.5 * x * x).exp() / (2.0 * PI).sqrt()
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Greeks {
    pub price: f64,
    pub delta: f64,
    pub gamma: f64,
    pub vega: f64,
    pub theta: f64,
}

/// Black-Scholes price + Greeks for a European option.
pub fn black_scholes(spot: f64, strike: f64, t: f64, r: f64, vol: f64, is_call: bool) -> Greeks {
    if t <= 0.0 || vol <= 0.0 || spot <= 0.0 || strike <= 0.0 {
        let intrinsic = if is_call {
            (spot - strike).max(0.0)
        } else {
            (strike - spot).max(0.0)
        };
        let delta = if is_call {
            if spot > strike { 1.0 } else { 0.0 }
        } else if spot < strike {
            -1.0
        } else {
            0.0
        };
        return Greeks {
            price: intrinsic,
            delta,
            ..Default::default()
        };
    }

    let sqrt_t = t.sqrt();
    let d1 = ((spot / strike).ln() + (r + 0.5 * vol * vol) * t) / (vol * sqrt_t);
    let d2 = d1 - vol * sqrt_t;
    let pdf_d1 = norm_pdf(d1);
    let disc = (-r * t).exp();
    let gamma = pdf_d1 / (spot * vol * sqrt_t);
    let vega = spot * pdf_d1 * sqrt_t;

    if is_call {
        let nd1 = norm_cdf(d1);
        let nd2 = norm_cdf(d2);
        Greeks {
            price: spot * nd1 - strike * disc * nd2,
            delta: nd1,
            gamma,
            vega,
            theta: -(spot * pdf_d1 * vol) / (2.0 * sqrt_t) - r * strike * disc * nd2,
        }
    } else {
        let nnd1 = norm_cdf(-d1);
        let nnd2 = norm_cdf(-d2);
        Greeks {
            price: strike * disc * nnd2 - spot * nnd1,
            delta: norm_cdf(d1) - 1.0,
            gamma,
            vega,
            theta: -(spot * pdf_d1 * vol) / (2.0 * sqrt_t) + r * strike * disc * nnd2,
        }
    }
}

/// Implied volatility from a market price (Newton-Raphson, bisection fallback).
pub fn implied_vol(
    price: f64,
    spot: f64,
    strike: f64,
    t: f64,
    r: f64,
    is_call: bool,
) -> Option<f64> {
    if price <= 0.0 || t <= 0.0 {
        return None;
    }
    let mut vol = 0.25;
    for _ in 0..100 {
        let g = black_scholes(spot, strike, t, r, vol, is_call);
        let diff = g.price - price;
        if diff.abs() < 1e-7 {
            return Some(vol);
        }
        if g.vega < 1e-10 {
            break;
        }
        vol -= diff / g.vega;
        if !vol.is_finite() || vol <= 0.0 {
            vol = 1e-4;
        }
        if vol > 5.0 {
            vol = 5.0;
        }
    }
    // bisection fallback on [1e-4, 5]
    let price_at = |v: f64| black_scholes(spot, strike, t, r, v, is_call).price;
    let (mut lo, mut hi) = (1e-4, 5.0);
    let (flo, fhi) = (price_at(lo) - price, price_at(hi) - price);
    if flo.signum() == fhi.signum() {
        return None;
    }
    for _ in 0..100 {
        let mid = 0.5 * (lo + hi);
        let fm = price_at(mid) - price;
        if fm.abs() < 1e-7 {
            return Some(mid);
        }
        if (price_at(lo) - price).signum() == fm.signum() {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Some(0.5 * (lo + hi))
}

#[derive(Clone, Debug)]
pub struct OptionRow {
    pub strike: f64,
    pub call: Greeks,
    pub put: Greeks,
}

#[derive(Clone, Debug)]
pub struct OptionChain {
    pub spot: f64,
    pub days: u32,
    pub rate: f64,
    pub atm_iv: f64,
    pub rows: Vec<OptionRow>,
}

fn nice_step(spot: f64) -> f64 {
    if spot >= 500.0 {
        10.0
    } else if spot >= 100.0 {
        5.0
    } else if spot >= 25.0 {
        2.5
    } else {
        1.0
    }
}

/// Build a synthetic chain around `spot` with an equity smile (convex + put
/// skew) anchored at `base_iv`. Prices/greeks are Black-Scholes-consistent.
pub fn synthetic_chain(spot: f64, days: u32, rate: f64, base_iv: f64) -> OptionChain {
    let t = days as f64 / 365.0;
    let step = nice_step(spot);
    let atm = (spot / step).round() * step;
    let mut rows = Vec::new();
    for k in -8..=8 {
        let strike = atm + k as f64 * step;
        if strike <= 0.0 {
            continue;
        }
        let m = (strike / spot).ln();
        let iv = (base_iv + 1.4 * m * m - 0.25 * m).max(0.02);
        rows.push(OptionRow {
            strike,
            call: black_scholes(spot, strike, t, rate, iv, true),
            put: black_scholes(spot, strike, t, rate, iv, false),
        });
    }
    OptionChain {
        spot,
        days,
        rate,
        atm_iv: base_iv,
        rows,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn norm_cdf_known_points() {
        assert!((norm_cdf(0.0) - 0.5).abs() < 1e-6);
        assert!((norm_cdf(1.96) - 0.975).abs() < 1e-3);
        assert!((norm_cdf(-1.96) - 0.025).abs() < 1e-3);
    }

    #[test]
    fn atm_call_price_matches_reference() {
        // S=100, K=100, t=1, r=5%, vol=20% → call ≈ 10.4506
        let g = black_scholes(100.0, 100.0, 1.0, 0.05, 0.20, true);
        assert!((g.price - 10.4506).abs() < 1e-3, "got {}", g.price);
        assert!(g.delta > 0.0 && g.delta < 1.0);
        assert!(g.gamma > 0.0 && g.vega > 0.0);
    }

    #[test]
    fn put_call_parity() {
        let (s, k, t, r, v) = (100.0, 110.0, 0.5, 0.04, 0.3);
        let c = black_scholes(s, k, t, r, v, true).price;
        let p = black_scholes(s, k, t, r, v, false).price;
        // C - P = S - K e^{-rt}
        let lhs = c - p;
        let rhs = s - k * (-r * t).exp();
        assert!((lhs - rhs).abs() < 1e-6);
    }

    #[test]
    fn put_delta_is_negative() {
        let g = black_scholes(100.0, 100.0, 1.0, 0.05, 0.2, false);
        assert!(g.delta < 0.0 && g.delta > -1.0);
    }

    #[test]
    fn implied_vol_round_trips() {
        let (s, k, t, r, true_vol) = (100.0, 95.0, 0.75, 0.03, 0.32);
        let price = black_scholes(s, k, t, r, true_vol, true).price;
        let iv = implied_vol(price, s, k, t, r, true).unwrap();
        assert!((iv - true_vol).abs() < 1e-4, "got {iv}");
    }

    #[test]
    fn chain_has_smile() {
        let chain = synthetic_chain(300.0, 30, 0.045, 0.28);
        assert!(!chain.rows.is_empty());
        let t = chain.days as f64 / 365.0;
        let iv_of = |r: &OptionRow| {
            implied_vol(r.call.price, chain.spot, r.strike, t, chain.rate, true).unwrap()
        };
        let atm = chain
            .rows
            .iter()
            .min_by(|a, b| {
                (a.strike - 300.0)
                    .abs()
                    .partial_cmp(&(b.strike - 300.0).abs())
                    .unwrap()
            })
            .unwrap();
        // equity skew: recovered IV at the low strike exceeds ATM (round-trips
        // through the implied-vol solver)
        let low_wing = chain.rows.first().unwrap();
        assert!(iv_of(low_wing) > iv_of(atm), "expected a downside put skew");
    }
}
