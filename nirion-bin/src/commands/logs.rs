use clap::Parser;
use nirion_lib::projects::TargetSelector;

use crate::{docker::compose_target_cmd, ClapSelector};
use nirion_lib::context::NirionContext;

/// View output from service containers
#[derive(Parser, Debug, Clone)]
pub struct LogsArgs {
    /// Target selector: *, project, or project.service
    #[arg(
        default_value = "*",
        value_parser = TargetSelector::clap_parse,
        add = TargetSelector::clap_completer()
    )]
    pub target: TargetSelector,

    /// Follow log output
    #[arg(short = 'f', long)]
    pub follow: bool,

    /// Re-run log streaming when containers are missing or recreated
    #[arg(long, requires = "follow")]
    pub reconnect: bool,

    /// Produce monochrome output
    #[arg(long)]
    pub no_color: bool,

    /// Don't print prefix in logs
    #[arg(long)]
    pub no_log_prefix: bool,

    /// Show logs since timestamp
    #[arg(long)]
    pub since: Option<String>,

    /// Show logs before timestamp
    #[arg(long)]
    pub until: Option<String>,

    /// Number of lines to show from the end
    #[arg(short = 'n', long)]
    pub tail: Option<String>,

    /// Show timestamps
    #[arg(short = 't', long)]
    pub timestamps: bool,
}

pub async fn handle_logs(
    args: &LogsArgs,
    context: &NirionContext,
) -> anyhow::Result<()> {
    let mut cmd = vec!["logs".into()];

    if args.follow {
        cmd.push("--follow".into());
    }
    if args.no_color {
        cmd.push("--no-color".into());
    }
    if args.no_log_prefix {
        cmd.push("--no-log-prefix".into());
    }
    if args.timestamps {
        cmd.push("--timestamps".into());
    }
    if let Some(ref since) = args.since {
        cmd.push("--since".into());
        cmd.push(since.clone());
    }
    if let Some(ref until) = args.until {
        cmd.push("--until".into());
        cmd.push(until.clone());
    }
    if let Some(ref tail) = args.tail {
        cmd.push("--tail".into());
        cmd.push(tail.clone());
    }

    let cmd_slices: Vec<&str> = cmd.iter().map(|s| s.as_str()).collect();

    loop {
        compose_target_cmd(context, &args.target, &cmd_slices).await?;
        if !args.reconnect {
            return Ok(());
        }

        eprintln!("logs exited; reconnecting");
    }
}
