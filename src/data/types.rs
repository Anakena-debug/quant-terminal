//! Core market-data domain types, shared by every provider.

/// Connection / data-quality state, shown in the header badge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionStatus {
    Simulated,
    Connecting,
    Live,
    Delayed,
    Disconnected,
}

impl ConnectionStatus {
    pub fn badge(self) -> &'static str {
        match self {
            ConnectionStatus::Simulated => "◆ SIM",
            ConnectionStatus::Connecting => "● CONNECTING",
            ConnectionStatus::Live => "● LIVE",
            ConnectionStatus::Delayed => "● DELAYED",
            ConnectionStatus::Disconnected => "✕ OFFLINE",
        }
    }
}

/// A top-of-book snapshot for one symbol.
#[derive(Clone, Debug)]
pub struct Quote {
    pub symbol: String,
    pub last: f64,
    pub prev_close: f64,
    pub bid: f64,
    pub ask: f64,
    pub bid_size: u32,
    pub ask_size: u32,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub volume: u64,
}

impl Quote {
    pub fn change(&self) -> f64 {
        self.last - self.prev_close
    }

    pub fn change_pct(&self) -> f64 {
        if self.prev_close.abs() < f64::EPSILON {
            0.0
        } else {
            self.change() / self.prev_close * 100.0
        }
    }

    pub fn spread(&self) -> f64 {
        (self.ask - self.bid).max(0.0)
    }
}

/// One OHLCV bar.
#[derive(Clone, Copy, Debug)]
pub struct Candle {
    pub time: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: u64,
}

/// Chart timeframes, mapped to bar granularity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Timeframe {
    D1,
    D5,
    M1,
    M6,
    Y1,
}

impl Timeframe {
    pub const ALL: [Timeframe; 5] = [
        Timeframe::D1,
        Timeframe::D5,
        Timeframe::M1,
        Timeframe::M6,
        Timeframe::Y1,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Timeframe::D1 => "1D",
            Timeframe::D5 => "5D",
            Timeframe::M1 => "1M",
            Timeframe::M6 => "6M",
            Timeframe::Y1 => "1Y",
        }
    }

    pub fn from_label(s: &str) -> Option<Timeframe> {
        match s.to_uppercase().as_str() {
            "1D" | "D" | "D1" => Some(Timeframe::D1),
            "5D" | "D5" => Some(Timeframe::D5),
            "1M" | "M" | "M1" => Some(Timeframe::M1),
            "6M" | "M6" => Some(Timeframe::M6),
            "1Y" | "Y" | "Y1" => Some(Timeframe::Y1),
            _ => None,
        }
    }

    /// How many bars to display/simulate for this timeframe.
    pub fn bar_count(self) -> usize {
        match self {
            Timeframe::D1 => 78,  // ~6.5h of 5-min bars
            Timeframe::D5 => 65,  // ~5d of 30-min bars
            Timeframe::M1 => 22,  // ~1m of daily bars
            Timeframe::M6 => 126, // ~6m of daily bars
            Timeframe::Y1 => 252, // ~1y of daily bars
        }
    }

    /// Seconds per bar, used to synthesise/space timestamps.
    pub fn bar_seconds(self) -> i64 {
        match self {
            Timeframe::D1 => 300,
            Timeframe::D5 => 1_800,
            Timeframe::M1 | Timeframe::M6 | Timeframe::Y1 => 86_400,
        }
    }
}

/// Events streamed from a [`super::DataProvider`].
#[derive(Clone, Debug)]
pub enum MarketEvent {
    Quote(Quote),
    Status(ConnectionStatus),
}

/// An open position in the account.
#[derive(Clone, Debug)]
pub struct Position {
    pub symbol: String,
    /// Signed quantity (negative = short).
    pub qty: f64,
    pub avg_cost: f64,
    pub last: f64,
}

impl Position {
    pub fn market_value(&self) -> f64 {
        self.qty * self.last
    }
    pub fn cost_basis(&self) -> f64 {
        self.qty * self.avg_cost
    }
    pub fn unrealized(&self) -> f64 {
        (self.last - self.avg_cost) * self.qty
    }
    pub fn unrealized_pct(&self) -> f64 {
        let basis = self.cost_basis().abs();
        if basis < f64::EPSILON {
            0.0
        } else {
            self.unrealized() / basis * 100.0
        }
    }
}

/// Account-level summary figures.
#[derive(Clone, Debug, Default)]
pub struct AccountSummary {
    pub account: String,
    pub net_liquidation: f64,
    pub total_cash: f64,
    pub buying_power: f64,
}
