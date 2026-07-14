use anyhow::Result;
use clap::Parser;
use tokio::time::Duration;

use crate::docker::compose_target_cmd;
use crate::progress::run_command_with_progress;
use crate::{ClapSelector, TargetSelector};
use nirion_lib::context::NirionContext;

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

    /// Disable real-time monitoring of container status after starting containers
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

    /// Skip health checks when determining if containers are ready
    #[arg(short, long)]
    pub skip_healthcheck: bool,
}

pub async fn handle_up(args: &UpArgs, context: &NirionContext) -> Result<()> {
    if !args.legacy && !matches!(args.target, TargetSelector::Service(_)) {
        run_command_with_progress(
            context,
            &args.target,
            &["up", "-d"],
            args.no_monitor,
            args.quiet,
            args.refresh,
            !args.skip_healthcheck,
        )
        .await?;
    } else {
        compose_target_cmd(context, &args.target, &["up", "-d"]).await?;
    }
    Ok(())
}
