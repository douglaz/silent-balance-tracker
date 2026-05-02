mod config;
mod dashboard;
mod db;
mod poller;
mod scheduler;
mod silent_link;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[derive(Parser, Debug)]
#[command(version, about = "silent.link multi-account balance tracker")]
struct Cli {
    /// Path to TOML config file. Env: SILENT_BALANCE_CONFIG.
    #[arg(short, long, env = "SILENT_BALANCE_CONFIG", global = true)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Run the dashboard HTTP server and the background poll loop.
    Serve,
    /// Run a single poll cycle for every configured account, then exit.
    Poll,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let cli = Cli::parse();
    let cfg = config::Config::load(cli.config.as_deref())?;
    let database = db::Db::open(std::path::Path::new(&cfg.database.path))?;

    match cli.cmd {
        Cmd::Poll => {
            poller::poll_once(&cfg, &database).await?;
        }
        Cmd::Serve => {
            let scheduler_cfg = cfg.clone();
            let scheduler_db = database.clone();
            tokio::spawn(async move {
                scheduler::run(scheduler_cfg, scheduler_db).await;
            });
            dashboard::serve(&cfg, database).await?;
        }
    }

    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,silent_balance_tracker=debug"));
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(false))
        .init();
}
