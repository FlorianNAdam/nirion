use anyhow::Result;
use clap::Parser;
use std::collections::BTreeMap;
use std::path::Path;
use tokio::time::Duration;

use crate::docker::compose_target_cmd;
use crate::progress::run_command_with_progress;
use crate::{Project, TargetSelector};

/// Stop and remove service containers, networks
#[derive(Parser, Debug, Clone)]
pub struct DownArgs {
    /// Target selector: *, project, or project.service
    #[arg(default_value = "*", value_parser = crate::clap_parse_selector)]
    pub target: TargetSelector,

    /// Disable real-time monitoring of container status after stopping containers
    #[arg(long)]
    pub no_monitor: bool,

    /// Refresh interval in seconds for status updates when monitoring
    #[arg(short = 'r', long, default_value = "1")]
    pub refresh: u64,

    /// Maximum number of containers to display detailed status for
    #[arg(short = 'm', long, default_value = "15")]
    pub max_display: usize,

    /// Suppress non-essential output
    #[arg(short, long)]
    pub quiet: bool,

    /// Use legacy restart method instead of the current implementation
    #[arg(short, long)]
    pub legacy: bool,
}

pub async fn handle_down(
    args: &DownArgs,
    projects: &BTreeMap<String, Project>,
    _locked_images: &BTreeMap<String, String>,
    _lock_file: &Path,
) -> Result<()> {
    if !args.legacy && !matches!(args.target, TargetSelector::Service(_)) {
        run_command_with_progress(
            &args.target,
            projects,
            &["down"],
            args.no_monitor,
            args.quiet,
            Duration::from_secs(args.refresh),
            false,
        )
        .await?;
    } else {
        compose_target_cmd(&args.target, projects, &["down"]).await?;
    }
    Ok(())
}
