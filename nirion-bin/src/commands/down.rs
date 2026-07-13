use anyhow::Result;
use clap::Parser;
use nirion_lib::projects::TargetSelector;
use tokio::time::Duration;

use crate::commands::NirionContext;
use crate::docker::compose_target_cmd;
use crate::progress::run_command_with_progress;
use crate::ClapSelector;

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

    /// Disable real-time monitoring of container status after stopping containers
    #[arg(long)]
    pub no_monitor: bool,

    /// Refresh interval in seconds for status updates when monitoring
    #[arg(short = 'r', long, default_value = "250ms", value_parser = humantime::parse_duration)]
    pub refresh: Duration,

    /// Maximum number of containers to display detailed status for
    #[arg(short = 'm', long, default_value = "15")]
    pub max_display: usize,

    /// Suppress non-essential output
    #[arg(short, long)]
    pub quiet: bool,

    /// Disable TUI progress output and use docker compose directly
    #[arg(short, long, alias = "no-tui")]
    pub legacy: bool,
}

pub async fn handle_down(
    args: &DownArgs,
    context: &NirionContext,
) -> Result<()> {
    if !args.legacy && !matches!(args.target, TargetSelector::Service(_)) {
        run_command_with_progress(
            &args.target,
            &context.projects,
            &["down"],
            args.no_monitor,
            args.quiet,
            args.refresh,
            false,
        )
        .await?;
    } else {
        compose_target_cmd(&args.target, &context.projects, &["down"]).await?;
    }
    Ok(())
}
