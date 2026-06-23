//! Markowitz optimizer view: the efficient frontier (vol vs return) with the
//! assets, min-variance and max-Sharpe portfolios, plus the optimal weights.
//!
//! Unconstrained mean-variance (long/short allowed), so weights can be large —
//! that's expected for analytical MVO; treat it as a research lens, not advice.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::widgets::canvas::{Canvas, Points};

use super::panel_block;
use super::theme::THEME;
use crate::analytics::QuantData;
use crate::app::App;

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel_block("OPTIMIZER  ·  Markowitz mean-variance · 1Y daily", false);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(q) = app.quant.as_ref() else {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "  computing analytics from 1Y daily history…  (F1 to return)",
                Style::new().fg(THEME.dim),
            ))),
            inner,
        );
        return;
    };
    if q.cloud.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "  not enough independent history to optimize",
                Style::new().fg(THEME.dim),
            ))),
            inner,
        );
        return;
    }

    let [frontier, sidebar] =
        Layout::horizontal([Constraint::Min(20), Constraint::Length(40)]).areas(inner);
    draw_frontier(frame, frontier, q);
    draw_sidebar(frame, sidebar, q);
}

fn draw_frontier(frame: &mut Frame, area: Rect, q: &QuantData) {
    // bounds over frontier + assets
    let cloud_xy: Vec<(f64, f64)> = q.cloud.iter().map(|&(v, r, _)| (v, r)).collect();
    let (mut xmin, mut xmax, mut ymin, mut ymax) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for &(x, y) in cloud_xy.iter().chain(q.assets.iter()) {
        xmin = xmin.min(x);
        xmax = xmax.max(x);
        ymin = ymin.min(y);
        ymax = ymax.max(y);
    }
    if !xmin.is_finite() {
        return;
    }
    xmin = xmin.min(0.0);
    let xpad = (xmax - xmin).max(1.0) * 0.08;
    let ypad = (ymax - ymin).max(1.0) * 0.10;
    xmax += xpad;
    ymin -= ypad;
    ymax += ypad;

    if area.height < 3 {
        return;
    }
    let [plot_row, xaxis] =
        Layout::vertical([Constraint::Min(2), Constraint::Length(1)]).areas(area);
    let [yaxis, plot] =
        Layout::horizontal([Constraint::Length(7), Constraint::Min(4)]).areas(plot_row);
    axis_y(frame, yaxis, ymin, ymax, "%");
    axis_x(frame, xaxis, 7, xmin, xmax, "vol%");

    let cloud = cloud_xy.clone();
    let assets = q.assets.clone();
    let symbols = q.symbols.clone();
    let mv = (q.min_var.vol, q.min_var.ret);
    let ms = (q.max_sharpe.vol, q.max_sharpe.ret);

    let canvas = Canvas::default()
        .marker(Marker::Braille)
        .x_bounds([xmin, xmax])
        .y_bounds([ymin, ymax])
        .paint(move |ctx| {
            // the random-portfolio cloud (its upper-left edge is the frontier)
            ctx.draw(&Points {
                coords: &cloud,
                color: THEME.border,
            });
            // individual assets (dots + labels)
            ctx.draw(&Points {
                coords: &assets,
                color: THEME.sma,
            });
            for (i, &(x, y)) in assets.iter().enumerate() {
                ctx.print(
                    x,
                    y,
                    Span::styled(format!(" {}", symbols[i]), Style::new().fg(THEME.dim)),
                );
            }
            // key portfolios
            ctx.print(
                mv.0,
                mv.1,
                Span::styled("◆ min-var", Style::new().fg(THEME.sma)),
            );
            ctx.print(
                ms.0,
                ms.1,
                Span::styled(
                    "★ max-Sharpe",
                    Style::new().fg(THEME.up).add_modifier(Modifier::BOLD),
                ),
            );
        });
    frame.render_widget(canvas, plot);
}

fn draw_sidebar(frame: &mut Frame, area: Rect, q: &QuantData) {
    let mut lines = vec![Line::raw("")];

    let port_line = |name: &str, p: &crate::analytics::Portfolio| {
        Line::from(vec![
            Span::styled(format!("  {name:<11}"), THEME.bold(THEME.heading)),
            Span::styled(
                format!(
                    "ret {:>6.1}%  vol {:>5.1}%  SR {:>4.2}",
                    p.ret, p.vol, p.sharpe
                ),
                Style::new().fg(THEME.fg),
            ),
        ])
    };
    lines.push(port_line("Max-Sharpe", &q.max_sharpe));
    lines.push(port_line("Min-Var", &q.min_var));
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "  MAX-SHARPE WEIGHTS  (long/short)",
        THEME.bold(THEME.heading),
    )));

    // weights sorted by magnitude
    let mut weighted: Vec<(String, f64)> = q
        .symbols
        .iter()
        .cloned()
        .zip(q.max_sharpe.weights.iter().copied())
        .collect();
    weighted.sort_by(|a, b| {
        b.1.abs()
            .partial_cmp(&a.1.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let max_abs = weighted
        .iter()
        .map(|(_, w)| w.abs())
        .fold(0.0_f64, f64::max)
        .max(1e-9);

    for (sym, w) in &weighted {
        let pct = w * 100.0;
        let color = if *w >= 0.0 { THEME.up } else { THEME.down };
        let bar_len = ((w.abs() / max_abs) * 12.0).round() as usize;
        let bar: String = "█".repeat(bar_len.min(12));
        lines.push(Line::from(vec![
            Span::styled(format!("  {sym:<6}"), Style::new().fg(THEME.fg)),
            Span::styled(format!("{pct:>7.1}%  "), Style::new().fg(color)),
            Span::styled(bar, Style::new().fg(color)),
        ]));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        format!("  {} obs · rf=0 · unconstrained", q.observations),
        Style::new().fg(THEME.dim),
    )));

    frame.render_widget(
        Paragraph::new(lines).block(panel_block("OPTIMAL PORTFOLIO", false)),
        area,
    );
}

// ── tiny shared axis helpers (kept local to avoid cross-module coupling) ────

fn axis_y(frame: &mut Frame, area: Rect, lo: f64, hi: f64, _unit: &str) {
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
            format!("{:>6.1}", lo + (hi - lo) * frac)
        } else {
            String::new()
        };
        lines.push(Line::from(Span::styled(text, Style::new().fg(THEME.dim))));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn axis_x(frame: &mut Frame, area: Rect, left_offset: u16, lo: f64, hi: f64, label: &str) {
    let [_, labels] =
        Layout::horizontal([Constraint::Length(left_offset), Constraint::Min(1)]).areas(area);
    let w = labels.width as usize;
    if w < 8 {
        return;
    }
    let left = format!("{lo:.0}");
    let right = format!("{hi:.0} {label}");
    let mid_room = w.saturating_sub(left.len() + right.len());
    let text = format!("{left}{}{right}", " ".repeat(mid_room));
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(text, Style::new().fg(THEME.dim)))),
        labels,
    );
}
