use anyhow::Result;
use clap::Parser;
use nirion_lib::projects::TargetSelector;

use crate::commands::LifecycleArgs;
use crate::progress::run_lifecycle_command;
use crate::ClapSelector;
use nirion_lib::context::NirionContext;

/// Stop and remove service containers, networks
#[derive(Parser, Debug, Clone)]
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
    context: &NirionContext,
) -> Result<()> {
    run_lifecycle_command(
        context,
        &args.target,
        &["down"],
        args.lifecycle.options(false),
    )
    .await
}
