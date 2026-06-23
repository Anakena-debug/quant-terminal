//! A self-contained simulated market: deterministic random-walk quotes and
//! synthetic OHLC history. No network, no API keys — the app is fully alive on
//! `cargo run`, and the real IBKR feed (M5) slots in behind the same trait.

use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use color_eyre::Result;
use futures::stream::{self, BoxStream};

use super::DataProvider;
use super::types::*;

/// Tiny deterministic PRNG (xorshift64) — reproducible without pulling in the
/// distribution machinery; perfectly adequate for a price simulator.
#[derive(Clone)]
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self((seed ^ 0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Uniform in `[0, 1)`.
    fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform in `[lo, hi)`.
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }

    /// Approx standard normal (Irwin–Hall, n=6 → mean 0, std ≈ 0.707).
    fn normal(&mut self) -> f64 {
        (0..6).map(|_| self.unit()).sum::<f64>() - 3.0
    }
}

fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |a, b| {
        (a ^ b as u64).wrapping_mul(0x0000_0100_0000_01B3)
    })
}

/// Salt for the shared market factor (so history is correlated across symbols).
const MARKET_SALT: u64 = 0x4D41_524B_4554_0001;

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// A plausible starting price: a few well-known names, otherwise derived from
/// the symbol text so it's stable across runs.
fn seed_price(symbol: &str) -> f64 {
    match symbol {
        "AAPL" => 228.5,
        "MSFT" => 421.0,
        "NVDA" => 131.2,
        "TSLA" => 248.9,
        "AMZN" => 197.4,
        "GOOGL" => 174.3,
        "META" => 563.7,
        "SPY" => 571.3,
        "QQQ" => 489.6,
        "BTC" => 67_204.0,
        other => 20.0 + (fnv(other) % 580) as f64,
    }
}

/// Per-symbol short-term volatility knob.
fn vol_of(symbol: &str) -> f64 {
    match symbol {
        "BTC" => 0.040,
        "NVDA" | "TSLA" => 0.022,
        "SPY" | "QQQ" => 0.008,
        _ => 0.014,
    }
}

pub struct SimulatedProvider {
    seed: u64,
}

impl SimulatedProvider {
    pub fn new(seed: u64) -> Self {
        Self { seed }
    }
}

/// Per-symbol live state mutated by the streaming task.
struct SymState {
    symbol: String,
    last: f64,
    prev_close: f64,
    open: f64,
    high: f64,
    low: f64,
    volume: u64,
    vol: f64,
    drift: f64,
}

impl SymState {
    fn new(symbol: String) -> Self {
        let p = seed_price(&symbol);
        // Stable per-symbol drift bias so some names trend up, others down.
        let drift = ((fnv(&symbol) % 1000) as f64 / 1000.0 - 0.5) * 0.00010;
        Self {
            vol: vol_of(&symbol),
            last: p,
            prev_close: p,
            open: p,
            high: p,
            low: p,
            volume: 0,
            drift,
            symbol,
        }
    }

    fn to_quote(&self) -> Quote {
        let spread = (self.last * 0.0002).max(0.01);
        Quote {
            symbol: self.symbol.clone(),
            last: round2(self.last),
            prev_close: round2(self.prev_close),
            bid: round2(self.last - spread),
            ask: round2(self.last + spread),
            bid_size: 1 + (self.volume / 1000 % 900) as u32,
            ask_size: 1 + (self.volume / 1300 % 900) as u32,
            open: round2(self.open),
            high: round2(self.high),
            low: round2(self.low),
            volume: self.volume,
        }
    }
}

struct StreamState {
    rng: Rng,
    syms: Vec<SymState>,
    cursor: usize,
}

#[async_trait]
impl DataProvider for SimulatedProvider {
    fn name(&self) -> &'static str {
        "simulated"
    }

    fn status(&self) -> ConnectionStatus {
        ConnectionStatus::Simulated
    }

    async fn history(&self, symbol: &str, timeframe: Timeframe) -> Result<Vec<Candle>> {
        let mut rng = Rng::new(self.seed ^ fnv(symbol) ^ (timeframe as u64).wrapping_mul(0x100));
        let n = timeframe.bar_count();
        let bar_secs = timeframe.bar_seconds();
        let now = Utc::now().timestamp();
        let vol = vol_of(symbol);

        // Market beta drives co-movement across symbols via a shared factor.
        let beta = 0.6 + (fnv(symbol) % 110) as f64 / 100.0;
        let mut price = seed_price(symbol) * rng.range(0.82, 1.0);
        let mut candles = Vec::with_capacity(n);
        for i in 0..n {
            // Shared market return at bar i (symbol-independent → correlation).
            let market_ret = {
                let mut m = Rng::new(
                    self.seed ^ MARKET_SALT ^ (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15),
                );
                m.normal() * 0.008
            };
            let ret = beta * market_ret + rng.normal() * vol + 0.0004;
            let open = price;
            let close = (price * (1.0 + ret)).max(0.01);
            let high = open.max(close) * (1.0 + rng.range(0.0, vol * 0.5));
            let low = (open.min(close) * (1.0 - rng.range(0.0, vol * 0.5))).max(0.01);
            let volume = (rng.range(0.5, 1.6) * 1_000_000.0) as u64;
            let time = now - bar_secs * (n - i) as i64;
            candles.push(Candle {
                time,
                open: round2(open),
                high: round2(high),
                low: round2(low),
                close: round2(close),
                volume,
            });
            price = close;
        }
        Ok(candles)
    }

    fn subscribe(&self, symbols: Vec<String>) -> BoxStream<'static, MarketEvent> {
        let syms = symbols.into_iter().map(SymState::new).collect();
        let state = StreamState {
            rng: Rng::new(self.seed.wrapping_mul(0x2545_F491_4F6C_DD1D)),
            syms,
            cursor: 0,
        };

        let s = stream::unfold(state, |mut st| async move {
            tokio::time::sleep(Duration::from_millis(110)).await;
            if st.syms.is_empty() {
                return None;
            }
            // Tick one symbol per step, round-robin, for a steady stream.
            let i = st.cursor % st.syms.len();
            st.cursor = st.cursor.wrapping_add(1);

            // Draw randomness first to avoid overlapping borrows of `st`.
            let shock_n = st.rng.normal();
            let vol_add = st.rng.range(80.0, 4200.0) as u64;

            let sym = &mut st.syms[i];
            let revert = (sym.prev_close - sym.last) * 0.003;
            sym.last =
                (sym.last + (shock_n * sym.vol * 0.05 + sym.drift) * sym.last + revert).max(0.01);
            sym.high = sym.high.max(sym.last);
            sym.low = sym.low.min(sym.last);
            sym.volume += vol_add;
            let quote = sym.to_quote();

            Some((MarketEvent::Quote(quote), st))
        });

        Box::pin(s)
    }

    async fn positions(&self) -> Result<Vec<Position>> {
        // A demo book: some longs and one short.
        let holdings = [
            ("AAPL", 100.0, 212.40),
            ("NVDA", 60.0, 118.75),
            ("MSFT", 40.0, 395.10),
            ("META", 15.0, 502.60),
            ("TSLA", -25.0, 268.30),
            ("SPY", 150.0, 548.90),
        ];
        let positions = holdings
            .into_iter()
            .map(|(symbol, qty, avg_cost)| Position {
                symbol: symbol.to_string(),
                qty,
                avg_cost,
                last: round2(seed_price(symbol)),
            })
            .collect();
        Ok(positions)
    }

    async fn account_summary(&self) -> Result<AccountSummary> {
        let positions = self.positions().await?;
        let market_value: f64 = positions.iter().map(|p| p.market_value()).sum();
        let cash = 50_000.0;
        let net_liq = cash + market_value;
        Ok(AccountSummary {
            account: "DU-SIM".to_string(),
            net_liquidation: net_liq,
            total_cash: cash,
            buying_power: net_liq * 2.0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    #[tokio::test]
    async fn history_is_finite_ordered_and_sized() {
        let provider = SimulatedProvider::new(42);
        for tf in Timeframe::ALL {
            let bars = provider.history("AAPL", tf).await.unwrap();
            assert_eq!(bars.len(), tf.bar_count(), "bar count for {}", tf.label());
            for c in &bars {
                assert!(c.open.is_finite() && c.close.is_finite());
                assert!(c.high >= c.low, "high < low");
                assert!(c.high + 1e-9 >= c.open.max(c.close));
                assert!(c.low - 1e-9 <= c.open.min(c.close));
                assert!(c.close > 0.0);
            }
        }
    }

    #[tokio::test]
    async fn stream_yields_finite_quotes() {
        let provider = SimulatedProvider::new(7);
        let mut stream = provider.subscribe(vec!["AAPL".into(), "BTC".into()]);
        for _ in 0..5 {
            let MarketEvent::Quote(q) = stream.next().await.unwrap() else {
                panic!("expected a quote");
            };
            assert!(q.last.is_finite() && q.last > 0.0);
            assert!(q.ask >= q.bid);
        }
    }

    #[test]
    fn quote_change_math() {
        let q = Quote {
            symbol: "X".into(),
            last: 101.0,
            prev_close: 100.0,
            bid: 100.9,
            ask: 101.1,
            bid_size: 1,
            ask_size: 1,
            open: 100.0,
            high: 102.0,
            low: 99.0,
            volume: 10,
        };
        assert!((q.change() - 1.0).abs() < 1e-9);
        assert!((q.change_pct() - 1.0).abs() < 1e-9);
        assert!(q.spread() > 0.0);
    }
}
