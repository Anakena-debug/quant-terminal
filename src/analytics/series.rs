//! A precomputed chart series: candles plus every indicator overlay/sub-pane
//! value, built once when history loads and read directly at render time.

use crate::data::{Candle, Timeframe};

use super::{bollinger, ema, macd, rsi, sma};

pub struct ChartSeries {
    pub symbol: String,
    pub timeframe: Timeframe,
    pub candles: Vec<Candle>,
    pub closes: Vec<f64>,
    // Price overlays
    pub sma20: Vec<Option<f64>>,
    pub ema50: Vec<Option<f64>>,
    pub bb_upper: Vec<Option<f64>>,
    pub bb_mid: Vec<Option<f64>>,
    pub bb_lower: Vec<Option<f64>>,
    // Sub-pane indicators
    pub rsi14: Vec<Option<f64>>,
    pub macd: Vec<Option<f64>>,
    pub macd_signal: Vec<Option<f64>>,
    pub macd_hist: Vec<Option<f64>>,
}

impl ChartSeries {
    pub fn build(symbol: String, timeframe: Timeframe, candles: Vec<Candle>) -> Self {
        let closes: Vec<f64> = candles.iter().map(|c| c.close).collect();
        let (bb_mid, bb_upper, bb_lower) = bollinger(&closes, 20, 2.0);
        let (macd_line, macd_signal, macd_hist) = macd(&closes, 12, 26, 9);
        Self {
            sma20: sma(&closes, 20),
            ema50: ema(&closes, 50),
            bb_upper,
            bb_mid,
            bb_lower,
            rsi14: rsi(&closes, 14),
            macd: macd_line,
            macd_signal,
            macd_hist,
            closes,
            candles,
            symbol,
            timeframe,
        }
    }

    /// Most recent defined value in an indicator series (for readouts).
    pub fn last(series: &[Option<f64>]) -> Option<f64> {
        series.iter().rev().find_map(|x| *x)
    }
}
