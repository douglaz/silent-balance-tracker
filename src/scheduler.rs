use crate::config::Config;
use crate::db::Db;
use crate::poller;
use std::time::Duration;
use tokio::time::{interval, MissedTickBehavior};

pub async fn run(cfg: Config, db: Db) {
    let period = Duration::from_secs(cfg.poller.interval_seconds.max(10));
    let mut tick = interval(period);
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);

    tracing::info!(
        interval_seconds = cfg.poller.interval_seconds,
        accounts = cfg.accounts.len(),
        "scheduler started"
    );

    loop {
        tick.tick().await;
        if let Err(err) = poller::poll_once(&cfg, &db).await {
            tracing::error!(error = %err, "poll cycle failed");
        }
    }
}
