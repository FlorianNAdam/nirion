use anyhow::Result;
use clap::Parser;

use crate::commands::LifecycleArgs;
use crate::lifecycle::run_lifecycle_command;
use crate::{ClapSelector, TargetSelector};
use nirion_lib::context::NirionContext;
use nirion_lib::wait::WaitTarget;

/// Create and start service containers
#[derive(Parser, Debug, Clone)]
pub struct UpArgs {
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

pub async fn handle_up(
    args: &UpArgs,
    context: &NirionContext,
) -> Result<()> {
    run_lifecycle_command(
        context,
        &args.target,
        &["up", "-d"],
        args.lifecycle
            .options(if args.skip_healthcheck {
                WaitTarget::NoWait
            } else {
                WaitTarget::Healthchecks
            }),
    )
    .await
}
