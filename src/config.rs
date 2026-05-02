use crate::silent_link;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub database: DatabaseConfig,
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub poller: PollerConfig,
    #[serde(default)]
    pub accounts: Vec<Account>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    pub path: String,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            path: "./silent_balance.sqlite".to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 8080,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PollerConfig {
    pub interval_seconds: u64,
    pub request_timeout_seconds: u64,
}

impl Default for PollerConfig {
    fn default() -> Self {
        Self {
            interval_seconds: 300,
            request_timeout_seconds: 20,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Account {
    /// Friendly label shown on the dashboard. Optional; falls back to a
    /// truncated token.
    #[serde(default)]
    pub name: Option<String>,
    /// Full silent.link order URL like https://silent.link/order/<TOKEN>.
    pub url: String,
}

impl Account {
    pub fn token(&self) -> Result<String> {
        silent_link::extract_token(&self.url)
    }

    /// Display label: configured name, or a truncated form of the token.
    pub fn display_name(&self, token: &str) -> String {
        self.name.clone().unwrap_or_else(|| short_token(token))
    }
}

pub fn short_token(token: &str) -> String {
    if token.len() <= 12 {
        token.to_string()
    } else {
        format!("{}…{}", &token[..6], &token[token.len() - 4..])
    }
}

impl Config {
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let mut cfg: Config = match path {
            Some(p) => {
                let body = std::fs::read_to_string(p)
                    .with_context(|| format!("reading config file {}", p.display()))?;
                toml::from_str(&body).with_context(|| format!("parsing {}", p.display()))?
            }
            None => Config::default(),
        };

        // Env overrides — these mirror the keys you'd set in a k8s Deployment.
        if let Ok(v) = std::env::var("DATABASE_PATH") {
            cfg.database.path = v;
        }
        if let Ok(v) = std::env::var("SERVER_HOST") {
            cfg.server.host = v;
        }
        if let Ok(v) = std::env::var("SERVER_PORT") {
            cfg.server.port = v.parse().context("SERVER_PORT must be a u16")?;
        }
        if let Ok(v) = std::env::var("POLLER_INTERVAL_SECONDS") {
            cfg.poller.interval_seconds =
                v.parse().context("POLLER_INTERVAL_SECONDS must be u64")?;
        }
        if let Ok(v) = std::env::var("POLLER_REQUEST_TIMEOUT_SECONDS") {
            cfg.poller.request_timeout_seconds = v
                .parse()
                .context("POLLER_REQUEST_TIMEOUT_SECONDS must be u64")?;
        }

        // Accounts can be supplied via env as a JSON array — this is how the k8s
        // Secret feeds them in. When set, it replaces whatever was in the TOML.
        if let Ok(json) = std::env::var("SILENT_ACCOUNTS_JSON") {
            cfg.accounts = serde_json::from_str(&json)
                .context("SILENT_ACCOUNTS_JSON must be a JSON array of {name, url}")?;
        }

        if cfg.accounts.is_empty() {
            anyhow::bail!(
                "no accounts configured: add [[accounts]] entries to the config file or set SILENT_ACCOUNTS_JSON"
            );
        }

        let mut seen = std::collections::HashSet::new();
        for a in &cfg.accounts {
            let token = a
                .token()
                .with_context(|| format!("invalid account URL: {}", a.url))?;
            if !seen.insert(token.clone()) {
                anyhow::bail!("duplicate account token: {}", short_token(&token));
            }
        }

        Ok(cfg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_config() -> Result<()> {
        let toml = r#"
            [database]
            path = "/tmp/foo.sqlite"

            [[accounts]]
            name = "primary"
            url = "https://silent.link/order/AAA"

            [[accounts]]
            url = "https://silent.link/order/BBB"
        "#;
        let cfg: Config = toml::from_str(toml)?;
        assert_eq!(cfg.database.path, "/tmp/foo.sqlite");
        assert_eq!(cfg.accounts.len(), 2);
        assert_eq!(cfg.accounts[0].name.as_deref(), Some("primary"));
        assert_eq!(cfg.accounts[1].name, None);
        assert_eq!(cfg.accounts[0].token()?, "AAA");
        // Defaults fill in
        assert_eq!(cfg.server.port, 8080);
        assert_eq!(cfg.poller.interval_seconds, 300);
        Ok(())
    }

    #[test]
    fn short_token_truncates_long_tokens() {
        assert_eq!(short_token("abcdef"), "abcdef");
        assert_eq!(
            short_token("AbCdEf0123456789GhIjKlMnOpQrStUvWxYz0123456789"),
            "AbCdEf…6789"
        );
    }
}
