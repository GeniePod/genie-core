mod control;
mod governor;
mod service_ctl;
mod store;
mod tegra_reader;

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
    tracing::info!("GeniePod governor starting");
    tracing::info!(
        poll_ms = config.governor.poll_interval_ms,
        night_start = config.governor.night_start_hour,
        day_start = config.governor.day_start_hour,
        night_swap = config.governor.night_model_swap,
        "configuration loaded"
    );

    let db_path = config.data_dir.join("governor.db");
    let store = store::Store::open(&db_path)?;
    tracing::info!(path = %db_path.display(), "database opened");

    let mut gov = governor::Governor::new(config, store);
    gov.run().await
}
