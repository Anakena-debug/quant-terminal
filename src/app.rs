//! Application state and the main run loop.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use color_eyre::Result;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind,
};
use tokio::sync::mpsc::UnboundedSender;

use crate::action::Action;
use crate::analytics::{ChartSeries, QuantData};
use crate::backtest::Strategy;
use crate::command::{self, Command};
use crate::config::{Config, ProviderKind};
use crate::data::{
    AccountSummary, Candle, ConnectionStatus, DataProvider, IbkrProvider, MarketDataManager,
    MarketEvent, Position, Quote, SimulatedProvider, Timeframe,
};
use crate::event::{AppEvent, EventHandler};
use crate::{ui, update};

/// How many recent prices to retain per symbol for the watchlist sparkline.
const HISTORY_CAP: usize = 64;

/// Which pane currently has keyboard focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
    Watchlist,
    Chart,
    Command,
}

impl Panel {
    pub fn next(self) -> Self {
        match self {
            Panel::Watchlist => Panel::Chart,
            Panel::Chart => Panel::Command,
            Panel::Command => Panel::Watchlist,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Panel::Watchlist => Panel::Command,
            Panel::Chart => Panel::Watchlist,
            Panel::Command => Panel::Chart,
        }
    }
}

/// Price-chart presentation style.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChartType {
    Candle,
    Line,
}

impl ChartType {
    pub fn label(self) -> &'static str {
        match self {
            ChartType::Candle => "candles",
            ChartType::Line => "line",
        }
    }

    pub fn toggle(self) -> Self {
        match self {
            ChartType::Candle => ChartType::Line,
            ChartType::Line => ChartType::Candle,
        }
    }
}

/// Top-level screen (Bloomberg-style function screens).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Monitor,
    Portfolio,
    Backtest,
    Risk,
    Optimizer,
    Options,
}

impl View {
    pub fn label(self) -> &'static str {
        match self {
            View::Monitor => "MONITOR",
            View::Portfolio => "PORTFOLIO",
            View::Backtest => "BACKTEST",
            View::Risk => "RISK",
            View::Optimizer => "OPTIMIZER",
            View::Options => "OPTIONS",
        }
    }
}

pub struct App {
    pub running: bool,
    pub focus: Panel,
    pub show_help: bool,
    pub command_input: String,
    /// Transient status/result message shown in the command bar.
    pub message: Option<String>,
    pub watchlist: Vec<String>,
    pub selected: usize,
    pub conn: ConnectionStatus,
    /// Latest quote per symbol.
    pub quotes: HashMap<String, Quote>,
    /// Recent last-prices per symbol (newest at the back) for sparklines.
    pub history: HashMap<String, VecDeque<f64>>,
    pub timeframe: Timeframe,
    pub chart_type: ChartType,
    /// Selected backtest strategy.
    pub strategy: Strategy,
    /// Loaded chart series for the active symbol/timeframe.
    pub series: Option<ChartSeries>,
    /// Active top-level screen.
    pub view: View,
    /// Account positions (portfolio view).
    pub positions: Vec<Position>,
    /// Account summary (portfolio view).
    pub account: Option<AccountSummary>,
    /// Selected row in the positions table.
    pub pos_selected: usize,
    portfolio_loaded: bool,
    /// Cross-sectional analytics (risk / correlation / optimizer views).
    pub quant: Option<QuantData>,
    quant_loaded: bool,

    cmd_history: Vec<String>,
    history_pos: Option<usize>,

    // Plumbing for on-demand history requests / subscriptions.
    provider: Option<Arc<dyn DataProvider>>,
    tx: Option<UnboundedSender<AppEvent>>,
    requested_key: Option<(String, Timeframe)>,
    config: Config,
}

impl App {
    pub fn new(config: Config) -> Self {
        let watchlist = config.watchlist.clone();

        Self {
            running: true,
            focus: Panel::Watchlist,
            show_help: false,
            command_input: String::new(),
            message: None,
            watchlist,
            selected: 0,
            conn: ConnectionStatus::Simulated,
            quotes: HashMap::new(),
            history: HashMap::new(),
            timeframe: Timeframe::D1,
            chart_type: ChartType::Candle,
            strategy: Strategy::SmaCross,
            series: None,
            view: View::Monitor,
            positions: Vec::new(),
            account: None,
            pos_selected: 0,
            portfolio_loaded: false,
            quant: None,
            quant_loaded: false,
            cmd_history: Vec::new(),
            history_pos: None,
            provider: None,
            tx: None,
            requested_key: None,
            config,
        }
    }

    pub async fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        let mut events = EventHandler::new(Duration::from_millis(250), Duration::from_millis(33));

        // Show CONNECTING during an IBKR handshake.
        if self.config.provider == ProviderKind::Ibkr {
            self.conn = ConnectionStatus::Connecting;
            terminal.draw(|frame| ui::draw(frame, self))?;
        }

        // Start the market-data feed for the configured provider (IBKR falls
        // back to simulated if the gateway is unreachable).
        let provider = self.create_provider().await;
        tracing::info!(
            provider = provider.name(),
            symbols = self.watchlist.len(),
            "data feed started"
        );
        MarketDataManager::spawn(provider.clone(), self.watchlist.clone(), events.sender());
        self.provider = Some(provider);
        self.tx = Some(events.sender());
        self.sync_series();

        terminal.draw(|frame| ui::draw(frame, self))?;

        while self.running {
            let Some(event) = events.next().await else {
                break;
            };
            match event {
                AppEvent::Render => {
                    terminal.draw(|frame| ui::draw(frame, self))?;
                }
                AppEvent::Tick => {}
                AppEvent::Mouse(mouse) => self.handle_mouse(mouse),
                AppEvent::Market(ev) => self.apply_market(ev),
                AppEvent::History {
                    symbol,
                    timeframe,
                    candles,
                } => self.on_history(symbol, timeframe, candles),
                AppEvent::Positions(positions) => self.positions = positions,
                AppEvent::Account(account) => self.account = Some(account),
                AppEvent::Quant(data) => self.quant = Some(*data),
                AppEvent::Key(key) => {
                    if let Some(action) = self.map_key(key) {
                        update::update(self, action);
                    }
                }
            }
            self.sync_series();
        }
        Ok(())
    }

    /// Build the configured data provider, falling back to simulated data if an
    /// IBKR connection can't be established.
    async fn create_provider(&mut self) -> Arc<dyn DataProvider> {
        match self.config.provider {
            ProviderKind::Simulated => Arc::new(SimulatedProvider::new(0x0C0F_FEE5)),
            ProviderKind::Ibkr => {
                let (host, port, client_id, delayed) = {
                    let c = &self.config.ibkr;
                    (c.host.clone(), c.port, c.client_id, c.delayed)
                };
                match IbkrProvider::connect(&host, port, client_id, delayed).await {
                    Ok(provider) => {
                        tracing::info!(%host, port, "connected to IBKR");
                        Arc::new(provider)
                    }
                    Err(err) => {
                        tracing::error!(error = ?err, "IBKR connect failed; using simulated data");
                        self.message = Some("IBKR unavailable — using simulated data".to_string());
                        Arc::new(SimulatedProvider::new(0x0C0F_FEE5))
                    }
                }
            }
        }
    }

    fn apply_market(&mut self, ev: MarketEvent) {
        match ev {
            MarketEvent::Status(status) => self.conn = status,
            MarketEvent::Quote(quote) => {
                let hist = self.history.entry(quote.symbol.clone()).or_default();
                hist.push_back(quote.last);
                while hist.len() > HISTORY_CAP {
                    hist.pop_front();
                }
                self.quotes.insert(quote.symbol.clone(), quote);
            }
        }
    }

    fn on_history(&mut self, symbol: String, timeframe: Timeframe, candles: Vec<Candle>) {
        if self.active_symbol() == Some(symbol.as_str()) && self.timeframe == timeframe {
            self.series = Some(ChartSeries::build(symbol, timeframe, candles));
        }
    }

    /// Request history for the active symbol/timeframe if not already loaded or
    /// in flight. Cheap to call every loop iteration.
    fn sync_series(&mut self) {
        let Some(symbol) = self.active_symbol().map(str::to_string) else {
            return;
        };
        let loaded = self
            .series
            .as_ref()
            .is_some_and(|s| s.symbol == symbol && s.timeframe == self.timeframe);
        if loaded {
            return;
        }
        let key = (symbol.clone(), self.timeframe);
        if self.requested_key.as_ref() == Some(&key) {
            return;
        }
        self.requested_key = Some(key);
        self.request_history(symbol, self.timeframe);
    }

    fn request_history(&self, symbol: String, timeframe: Timeframe) {
        let (Some(provider), Some(tx)) = (self.provider.clone(), self.tx.clone()) else {
            return;
        };
        tokio::spawn(async move {
            match provider.history(&symbol, timeframe).await {
                Ok(candles) => {
                    let _ = tx.send(AppEvent::History {
                        symbol,
                        timeframe,
                        candles,
                    });
                }
                Err(err) => tracing::error!(error = ?err, %symbol, "history request failed"),
            }
        });
    }

    /// Start streaming quotes for a newly-added symbol.
    fn subscribe_symbol(&self, symbol: String) {
        if let (Some(provider), Some(tx)) = (&self.provider, &self.tx) {
            MarketDataManager::spawn(provider.clone(), vec![symbol], tx.clone());
        }
    }

    /// Symbol currently driving the chart/stats panes.
    pub fn active_symbol(&self) -> Option<&str> {
        self.watchlist.get(self.selected).map(String::as_str)
    }

    /// Move the active selection within the current view.
    pub fn select_next(&mut self) {
        match self.view {
            View::Portfolio => {
                let n = self.positions.len();
                if n > 0 {
                    self.pos_selected = (self.pos_selected + 1) % n;
                }
            }
            _ => {
                if !self.watchlist.is_empty() {
                    self.selected = (self.selected + 1) % self.watchlist.len();
                }
            }
        }
    }

    pub fn select_prev(&mut self) {
        match self.view {
            View::Portfolio => {
                let n = self.positions.len();
                if n > 0 {
                    self.pos_selected = (self.pos_selected + n - 1) % n;
                }
            }
            _ => {
                if !self.watchlist.is_empty() {
                    let n = self.watchlist.len();
                    self.selected = (self.selected + n - 1) % n;
                }
            }
        }
    }

    /// Lazily load positions + account summary the first time the portfolio view
    /// is opened.
    pub fn ensure_portfolio_loaded(&mut self) {
        if self.portfolio_loaded {
            return;
        }
        self.portfolio_loaded = true;
        self.request_portfolio();
    }

    fn request_portfolio(&self) {
        let (Some(provider), Some(tx)) = (self.provider.clone(), self.tx.clone()) else {
            return;
        };
        let provider_acct = provider.clone();
        let tx_acct = tx.clone();
        tokio::spawn(async move {
            match provider.positions().await {
                Ok(positions) => {
                    let _ = tx.send(AppEvent::Positions(positions));
                }
                Err(err) => tracing::error!(error = ?err, "positions request failed"),
            }
        });
        tokio::spawn(async move {
            match provider_acct.account_summary().await {
                Ok(account) => {
                    let _ = tx_acct.send(AppEvent::Account(account));
                }
                Err(err) => tracing::error!(error = ?err, "account request failed"),
            }
        });
    }

    /// Lazily compute cross-sectional analytics (correlation / risk / optimizer)
    /// from 1Y daily history of the watchlist, the first time they're shown.
    pub fn ensure_quant_loaded(&mut self) {
        if self.quant_loaded {
            return;
        }
        self.quant_loaded = true;
        self.request_quant();
    }

    fn request_quant(&self) {
        let (Some(provider), Some(tx)) = (self.provider.clone(), self.tx.clone()) else {
            return;
        };
        let symbols = self.watchlist.clone();
        tokio::spawn(async move {
            let mut series = Vec::new();
            for sym in &symbols {
                if let Ok(candles) = provider.history(sym, Timeframe::Y1).await {
                    let closes: Vec<f64> = candles.iter().map(|c| c.close).collect();
                    if closes.len() >= 30 {
                        series.push((sym.clone(), closes));
                    }
                }
            }
            if series.is_empty() {
                return;
            }
            // SPY as the beta benchmark (falls back to the first symbol).
            let benchmark = match provider.history("SPY", Timeframe::Y1).await {
                Ok(candles) if !candles.is_empty() => candles.iter().map(|c| c.close).collect(),
                _ => series[0].1.clone(),
            };
            if let Some(data) = QuantData::build(&series, &benchmark, 252.0) {
                let _ = tx.send(AppEvent::Quant(Box::new(data)));
            }
        });
    }

    // ── input ────────────────────────────────────────────────────────────

    fn map_key(&mut self, key: KeyEvent) -> Option<Action> {
        if key.kind != KeyEventKind::Press {
            return None;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Some(Action::Quit);
        }
        if self.show_help {
            return Some(Action::ToggleHelp); // any key dismisses help
        }
        if self.focus == Panel::Command {
            self.handle_command_key(key);
            return None;
        }
        match key.code {
            KeyCode::Char('/') | KeyCode::Char(':') => {
                self.enter_command();
                None
            }
            KeyCode::Char('q') => Some(Action::Quit),
            KeyCode::Char('?') => Some(Action::ToggleHelp),
            KeyCode::Tab => Some(Action::FocusNext),
            KeyCode::BackTab => Some(Action::FocusPrev),
            KeyCode::Down | KeyCode::Char('j') => Some(Action::SelectNext),
            KeyCode::Up | KeyCode::Char('k') => Some(Action::SelectPrev),
            KeyCode::Char('c') => Some(Action::ToggleChartType),
            KeyCode::Char('s') => Some(Action::CycleStrategy),
            KeyCode::Char(d @ '1'..='5') => Some(Action::SetTimeframe(
                Timeframe::ALL[d as usize - '1' as usize],
            )),
            KeyCode::F(1) => Some(Action::SetView(View::Monitor)),
            KeyCode::F(2) => Some(Action::SetView(View::Portfolio)),
            KeyCode::F(3) => Some(Action::SetView(View::Backtest)),
            KeyCode::F(4) => Some(Action::SetView(View::Risk)),
            KeyCode::F(5) => Some(Action::SetView(View::Optimizer)),
            KeyCode::F(6) => Some(Action::SetView(View::Options)),
            _ => None,
        }
    }

    /// Mouse-wheel scrolling moves the watchlist selection (when the terminal
    /// has mouse capture enabled).
    fn handle_mouse(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::ScrollDown => update::update(self, Action::SelectNext),
            MouseEventKind::ScrollUp => update::update(self, Action::SelectPrev),
            _ => {}
        }
    }

    fn enter_command(&mut self) {
        self.focus = Panel::Command;
        self.message = None;
        self.history_pos = None;
    }

    fn handle_command_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.command_input.clear();
                self.history_pos = None;
                self.focus = Panel::Watchlist;
            }
            KeyCode::Enter => {
                let cmd = self.command_input.trim().to_string();
                self.command_input.clear();
                self.history_pos = None;
                self.focus = Panel::Watchlist;
                if !cmd.is_empty() {
                    self.cmd_history.push(cmd.clone());
                    self.execute_command(&cmd);
                }
            }
            KeyCode::Backspace => {
                self.command_input.pop();
            }
            KeyCode::Up => self.history_recall(true),
            KeyCode::Down => self.history_recall(false),
            KeyCode::Char(c) => {
                // Uppercase letters for the authentic Bloomberg command-line feel.
                self.command_input.push(if c.is_ascii_alphabetic() {
                    c.to_ascii_uppercase()
                } else {
                    c
                });
            }
            _ => {}
        }
    }

    fn history_recall(&mut self, prev: bool) {
        if self.cmd_history.is_empty() {
            return;
        }
        let len = self.cmd_history.len();
        self.history_pos = match (self.history_pos, prev) {
            (None, true) => Some(len - 1),
            (None, false) => None,
            (Some(p), true) => Some(p.saturating_sub(1)),
            (Some(p), false) if p + 1 < len => Some(p + 1),
            (Some(_), false) => None,
        };
        self.command_input = self
            .history_pos
            .map(|p| self.cmd_history[p].clone())
            .unwrap_or_default();
    }

    fn execute_command(&mut self, input: &str) {
        match command::parse(input) {
            Command::Quit => self.running = false,
            Command::Help => self.show_help = true,
            Command::SetTimeframe(tf) => {
                self.timeframe = tf;
                self.message = Some(format!("timeframe {}", tf.label()));
            }
            Command::SetChart(ct) => {
                self.chart_type = ct;
                self.message = Some(format!("chart {}", ct.label()));
            }
            Command::LoadSymbol(sym) => self.add_symbol(sym, true),
            Command::AddSymbol(sym) => self.add_symbol(sym, false),
            Command::RemoveSymbol(sym) => self.remove_symbol(&sym),
            Command::SetView(view) => {
                self.view = view;
                match view {
                    View::Portfolio => self.ensure_portfolio_loaded(),
                    View::Risk | View::Optimizer => self.ensure_quant_loaded(),
                    _ => {}
                }
                self.message = Some(view.label().to_string());
            }
            Command::Unknown(s) => self.message = Some(format!("unknown: {s}")),
        }
    }

    fn add_symbol(&mut self, symbol: String, select: bool) {
        let symbol = symbol.to_uppercase();
        if let Some(idx) = self.watchlist.iter().position(|s| *s == symbol) {
            if select {
                self.selected = idx;
                self.message = Some(symbol.clone());
            }
        } else {
            self.watchlist.push(symbol.clone());
            if select {
                self.selected = self.watchlist.len() - 1;
            }
            self.subscribe_symbol(symbol.clone());
            self.message = Some(format!("added {symbol}"));
        }
    }

    fn remove_symbol(&mut self, symbol: &str) {
        let symbol = symbol.to_uppercase();
        if let Some(idx) = self.watchlist.iter().position(|s| *s == symbol) {
            self.watchlist.remove(idx);
            if self.selected >= self.watchlist.len() {
                self.selected = self.watchlist.len().saturating_sub(1);
            }
            self.message = Some(format!("removed {symbol}"));
        } else {
            self.message = Some(format!("not in watchlist: {symbol}"));
        }
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new(Config::default())
    }
}
