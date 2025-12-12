use anyhow::Result;
use clap::Parser;
use std::{collections::BTreeMap, path::Path};

use crate::{docker::compose_target_cmd, Project, TargetSelector};

#[derive(Parser, Debug, Clone)]
pub struct PsArgs {
    /// Target selector: *, project, or project.service
    #[arg(default_value = "*", value_parser = crate::clap_parse_selector)]
    pub target: TargetSelector,

    /// Show all containers (including stopped ones)
    #[arg(short = 'a', long)]
    pub all: bool,

    /// Filter services by a property (currently only 'status')
    #[arg(long)]
    pub filter: Option<String>,

    /// Format output (table, json, Go template)
    #[arg(short = 'f', long)]
    pub format: Option<String>,

    /// Short format
    #[arg(short = 's', long, conflicts_with = "format")]
    pub short: bool,

    /// Don't truncate output
    #[arg(long)]
    pub no_trunc: bool,

    /// Include orphaned services
    #[arg(long)]
    pub orphans: Option<bool>,

    /// Only display container IDs
    #[arg(short = 'q', long)]
    pub quiet: bool,

    /// Display services
    #[arg(long)]
    pub services: bool,

    /// Filter by status (can be repeated)
    #[arg(long)]
    pub status: Vec<String>,
}

pub async fn handle_ps(
    args: &PsArgs,
    projects: &BTreeMap<String, Project>,
    _locked_images: &BTreeMap<String, String>,
    _lock_file: &Path,
) -> Result<()> {
    // Build ps-specific arguments
    let mut cmd_args: Vec<String> = vec!["ps".into()];

    if args.all {
        cmd_args.push("--all".into());
    }

    if let Some(filter) = &args.filter {
        cmd_args.push("--filter".into());
        cmd_args.push(filter.clone());
    }

    if let Some(format) = &args.format {
        cmd_args.push("--format".into());
        cmd_args.push(format.clone());
    } else if args.short {
        cmd_args.push("--format".into());
        cmd_args.push(
            "table{{.Name}}\t{{.RunningFor}}\t{{.Status}}\t{{.Ports}}"
                .to_string(),
        );
    }

    if args.no_trunc {
        cmd_args.push("--no-trunc".into());
    }

    if let Some(orphans) = args.orphans {
        cmd_args.push(format!("--orphans={orphans}"));
    }

    if args.quiet {
        cmd_args.push("--quiet".into());
    }

    if args.services {
        cmd_args.push("--services".into());
    }

    for s in &args.status {
        cmd_args.push("--status".into());
        cmd_args.push(s.clone());
    }

    let cmd_slices: Vec<&str> = cmd_args
        .iter()
        .map(|s| s.as_str())
        .collect();

    compose_target_cmd(&args.target, projects, &cmd_slices).await
}
