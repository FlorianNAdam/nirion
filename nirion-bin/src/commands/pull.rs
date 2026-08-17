use anyhow::Result;
use clap::Args;

use crate::docker::render_operation_events;
use crate::{ClapSelector, TargetSelector};
use nirion_lib::{
    backend::{NirionBackend, PullOperation},
    compose::ComposeConcurrency,
};

/// Pull service images
#[derive(Args, Debug, Clone)]
pub struct PullArgs {
    /// Target selector: *, project, or project.service
    #[arg(
        default_value = "*",
        value_parser = TargetSelector::clap_parse,
        add = TargetSelector::clap_completer()
    )]
    pub target: TargetSelector,
}

pub async fn handle_pull(
    args: &PullArgs,
    backend: &dyn NirionBackend,
) -> Result<()> {
    render_operation_events(
        backend
            .pull(PullOperation {
                target: args.target.clone(),
                concurrency: ComposeConcurrency::sequential(),
            })
            .await,
    )
    .await
}
