use clap::{Args, ValueEnum};
use futures::StreamExt;
use nirion_lib::{
    context::NirionContext,
    logs::{logs_stream, LogStreamOptions},
    projects::TargetSelector,
};
use std::time::Duration;

use crate::{log_render::LogRenderer, ClapSelector};

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLabelFormat {
    ProjectService,
    Service,
    Container,
    None,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogEventsMode {
    Auto,
    Always,
    Never,
}

/// View output from service containers
#[derive(Args, Debug, Clone)]
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

    /// Refresh interval for discovering container changes when following
    #[arg(short = 'r', long, default_value = "250ms", value_parser = humantime::parse_duration)]
    pub refresh: Duration,

    /// Don't print prefix in logs
    #[arg(long, value_enum, default_value = "project-service")]
    pub label: LogLabelFormat,

    /// Print lifecycle events such as attach, detach, and exit
    #[arg(long, value_enum, default_value = "auto")]
    pub events: LogEventsMode,

    /// Show logs since timestamp
    #[arg(long)]
    pub since: Option<String>,

    /// Show logs before timestamp
    #[arg(long, conflicts_with = "follow")]
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
    let options = LogStreamOptions {
        follow: args.follow,
        refresh_interval: args.refresh,
        since: args.since.clone(),
        until: args.until.clone(),
        tail: args.tail.clone(),
        timestamps: args.timestamps,
    };
    let mut renderer = LogRenderer::new(args.label, args.events, args.follow);
    let mut stream = logs_stream(context.clone(), args.target.clone(), options);
    let shutdown = tokio::signal::ctrl_c();
    tokio::pin!(shutdown);

    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            event = stream.next() => {
                let Some(event) = event else {
                    break;
                };
                renderer.render(event?)?;
            }
        }
    }

    Ok(())
}
