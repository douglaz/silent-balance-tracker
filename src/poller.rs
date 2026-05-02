use crate::config::{Account, Config};
use crate::db::{BalanceRow, Db};
use crate::silent_link::Client;
use anyhow::Result;
use chrono::Utc;
use std::time::Duration;

pub async fn poll_once(cfg: &Config, db: &Db) -> Result<()> {
    let client = Client::new(Duration::from_secs(cfg.poller.request_timeout_seconds))?;
    for account in &cfg.accounts {
        let token = match account.token() {
            Ok(t) => t,
            Err(err) => {
                tracing::error!(url = %account.url, error = %err, "skipping account: invalid URL");
                continue;
            }
        };
        let label = account.display_name(&token);
        let row = poll_account(&client, account, token).await;
        if let Err(err) = db.insert_row(&row) {
            tracing::error!(account = %label, error = %err, "failed to write row");
        } else {
            tracing::info!(
                account = %label,
                status = %row.status,
                balance = ?row.balance_usd,
                "polled"
            );
        }
    }
    Ok(())
}

async fn poll_account(client: &Client, account: &Account, token: String) -> BalanceRow {
    let timestamp_utc = Utc::now();
    match client.fetch_balance(&account.url).await {
        Ok(snap) => BalanceRow {
            account_token: token,
            timestamp_utc,
            balance_usd: snap.balance_usd,
            msisdn: snap.msisdn,
            last_update: snap.last_update,
            status: snap.status,
            note: None,
        },
        Err(err) => {
            tracing::warn!(token = %crate::config::short_token(&token), error = %err, "poll failed");
            BalanceRow {
                account_token: token,
                timestamp_utc,
                balance_usd: None,
                msisdn: None,
                last_update: None,
                status: "error".to_string(),
                note: Some(format!("{err:#}")),
            }
        }
    }
}
