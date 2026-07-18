use clap::Parser;
use futures::stream;
use nirion_lib::{
    context::NirionContext, docker::status_stream, wait::WaitTarget,
};
use std::time::Duration;

use crate::progress::run_progress;
use crate::progress_render::StaticStatusRenderer;
use crate::{ClapSelector, TargetSelector};

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
    run_progress(
        context,
        &args.target,
        stream::empty(),
        status_stream(context, args.target.clone(), args.refresh),
        StaticStatusRenderer::default(),
        WaitTarget::Forever,
    )
    .await?;

    Ok(())
}
