use anyhow::Result;
use clap::Args;

use crate::commands::LifecycleArgs;
use crate::lifecycle::run_lifecycle_command;
use crate::{ClapSelector, TargetSelector};
use nirion_lib::{
    backend::{LifecycleAction, NirionBackend},
    wait::WaitTarget,
};

/// Start service containers
#[derive(Args, Debug, Clone)]
pub struct StartArgs {
    /// Target selector: *, project, or project.service
    #[arg(
        default_value = "*",
        value_parser = TargetSelector::clap_parse,
        add = TargetSelector::clap_completer()
    )]
    pub target: TargetSelector,

    #[command(flatten)]
    pub lifecycle: LifecycleArgs,

    /// Skip health checks when determining if containers are ready
    #[arg(short, long)]
    pub skip_healthcheck: bool,
}

pub async fn handle_start(
    args: &StartArgs,
    backend: &impl NirionBackend,
) -> Result<()> {
    run_lifecycle_command(
        backend,
        &args.target,
        LifecycleAction::Start,
        args.lifecycle
            .options(if args.skip_healthcheck {
                WaitTarget::NoWait
            } else {
                WaitTarget::Healthchecks
            }),
    )
    .await
}
