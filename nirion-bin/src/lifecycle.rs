use futures::{StreamExt, stream};
use nirion_lib::{
    compose::{ComposeConcurrency, compose_stream},
    context::NirionContext,
    docker::status_stream,
    wait::{WaitTarget, wait_finished},
};
use std::collections::BTreeMap;
use tokio::time::Duration;

use crate::TargetSelector;
use crate::progress::{ProgressExit, run_progress};
use crate::progress_render::{ProgressPresentation, progress_renderer};

#[derive(Debug, Clone, Copy)]
pub struct LifecycleOptions {
    pub presentation: ProgressPresentation,
    pub jobs: usize,
    pub refresh_interval: Duration,
    pub wait: WaitTarget,
}

pub async fn run_lifecycle_command(
    context: &NirionContext,
    target: &TargetSelector,
    args: &[&str],
    options: LifecycleOptions,
) -> anyhow::Result<()> {
    let args = args
        .iter()
        .map(|arg| arg.to_string())
        .collect::<Vec<_>>();
    let compose_events = compose_stream(
        context.clone(),
        target.clone(),
        args,
        ComposeConcurrency::Jobs(options.jobs),
    );

    let renderer = progress_renderer(options.presentation);

    let needs_status = renderer.needs_status_during_compose()
        || (options.wait == WaitTarget::Healthchecks
            && !wait_finished(
                target,
                &context.projects,
                &BTreeMap::new(),
                WaitTarget::Healthchecks,
            ));
    let status_events = if needs_status {
        status_stream(context, target.clone(), options.refresh_interval)
    } else {
        stream::pending().boxed()
    };

    match run_progress(
        context,
        target,
        compose_events,
        status_events,
        renderer,
        options.wait,
    )
    .await?
    {
        ProgressExit::Completed => Ok(()),
        ProgressExit::Cancelled => Err(anyhow::anyhow!("interrupted")),
    }
}
