use anyhow::Result;
use clap::Parser;
use std::collections::BTreeMap;
use std::path::Path;
use tokio::time::Duration;

use crate::docker::compose_target_cmd;
use crate::progress::run_command_with_progress;
use crate::{Project, TargetSelector};

/// Create and start service containers
#[derive(Parser, Debug, Clone)]
pub struct UpArgs {
    /// Target selector: *, project, or project.service
    #[arg(default_value = "*", value_parser = crate::clap_parse_selector)]
    pub target: TargetSelector,

    /// Disable real-time monitoring of container status after starting containers
    #[arg(long)]
    pub no_monitor: bool,

    /// Refresh interval in seconds for status updates when monitoring
    #[arg(short = 'r', long, default_value = "250ms", value_parser = humantime::parse_duration)]
    pub refresh: Duration,

    /// Maximum number of containers to display detailed status for
    #[arg(short = 'm', long, default_value = "15")]
    pub max_display: usize,

    /// Suppress non-essential output
    #[arg(short, long)]
    pub quiet: bool,

    /// Use legacy restart method instead of the current implementation
    #[arg(short, long)]
    pub legacy: bool,

    /// Skip health checks when determining if containers are ready
    #[arg(short, long)]
    pub skip_healthcheck: bool,
}

pub async fn handle_up(
    args: &UpArgs,
    projects: &BTreeMap<String, Project>,
    _locked_images: &BTreeMap<String, String>,
    _lock_file: &Path,
) -> Result<()> {
    if !args.legacy && !matches!(args.target, TargetSelector::Service(_)) {
        run_command_with_progress(
            &args.target,
            projects,
            &["up", "-d"],
            args.no_monitor,
            args.quiet,
            args.refresh,
            !args.skip_healthcheck,
        )
        .await?;
    } else {
        compose_target_cmd(&args.target, projects, &["up", "-d"]).await?;
    }
    Ok(())
}
