//! Configuration loaded from `config.toml` (or `config.local.toml`, which is
//! gitignored and takes precedence). Falls back to sensible defaults so the
//! app always runs with zero setup.

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    #[default]
    Simulated,
    Ibkr,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct IbkrConfig {
    pub host: String,
    pub port: u16,
    pub client_id: i32,
    /// Show the `DELAYED` badge (paper accounts usually get delayed data).
    pub delayed: bool,
}

impl Default for IbkrConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 4002, // IB Gateway paper; TWS paper is 7497
            client_id: 100,
            delayed: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub provider: ProviderKind,
    pub ibkr: IbkrConfig,
    pub watchlist: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            provider: ProviderKind::Simulated,
            ibkr: IbkrConfig::default(),
            watchlist: default_watchlist(),
        }
    }
}

pub fn default_watchlist() -> Vec<String> {
    [
        "AAPL", "MSFT", "NVDA", "TSLA", "AMZN", "GOOGL", "META", "SPY", "QQQ",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Load config, preferring `config.local.toml`. Any read/parse failure logs and
/// falls back to defaults.
pub fn load() -> Config {
    for path in ["config.local.toml", "config.toml"] {
        match std::fs::read_to_string(path) {
            Ok(contents) => match toml::from_str::<Config>(&contents) {
                Ok(mut config) => {
                    if config.watchlist.is_empty() {
                        config.watchlist = default_watchlist();
                    }
                    tracing::info!(path, "loaded config");
                    return config;
                }
                Err(e) => tracing::warn!(path, error = %e, "invalid config; ignoring"),
            },
            Err(_) => continue, // file absent
        }
    }
    Config::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_ibkr_config() {
        let toml = r#"
            provider = "ibkr"
            watchlist = ["AAPL", "TSLA"]
            [ibkr]
            host = "10.0.0.5"
            port = 7497
            client_id = 7
            delayed = false
        "#;
        let c: Config = toml::from_str(toml).unwrap();
        assert_eq!(c.provider, ProviderKind::Ibkr);
        assert_eq!(c.ibkr.host, "10.0.0.5");
        assert_eq!(c.ibkr.port, 7497);
        assert!(!c.ibkr.delayed);
        assert_eq!(c.watchlist, vec!["AAPL".to_string(), "TSLA".to_string()]);
    }

    #[test]
    fn fills_defaults_for_missing_fields() {
        let c: Config = toml::from_str("provider = \"simulated\"").unwrap();
        assert_eq!(c.provider, ProviderKind::Simulated);
        assert_eq!(c.ibkr.port, 4002); // default
        assert!(!c.watchlist.is_empty()); // default watchlist
    }
}
