//! Backtest view: strategy stats + equity curve (vs buy-&-hold) + an SMA
//! parameter-sweep heatmap. Press `s` to cycle strategies.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Context, Line as CanvasLine};
use ratatui::widgets::{Cell, Paragraph, Row, Table};

use super::panel_block;
use super::theme::THEME;
use crate::app::App;
use crate::backtest::{self, BacktestResult};

const INITIAL: f64 = 100_000.0;
const MIN_BARS: usize = 60;

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let sym = app.active_symbol().unwrap_or("—");
    let title = format!(
        "BACKTEST  {sym} · {} · {}   (s: strategy)",
        app.strategy.label(),
        app.timeframe.label()
    );
    let block = panel_block(&title, false);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let series = app.series.as_ref().filter(|s| s.symbol == sym);
    let Some(series) = series else {
        msg(frame, inner, "  loading history…");
        return;
    };
    if series.closes.len() < MIN_BARS {
        msg(
            frame,
            inner,
            "  not enough history — switch to a longer timeframe (5 / 1Y)",
        );
        return;
    }

    let result = backtest::run_strategy(&series.closes, app.strategy, INITIAL);
    let [stats, body] = Layout::vertical([Constraint::Length(4), Constraint::Min(0)]).areas(inner);
    draw_stats(frame, stats, &result);

    let [curve, sweep] =
        Layout::horizontal([Constraint::Min(20), Constraint::Length(46)]).areas(body);
    draw_curve(frame, curve, &result);
    draw_sweep(frame, sweep, &series.closes);
}

fn msg(frame: &mut Frame, area: Rect, text: &str) {
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(text, Style::new().fg(THEME.dim)))),
        area,
    );
}

fn draw_stats(frame: &mut Frame, area: Rect, r: &BacktestResult) {
    let ret = r.total_return_pct();
    let bh = r.buy_hold_return_pct();
    let kv = |k: &str, v: String, c: Color| {
        [
            Span::styled(format!("  {k} "), Style::new().fg(THEME.dim)),
            Span::styled(v, Style::new().fg(c).add_modifier(Modifier::BOLD)),
        ]
    };
    let line1: Vec<Span> = [
        kv("Final", format!("${:.0}", r.final_equity()), THEME.fg),
        kv("Return", format!("{ret:+.1}%"), THEME.change_color(ret)),
        kv("Buy&Hold", format!("{bh:+.1}%"), THEME.change_color(bh)),
        kv(
            "vs B&H",
            format!("{:+.1}%", ret - bh),
            THEME.change_color(ret - bh),
        ),
    ]
    .into_iter()
    .flatten()
    .collect();
    let line2: Vec<Span> = [
        kv(
            "MaxDD",
            format!("-{:.1}%", r.max_drawdown_pct()),
            THEME.down,
        ),
        kv("Sharpe", format!("{:.2}", r.sharpe()), THEME.fg),
        kv("Trades", r.num_trades().to_string(), THEME.fg),
        kv("Win", format!("{:.0}%", r.win_rate_pct()), THEME.fg),
    ]
    .into_iter()
    .flatten()
    .collect();
    let legend = Line::from(vec![
        Span::styled("  ── ", Style::new().fg(THEME.accent)),
        Span::styled("strategy   ", Style::new().fg(THEME.dim)),
        Span::styled("── ", Style::new().fg(THEME.dim)),
        Span::styled("buy & hold", Style::new().fg(THEME.dim)),
    ]);
    frame.render_widget(
        Paragraph::new(vec![Line::from(line1), Line::from(line2), legend]),
        area,
    );
}

fn draw_curve(frame: &mut Frame, area: Rect, r: &BacktestResult) {
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for &e in r.equity.iter().chain(r.buy_hold.iter()) {
        lo = lo.min(e);
        hi = hi.max(e);
    }
    if !lo.is_finite() || area.height < 2 {
        return;
    }
    let pad = (hi - lo).max(1.0) * 0.05;
    lo -= pad;
    hi += pad;
    let n = r.equity.len() as f64;

    let [yaxis, plot] = Layout::horizontal([Constraint::Length(8), Constraint::Min(4)]).areas(area);
    draw_yaxis(frame, yaxis, lo, hi);

    let equity = r.equity.clone();
    let buy_hold = r.buy_hold.clone();
    let canvas = Canvas::default()
        .marker(Marker::Braille)
        .x_bounds([0.0, n])
        .y_bounds([lo, hi])
        .paint(move |ctx| {
            polyline(ctx, &buy_hold, THEME.dim);
            polyline(ctx, &equity, THEME.accent);
        });
    frame.render_widget(canvas, plot);
}

fn draw_sweep(frame: &mut Frame, area: Rect, closes: &[f64]) {
    let fasts = [5usize, 10, 15, 20, 30, 50];
    let slows = [20usize, 30, 50, 80, 120, 150];
    let grid = backtest::sma_sweep(closes, &fasts, &slows, INITIAL);

    let (mut lo, mut hi) = (f64::MAX, f64::MIN);
    for row in &grid {
        for &v in row {
            if v.is_finite() {
                lo = lo.min(v);
                hi = hi.max(v);
            }
        }
    }
    let range = (hi - lo).max(1e-9);

    let label_style = Style::new().fg(THEME.heading).add_modifier(Modifier::BOLD);
    let mut header_cells = vec![Cell::from("f\\s").style(Style::new().fg(THEME.dim))];
    header_cells.extend(
        slows
            .iter()
            .map(|s| Cell::from(s.to_string()).style(label_style)),
    );
    let header = Row::new(header_cells);

    let rows: Vec<Row> = fasts
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let mut cells = vec![Cell::from(f.to_string()).style(label_style)];
            for &v in &grid[i] {
                if v.is_finite() {
                    let t = (v - lo) / range;
                    cells.push(
                        Cell::from(format!("{v:+.0}"))
                            .style(Style::new().fg(Color::Rgb(15, 15, 15)).bg(heat_color(t))),
                    );
                } else {
                    cells.push(Cell::from(" ·").style(Style::new().fg(THEME.dim)));
                }
            }
            Row::new(cells)
        })
        .collect();

    let widths = vec![Constraint::Length(5); slows.len() + 1];
    frame.render_widget(
        Table::new(rows, widths)
            .header(header)
            .column_spacing(0)
            .block(panel_block(
                "SMA SWEEP · return% · fast\\slow (overfit risk)",
                false,
            )),
        area,
    );
}

/// Red → yellow → green for `t` in `[0, 1]`.
fn heat_color(t: f64) -> Color {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        let u = t * 2.0;
        Color::Rgb(205, (50.0 + 150.0 * u) as u8, 45)
    } else {
        let u = (t - 0.5) * 2.0;
        Color::Rgb((205.0 - 160.0 * u) as u8, 200, 55)
    }
}

fn draw_yaxis(frame: &mut Frame, area: Rect, lo: f64, hi: f64) {
    let h = area.height;
    if h == 0 {
        return;
    }
    let denom = (h as f64 - 1.0).max(1.0);
    let mut lines = Vec::with_capacity(h as usize);
    for row in 0..h {
        let show = row == 0 || row == h - 1 || row == h / 2;
        let text = if show {
            let frac = 1.0 - row as f64 / denom;
            format!("{:>7}", money_short(lo + (hi - lo) * frac))
        } else {
            String::new()
        };
        lines.push(Line::from(Span::styled(text, Style::new().fg(THEME.dim))));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn money_short(v: f64) -> String {
    if v.abs() >= 1e6 {
        format!("${:.2}M", v / 1e6)
    } else if v.abs() >= 1e3 {
        format!("${:.0}k", v / 1e3)
    } else {
        format!("${v:.0}")
    }
}

fn polyline(ctx: &mut Context<'_>, values: &[f64], color: Color) {
    let mut prev: Option<(f64, f64)> = None;
    for (i, &v) in values.iter().enumerate() {
        let point = (i as f64, v);
        if let Some(p) = prev {
            ctx.draw(&CanvasLine {
                x1: p.0,
                y1: p.1,
                x2: point.0,
                y2: point.1,
                color,
            });
        }
        prev = Some(point);
    }
}
