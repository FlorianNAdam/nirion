use clap::Parser;
use std::{collections::BTreeMap, path::Path};

use crate::{
    clap_parse_selector, docker::compose_target_cmd, Project, TargetSelector,
};

#[derive(Parser, Debug, Clone)]
pub struct LogsArgs {
    #[arg(default_value = "*", value_parser = clap_parse_selector)]
    pub target: TargetSelector,

    /// Follow log output
    #[arg(short = 'f', long)]
    pub follow: bool,

    /// Produce monochrome output
    #[arg(long)]
    pub no_color: bool,

    /// Don't print prefix in logs
    #[arg(long)]
    pub no_log_prefix: bool,

    /// Show logs since timestamp
    #[arg(long)]
    pub since: Option<String>,

    /// Show logs before timestamp
    #[arg(long)]
    pub until: Option<String>,

    /// Number of lines to show from the end
    #[arg(short = 'n', long)]
    pub tail: Option<String>,

    /// Show timestamps
    #[arg(short = 't', long)]
    pub timestamps: bool,
}

pub async fn handle_logs(
    args: &LogsArgs,
    projects: &BTreeMap<String, Project>,
    _locked_images: &BTreeMap<String, String>,
    _lock_file: &Path,
) -> anyhow::Result<()> {
    let mut cmd = vec!["logs".into()];

    if args.follow {
        cmd.push("--follow".into());
    }
    if args.no_color {
        cmd.push("--no-color".into());
    }
    if args.no_log_prefix {
        cmd.push("--no-log-prefix".into());
    }
    if args.timestamps {
        cmd.push("--timestamps".into());
    }
    if let Some(ref since) = args.since {
        cmd.push("--since".into());
        cmd.push(since.clone());
    }
    if let Some(ref until) = args.until {
        cmd.push("--until".into());
        cmd.push(until.clone());
    }
    if let Some(ref tail) = args.tail {
        cmd.push("--tail".into());
        cmd.push(tail.clone());
    }

    let cmd_slices: Vec<&str> = cmd.iter().map(|s| s.as_str()).collect();

    compose_target_cmd(&args.target, projects, &cmd_slices)
}
