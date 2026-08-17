use anyhow::Result;
use clap::Args;

use crate::commands::LifecycleArgs;
use crate::lifecycle::run_lifecycle_command;
use crate::{ClapSelector, TargetSelector};
use nirion_lib::{
    backend::{LifecycleAction, NirionBackend},
    wait::WaitTarget,
};

/// Stop service containers
#[derive(Args, Debug, Clone)]
pub struct StopArgs {
    /// Target selector: *, project, or project.service
    #[arg(
        default_value = "*",
        value_parser = TargetSelector::clap_parse,
        add = TargetSelector::clap_completer()
    )]
    pub target: TargetSelector,

    #[command(flatten)]
    pub lifecycle: LifecycleArgs,
}

pub async fn handle_stop(
    args: &StopArgs,
    backend: &impl NirionBackend,
) -> Result<()> {
    run_lifecycle_command(
        backend,
        &args.target,
        LifecycleAction::Stop,
        args.lifecycle
            .options(WaitTarget::NoWait),
    )
    .await
}
