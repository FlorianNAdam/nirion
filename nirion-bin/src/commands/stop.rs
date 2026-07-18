use anyhow::Result;
use clap::Parser;

use crate::commands::LifecycleArgs;
use crate::lifecycle::run_lifecycle_command;
use crate::{ClapSelector, TargetSelector};
use nirion_lib::context::NirionContext;
use nirion_lib::wait::WaitTarget;

/// Stop service containers
#[derive(Parser, Debug, Clone)]
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
    context: &NirionContext,
) -> Result<()> {
    run_lifecycle_command(
        context,
        &args.target,
        &["stop"],
        args.lifecycle
            .options(WaitTarget::NoWait),
    )
    .await
}
