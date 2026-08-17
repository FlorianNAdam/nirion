use futures::{StreamExt, stream};
use nirion_lib::{
    backend::{
        DispatchRequest, LifecycleAction, LifecycleRequest, NirionBackend,
        StatusStreamRequest,
    },
    compose::ComposeConcurrency,
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
    backend: &dyn NirionBackend,
    target: &TargetSelector,
    action: LifecycleAction,
    options: LifecycleOptions,
) -> anyhow::Result<()> {
    let compose_events =
        backend.dispatch(DispatchRequest::Lifecycle(LifecycleRequest {
            target: target.clone(),
            action,
            concurrency: ComposeConcurrency::Jobs(options.jobs),
        }));

    let renderer = progress_renderer(options.presentation);
    let projects = backend.projects();

    let needs_status = renderer.needs_status_during_compose()
        || (options.wait == WaitTarget::Healthchecks
            && !wait_finished(
                target,
                &projects,
                &BTreeMap::new(),
                WaitTarget::Healthchecks,
            ));
    let status_events = if needs_status {
        backend.status_stream(StatusStreamRequest {
            target: target.clone(),
            refresh_interval: options.refresh_interval,
        })
    } else {
        stream::pending().boxed()
    };

    match run_progress(
        backend,
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
