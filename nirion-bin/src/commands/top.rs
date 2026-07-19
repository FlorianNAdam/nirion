use clap::Args;

use crate::{docker::compose_target_cmd, ClapSelector, TargetSelector};
use nirion_lib::context::NirionContext;

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
    context: &NirionContext,
) -> anyhow::Result<()> {
    // docker compose top has no flags: just ["top"]
    let cmd: Vec<&str> = vec!["top"];

    compose_target_cmd(context, &args.target, &cmd).await
}
