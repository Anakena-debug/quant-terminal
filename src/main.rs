//! Quant Terminal (`qterm`) — a Bloomberg-style terminal for quantitative analysts.
//!
//! Architecture: an async (tokio) event loop feeds [`event::AppEvent`]s into a
//! single channel; the [`app::App`] reduces them via [`action::Action`]s and the
//! [`ui`] module renders the current state with ratatui.

mod action;
mod analytics;
mod app;
mod backtest;
mod command;
mod config;
mod data;
mod event;
mod logging;
mod snapshot;
mod tui;
mod ui;
mod update;

use app::App;

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let args: Vec<String> = std::env::args().collect();

    // Headless IBKR connectivity check (no TTY needed):
    // `qterm --ibkr-check [host:port] [client_id]`
    if let Some(pos) = args.iter().position(|a| a == "--ibkr-check") {
        let address = args
            .get(pos + 1)
            .filter(|a| a.contains(':'))
            .cloned()
            .unwrap_or_else(|| "127.0.0.1:7496".to_string());
        let client_id = args
            .get(pos + 2)
            .and_then(|s| s.parse().ok())
            .unwrap_or(1101);
        print!("{}", data::ibkr::diagnostic(&address, client_id).await?);
        return Ok(());
    }

    // Offline preview mode: render the UI and exit (no TTY needed).
    if args.iter().any(|a| a == "--snapshot") {
        print!("{}", snapshot::render(&args).await?);
        return Ok(());
    }

    let _log_guard = logging::init()?;
    tui::install_panic_hook();

    tracing::info!("starting Quant Terminal");
    let config = config::load();
    let mut terminal = ratatui::init();
    let mut app = App::new(config);
    let result = app.run(&mut terminal).await;
    ratatui::restore();

    match &result {
        Ok(()) => tracing::info!("clean shutdown"),
        Err(err) => tracing::error!(?err, "exited with error"),
    }
    result
}
