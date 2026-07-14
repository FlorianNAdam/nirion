use anyhow::Result;
use clap::Parser;

use crate::{
    commands::NirionContext, docker::compose_target_cmd, ClapSelector,
    TargetSelector,
};

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
    context: &NirionContext,
) -> Result<()> {
    let mut cmd: Vec<String> =
        vec!["volumes".into(), "--format".into(), args.format.clone()];

    if args.quiet {
        cmd.push("--quiet".into());
    }

    let cmd_slices: Vec<&str> = cmd.iter().map(|s| s.as_str()).collect();

    compose_target_cmd(
        &context.docker_binary,
        &args.target,
        &context.projects,
        &cmd_slices,
    )
    .await
}
