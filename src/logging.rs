//! File-based logging. The TUI owns stdout, so logs go to `logs/quantterm.log`.
//!
//! Override the level with `QUANTTERM_LOG` or `RUST_LOG`, e.g.
//! `QUANTTERM_LOG=debug qterm`.

use color_eyre::Result;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;

/// Initialise tracing. Keep the returned guard alive for the lifetime of the
/// program so buffered log lines are flushed on exit.
pub fn init() -> Result<WorkerGuard> {
    std::fs::create_dir_all("logs")?;
    let file = tracing_appender::rolling::never("logs", "quantterm.log");
    let (writer, guard) = tracing_appender::non_blocking(file);

    let filter = EnvFilter::try_from_env("QUANTTERM_LOG")
        .or_else(|_| EnvFilter::try_from_default_env())
        .unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_writer(writer)
        .with_env_filter(filter)
        .with_ansi(false)
        .with_target(false)
        .init();

    Ok(guard)
}
