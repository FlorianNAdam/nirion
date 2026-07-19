use anyhow::Result;
use clap::{Args, Subcommand};
use futures::StreamExt;
use nirion_lib::{
    context::NirionContext,
    health::{health_logs_stream, HealthLogStreamOptions},
    projects::TargetSelector,
};
use std::time::Duration;

use crate::{health_render::HealthRenderer, ClapSelector};

/// Inspect service healthchecks
#[derive(Args, Debug, Clone)]
pub struct HealthArgs {
    #[command(subcommand)]
    command: HealthCommand,
}

#[derive(Subcommand, Debug, Clone)]
enum HealthCommand {
    /// Show healthcheck logs
    Logs(HealthLogsArgs),
}

#[derive(Args, Debug, Clone)]
struct HealthLogsArgs {
    /// Target selector: *, project, or project.service
    #[arg(
        default_value = "*",
        value_parser = TargetSelector::clap_parse,
        add = TargetSelector::clap_completer()
    )]
    target: TargetSelector,

    /// Follow healthcheck log output
    #[arg(short = 'f', long)]
    follow: bool,

    /// Refresh interval for discovering healthcheck changes when following
    #[arg(short = 'r', long, default_value = "250ms", value_parser = humantime::parse_duration)]
    refresh: Duration,
}

pub async fn handle_health(
    args: &HealthArgs,
    context: &NirionContext,
) -> Result<()> {
    match &args.command {
        HealthCommand::Logs(args) => handle_health_logs(args, context).await?,
    }

    Ok(())
}

async fn handle_health_logs(
    args: &HealthLogsArgs,
    context: &NirionContext,
) -> Result<()> {
    let options = HealthLogStreamOptions {
        follow: args.follow,
        refresh_interval: args.refresh,
    };
    let mut renderer = HealthRenderer::new();
    let mut stream =
        health_logs_stream(context.clone(), args.target.clone(), options);

    while let Some(event) = stream.next().await {
        renderer.render(event?)?;
    }
    Ok(())
}
