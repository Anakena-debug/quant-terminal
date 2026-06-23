//! The command line: parses Bloomberg-style input into a [`Command`].
//!
//! Supported forms:
//! - `AAPL` or `AAPL GP` — load a symbol's chart ("graph price")
//! - `add NVDA` / `rm TSLA` — watchlist management
//! - `tf 6M` — set timeframe (`1D 5D 1M 6M 1Y`)
//! - `chart line|candle` — set chart style
//! - `help` / `q` — help / quit

use crate::app::{ChartType, View};
use crate::data::Timeframe;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Add if missing, then select (drives the chart).
    LoadSymbol(String),
    AddSymbol(String),
    RemoveSymbol(String),
    SetTimeframe(Timeframe),
    SetChart(ChartType),
    SetView(View),
    Help,
    Quit,
    Unknown(String),
}

pub fn parse(input: &str) -> Command {
    let tokens: Vec<String> = input.split_whitespace().map(|s| s.to_uppercase()).collect();
    let Some(head) = tokens.first() else {
        return Command::Unknown(String::new());
    };
    let unknown = || Command::Unknown(input.trim().to_string());

    match head.as_str() {
        "Q" | "QUIT" | "EXIT" => Command::Quit,
        "HELP" | "H" | "?" => Command::Help,
        "ADD" | "A" | "W" => tokens
            .get(1)
            .map(|s| Command::AddSymbol(s.clone()))
            .unwrap_or_else(unknown),
        "RM" | "REMOVE" | "DEL" | "DELETE" => tokens
            .get(1)
            .map(|s| Command::RemoveSymbol(s.clone()))
            .unwrap_or_else(unknown),
        "TF" | "T" => tokens
            .get(1)
            .and_then(|s| Timeframe::from_label(s))
            .map(Command::SetTimeframe)
            .unwrap_or_else(unknown),
        "CHART" | "GC" => tokens
            .get(1)
            .and_then(|s| chart_from(s))
            .map(Command::SetChart)
            .unwrap_or_else(unknown),
        "MON" | "MONITOR" => Command::SetView(View::Monitor),
        "PORT" | "PORTFOLIO" | "PRTU" | "PTU" => Command::SetView(View::Portfolio),
        "BT" | "BACKTEST" => Command::SetView(View::Backtest),
        "RISK" | "CORR" => Command::SetView(View::Risk),
        "OPT" | "OPTIMIZE" | "EF" => Command::SetView(View::Optimizer),
        "OPTIONS" | "OMON" | "OPX" => Command::SetView(View::Options),
        // Bloomberg-style: "<TICKER> [FUNCTION]"
        _ => match tokens.get(1).map(String::as_str) {
            None => Command::LoadSymbol(head.clone()),
            Some("GP" | "GIP" | "G" | "PX") => Command::LoadSymbol(head.clone()),
            Some("TF") => tokens
                .get(2)
                .and_then(|s| Timeframe::from_label(s))
                .map(Command::SetTimeframe)
                .unwrap_or_else(unknown),
            Some(_) => unknown(),
        },
    }
}

fn chart_from(s: &str) -> Option<ChartType> {
    match s {
        "LINE" | "L" => Some(ChartType::Line),
        "CANDLE" | "CANDLES" | "C" | "OHLC" => Some(ChartType::Candle),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bloomberg_style_load() {
        assert_eq!(parse("aapl"), Command::LoadSymbol("AAPL".into()));
        assert_eq!(parse("aapl gp"), Command::LoadSymbol("AAPL".into()));
        assert_eq!(parse("  NvDa  GP "), Command::LoadSymbol("NVDA".into()));
    }

    #[test]
    fn watchlist_and_settings() {
        assert_eq!(parse("add tsla"), Command::AddSymbol("TSLA".into()));
        assert_eq!(parse("rm spy"), Command::RemoveSymbol("SPY".into()));
        assert_eq!(parse("tf 6m"), Command::SetTimeframe(Timeframe::M6));
        assert_eq!(parse("chart line"), Command::SetChart(ChartType::Line));
        assert_eq!(parse("q"), Command::Quit);
        assert_eq!(parse("help"), Command::Help);
    }

    #[test]
    fn unknowns() {
        assert!(matches!(parse("tf zz"), Command::Unknown(_)));
        assert!(matches!(parse("aapl wat"), Command::Unknown(_)));
        assert!(matches!(parse(""), Command::Unknown(_)));
    }
}
