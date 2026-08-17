use clap::Args;
use futures::stream;
use nirion_lib::{
    backend::{NirionBackend, StatusStreamRequest},
    wait::WaitTarget,
};
use std::time::Duration;

use crate::progress::run_progress;
use crate::progress_render::StatusProgressRenderer;
use crate::{ClapSelector, TargetSelector};

#[derive(Args, Debug, Clone)]
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
    backend: &dyn NirionBackend,
) -> anyhow::Result<()> {
    run_progress(
        backend,
        &args.target,
        stream::empty(),
        backend.status_stream(StatusStreamRequest {
            target: args.target.clone(),
            refresh_interval: args.refresh,
        }),
        StatusProgressRenderer::without_spinner(),
        WaitTarget::Forever,
    )
    .await?;

    Ok(())
}
