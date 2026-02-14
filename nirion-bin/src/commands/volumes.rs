use anyhow::Result;
use clap::Parser;
use nirion_lib::{lock::LockedImages, projects::Projects};
use std::path::Path;

use crate::{docker::compose_target_cmd, ClapSelector, TargetSelector};

/// List volumes
#[derive(Parser, Debug, Clone)]
pub struct VolumesArgs {
    /// Target selector: *, project, or project.service
    #[arg(
        default_value = "*",
        value_parser = TargetSelector::clap_parse,
        add = TargetSelector::clap_completer()
    )]
    pub target: TargetSelector,

    /// Output format (table, json, Go template)
    #[arg(long, default_value = "table")]
    pub format: String,

    /// Only display volume names
    #[arg(short = 'q', long)]
    pub quiet: bool,
}

pub async fn handle_volumes(
    args: &VolumesArgs,
    projects: &Projects,
    _locked_images: &LockedImages,
    _lock_file: &Path,
) -> Result<()> {
    let mut cmd: Vec<String> =
        vec!["volumes".into(), "--format".into(), args.format.clone()];

    if args.quiet {
        cmd.push("--quiet".into());
    }

    let cmd_slices: Vec<&str> = cmd.iter().map(|s| s.as_str()).collect();

    compose_target_cmd(&args.target, projects, &cmd_slices).await
}
