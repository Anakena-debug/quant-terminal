//! Market-data abstraction.
//!
//! IBKR is the primary provider (M5); the simulated provider keeps the whole
//! app runnable with zero external setup. Everything downstream depends only on
//! the [`DataProvider`] trait, so swapping the source is a one-line change.

pub mod ibkr;
pub mod manager;
pub mod simulated;
pub mod types;

use async_trait::async_trait;
use color_eyre::Result;
use futures::stream::BoxStream;

pub use ibkr::IbkrProvider;
pub use manager::MarketDataManager;
pub use simulated::SimulatedProvider;
pub use types::*;

#[async_trait]
pub trait DataProvider: Send + Sync {
    /// Human-readable provider name (for logs/UI).
    fn name(&self) -> &'static str;

    /// Current connection / data-quality state.
    fn status(&self) -> ConnectionStatus;

    /// Historical OHLCV bars for a symbol over a timeframe.
    async fn history(&self, symbol: &str, timeframe: Timeframe) -> Result<Vec<Candle>>;

    /// A continuous stream of market events for the given symbols.
    fn subscribe(&self, symbols: Vec<String>) -> BoxStream<'static, MarketEvent>;

    /// Open account positions (with a recent `last` price filled in).
    async fn positions(&self) -> Result<Vec<Position>>;

    /// Account-level summary figures.
    async fn account_summary(&self) -> Result<AccountSummary>;
}
