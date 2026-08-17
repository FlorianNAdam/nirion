use anyhow::Result;
use clap::Args;

use crate::{docker::render_operation_events, ClapSelector, TargetSelector};
use nirion_lib::backend::{DispatchRequest, NirionBackend, VolumesRequest};

/// List volumes
#[derive(Args, Debug, Clone)]
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
    backend: &dyn NirionBackend,
) -> Result<()> {
    render_operation_events(backend.dispatch(DispatchRequest::Volumes(
        VolumesRequest {
            target: args.target.clone(),
            format: args.format.clone(),
            quiet: args.quiet,
        },
    )))
    .await
}
