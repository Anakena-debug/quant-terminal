//! Risk view: a correlation heatmap + a per-symbol risk-stats table.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Table};

use super::panel_block;
use super::theme::THEME;
use crate::analytics::QuantData;
use crate::app::App;

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let Some(q) = app.quant.as_ref() else {
        let block = panel_block("RISK", false);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "  computing analytics from 1Y daily history…  (F1 to return)",
                Style::new().fg(THEME.dim),
            ))),
            inner,
        );
        return;
    };

    let heat_rows = q.symbols.len() as u16 + 4; // title + header + n + border
    let [heat, stats] = Layout::vertical([
        Constraint::Length(heat_rows.min(area.height / 2 + 2)),
        Constraint::Min(0),
    ])
    .areas(area);
    draw_heatmap(frame, heat, q);
    draw_stats(frame, stats, q);
}

fn short(s: &str) -> String {
    s.chars().take(5).collect()
}

/// Diverging colour: green for positive correlation, red for negative.
fn corr_color(v: f64) -> Color {
    let v = v.clamp(-1.0, 1.0);
    if v >= 0.0 {
        let t = v;
        Color::Rgb(
            (18.0 + 24.0 * t) as u8,
            (22.0 + 150.0 * t) as u8,
            (26.0 + 60.0 * t) as u8,
        )
    } else {
        let t = -v;
        Color::Rgb(
            (26.0 + 174.0 * t) as u8,
            (22.0 + 28.0 * t) as u8,
            (26.0 + 28.0 * t) as u8,
        )
    }
}

fn draw_heatmap(frame: &mut Frame, area: Rect, q: &QuantData) {
    let n = q.symbols.len();
    let labels: Vec<String> = q.symbols.iter().map(|s| short(s)).collect();
    let label_style = Style::new().fg(THEME.heading).add_modifier(Modifier::BOLD);

    let mut header_cells = vec![Cell::from("")];
    header_cells.extend(
        labels
            .iter()
            .map(|l| Cell::from(l.clone()).style(label_style)),
    );
    let header = Row::new(header_cells);

    let rows: Vec<Row> = (0..n)
        .map(|i| {
            let mut cells = vec![Cell::from(labels[i].clone()).style(label_style)];
            for j in 0..n {
                let v = q.corr[i][j];
                cells.push(
                    Cell::from(format!("{v:+.2}"))
                        .style(Style::new().fg(Color::Rgb(235, 235, 235)).bg(corr_color(v))),
                );
            }
            Row::new(cells)
        })
        .collect();

    let widths = vec![Constraint::Length(6); n + 1];
    frame.render_widget(
        Table::new(rows, widths)
            .header(header)
            .column_spacing(0)
            .block(panel_block("CORRELATION  ·  1Y daily returns", false)),
        area,
    );
}

fn draw_stats(frame: &mut Frame, area: Rect, q: &QuantData) {
    let header = Row::new([
        "SYM", "RET%", "VOL%", "BETA", "SHRP", "SORT", "VaR95", "VaR99", "MAXDD", "SKEW", "KURT",
    ])
    .style(Style::new().fg(THEME.heading).add_modifier(Modifier::BOLD));

    let rows: Vec<Row> = q
        .stats
        .iter()
        .map(|s| {
            Row::new(vec![
                Cell::from(s.symbol.clone()).style(Style::new().fg(THEME.fg)),
                Cell::from(format!("{:+.1}", s.ret_annual))
                    .style(Style::new().fg(THEME.change_color(s.ret_annual))),
                Cell::from(format!("{:.1}", s.vol_annual)),
                Cell::from(format!("{:.2}", s.beta)),
                Cell::from(format!("{:.2}", s.sharpe))
                    .style(Style::new().fg(THEME.change_color(s.sharpe))),
                Cell::from(format!("{:.2}", s.sortino)),
                Cell::from(format!("{:.2}", s.var95)).style(Style::new().fg(THEME.down)),
                Cell::from(format!("{:.2}", s.var99)).style(Style::new().fg(THEME.down)),
                Cell::from(format!("{:.1}", s.max_dd)),
                Cell::from(format!("{:+.2}", s.skew)),
                Cell::from(format!("{:+.2}", s.kurt)),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(7),
        Constraint::Length(7),
        Constraint::Length(7),
        Constraint::Length(6),
        Constraint::Length(6),
        Constraint::Length(6),
        Constraint::Length(7),
        Constraint::Length(7),
        Constraint::Length(7),
        Constraint::Length(7),
        Constraint::Length(7),
    ];

    frame.render_widget(
        Table::new(rows, widths)
            .header(header)
            .column_spacing(1)
            .block(panel_block(
                "RISK STATS  ·  annualized · β vs SPY · VaR95 hist / VaR99 param (1d)",
                false,
            )),
        area,
    );
}
