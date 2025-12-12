use anyhow::Result;
use clap::Parser;
use std::collections::BTreeMap;
use std::path::Path;
use tokio::time::Duration;

use crate::docker::compose_target_cmd;
use crate::progress::run_command_with_progress;
use crate::{Project, TargetSelector};

#[derive(Parser, Debug, Clone)]
pub struct UpArgs {
    #[arg(default_value = "*", value_parser = crate::clap_parse_selector)]
    pub target: TargetSelector,

    #[arg(long)]
    pub no_monitor: bool,

    #[arg(short = 'r', long, default_value = "1")]
    pub refresh: u64,

    #[arg(short = 'm', long, default_value = "15")]
    pub max_display: usize,

    #[arg(short, long)]
    pub quiet: bool,

    #[arg(short, long)]
    pub boring: bool,
}

pub async fn handle_up(
    args: &UpArgs,
    projects: &BTreeMap<String, Project>,
    _locked_images: &BTreeMap<String, String>,
    _lock_file: &Path,
) -> Result<()> {
    if !args.boring && !matches!(args.target, TargetSelector::Image(_)) {
        run_command_with_progress(
            &args.target,
            projects,
            &["up", "-d"],
            args.no_monitor,
            args.quiet,
            Duration::from_secs(args.refresh),
        )
        .await?;
    } else {
        compose_target_cmd(&args.target, projects, &["up", "-d"])?;
    }
    Ok(())
}
