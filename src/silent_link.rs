use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use url::Url;

#[derive(Debug, Clone)]
pub struct Client {
    http: reqwest::Client,
    api_base: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalanceSnapshot {
    pub balance_usd: Option<f64>,
    pub msisdn: Option<String>,
    pub last_update: Option<String>,
    pub status: String,
}

impl Client {
    pub fn new(timeout: Duration) -> Result<Self> {
        Self::with_api_base(timeout, "https://silent.link".to_string())
    }

    pub fn with_api_base(timeout: Duration, api_base: String) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .user_agent(concat!(
                "silent-balance-tracker/",
                env!("CARGO_PKG_VERSION")
            ))
            .build()
            .context("building reqwest client")?;
        Ok(Self { http, api_base })
    }

    pub async fn fetch_balance(&self, order_url: &str) -> Result<BalanceSnapshot> {
        let token = extract_token(order_url)
            .with_context(|| format!("extracting token from URL: {order_url}"))?;

        let order_endpoint = format!("{}/api/v1/order", self.api_base);
        let order: serde_json::Value = self
            .http
            .post(&order_endpoint)
            .json(&serde_json::json!({ "token": token }))
            .send()
            .await
            .context("order request failed")?
            .error_for_status()
            .context("order endpoint returned error status")?
            .json()
            .await
            .context("decoding order response")?;

        let imsi = order
            .get("data")
            .and_then(|d| d.get("imsi"))
            .and_then(|v| v.as_str())
            .context("no imsi in order response")?
            .to_string();

        let balance_endpoint = format!("{}/api/v1/checkbalance", self.api_base);
        let bal: serde_json::Value = self
            .http
            .post(&balance_endpoint)
            .json(&serde_json::json!({ "phone": imsi, "token": token }))
            .send()
            .await
            .context("checkbalance request failed")?
            .error_for_status()
            .context("checkbalance endpoint returned error status")?
            .json()
            .await
            .context("decoding checkbalance response")?;

        let status = bal
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        let data = bal.get("data");
        let balance_usd = data.and_then(|d| d.get("BALANCE")).and_then(json_to_f64);
        let msisdn = data
            .and_then(|d| d.get("MSISDN"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let last_update = data
            .and_then(|d| d.get("LASTUPDATE"))
            .and_then(|v| v.as_str())
            .map(|s| s.replace(',', " "));

        Ok(BalanceSnapshot {
            balance_usd,
            msisdn,
            last_update,
            status,
        })
    }
}

fn json_to_f64(v: &serde_json::Value) -> Option<f64> {
    if let Some(n) = v.as_f64() {
        return Some(n);
    }
    if let Some(s) = v.as_str() {
        return s.trim().parse().ok();
    }
    None
}

/// Extract the token from a URL like `https://silent.link/order/<TOKEN>`.
/// Also accepts a bare token (no scheme/host) for convenience.
pub fn extract_token(input: &str) -> Result<String> {
    if !input.contains("://") && !input.contains('/') {
        // Looks like a bare token
        return Ok(input.to_string());
    }

    let parsed = Url::parse(input).with_context(|| format!("not a valid URL: {input}"))?;
    let segments: Vec<_> = parsed
        .path_segments()
        .map(|s| s.filter(|seg| !seg.is_empty()).collect())
        .unwrap_or_default();

    // Expected shape: /order/<TOKEN>
    if segments.len() >= 2 && segments[segments.len() - 2] == "order" {
        return Ok(segments[segments.len() - 1].to_string());
    }
    // Fallback: last non-empty path segment
    if let Some(last) = segments.last() {
        return Ok((*last).to_string());
    }
    anyhow::bail!("could not extract token from URL: {input}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_from_full_url() -> Result<()> {
        let t = extract_token("https://silent.link/order/AbCdEf123")?;
        assert_eq!(t, "AbCdEf123");
        Ok(())
    }

    #[test]
    fn token_from_url_with_trailing_slash() -> Result<()> {
        let t = extract_token("https://silent.link/order/AbCdEf123/")?;
        assert_eq!(t, "AbCdEf123");
        Ok(())
    }

    #[test]
    fn token_from_bare_string() -> Result<()> {
        let t = extract_token("AbCdEf123")?;
        assert_eq!(t, "AbCdEf123");
        Ok(())
    }
}
