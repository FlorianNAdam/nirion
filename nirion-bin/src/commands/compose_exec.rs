use clap::Parser;
use nirion_lib::projects::TargetSelector;

use crate::{docker::compose_target_cmd, ClapSelector};
use nirion_lib::context::NirionContext;

/// Run a docker compose command for a project or service
#[derive(Parser, Debug, Clone)]
pub struct ComposeExecArgs {
    /// Target selector: *, project, or project.service
    #[arg(
        default_value = "*",
        value_parser = TargetSelector::clap_parse,
        add = TargetSelector::clap_completer()
    )]
    pub target: TargetSelector,

    /// Command to execute in container
    cmd: Vec<String>,
}

pub async fn handle_compose_exec(
    args: &ComposeExecArgs,
    context: &NirionContext,
) -> anyhow::Result<()> {
    let cmd_slices: Vec<&str> = args
        .cmd
        .iter()
        .map(|s| s.as_str())
        .collect();

    compose_target_cmd(context, &args.target, &cmd_slices).await
}
