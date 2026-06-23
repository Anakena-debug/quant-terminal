//! The reducer: applies an [`Action`] to mutate [`App`] state.

use crate::action::Action;
use crate::app::{App, View};

pub fn update(app: &mut App, action: Action) {
    match action {
        Action::Quit => app.running = false,
        Action::ToggleHelp => app.show_help = !app.show_help,
        Action::FocusNext => app.focus = app.focus.next(),
        Action::FocusPrev => app.focus = app.focus.prev(),
        Action::SelectNext => app.select_next(),
        Action::SelectPrev => app.select_prev(),
        Action::ToggleChartType => app.chart_type = app.chart_type.toggle(),
        Action::CycleStrategy => app.strategy = app.strategy.next(),
        Action::SetTimeframe(tf) => app.timeframe = tf,
        Action::SetView(view) => {
            app.view = view;
            match view {
                View::Portfolio => app.ensure_portfolio_loaded(),
                View::Risk | View::Optimizer => app.ensure_quant_loaded(),
                _ => {}
            }
        }
    }
}
