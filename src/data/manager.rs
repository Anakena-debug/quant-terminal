//! Owns the active provider and pumps its event stream into the app's channel.

use std::sync::Arc;

use futures::StreamExt;
use tokio::sync::mpsc::UnboundedSender;

use super::{ConnectionStatus, DataProvider, MarketEvent};
use crate::event::AppEvent;

pub struct MarketDataManager;

impl MarketDataManager {
    /// Spawn a background task that forwards the provider's market events into
    /// the event loop as [`AppEvent::Market`]. Returns immediately.
    pub fn spawn(
        provider: Arc<dyn DataProvider>,
        symbols: Vec<String>,
        tx: UnboundedSender<AppEvent>,
    ) {
        let _ = tx.send(AppEvent::Market(MarketEvent::Status(provider.status())));
        tokio::spawn(async move {
            let mut stream = provider.subscribe(symbols);
            while let Some(ev) = stream.next().await {
                if tx.send(AppEvent::Market(ev)).is_err() {
                    return; // app gone
                }
            }
            // The feed ended (e.g. IBKR streams all closed).
            let _ = tx.send(AppEvent::Market(MarketEvent::Status(
                ConnectionStatus::Disconnected,
            )));
        });
    }
}
