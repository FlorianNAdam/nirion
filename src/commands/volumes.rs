use anyhow::Result;
use clap::Parser;
use std::collections::BTreeMap;

use crate::{docker::compose_target_cmd, Project, TargetSelector};

#[derive(Parser, Debug, Clone)]
pub struct VolumesArgs {
    /// Target selector: *, project, or project.service
    #[arg(default_value = "*", value_parser = crate::clap_parse_selector)]
    pub target: TargetSelector,

    /// Output format (table, json, Go template)
    #[arg(long, default_value = "table")]
    pub format: String,

    /// Only display volume names
    #[arg(short = 'q', long)]
    pub quiet: bool,
}

pub fn handle_volumes(
    args: &VolumesArgs,
    projects: &BTreeMap<String, Project>,
) -> Result<()> {
    let mut cmd: Vec<String> =
        vec!["volumes".into(), "--format".into(), args.format.clone()];

    if args.quiet {
        cmd.push("--quiet".into());
    }

    let cmd_slices: Vec<&str> = cmd.iter().map(|s| s.as_str()).collect();

    compose_target_cmd(&args.target, projects, &cmd_slices)
}
