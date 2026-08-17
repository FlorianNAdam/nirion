use clap::Args;
use nirion_lib::{
    backend::{ComposePassthroughOperation, NirionBackend},
    projects::TargetSelector,
};

use crate::{docker::render_command_output_events, ClapSelector};

/// Run a docker compose command for a project or service
#[derive(Args, Debug, Clone)]
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
    backend: &dyn NirionBackend,
) -> anyhow::Result<()> {
    render_command_output_events(
        backend
            .compose_passthrough(ComposePassthroughOperation {
                target: args.target.clone(),
                args: args.cmd.clone(),
            })
            .await,
    )
    .await
}
