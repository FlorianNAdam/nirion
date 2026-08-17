use clap::Args;

use crate::{docker::render_operation_events, ClapSelector, TargetSelector};
use nirion_lib::backend::{DispatchRequest, NirionBackend, TopRequest};

/// Display the running processes of a service container
#[derive(Args, Debug, Clone)]
pub struct TopArgs {
    /// Target selector: *, project, or project.service
    #[arg(
        default_value = "*",
        value_parser = TargetSelector::clap_parse,
        add = TargetSelector::clap_completer()
    )]
    pub target: TargetSelector,
}

pub async fn handle_top(
    args: &TopArgs,
    backend: &dyn NirionBackend,
) -> anyhow::Result<()> {
    render_operation_events(backend.dispatch(DispatchRequest::Top(
        TopRequest {
            target: args.target.clone(),
        },
    )))
    .await
}
