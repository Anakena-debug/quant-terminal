//! User-intent actions, decoupled from raw key events so the reducer in
//! [`crate::update`] stays independent of input handling.

use crate::app::View;
use crate::data::Timeframe;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Quit,
    ToggleHelp,
    FocusNext,
    FocusPrev,
    SelectNext,
    SelectPrev,
    ToggleChartType,
    SetTimeframe(Timeframe),
    SetView(View),
    CycleStrategy,
}
