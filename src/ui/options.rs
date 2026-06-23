//! Options view: spot/expiry/rate/ATM-IV header, an IV smile chart, and a
//! Black-Scholes greeks chain. The chain is synthetic (BS-consistent with an
//! equity skew); the greeks/IV math is exact.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Line as CanvasLine};
use ratatui::widgets::{Cell, Paragraph, Row, Table};

use super::panel_block;
use super::theme::THEME;
use crate::analytics::options::{self, OptionChain};
use crate::app::App;

const DAYS: u32 = 30;
const RATE: f64 = 0.045;

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let sym = app.active_symbol().unwrap_or("—");
    let block = panel_block(&format!("OPTIONS  ·  {sym}"), false);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(spot) = spot_for(app, sym) else {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "  no quote yet for this symbol",
                Style::new().fg(THEME.dim),
            ))),
            inner,
        );
        return;
    };

    let base_iv = base_iv(app, sym);
    let chain = options::synthetic_chain(spot, DAYS, RATE, base_iv);

    if inner.height < 4 {
        return;
    }
    let [header, body] = Layout::vertical([Constraint::Length(1), Constraint::Min(3)]).areas(inner);
    draw_header(frame, header, &chain);

    let [smile, table] = Layout::vertical([
        Constraint::Length(11.min(body.height / 2 + 3)),
        Constraint::Min(3),
    ])
    .areas(body);
    draw_smile(frame, smile, &chain);
    draw_chain(frame, table, &chain);
}

/// Spot from the latest quote, falling back to the last charted close.
fn spot_for(app: &App, sym: &str) -> Option<f64> {
    app.quotes.get(sym).map(|q| q.last).or_else(|| {
        app.series
            .as_ref()
            .filter(|s| s.symbol == sym)?
            .closes
            .last()
            .copied()
    })
}

/// A plausible ATM implied vol: realized vol from quant analytics if available,
/// else a deterministic per-symbol value. (Real IV needs live option prices.)
fn base_iv(app: &App, sym: &str) -> f64 {
    if let Some(q) = &app.quant
        && let Some(s) = q.stats.iter().find(|s| s.symbol == sym)
    {
        return (s.vol_annual / 100.0).clamp(0.12, 1.0);
    }
    let h = sym
        .bytes()
        .fold(0u32, |a, b| a.wrapping_mul(131).wrapping_add(b as u32));
    0.22 + (h % 24) as f64 / 100.0
}

/// Implied vol (in %) recovered from a call price via the BS solver.
fn iv_pct(chain: &OptionChain, strike: f64, call_price: f64, t: f64) -> f64 {
    options::implied_vol(call_price, chain.spot, strike, t, chain.rate, true).unwrap_or(0.0) * 100.0
}

fn draw_header(frame: &mut Frame, area: Rect, chain: &OptionChain) {
    let kv = |k: &str, v: String, c: Color| {
        [
            Span::styled(format!("  {k} "), Style::new().fg(THEME.dim)),
            Span::styled(v, Style::new().fg(c).add_modifier(Modifier::BOLD)),
        ]
    };
    let spans: Vec<Span> = [
        kv("Spot", format!("{:.2}", chain.spot), THEME.heading),
        kv("Expiry", format!("{}d", chain.days), THEME.fg),
        kv("Rate", format!("{:.1}%", chain.rate * 100.0), THEME.fg),
        kv(
            "ATM IV",
            format!("{:.1}%", chain.atm_iv * 100.0),
            THEME.accent,
        ),
    ]
    .into_iter()
    .flatten()
    .collect();
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_smile(frame: &mut Frame, area: Rect, chain: &OptionChain) {
    let block = panel_block("IV SMILE · implied vol vs strike", false);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height < 2 || chain.rows.len() < 2 {
        return;
    }

    let t = chain.days as f64 / 365.0;
    let xs: Vec<f64> = chain.rows.iter().map(|r| r.strike).collect();
    let ys: Vec<f64> = chain
        .rows
        .iter()
        .map(|r| iv_pct(chain, r.strike, r.call.price, t))
        .collect();
    let (xmin, xmax) = (xs[0], *xs.last().unwrap());
    let (mut ymin, mut ymax) = (f64::MAX, f64::MIN);
    for &y in &ys {
        ymin = ymin.min(y);
        ymax = ymax.max(y);
    }
    let pad = (ymax - ymin).max(1.0) * 0.15;
    ymin -= pad;
    ymax += pad;

    let [yaxis, plot] =
        Layout::horizontal([Constraint::Length(6), Constraint::Min(4)]).areas(inner);
    draw_yaxis(frame, yaxis, ymin, ymax);

    let pts: Vec<(f64, f64)> = xs.iter().zip(&ys).map(|(&x, &y)| (x, y)).collect();
    let spot = chain.spot;
    let canvas = Canvas::default()
        .marker(Marker::Braille)
        .x_bounds([xmin, xmax])
        .y_bounds([ymin, ymax])
        .paint(move |ctx| {
            for w in pts.windows(2) {
                ctx.draw(&CanvasLine {
                    x1: w[0].0,
                    y1: w[0].1,
                    x2: w[1].0,
                    y2: w[1].1,
                    color: THEME.accent,
                });
            }
            // ATM marker
            if let Some(p) = pts
                .iter()
                .min_by(|a, b| (a.0 - spot).abs().partial_cmp(&(b.0 - spot).abs()).unwrap())
            {
                ctx.print(p.0, p.1, Span::styled("◆ ATM", Style::new().fg(THEME.up)));
            }
        });
    frame.render_widget(canvas, plot);
}

fn draw_chain(frame: &mut Frame, area: Rect, chain: &OptionChain) {
    let t = chain.days as f64 / 365.0;
    let header = Row::new([
        "STRIKE", "IV%", "C-Px", "C-Δ", "C-Γ", "C-ν", "C-Θ", "P-Px", "P-Δ", "P-Θ",
    ])
    .style(Style::new().fg(THEME.heading).add_modifier(Modifier::BOLD));

    // ATM = strike closest to spot
    let atm_idx = chain
        .rows
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            (a.strike - chain.spot)
                .abs()
                .partial_cmp(&(b.strike - chain.spot).abs())
                .unwrap()
        })
        .map(|(i, _)| i)
        .unwrap_or(0);

    let rows: Vec<Row> = chain
        .rows
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let c = &r.call;
            let p = &r.put;
            let row = Row::new(vec![
                Cell::from(format!("{:.1}", r.strike))
                    .style(Style::new().fg(THEME.heading).add_modifier(Modifier::BOLD)),
                Cell::from(format!("{:.1}", iv_pct(chain, r.strike, c.price, t)))
                    .style(Style::new().fg(THEME.accent)),
                Cell::from(format!("{:.2}", c.price)),
                Cell::from(format!("{:.2}", c.delta)).style(Style::new().fg(THEME.up)),
                Cell::from(format!("{:.3}", c.gamma)),
                Cell::from(format!("{:.2}", c.vega * 0.01)),
                Cell::from(format!("{:.2}", c.theta / 365.0)).style(Style::new().fg(THEME.down)),
                Cell::from(format!("{:.2}", p.price)),
                Cell::from(format!("{:.2}", p.delta)).style(Style::new().fg(THEME.down)),
                Cell::from(format!("{:.2}", p.theta / 365.0)).style(Style::new().fg(THEME.down)),
            ]);
            if i == atm_idx {
                row.style(Style::new().bg(THEME.selection_bg))
            } else {
                row
            }
        })
        .collect();

    let widths = [
        Constraint::Length(8),
        Constraint::Length(6),
        Constraint::Length(7),
        Constraint::Length(6),
        Constraint::Length(6),
        Constraint::Length(6),
        Constraint::Length(6),
        Constraint::Length(7),
        Constraint::Length(6),
        Constraint::Length(6),
    ];
    frame.render_widget(
        Table::new(rows, widths)
            .header(header)
            .column_spacing(1)
            .block(panel_block(
                "GREEKS CHAIN · ν per 1% · Θ per day · BS synthetic",
                false,
            )),
        area,
    );
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
            format!("{:>4.0}%", lo + (hi - lo) * frac)
        } else {
            String::new()
        };
        lines.push(Line::from(Span::styled(text, Style::new().fg(THEME.dim))));
    }
    frame.render_widget(Paragraph::new(lines), area);
}
