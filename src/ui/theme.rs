//! Bloomberg-inspired palette, shared style helpers, and the ET market clock.

use chrono::{DateTime, Datelike, Timelike, Utc};
use chrono_tz::America::New_York;
use chrono_tz::Tz;
use ratatui::style::{Color, Modifier, Style};

pub struct Theme {
    pub bg: Color,
    pub panel_bg: Color,
    pub fg: Color,
    pub dim: Color,
    pub border: Color,
    pub border_focus: Color,
    pub accent: Color,
    pub accent_alt: Color,
    pub heading: Color,
    pub up: Color,
    pub down: Color,
    pub sma: Color,
    pub ema: Color,
    pub bb: Color,
    pub selection_bg: Color,
}

/// The single global theme. `const` so it costs nothing at runtime.
pub const THEME: Theme = Theme {
    bg: Color::Rgb(8, 9, 12),
    panel_bg: Color::Rgb(12, 13, 17),
    fg: Color::Rgb(220, 220, 208),
    dim: Color::Rgb(120, 122, 130),
    border: Color::Rgb(70, 62, 34),
    border_focus: Color::Rgb(255, 176, 0),
    accent: Color::Rgb(255, 176, 0),
    accent_alt: Color::Rgb(255, 122, 26),
    heading: Color::Rgb(255, 198, 64),
    up: Color::Rgb(46, 204, 113),
    down: Color::Rgb(231, 76, 60),
    sma: Color::Rgb(80, 200, 220),
    ema: Color::Rgb(200, 130, 255),
    bb: Color::Rgb(96, 104, 124),
    selection_bg: Color::Rgb(38, 32, 8),
};

impl Theme {
    pub fn bold(&self, c: Color) -> Style {
        Style::new().fg(c).add_modifier(Modifier::BOLD)
    }

    /// Green for gains, red for losses, dim for flat.
    pub fn change_color(&self, delta: f64) -> Color {
        if delta > 0.0 {
            self.up
        } else if delta < 0.0 {
            self.down
        } else {
            self.dim
        }
    }
}

/// `(formatted ET clock, is_regular_session_open)`.
pub fn market_clock() -> (String, bool) {
    let now: DateTime<Tz> = Utc::now().with_timezone(&New_York);
    (
        now.format("%H:%M:%S ET").to_string(),
        is_regular_session(&now),
    )
}

/// Mon–Fri, 09:30–16:00 ET. Holidays are not yet accounted for.
fn is_regular_session(now: &DateTime<Tz>) -> bool {
    if now.weekday().number_from_monday() >= 6 {
        return false;
    }
    let minutes = now.hour() * 60 + now.minute();
    (9 * 60 + 30..16 * 60).contains(&minutes)
}
