use clap::Args;
use nirion_lib::{
    backend::{ComposePassthroughRequest, DispatchRequest, NirionBackend},
    projects::TargetSelector,
};

use crate::{docker::render_operation_events, ClapSelector};

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
    render_operation_events(backend.dispatch(
        DispatchRequest::ComposePassthrough(ComposePassthroughRequest {
            target: args.target.clone(),
            args: args.cmd.clone(),
        }),
    ))
    .await
}
