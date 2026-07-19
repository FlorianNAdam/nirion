use anyhow::Result;
use clap::Args;

use crate::docker::compose_target_cmd;
use crate::{ClapSelector, TargetSelector};
use nirion_lib::context::NirionContext;

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
    context: &NirionContext,
) -> Result<()> {
    compose_target_cmd(context, &args.target, &["pull"]).await?;
    Ok(())
}
