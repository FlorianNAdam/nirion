use anyhow::Result;
use clap::Parser;
use std::collections::BTreeMap;
use std::path::Path;
use tokio::time::Duration;

use crate::docker::compose_target_cmd;
use crate::progress::run_command_with_progress;
use crate::{ClapSelector, Project, TargetSelector};

/// Stop service containers
#[derive(Parser, Debug, Clone)]
pub struct StopArgs {
    /// Target selector: *, project, or project.service
    #[arg(
        default_value = "*",
        value_parser = TargetSelector::clap_parse,
        add = TargetSelector::clap_completer()
    )]
    pub target: TargetSelector,

    /// Disable real-time monitoring of container status after starting containers
    #[arg(long)]
    pub no_monitor: bool,

    /// Refresh interval in seconds for status updates when monitoring
    #[arg(short = 'r', long, default_value = "250ms", value_parser = humantime::parse_duration)]
    pub refresh: Duration,

    /// Suppress non-essential output
    #[arg(short, long)]
    pub quiet: bool,

    /// Use legacy restart method instead of the current implementation
    #[arg(short, long)]
    pub legacy: bool,
}

pub async fn handle_stop(
    args: &StopArgs,
    projects: &BTreeMap<String, Project>,
    _locked_images: &BTreeMap<String, String>,
    _lock_file: &Path,
) -> Result<()> {
    if !args.legacy && !matches!(args.target, TargetSelector::Service(_)) {
        run_command_with_progress(
            &args.target,
            projects,
            &["stop"],
            args.no_monitor,
            args.quiet,
            args.refresh,
            false,
        )
        .await?;
    } else {
        compose_target_cmd(&args.target, projects, &["stop"]).await?;
    }
    Ok(())
}
