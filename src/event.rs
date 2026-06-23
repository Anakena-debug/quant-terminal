//! Async event loop: multiplexes terminal input, a periodic tick, and render
//! frames onto a single channel the app consumes.

use std::time::Duration;

use ratatui::crossterm::event::{self, Event as CtEvent, KeyEvent, MouseEvent};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use crate::analytics::QuantData;
use crate::data::{AccountSummary, Candle, MarketEvent, Position, Timeframe};

/// Everything the main loop reacts to.
#[derive(Clone, Debug)]
pub enum AppEvent {
    /// Low-frequency logical tick (clock, housekeeping).
    Tick,
    /// Time to redraw.
    Render,
    Key(KeyEvent),
    Mouse(MouseEvent),
    /// A market-data update injected by the data manager.
    Market(MarketEvent),
    /// Historical bars resolved for a symbol/timeframe.
    History {
        symbol: String,
        timeframe: Timeframe,
        candles: Vec<Candle>,
    },
    /// Account positions resolved.
    Positions(Vec<Position>),
    /// Account summary resolved.
    Account(AccountSummary),
    /// Cross-sectional quant analytics computed.
    Quant(Box<QuantData>),
}

/// Owns the event channel and the background producers feeding it.
pub struct EventHandler {
    rx: UnboundedReceiver<AppEvent>,
    tx: UnboundedSender<AppEvent>,
}

impl EventHandler {
    pub fn new(tick_rate: Duration, frame_rate: Duration) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        spawn_input_thread(tx.clone());
        spawn_ticker(tx.clone(), tick_rate, AppEvent::Tick);
        spawn_ticker(tx.clone(), frame_rate, AppEvent::Render);
        Self { rx, tx }
    }

    /// A sender other subsystems (e.g. the market-data manager) can use to
    /// inject events into the loop.
    pub fn sender(&self) -> UnboundedSender<AppEvent> {
        self.tx.clone()
    }

    pub async fn next(&mut self) -> Option<AppEvent> {
        self.rx.recv().await
    }
}

fn spawn_ticker(tx: UnboundedSender<AppEvent>, period: Duration, ev: AppEvent) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(period);
        loop {
            interval.tick().await;
            if tx.send(ev.clone()).is_err() {
                break;
            }
        }
    });
}

/// crossterm reads are blocking, so run them on a dedicated OS thread and
/// forward decoded events into the async channel. The 100ms poll lets the
/// thread notice a closed channel and exit cleanly on shutdown.
fn spawn_input_thread(tx: UnboundedSender<AppEvent>) {
    std::thread::spawn(move || {
        loop {
            match event::poll(Duration::from_millis(100)) {
                Ok(true) => match event::read() {
                    Ok(CtEvent::Key(k)) => {
                        if tx.send(AppEvent::Key(k)).is_err() {
                            break;
                        }
                    }
                    Ok(CtEvent::Mouse(m)) => {
                        let _ = tx.send(AppEvent::Mouse(m));
                    }
                    Ok(CtEvent::Resize(_, _)) => {
                        // A redraw picks up the new size from `frame.area()`.
                        let _ = tx.send(AppEvent::Render);
                    }
                    Ok(_) => {}
                    Err(_) => break,
                },
                Ok(false) => {
                    if tx.is_closed() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
}
