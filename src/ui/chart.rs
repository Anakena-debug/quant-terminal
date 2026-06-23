//! The price chart and indicator sub-panes.
//!
//! The price pane is a Canvas (Braille markers) so candlesticks, the close
//! line, and the moving-average / Bollinger overlays all share one continuous
//! coordinate space. Price and time axes are drawn manually around it.

use chrono::{TimeZone, Utc};
use chrono_tz::America::New_York;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::widgets::canvas::{Canvas, Context, Line as CanvasLine, Rectangle};

use crate::analytics::ChartSeries;
use crate::app::{App, ChartType, Panel};

use super::theme::THEME;
use super::{fmt_volume, panel_block};

/// Resolve the loaded series for the active symbol/timeframe, or `None` if it
/// hasn't arrived yet (or is stale).
fn active_series(app: &App) -> Option<&ChartSeries> {
    let sym = app.active_symbol()?;
    app.series
        .as_ref()
        .filter(|s| s.symbol == sym && s.timeframe == app.timeframe)
}

pub fn draw_chart(frame: &mut Frame, area: Rect, app: &App) {
    let sym = app.active_symbol().unwrap_or("—");
    let title = format!(
        "{sym} · {} · {}",
        app.timeframe.label(),
        app.chart_type.label()
    );
    let block = panel_block(&title, app.focus == Panel::Chart);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height < 3 {
        return;
    }

    let [header, body] = Layout::vertical([Constraint::Length(1), Constraint::Min(2)]).areas(inner);
    draw_quote_header(frame, header, app, sym);

    let Some(series) = active_series(app) else {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "  loading history…",
                Style::new().fg(THEME.dim),
            ))),
            body,
        );
        return;
    };
    if series.candles.len() < 2 {
        return;
    }

    let (mut pmin, mut pmax) = (f64::INFINITY, f64::NEG_INFINITY);
    for c in &series.candles {
        pmin = pmin.min(c.low);
        pmax = pmax.max(c.high);
    }
    for band in [&series.bb_upper, &series.bb_lower] {
        for v in band.iter().flatten() {
            pmin = pmin.min(*v);
            pmax = pmax.max(*v);
        }
    }
    let pad = (pmax - pmin).max(1e-6) * 0.05;
    pmin -= pad;
    pmax += pad;
    let n = series.candles.len() as f64;

    // body → [ plot row | time axis ]; plot row → [ price gutter | canvas ]
    let [plot_row, time_axis] =
        Layout::vertical([Constraint::Min(2), Constraint::Length(1)]).areas(body);
    let [price_axis, plot] =
        Layout::horizontal([Constraint::Length(9), Constraint::Min(4)]).areas(plot_row);

    draw_price_axis(frame, price_axis, pmin, pmax);
    draw_time_axis(frame, time_axis, series, price_axis.width);

    let chart_type = app.chart_type;
    let canvas = Canvas::default()
        .marker(Marker::Braille)
        .x_bounds([0.0, n])
        .y_bounds([pmin, pmax])
        .paint(move |ctx| {
            // Bollinger band first (background), then MAs, then price on top.
            polyline(ctx, &series.bb_upper, THEME.bb);
            polyline(ctx, &series.bb_lower, THEME.bb);
            polyline(ctx, &series.bb_mid, THEME.bb);

            match chart_type {
                ChartType::Candle => {
                    for (i, c) in series.candles.iter().enumerate() {
                        let x = i as f64 + 0.5;
                        let up = c.close >= c.open;
                        let color = if up { THEME.up } else { THEME.down };
                        // wick
                        ctx.draw(&CanvasLine {
                            x1: x,
                            y1: c.low,
                            x2: x,
                            y2: c.high,
                            color,
                        });
                        // body
                        let body_lo = c.open.min(c.close);
                        let height = (c.open - c.close).abs().max((pmax - pmin) * 0.002);
                        ctx.draw(&Rectangle {
                            x: x - 0.3,
                            y: body_lo,
                            width: 0.6,
                            height,
                            color,
                        });
                    }
                }
                ChartType::Line => {
                    let closes: Vec<Option<f64>> = series.closes.iter().map(|c| Some(*c)).collect();
                    polyline(ctx, &closes, THEME.accent);
                }
            }

            polyline(ctx, &series.sma20, THEME.sma);
            polyline(ctx, &series.ema50, THEME.ema);
        });

    frame.render_widget(canvas, plot);
}

/// The big price + change readout above the plot.
fn draw_quote_header(frame: &mut Frame, area: Rect, app: &App, sym: &str) {
    let line = match app.quotes.get(sym) {
        Some(q) => {
            let pct = q.change_pct();
            let change = q.change();
            let color = THEME.change_color(pct);
            let arrow = if change >= 0.0 { "▲" } else { "▼" };
            Line::from(vec![
                Span::styled(format!("  {sym} "), THEME.bold(THEME.heading)),
                Span::styled(format!("{:.2}  ", q.last), THEME.bold(color)),
                Span::styled(
                    format!("{arrow} {change:+.2} ({pct:+.2}%)"),
                    Style::new().fg(color),
                ),
                Span::styled(
                    format!(
                        "    O {:.2}  H {:.2}  L {:.2}  Vol {}",
                        q.open,
                        q.high,
                        q.low,
                        fmt_volume(q.volume)
                    ),
                    Style::new().fg(THEME.dim),
                ),
            ])
        }
        None => Line::from(Span::styled("  loading…", Style::new().fg(THEME.dim))),
    };
    frame.render_widget(Paragraph::new(line), area);
}

/// Draw a connected line through the defined points of an indicator series,
/// breaking the line across `None` gaps.
fn polyline(ctx: &mut Context<'_>, values: &[Option<f64>], color: Color) {
    let mut prev: Option<(f64, f64)> = None;
    for (i, v) in values.iter().enumerate() {
        match v {
            Some(y) => {
                let point = (i as f64 + 0.5, *y);
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
            None => prev = None,
        }
    }
}

fn draw_price_axis(frame: &mut Frame, area: Rect, pmin: f64, pmax: f64) {
    let h = area.height;
    if h == 0 {
        return;
    }
    let denom = (h as f64 - 1.0).max(1.0);
    let mut lines = Vec::with_capacity(h as usize);
    for row in 0..h {
        let show = row == 0 || row == h - 1 || row == h / 2 || row == h / 4 || row == 3 * h / 4;
        let text = if show {
            let frac = 1.0 - row as f64 / denom;
            format!("{:>8.2}", pmin + (pmax - pmin) * frac)
        } else {
            String::new()
        };
        lines.push(Line::from(Span::styled(text, Style::new().fg(THEME.dim))));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn fmt_time(secs: i64, app_intraday: bool) -> String {
    match Utc.timestamp_opt(secs, 0).single() {
        Some(dt) => {
            let et = dt.with_timezone(&New_York);
            if app_intraday {
                et.format("%H:%M").to_string()
            } else {
                et.format("%m/%d").to_string()
            }
        }
        None => String::new(),
    }
}

fn draw_time_axis(frame: &mut Frame, area: Rect, series: &ChartSeries, left_offset: u16) {
    use crate::data::Timeframe;
    let intraday = matches!(series.timeframe, Timeframe::D1 | Timeframe::D5);
    let candles = &series.candles;

    let [_, labels_area] =
        Layout::horizontal([Constraint::Length(left_offset), Constraint::Min(1)]).areas(area);
    let w = labels_area.width as usize;
    if w < 6 || candles.is_empty() {
        return;
    }

    let left = fmt_time(candles.first().unwrap().time, intraday);
    let mid = fmt_time(candles[candles.len() / 2].time, intraday);
    let right = fmt_time(candles.last().unwrap().time, intraday);

    let mut buf = vec![' '; w];
    let place = |buf: &mut Vec<char>, pos: usize, text: &str| {
        for (k, ch) in text.chars().enumerate() {
            if pos + k < buf.len() {
                buf[pos + k] = ch;
            }
        }
    };
    place(&mut buf, 0, &left);
    place(&mut buf, w.saturating_sub(right.len()), &right);
    place(&mut buf, w / 2 - (mid.len() / 2).min(w / 2), &mid);

    let s: String = buf.into_iter().collect();
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(s, Style::new().fg(THEME.dim)))),
        labels_area,
    );
}

// ── indicator sub-panes ─────────────────────────────────────────────────────

pub fn draw_indicators(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel_block("INDICATORS", false);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(series) = active_series(app) else {
        return;
    };
    if inner.height < 2 {
        return;
    }

    let [readouts, body] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(inner);
    draw_readouts(frame, readouts, series);

    let [vol, rsi] = Layout::horizontal([Constraint::Min(10), Constraint::Length(28)]).areas(body);
    draw_volume(frame, vol, series);
    draw_rsi(frame, rsi, series);
}

fn fnum(v: Option<f64>) -> String {
    v.map(|x| format!("{x:.2}"))
        .unwrap_or_else(|| "—".to_string())
}

fn draw_readouts(frame: &mut Frame, area: Rect, series: &ChartSeries) {
    let rsi = ChartSeries::last(&series.rsi14);
    let macd = ChartSeries::last(&series.macd);
    let signal = ChartSeries::last(&series.macd_signal);
    let hist = ChartSeries::last(&series.macd_hist);

    let rsi_color = match rsi {
        Some(v) if v >= 70.0 => THEME.down,
        Some(v) if v <= 30.0 => THEME.up,
        _ => THEME.fg,
    };
    let hist_color = match hist {
        Some(v) if v >= 0.0 => THEME.up,
        Some(_) => THEME.down,
        None => THEME.dim,
    };
    let arrow = match hist {
        Some(v) if v >= 0.0 => "▲",
        Some(_) => "▼",
        None => " ",
    };

    let label = |s: &str| Span::styled(format!("  {s} "), Style::new().fg(THEME.dim));
    let val =
        |s: String, c: Color| Span::styled(s, Style::new().fg(c).add_modifier(Modifier::BOLD));

    let spans = vec![
        label("RSI"),
        val(fnum(rsi), rsi_color),
        label("MACD"),
        val(fnum(macd), THEME.fg),
        label("sig"),
        val(format!("{} {arrow}", fnum(signal)), hist_color),
        label("SMA20"),
        val(fnum(ChartSeries::last(&series.sma20)), THEME.sma),
        label("EMA50"),
        val(fnum(ChartSeries::last(&series.ema50)), THEME.ema),
        label("BB"),
        val(
            format!(
                "{} / {}",
                fnum(ChartSeries::last(&series.bb_upper)),
                fnum(ChartSeries::last(&series.bb_lower))
            ),
            THEME.bb,
        ),
    ];
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_volume(frame: &mut Frame, area: Rect, series: &ChartSeries) {
    if area.height == 0 {
        return;
    }
    let n = series.candles.len() as f64;
    let max_vol = series
        .candles
        .iter()
        .map(|c| c.volume)
        .max()
        .unwrap_or(1)
        .max(1) as f64;

    let canvas = Canvas::default()
        .marker(Marker::Braille)
        .x_bounds([0.0, n])
        .y_bounds([0.0, max_vol])
        .paint(move |ctx| {
            for (i, c) in series.candles.iter().enumerate() {
                let color = if c.close >= c.open {
                    THEME.up
                } else {
                    THEME.down
                };
                ctx.draw(&Rectangle {
                    x: i as f64 + 0.2,
                    y: 0.0,
                    width: 0.6,
                    height: c.volume as f64,
                    color,
                });
            }
        });
    frame.render_widget(canvas, area);
}

fn draw_rsi(frame: &mut Frame, area: Rect, series: &ChartSeries) {
    if area.height == 0 {
        return;
    }
    let n = series.rsi14.len() as f64;
    let canvas = Canvas::default()
        .marker(Marker::Braille)
        .x_bounds([0.0, n.max(1.0)])
        .y_bounds([0.0, 100.0])
        .paint(move |ctx| {
            // 30 / 70 reference lines
            for level in [30.0, 70.0] {
                ctx.draw(&CanvasLine {
                    x1: 0.0,
                    y1: level,
                    x2: n,
                    y2: level,
                    color: THEME.border,
                });
            }
            polyline(ctx, &series.rsi14, THEME.accent);
        });
    frame.render_widget(canvas, area);
}
