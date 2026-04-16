mod checker;

use anyhow::Result;
use genie_common::config::Config;
use tracing_subscriber::EnvFilter;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .compact()
        .init();

    let config = Config::load()?;
    tracing::info!("GeniePod health monitor starting");

    let mut monitor = checker::HealthMonitor::new(config)?;
    monitor.run().await
}
