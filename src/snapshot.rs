//! Offline preview: render the real UI into an in-memory `TestBackend` buffer
//! so the interface can be previewed without a TTY. Output is monospace `text`
//! (default), truecolor `ansi`, or `svg`.
//!
//! Invoke with `qterm --snapshot [WIDTHxHEIGHT] [line] [help] [text|ansi|svg]`.

use std::collections::VecDeque;

use color_eyre::Result;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::Color;

use crate::analytics::ChartSeries;
use crate::app::{App, ChartType, View};
use crate::config::Config;
use crate::data::{
    ConnectionStatus, DataProvider, IbkrProvider, Quote, SimulatedProvider, Timeframe,
};

#[derive(Clone, Copy)]
enum Out {
    Text,
    Ansi,
    Svg,
}

pub async fn render(args: &[String]) -> Result<String> {
    let mut width = 100u16;
    let mut height = 32u16;
    let mut chart = ChartType::Candle;
    let mut help = false;
    let mut mode = Out::Text;
    let mut live = false;
    let mut portfolio = false;
    let mut backtest = false;
    let mut risk = false;
    let mut opt = false;
    let mut options_view = false;

    for a in args {
        if let Some((w, h)) = a.split_once('x')
            && let (Ok(w), Ok(h)) = (w.parse::<u16>(), h.parse::<u16>())
        {
            width = w;
            height = h;
        }
        match a.as_str() {
            "line" => chart = ChartType::Line,
            "help" => help = true,
            "ansi" => mode = Out::Ansi,
            "svg" => mode = Out::Svg,
            "live" => live = true,
            "port" => portfolio = true,
            "bt" => backtest = true,
            "risk" => risk = true,
            "opt" => opt = true,
            "options" | "opx" => options_view = true,
            _ => {}
        }
    }

    let mut app = App::new(Config::default());
    app.chart_type = chart;
    app.show_help = help;
    if backtest {
        app.timeframe = Timeframe::Y1; // a year of bars gives the strategy room
    }

    // Live mode connects to IBKR (per config) and seeds from real history;
    // otherwise use the simulated feed. Falls back to simulated on error.
    let mut used_live = false;
    if live {
        let cfg = crate::config::load();
        match IbkrProvider::connect(
            &cfg.ibkr.host,
            cfg.ibkr.port,
            cfg.ibkr.client_id,
            cfg.ibkr.delayed,
        )
        .await
        {
            Ok(provider) => {
                populate(&mut app, &provider, portfolio).await;
                app.conn = if cfg.ibkr.delayed {
                    ConnectionStatus::Delayed
                } else {
                    ConnectionStatus::Live
                };
                used_live = true;
            }
            Err(e) => {
                app.message = Some(format!("IBKR unavailable: {e}"));
            }
        }
    }
    if !used_live {
        populate_simulated(&mut app, portfolio).await;
    }
    if portfolio {
        app.view = View::Portfolio;
    }
    if backtest {
        app.view = View::Backtest;
    }
    if risk || opt {
        load_quant(&mut app, &SimulatedProvider::new(0x5EED_1234)).await;
    }
    if risk {
        app.view = View::Risk;
    }
    if opt {
        app.view = View::Optimizer;
    }
    if options_view {
        app.view = View::Options;
    }

    let mut terminal = Terminal::new(TestBackend::new(width, height))?;
    terminal.draw(|frame| crate::ui::draw(frame, &app))?;
    let buffer = terminal.backend().buffer();
    Ok(match mode {
        Out::Text => buffer_to_text(buffer),
        Out::Ansi => buffer_to_ansi(buffer),
        Out::Svg => buffer_to_svg(buffer),
    })
}

/// Seed the watchlist + active chart from a live provider's daily history.
async fn populate(app: &mut App, provider: &dyn DataProvider, want_portfolio: bool) {
    let symbols = app.watchlist.clone();
    for sym in &symbols {
        let daily = provider
            .history(sym, Timeframe::M1)
            .await
            .unwrap_or_default();
        let Some(last) = daily.last() else { continue };
        let prev_close = if daily.len() >= 2 {
            daily[daily.len() - 2].close
        } else {
            last.open
        };
        let spread = (last.close * 0.0002).max(0.01);
        app.quotes.insert(
            sym.clone(),
            Quote {
                symbol: sym.clone(),
                last: last.close,
                prev_close,
                bid: last.close - spread,
                ask: last.close + spread,
                bid_size: 0,
                ask_size: 0,
                open: last.open,
                high: last.high,
                low: last.low,
                volume: last.volume,
            },
        );
        let hist: VecDeque<f64> = daily.iter().rev().take(24).rev().map(|c| c.close).collect();
        app.history.insert(sym.clone(), hist);
    }
    load_active_series(app, provider).await;
    if want_portfolio {
        app.positions = provider.positions().await.unwrap_or_default();
        app.account = provider.account_summary().await.ok();
    }
}

/// Seed from the simulated feed, with a deterministic green/red mix.
async fn populate_simulated(app: &mut App, want_portfolio: bool) {
    let provider = SimulatedProvider::new(0x5EED_1234);
    let symbols = app.watchlist.clone();
    for (i, sym) in symbols.iter().enumerate() {
        let candles = provider
            .history(sym, Timeframe::D1)
            .await
            .unwrap_or_default();
        let Some(last_bar) = candles.last() else {
            continue;
        };
        let last = last_bar.close;
        let chg_pct = ((hash(sym) % 700) as f64 / 100.0) - 3.3;
        let prev_close = last / (1.0 + chg_pct / 100.0);
        let high = candles.iter().map(|c| c.high).fold(f64::MIN, f64::max);
        let low = candles.iter().map(|c| c.low).fold(f64::MAX, f64::min);
        let volume: u64 = candles.iter().map(|c| c.volume).sum();
        let spread = (last * 0.0002).max(0.01);
        app.quotes.insert(
            sym.clone(),
            Quote {
                symbol: sym.clone(),
                last,
                prev_close,
                bid: last - spread,
                ask: last + spread,
                bid_size: 100 + (i as u32 * 37) % 800,
                ask_size: 100 + (i as u32 * 53) % 800,
                open: candles.first().map(|c| c.open).unwrap_or(last),
                high,
                low,
                volume,
            },
        );
        let hist: VecDeque<f64> = candles
            .iter()
            .rev()
            .take(24)
            .rev()
            .map(|c| c.close)
            .collect();
        app.history.insert(sym.clone(), hist);
    }
    load_active_series(app, &provider).await;
    if want_portfolio {
        app.positions = provider.positions().await.unwrap_or_default();
        app.account = provider.account_summary().await.ok();
    }
}

async fn load_active_series(app: &mut App, provider: &dyn DataProvider) {
    if let Some(active) = app.active_symbol().map(str::to_string) {
        let candles = provider
            .history(&active, app.timeframe)
            .await
            .unwrap_or_default();
        if !candles.is_empty() {
            app.series = Some(ChartSeries::build(active, app.timeframe, candles));
        }
    }
}

async fn load_quant(app: &mut App, provider: &dyn DataProvider) {
    let symbols = app.watchlist.clone();
    let mut series = Vec::new();
    for sym in &symbols {
        if let Ok(c) = provider.history(sym, Timeframe::Y1).await {
            let closes: Vec<f64> = c.iter().map(|x| x.close).collect();
            if closes.len() >= 30 {
                series.push((sym.clone(), closes));
            }
        }
    }
    if series.is_empty() {
        return;
    }
    let bench = match provider.history("SPY", Timeframe::Y1).await {
        Ok(c) if !c.is_empty() => c.iter().map(|x| x.close).collect(),
        _ => series[0].1.clone(),
    };
    app.quant = crate::analytics::QuantData::build(&series, &bench, 252.0);
}

fn buffer_to_text(buf: &Buffer) -> String {
    let area = buf.area;
    let mut out = String::new();
    for y in 0..area.height {
        for x in 0..area.width {
            out.push_str(buf.cell((x, y)).map(|c| c.symbol()).unwrap_or(" "));
        }
        out.push('\n');
    }
    out
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn buffer_to_svg(buf: &Buffer) -> String {
    let area = buf.area;
    let (cw, ch) = (8.4_f64, 17.0_f64);
    let (w, h) = (area.width as f64 * cw, area.height as f64 * ch);
    let mut s = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w:.0}" height="{h:.0}" viewBox="0 0 {w:.0} {h:.0}" font-family="Menlo,'DejaVu Sans Mono',Consolas,monospace" font-size="13px"><rect width="{w:.0}" height="{h:.0}" fill="#08090c"/>"##
    );
    for y in 0..area.height {
        for x in 0..area.width {
            let Some(cell) = buf.cell((x, y)) else {
                continue;
            };
            let (px, py) = (x as f64 * cw, y as f64 * ch);
            if let Some((r, g, b)) = rgb(cell.bg) {
                s.push_str(&format!(
                    r##"<rect x="{px:.2}" y="{py:.2}" width="{cw:.2}" height="{ch:.2}" fill="#{r:02x}{g:02x}{b:02x}"/>"##
                ));
            }
            let sym = cell.symbol();
            if !sym.trim().is_empty() {
                let (r, g, b) = rgb(cell.fg).unwrap_or((220, 220, 210));
                s.push_str(&format!(
                    r##"<text x="{:.2}" y="{:.2}" fill="#{r:02x}{g:02x}{b:02x}" text-anchor="middle">{}</text>"##,
                    px + cw / 2.0,
                    py + ch * 0.76,
                    xml_escape(sym)
                ));
            }
        }
    }
    s.push_str("</svg>");
    s
}

fn hash(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |a, b| {
        (a ^ b as u64).wrapping_mul(0x0000_0100_0000_01B3)
    })
}

/// Map a ratatui colour to RGB for a truecolor escape (`None` = terminal default).
fn rgb(c: Color) -> Option<(u8, u8, u8)> {
    match c {
        Color::Rgb(r, g, b) => Some((r, g, b)),
        Color::Black => Some((0, 0, 0)),
        Color::White => Some((229, 229, 229)),
        Color::Red => Some((231, 76, 60)),
        Color::Green => Some((46, 204, 113)),
        Color::Yellow => Some((255, 198, 64)),
        Color::Blue => Some((52, 152, 219)),
        Color::Cyan => Some((80, 200, 220)),
        Color::Magenta => Some((200, 130, 255)),
        Color::Gray => Some((160, 160, 160)),
        Color::DarkGray => Some((100, 100, 100)),
        _ => None,
    }
}

fn buffer_to_ansi(buf: &Buffer) -> String {
    let area = buf.area;
    let mut out = String::with_capacity(area.width as usize * area.height as usize * 18);
    for y in 0..area.height {
        for x in 0..area.width {
            if let Some(cell) = buf.cell((x, y)) {
                if let Some((r, g, b)) = rgb(cell.fg) {
                    out.push_str(&format!("\x1b[38;2;{r};{g};{b}m"));
                }
                if let Some((r, g, b)) = rgb(cell.bg) {
                    out.push_str(&format!("\x1b[48;2;{r};{g};{b}m"));
                }
                out.push_str(cell.symbol());
                out.push_str("\x1b[0m");
            }
        }
        out.push('\n');
    }
    out
}
