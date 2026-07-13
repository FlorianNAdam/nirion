use clap::Parser;
use std::time::Duration;

use crate::{
    commands::NirionContext, monitor::monitor, ClapSelector, TargetSelector,
};

#[derive(Parser, Debug, Clone)]
pub struct MonitorArgs {
    /// Target selector: *, project, or project.service
    #[arg(
        default_value = "*",
        value_parser = TargetSelector::clap_parse,
        add = TargetSelector::clap_completer()
    )]
    pub target: TargetSelector,

    /// Refresh interval in seconds for status updates when monitoring
    #[arg(short = 'r', long, default_value = "250ms", value_parser = humantime::parse_duration)]
    pub refresh: Duration,
}

pub async fn handle_monitor(
    args: &MonitorArgs,
    context: &NirionContext,
) -> anyhow::Result<()> {
    monitor(
        &context.docker_binary,
        &args.target,
        &context.projects,
        args.refresh,
    )
    .await?;
    Ok(())
}
