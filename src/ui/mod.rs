//! Rendering: top-level layout and individual panels.

pub mod theme;

mod backtest;
mod chart;
mod optimizer;
mod options;
mod portfolio;
mod risk;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, List, ListItem, Paragraph, Wrap};

use crate::app::{App, Panel, View};
use crate::data::ConnectionStatus;
use theme::{THEME, market_clock};

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    frame.render_widget(Block::new().style(Style::new().bg(THEME.bg)), area);

    let [header, body, status] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);

    draw_header(frame, header, app);
    match app.view {
        View::Monitor => draw_body(frame, body, app),
        View::Portfolio => portfolio::draw(frame, body, app),
        View::Backtest => backtest::draw(frame, body, app),
        View::Risk => risk::draw(frame, body, app),
        View::Optimizer => optimizer::draw(frame, body, app),
        View::Options => options::draw(frame, body, app),
    }
    draw_status(frame, status, app);

    if app.show_help {
        draw_help(frame, area);
    }
}

// ── shared helpers ──────────────────────────────────────────────────────────

/// A bordered panel with a heading, brighter when focused.
pub(crate) fn panel_block(title: &str, focused: bool) -> Block<'static> {
    let color = if focused {
        THEME.border_focus
    } else {
        THEME.border
    };
    Block::bordered()
        .border_type(if focused {
            BorderType::Thick
        } else {
            BorderType::Plain
        })
        .border_style(Style::new().fg(color))
        .title(Span::styled(
            format!(" {title} "),
            Style::new().fg(THEME.heading).add_modifier(Modifier::BOLD),
        ))
        .style(Style::new().bg(THEME.bg))
}

fn status_color(status: ConnectionStatus) -> ratatui::style::Color {
    match status {
        ConnectionStatus::Live => THEME.up,
        ConnectionStatus::Delayed => THEME.accent,
        ConnectionStatus::Simulated => THEME.accent_alt,
        ConnectionStatus::Connecting => THEME.dim,
        ConnectionStatus::Disconnected => THEME.down,
    }
}

/// Compact block-character sparkline from a sequence of values.
fn sparkline(values: impl Iterator<Item = f64>, width: usize) -> String {
    const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let all: Vec<f64> = values.collect();
    if all.is_empty() {
        return " ".repeat(width);
    }
    let slice = if all.len() > width {
        &all[all.len() - width..]
    } else {
        &all[..]
    };
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for &v in slice {
        lo = lo.min(v);
        hi = hi.max(v);
    }
    let range = (hi - lo).max(1e-9);
    let mut s: String = slice
        .iter()
        .map(|&v| {
            let idx = (((v - lo) / range) * 7.0).round() as usize;
            BLOCKS[idx.min(7)]
        })
        .collect();
    let count = s.chars().count();
    if count < width {
        let mut padded = " ".repeat(width - count);
        padded.push_str(&s);
        s = padded;
    }
    s
}

pub(crate) fn fmt_volume(v: u64) -> String {
    let f = v as f64;
    if f >= 1e9 {
        format!("{:.2}B", f / 1e9)
    } else if f >= 1e6 {
        format!("{:.2}M", f / 1e6)
    } else if f >= 1e3 {
        format!("{:.1}K", f / 1e3)
    } else {
        v.to_string()
    }
}

// ── header ──────────────────────────────────────────────────────────────────

fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    let (clock, open) = market_clock();
    let title = Line::from(vec![
        Span::styled(
            " QUANT TERMINAL ",
            Style::new()
                .fg(THEME.bg)
                .bg(THEME.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(clock, Style::new().fg(THEME.dim)),
        Span::raw(" "),
        Span::styled(
            if open { "● OPEN" } else { "○ CLOSED" },
            Style::new().fg(if open { THEME.up } else { THEME.down }),
        ),
        Span::raw("  "),
        Span::styled(
            app.conn.badge(),
            Style::new()
                .fg(status_color(app.conn))
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
    ]);

    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(Style::new().fg(THEME.border))
        .title(title)
        .style(Style::new().bg(THEME.bg));

    let mut spans = vec![
        Span::styled(
            " > ",
            Style::new().fg(THEME.accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled(app.command_input.as_str(), Style::new().fg(THEME.fg)),
    ];
    if app.focus == Panel::Command {
        spans.push(Span::styled("▮", Style::new().fg(THEME.accent)));
    }
    if app.command_input.is_empty() {
        match &app.message {
            Some(msg) => spans.push(Span::styled(
                format!("   {msg}"),
                Style::new().fg(THEME.accent_alt),
            )),
            None => spans.push(Span::styled(
                "   ticker + function · e.g. AAPL GP · add NVDA · tf 6M · ? help",
                Style::new().fg(THEME.dim),
            )),
        }
    }

    frame.render_widget(Paragraph::new(Line::from(spans)).block(block), area);
}

// ── body: watchlist + main column ───────────────────────────────────────────

fn draw_body(frame: &mut Frame, area: Rect, app: &App) {
    let [left, right] =
        Layout::horizontal([Constraint::Length(34), Constraint::Min(0)]).areas(area);
    draw_watchlist(frame, left, app);
    draw_main(frame, right, app);
}

fn draw_watchlist(frame: &mut Frame, area: Rect, app: &App) {
    let focused = app.focus == Panel::Watchlist;
    let items: Vec<ListItem> = app
        .watchlist
        .iter()
        .enumerate()
        .map(|(i, sym)| {
            let selected = i == app.selected;
            let quote = app.quotes.get(sym);
            let pct = quote.map(|q| q.change_pct()).unwrap_or(0.0);
            let change_color = THEME.change_color(pct);

            let last = quote
                .map(|q| format!("{:>8.2}", q.last))
                .unwrap_or_else(|| "    ----".to_string());
            let pct_str = quote
                .map(|_| format!("{pct:>+6.2}%"))
                .unwrap_or_else(|| "       ".to_string());
            let spark = app
                .history
                .get(sym)
                .map(|h| sparkline(h.iter().copied(), 6))
                .unwrap_or_else(|| " ".repeat(6));

            let marker = if selected { "►" } else { " " };
            let name_style = if selected {
                Style::new().fg(THEME.accent).add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(THEME.fg)
            };

            let line = Line::from(vec![
                Span::styled(marker.to_string(), Style::new().fg(THEME.accent)),
                Span::styled(format!(" {sym:<5}"), name_style),
                Span::styled(last, Style::new().fg(change_color)),
                Span::styled(format!(" {pct_str}"), Style::new().fg(change_color)),
                Span::styled(format!(" {spark}"), Style::new().fg(change_color)),
            ]);

            let mut item = ListItem::new(line);
            if selected {
                item = item.style(Style::new().bg(THEME.selection_bg));
            }
            item
        })
        .collect();

    frame.render_widget(
        List::new(items).block(panel_block("WATCHLIST", focused)),
        area,
    );
}

fn draw_main(frame: &mut Frame, area: Rect, app: &App) {
    let [chart, indicators, stats] = Layout::vertical([
        Constraint::Min(10),
        Constraint::Length(9),
        Constraint::Length(3),
    ])
    .areas(area);

    chart::draw_chart(frame, chart, app);
    chart::draw_indicators(frame, indicators, app);
    draw_stats(frame, stats, app);
}

fn draw_stats(frame: &mut Frame, area: Rect, app: &App) {
    let sym = app.active_symbol().unwrap_or("—");
    let line = match app.quotes.get(sym) {
        Some(q) => {
            let kv = |k: &str, v: String| {
                [
                    Span::styled(format!("  {k} "), Style::new().fg(THEME.dim)),
                    Span::styled(v, Style::new().fg(THEME.fg)),
                ]
            };
            let spans: Vec<Span> = [
                kv("Bid", format!("{:.2}×{}", q.bid, q.bid_size)),
                kv("Ask", format!("{:.2}×{}", q.ask, q.ask_size)),
                kv("Spr", format!("{:.2}", q.spread())),
                kv("O", format!("{:.2}", q.open)),
                kv("H", format!("{:.2}", q.high)),
                kv("L", format!("{:.2}", q.low)),
                kv("Vol", fmt_volume(q.volume)),
            ]
            .into_iter()
            .flatten()
            .collect();
            Line::from(spans)
        }
        None => Line::from(Span::styled("  no quote yet", Style::new().fg(THEME.dim))),
    };
    frame.render_widget(
        Paragraph::new(line).block(panel_block("STATS", false)),
        area,
    );
}

// ── status bar + help ───────────────────────────────────────────────────────

fn draw_status(frame: &mut Frame, area: Rect, app: &App) {
    let focus = match app.focus {
        Panel::Watchlist => "WATCHLIST",
        Panel::Chart => "CHART",
        Panel::Command => "COMMAND",
    };
    let hint = |k: &str, d: &str| {
        [
            Span::styled(
                format!(" {k} "),
                Style::new()
                    .fg(THEME.bg)
                    .bg(THEME.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {d}  "), Style::new().fg(THEME.dim)),
        ]
    };
    let view_tab = |label: &str, fkey: &str, active: bool| {
        let style = if active {
            Style::new()
                .fg(THEME.bg)
                .bg(THEME.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(THEME.dim)
        };
        Span::styled(format!(" {fkey} {label} "), style)
    };
    let mut spans: Vec<Span> = vec![
        view_tab("MON", "F1", app.view == View::Monitor),
        Span::raw(" "),
        view_tab("PORT", "F2", app.view == View::Portfolio),
        Span::raw(" "),
        view_tab("BT", "F3", app.view == View::Backtest),
        Span::raw(" "),
        view_tab("RISK", "F4", app.view == View::Risk),
        Span::raw(" "),
        view_tab("OPT", "F5", app.view == View::Optimizer),
        Span::raw(" "),
        view_tab("OPTS", "F6", app.view == View::Options),
        Span::raw("   "),
    ];
    spans.extend(hint("/", "cmd"));
    spans.extend(hint("?", "help"));
    spans.extend(hint("q", "quit"));
    if app.view == View::Monitor {
        spans.push(Span::styled(
            format!("  {focus}"),
            Style::new().fg(THEME.accent_alt),
        ));
    }

    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::new().bg(THEME.panel_bg)),
        area,
    );
}

fn draw_help(frame: &mut Frame, full: Rect) {
    let w = 64u16.min(full.width.saturating_sub(4));
    let h = 27u16.min(full.height.saturating_sub(2));
    let x = full.x + full.width.saturating_sub(w) / 2;
    let y = full.y + full.height.saturating_sub(h) / 2;
    let area = Rect::new(x, y, w, h);

    frame.render_widget(Clear, area);

    let row = |k: &str, d: &str, key_color: ratatui::style::Color| {
        Line::from(vec![
            Span::styled(
                format!("  {k:<16}"),
                Style::new().fg(key_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(d.to_string(), Style::new().fg(THEME.fg)),
        ])
    };
    let key = |k: &str, d: &str| row(k, d, THEME.accent);
    let cmd = |k: &str, d: &str| row(k, d, THEME.accent_alt);
    let head = |t: &str| Line::from(Span::styled(format!("  {t}"), THEME.bold(THEME.heading)));

    let lines = vec![
        Line::raw(""),
        head("KEYS"),
        key("Tab / S-Tab", "cycle panel focus"),
        key("↑ ↓  /  k j", "move watchlist selection"),
        key("1 2 3 4 5", "timeframe  1D 5D 1M 6M 1Y"),
        key("F1 – F6", "screen  mon / port / bt / risk / opt / options"),
        key("c", "toggle candle / line chart"),
        key("s", "cycle backtest strategy"),
        key("/  or  :", "focus the command line"),
        key("?", "toggle this help"),
        key("q / Ctrl-C", "quit"),
        Line::raw(""),
        head("COMMANDS  (type after /)"),
        cmd("AAPL · AAPL GP", "load a symbol's chart"),
        cmd("add NVDA", "add symbol to watchlist"),
        cmd("rm TSLA", "remove symbol"),
        cmd("tf 6M", "set timeframe"),
        cmd("chart line", "set chart style"),
        cmd("risk opt options", "switch screen"),
        cmd("q", "quit"),
        Line::raw(""),
        Line::from(Span::styled(
            "  press any key to close",
            Style::new().fg(THEME.dim),
        )),
    ];

    let block = Block::bordered()
        .border_type(BorderType::Double)
        .border_style(Style::new().fg(THEME.accent))
        .title(Span::styled(" HELP ", THEME.bold(THEME.heading)))
        .style(Style::new().bg(THEME.panel_bg));

    frame.render_widget(
        Paragraph::new(lines).block(block).wrap(Wrap { trim: true }),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analytics::ChartSeries;
    use crate::app::ChartType;
    use crate::data::{Candle, Quote, Timeframe};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn sample_app() -> App {
        let mut app = App::new(crate::config::Config::default());
        let sym = app.active_symbol().unwrap().to_string();
        app.quotes.insert(
            sym.clone(),
            Quote {
                symbol: sym.clone(),
                last: 228.5,
                prev_close: 227.0,
                bid: 228.4,
                ask: 228.6,
                bid_size: 3,
                ask_size: 2,
                open: 227.2,
                high: 229.1,
                low: 226.8,
                volume: 52_000_000,
            },
        );
        let candles: Vec<Candle> = (0..120)
            .map(|i| {
                let base = 220.0 + ((i as f64) * 0.05).sin() * 8.0;
                Candle {
                    time: 1_700_000_000 + i * 300,
                    open: base,
                    high: base + 1.2,
                    low: base - 1.1,
                    close: base + 0.3,
                    volume: 1_000_000 + (i as u64) * 1000,
                }
            })
            .collect();
        app.series = Some(ChartSeries::build(sym, Timeframe::D1, candles));
        app
    }

    #[test]
    fn renders_without_panicking_at_many_sizes() {
        let mut app = sample_app();
        for (w, h) in [(160u16, 48u16), (120, 40), (80, 24), (40, 12), (16, 6)] {
            let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
            term.draw(|f| draw(f, &app)).unwrap();
        }
        // exercise the line-chart path and the help overlay too
        app.chart_type = ChartType::Line;
        app.show_help = true;
        let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
        term.draw(|f| draw(f, &app)).unwrap();

        // portfolio + backtest screens
        app.show_help = false;
        app.positions = vec![crate::data::Position {
            symbol: "AAPL".into(),
            qty: 100.0,
            avg_cost: 210.0,
            last: 228.5,
        }];
        app.account = Some(crate::data::AccountSummary {
            account: "TEST".into(),
            net_liquidation: 100_000.0,
            total_cash: 50_000.0,
            buying_power: 200_000.0,
        });
        for view in [crate::app::View::Portfolio, crate::app::View::Backtest] {
            app.view = view;
            let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
            term.draw(|f| draw(f, &app)).unwrap();
        }

        // risk + optimizer screens (need quant analytics)
        let s1: Vec<f64> = (0..160)
            .map(|i| 100.0 + (i as f64 * 0.1).sin() * 6.0)
            .collect();
        let s2: Vec<f64> = (0..160)
            .map(|i| 200.0 + (i as f64 * 0.13).cos() * 9.0)
            .collect();
        let bench: Vec<f64> = (0..160).map(|i| 300.0 + i as f64 * 0.1).collect();
        app.quant = crate::analytics::QuantData::build(
            &[("AAPL".into(), s1), ("MSFT".into(), s2)],
            &bench,
            252.0,
        );
        for view in [
            crate::app::View::Risk,
            crate::app::View::Optimizer,
            crate::app::View::Options,
        ] {
            app.view = view;
            let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
            term.draw(|f| draw(f, &app)).unwrap();
        }
    }
}
