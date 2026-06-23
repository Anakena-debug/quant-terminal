//! Interactive Brokers provider via the `ibapi` crate (TWS / IB Gateway).
//!
//! Requires a running IB Gateway or TWS with the API enabled (paper port 4002
//! or TWS 7497). History uses `historical_data`; the live feed uses 5-second
//! `realtime_bars`, each bar mapped to a [`Quote`]. Per-symbol realtime streams
//! borrow the client, so each runs inside its own task that owns an
//! `Arc<Client>` clone and forwards owned events over a channel — giving the
//! `'static` stream the [`DataProvider`] trait requires.
//!
//! NOTE: this path is verified to compile against ibapi 3.1; live behaviour
//! depends on your IBKR market-data subscriptions. The app falls back to the
//! simulated provider if the gateway is unreachable (see `app.rs`).

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use color_eyre::Result;
use color_eyre::eyre::eyre;
use futures::stream::{self, BoxStream};
use ibapi::market_data::historical::{
    BarSize as HBarSize, BarTimestamp, Duration as HDuration, WhatToShow as HWhat,
};
use ibapi::prelude::*;
use tokio::sync::mpsc;

use super::DataProvider;
use super::types::*;

pub struct IbkrProvider {
    client: Arc<Client>,
    status: ConnectionStatus,
}

impl IbkrProvider {
    /// Connect to IB Gateway / TWS. Returns an error if the gateway is
    /// unreachable so the caller can fall back to the simulated provider.
    pub async fn connect(host: &str, port: u16, client_id: i32, delayed: bool) -> Result<Self> {
        let address = format!("{host}:{port}");
        let client = Client::connect(&address, client_id)
            .await
            .map_err(|e| eyre!("IBKR connect to {address} failed: {e}"))?;
        Ok(Self {
            client: Arc::new(client),
            status: if delayed {
                ConnectionStatus::Delayed
            } else {
                ConnectionStatus::Live
            },
        })
    }

    /// Most recent daily close for a symbol, if available.
    async fn last_close(&self, symbol: &str) -> Option<f64> {
        let contract = Contract::stock(symbol).build();
        let data = self
            .client
            .historical_data(&contract, HBarSize::Day)
            .what_to_show(HWhat::Trades)
            .duration(HDuration::days(5))
            .fetch()
            .await
            .ok()?;
        data.bars.last().map(|b| b.close)
    }
}

/// Map our timeframe to an IBKR `(bar size, duration)`.
fn map_timeframe(tf: Timeframe) -> (HBarSize, HDuration) {
    match tf {
        Timeframe::D1 => (HBarSize::Min5, HDuration::days(1)),
        Timeframe::D5 => (HBarSize::Min30, HDuration::days(5)),
        Timeframe::M1 => (HBarSize::Day, HDuration::months(1)),
        Timeframe::M6 => (HBarSize::Day, HDuration::months(6)),
        Timeframe::Y1 => (HBarSize::Day, HDuration::years(1)),
    }
}

/// IBKR bar timestamp → unix seconds.
fn bar_time(ts: &BarTimestamp) -> i64 {
    match ts {
        BarTimestamp::DateTime(dt) => dt.unix_timestamp(),
        BarTimestamp::Date(d) => d.midnight().assume_utc().unix_timestamp(),
    }
}

#[async_trait]
impl DataProvider for IbkrProvider {
    fn name(&self) -> &'static str {
        "ibkr"
    }

    fn status(&self) -> ConnectionStatus {
        self.status
    }

    async fn history(&self, symbol: &str, timeframe: Timeframe) -> Result<Vec<Candle>> {
        let contract = Contract::stock(symbol).build();
        let (bar_size, duration) = map_timeframe(timeframe);
        let data = self
            .client
            .historical_data(&contract, bar_size)
            .what_to_show(HWhat::Trades)
            .duration(duration)
            .fetch()
            .await
            .map_err(|e| eyre!("IBKR historical_data for {symbol} failed: {e}"))?;

        let candles = data
            .bars
            .iter()
            .map(|b| Candle {
                time: bar_time(&b.date),
                open: b.open,
                high: b.high,
                low: b.low,
                close: b.close,
                volume: b.volume.max(0.0) as u64,
            })
            .collect();
        Ok(candles)
    }

    fn subscribe(&self, symbols: Vec<String>) -> BoxStream<'static, MarketEvent> {
        let (tx, rx) = mpsc::unbounded_channel::<MarketEvent>();

        for symbol in symbols {
            let client = self.client.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                let contract = Contract::stock(symbol.as_str()).build();

                // Seed a quote from recent daily bars so the watchlist populates
                // immediately with the correct change% vs prior close, even
                // outside market hours.
                let mut prev_close = 0.0_f64;
                if let Ok(data) = client
                    .historical_data(&contract, HBarSize::Day)
                    .what_to_show(HWhat::Trades)
                    .duration(HDuration::days(5))
                    .fetch()
                    .await
                    && let Some(last) = data.bars.last()
                {
                    prev_close = if data.bars.len() >= 2 {
                        data.bars[data.bars.len() - 2].close
                    } else {
                        last.open
                    };
                    let spread = (last.close * 0.0002).max(0.01);
                    let _ = tx.send(MarketEvent::Quote(Quote {
                        symbol: symbol.clone(),
                        last: last.close,
                        prev_close,
                        bid: last.close - spread,
                        ask: last.close + spread,
                        bid_size: 0,
                        ask_size: 0,
                        open: last.open,
                        high: last.high,
                        low: last.low,
                        volume: last.volume.max(0.0) as u64,
                    }));
                }

                // Stream live 5-second bars (active during market hours).
                let subscription = match client.realtime_bars(&contract).subscribe().await {
                    Ok(sub) => sub,
                    Err(e) => {
                        tracing::error!(%symbol, error = ?e, "IBKR realtime_bars failed");
                        return;
                    }
                };

                // Accumulate session open/high/low/volume across 5s bars.
                let mut session_open: Option<f64> = None;
                let mut high = f64::MIN;
                let mut low = f64::MAX;
                let mut volume: u64 = 0;

                let mut stream = subscription.filter_data();
                while let Some(item) = stream.next().await {
                    let bar = match item {
                        Ok(bar) => bar,
                        Err(e) => {
                            tracing::warn!(%symbol, error = ?e, "IBKR realtime bar error");
                            continue;
                        }
                    };
                    let open = *session_open.get_or_insert(bar.open);
                    high = high.max(bar.high);
                    low = low.min(bar.low);
                    volume = volume.saturating_add(bar.volume.max(0.0) as u64);

                    let baseline = if prev_close > 0.0 { prev_close } else { open };
                    let spread = (bar.close * 0.0002).max(0.01);
                    let quote = Quote {
                        symbol: symbol.clone(),
                        last: bar.close,
                        prev_close: baseline,
                        bid: bar.close - spread,
                        ask: bar.close + spread,
                        bid_size: 0,
                        ask_size: 0,
                        open,
                        high,
                        low,
                        volume,
                    };
                    if tx.send(MarketEvent::Quote(quote)).is_err() {
                        break; // app gone
                    }
                }
            });
        }

        // Bridge the channel into a `'static` stream.
        let s = stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|e| (e, rx)) });
        Box::pin(s)
    }

    async fn positions(&self) -> Result<Vec<Position>> {
        let subscription = self
            .client
            .positions()
            .await
            .map_err(|e| eyre!("IBKR positions failed: {e}"))?;
        let mut sub = subscription.filter_data();

        let mut raw: Vec<(String, f64, f64)> = Vec::new();
        while let Some(item) = sub.next().await {
            match item {
                Ok(PositionUpdate::Position(p)) => {
                    if p.position != 0.0 {
                        raw.push((p.contract.symbol.to_string(), p.position, p.average_cost));
                    }
                }
                Ok(PositionUpdate::PositionEnd) => break,
                Err(e) => {
                    tracing::warn!(error = ?e, "IBKR positions error");
                    break;
                }
            }
        }

        // Fill a recent last price per symbol (daily close).
        let mut out = Vec::with_capacity(raw.len());
        for (symbol, qty, avg_cost) in raw {
            let last = self.last_close(&symbol).await.unwrap_or(avg_cost);
            out.push(Position {
                symbol,
                qty,
                avg_cost,
                last,
            });
        }
        Ok(out)
    }

    async fn account_summary(&self) -> Result<AccountSummary> {
        use ibapi::accounts::types::AccountGroup;

        let tags = &[
            AccountSummaryTags::NET_LIQUIDATION,
            AccountSummaryTags::TOTAL_CASH_VALUE,
            AccountSummaryTags::BUYING_POWER,
        ];
        let subscription = self
            .client
            .account_summary(&AccountGroup("All".to_string()), tags)
            .await
            .map_err(|e| eyre!("IBKR account_summary failed: {e}"))?;
        let mut sub = subscription.filter_data();

        let mut summary = AccountSummary::default();
        while let Some(item) = sub.next().await {
            match item {
                Ok(AccountSummaryResult::Summary(s)) => {
                    if summary.account.is_empty() {
                        summary.account = s.account.clone();
                    }
                    let value = s.value.parse::<f64>().unwrap_or(0.0);
                    match s.tag.as_str() {
                        t if t == AccountSummaryTags::NET_LIQUIDATION => {
                            summary.net_liquidation = value
                        }
                        t if t == AccountSummaryTags::TOTAL_CASH_VALUE => {
                            summary.total_cash = value
                        }
                        t if t == AccountSummaryTags::BUYING_POWER => summary.buying_power = value,
                        _ => {}
                    }
                }
                Ok(AccountSummaryResult::End) => break,
                Err(e) => {
                    tracing::warn!(error = ?e, "IBKR account_summary error");
                    break;
                }
            }
        }
        Ok(summary)
    }
}

/// Headless connectivity check: connect to TWS/Gateway, pull AAPL history, and
/// try a few live bars. Returns a human-readable report. **Read-only — places
/// no orders.**
pub async fn diagnostic(address: &str, client_id: i32) -> Result<String> {
    use std::fmt::Write as _;

    let mut out = String::new();
    let _ = writeln!(
        out,
        "IBKR check → {address}  (client_id {client_id}, read-only)\n"
    );

    let client = match tokio::time::timeout(
        Duration::from_secs(20),
        Client::connect(address, client_id),
    )
    .await
    {
        Ok(Ok(client)) => {
            let _ = writeln!(out, "  ✓ connected");
            client
        }
        Ok(Err(e)) => {
            let _ = writeln!(out, "  ✗ connect error: {e}");
            return Ok(out);
        }
        Err(_) => {
            let _ = writeln!(
                out,
                "  ✗ connect timed out — in TWS check: API enabled, 127.0.0.1 trusted, and \n    accept any 'incoming connection' popup, then retry"
            );
            return Ok(out);
        }
    };

    let contract = Contract::stock("AAPL").build();

    let _ = writeln!(out, "\n  Historical daily bars (AAPL, 1 month):");
    let hist = client
        .historical_data(&contract, HBarSize::Day)
        .what_to_show(HWhat::Trades)
        .duration(HDuration::months(1))
        .fetch();
    match tokio::time::timeout(Duration::from_secs(20), hist).await {
        Ok(Ok(data)) => {
            let _ = writeln!(out, "    ✓ {} bars received", data.bars.len());
            for b in data.bars.iter().rev().take(3).rev() {
                let _ = writeln!(
                    out,
                    "      {:?}   O {:.2}  H {:.2}  L {:.2}  C {:.2}  V {:.0}",
                    b.date, b.open, b.high, b.low, b.close, b.volume
                );
            }
        }
        Ok(Err(e)) => {
            let _ = writeln!(out, "    ✗ error: {e}");
        }
        Err(_) => {
            let _ = writeln!(out, "    ✗ timed out");
        }
    }

    let _ = writeln!(out, "\n  Real-time 5s bars (AAPL, up to ~10s):");
    match client.realtime_bars(&contract).subscribe().await {
        Ok(sub) => {
            let mut stream = sub.filter_data();
            let mut got = 0u32;
            loop {
                match tokio::time::timeout(Duration::from_secs(10), stream.next()).await {
                    Ok(Some(Ok(bar))) => {
                        let _ = writeln!(
                            out,
                            "      {}   O {:.2}  H {:.2}  L {:.2}  C {:.2}  V {:.0}",
                            bar.date, bar.open, bar.high, bar.low, bar.close, bar.volume
                        );
                        got += 1;
                        if got >= 2 {
                            break;
                        }
                    }
                    Ok(Some(Err(e))) => {
                        let _ = writeln!(out, "      ✗ bar error: {e}");
                        break;
                    }
                    Ok(None) => {
                        let _ = writeln!(out, "      stream ended");
                        break;
                    }
                    Err(_) => {
                        let _ = writeln!(
                            out,
                            "      (no bars in window — market likely closed; history above confirms the feed)"
                        );
                        break;
                    }
                }
            }
        }
        Err(e) => {
            let _ = writeln!(out, "      ✗ subscribe error: {e}");
        }
    }

    client.disconnect().await;
    let _ = writeln!(out, "\nDone.");
    Ok(out)
}
