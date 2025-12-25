use clap::Parser;
use std::{collections::BTreeMap, path::Path, time::Duration};

use crate::{
    monitor::{create_monitors, monitor},
    Project, TargetSelector,
};

#[derive(Parser, Debug, Clone)]
pub struct MonitorArgs {
    /// Target selector: *, project, or project.service
    #[arg(default_value = "*", value_parser = crate::clap_parse_selector)]
    pub target: TargetSelector,

    /// Refresh interval in seconds for status updates when monitoring
    #[arg(short = 'r', long, default_value = "1")]
    pub refresh: u64,
}

pub async fn handle_monitor(
    args: &MonitorArgs,
    projects: &BTreeMap<String, Project>,
    _locked_images: &BTreeMap<String, String>,
    _lock_file: &Path,
) -> anyhow::Result<()> {
    let monitors = create_monitors(
        &args.target,
        projects,
        Duration::from_secs(args.refresh),
    )
    .await;
    monitor(&monitors, projects).await?;
    Ok(())
}
