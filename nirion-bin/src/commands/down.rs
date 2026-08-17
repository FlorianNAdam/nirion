use anyhow::Result;
use clap::Args;
use nirion_lib::projects::TargetSelector;

use crate::commands::LifecycleArgs;
use crate::lifecycle::run_lifecycle_command;
use crate::ClapSelector;
use nirion_lib::{
    backend::{LifecycleAction, NirionBackend},
    wait::WaitTarget,
};

/// Stop and remove service containers, networks
#[derive(Args, Debug, Clone)]
pub struct DownArgs {
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

pub async fn handle_down(
    args: &DownArgs,
    backend: &dyn NirionBackend,
) -> Result<()> {
    run_lifecycle_command(
        backend,
        &args.target,
        LifecycleAction::Down,
        args.lifecycle
            .options(WaitTarget::NoWait),
    )
    .await
}
