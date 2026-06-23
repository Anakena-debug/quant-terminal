//! Portfolio view: account summary header + positions table.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Table};

use super::panel_block;
use super::theme::THEME;
use crate::app::App;

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let [summary, positions] =
        Layout::vertical([Constraint::Length(4), Constraint::Min(0)]).areas(area);
    draw_summary(frame, summary, app);
    draw_positions(frame, positions, app);
}

/// Group an integer dollar amount with thousands separators, e.g. `$1,234,567`.
fn money(v: f64) -> String {
    let negative = v < 0.0;
    let digits = (v.abs().round() as i64).to_string();
    let bytes = digits.as_bytes();
    let mut grouped = String::new();
    for (i, c) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(*c as char);
    }
    format!("{}${grouped}", if negative { "-" } else { "" })
}

fn signed_money(v: f64) -> String {
    if v >= 0.0 {
        format!("+{}", money(v))
    } else {
        money(v)
    }
}

fn draw_summary(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel_block("ACCOUNT", false);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let unrealized: f64 = app.positions.iter().map(|p| p.unrealized()).sum();
    let market_value: f64 = app.positions.iter().map(|p| p.market_value()).sum();
    let (account, net_liq, cash, buying_power) = app
        .account
        .as_ref()
        .map(|a| {
            (
                a.account.clone(),
                a.net_liquidation,
                a.total_cash,
                a.buying_power,
            )
        })
        .unwrap_or_else(|| ("—".to_string(), 0.0, 0.0, 0.0));

    let kv = |k: &str, v: String, color| {
        [
            Span::styled(format!("  {k} "), Style::new().fg(THEME.dim)),
            Span::styled(v, Style::new().fg(color).add_modifier(Modifier::BOLD)),
        ]
    };

    let line1: Vec<Span> = [
        kv("Account", account, THEME.heading),
        kv("NetLiq", money(net_liq), THEME.fg),
        kv("Cash", money(cash), THEME.fg),
        kv("BuyPwr", money(buying_power), THEME.fg),
    ]
    .into_iter()
    .flatten()
    .collect();

    let line2: Vec<Span> = [
        kv("Positions", app.positions.len().to_string(), THEME.fg),
        kv("Mkt Value", money(market_value), THEME.fg),
        kv(
            "Unrealized P&L",
            signed_money(unrealized),
            THEME.change_color(unrealized),
        ),
    ]
    .into_iter()
    .flatten()
    .collect();

    frame.render_widget(
        Paragraph::new(vec![Line::from(line1), Line::from(line2)]),
        inner,
    );
}

fn draw_positions(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel_block("POSITIONS", true);

    if app.positions.is_empty() {
        let msg = "  loading positions…  (F1 to return to the monitor)";
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(msg, Style::new().fg(THEME.dim)))).block(block),
            area,
        );
        return;
    }

    let header = Row::new([
        "SYMBOL",
        "QTY",
        "AVG COST",
        "LAST",
        "MKT VALUE",
        "UNREAL P&L",
        "%",
    ])
    .style(Style::new().fg(THEME.heading).add_modifier(Modifier::BOLD));

    let rows: Vec<Row> = app
        .positions
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let pnl = p.unrealized();
            let pnl_style = Style::new().fg(THEME.change_color(pnl));
            let selected = i == app.pos_selected;
            let sym_style = if selected {
                Style::new().fg(THEME.accent).add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(THEME.fg)
            };
            let row = Row::new(vec![
                Cell::from(p.symbol.clone()).style(sym_style),
                Cell::from(format!("{:.0}", p.qty)),
                Cell::from(format!("{:.2}", p.avg_cost)),
                Cell::from(format!("{:.2}", p.last)),
                Cell::from(money(p.market_value())),
                Cell::from(signed_money(pnl)).style(pnl_style),
                Cell::from(format!("{:+.2}%", p.unrealized_pct())).style(pnl_style),
            ]);
            if selected {
                row.style(Style::new().bg(THEME.selection_bg))
            } else {
                row
            }
        })
        .collect();

    let widths = [
        Constraint::Length(8),
        Constraint::Length(9),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(14),
        Constraint::Length(14),
        Constraint::Length(9),
    ];

    frame.render_widget(
        Table::new(rows, widths)
            .header(header)
            .column_spacing(1)
            .block(block),
        area,
    );
}
