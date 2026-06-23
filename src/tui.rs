//! Terminal lifecycle helpers.

/// Install a panic hook that restores the terminal before printing the panic,
/// so a crash never leaves the user's terminal stuck in raw mode.
pub fn install_panic_hook() {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = ratatui::try_restore();
        original(info);
    }));
}
