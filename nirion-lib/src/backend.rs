use std::{future::Future, ops::Deref, time::Duration};

use futures::stream::BoxStream;

use crate::{
    compose::{ComposeConcurrency, compose_stream},
    context::NirionContext,
    docker::{
        ProjectStatus, ProjectStatusEvent, query_project_status, status_stream,
    },
    events::ComposeEvent,
    projects::{Projects, TargetSelector},
};

pub type OperationEvent = ComposeEvent;
pub type OperationEventStream =
    BoxStream<'static, anyhow::Result<OperationEvent>>;

#[derive(Debug, Clone)]
pub enum DispatchRequest {
    Lifecycle(LifecycleRequest),
    Pull(PullRequest),
    Top(TopRequest),
    Volumes(VolumesRequest),
}

#[derive(Debug, Clone)]
pub struct LifecycleRequest {
    pub target: TargetSelector,
    pub action: LifecycleAction,
    pub concurrency: ComposeConcurrency,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleAction {
    Up,
    Down,
    Start,
    Stop,
    Restart,
}

#[derive(Debug, Clone)]
pub struct PullRequest {
    pub target: TargetSelector,
    pub concurrency: ComposeConcurrency,
}

#[derive(Debug, Clone)]
pub struct TopRequest {
    pub target: TargetSelector,
}

#[derive(Debug, Clone)]
pub struct VolumesRequest {
    pub target: TargetSelector,
    pub format: String,
    pub quiet: bool,
}

#[derive(Debug, Clone)]
pub struct StatusStreamRequest {
    pub target: TargetSelector,
    pub refresh_interval: Duration,
}

#[derive(Debug, Clone)]
pub struct ProjectStatusRequest {
    pub project: String,
}

pub trait NirionBackend {
    fn projects(&self) -> Projects;

    fn dispatch(
        &self,
        request: DispatchRequest,
    ) -> OperationEventStream;

    fn status_stream(
        &self,
        request: StatusStreamRequest,
    ) -> BoxStream<'static, anyhow::Result<ProjectStatusEvent>>;

    fn project_status(
        &self,
        request: ProjectStatusRequest,
    ) -> impl Future<Output = anyhow::Result<ProjectStatus>> + Send;
}

#[derive(Clone)]
pub struct LocalBackend {
    context: NirionContext,
}

impl LocalBackend {
    pub fn new(context: NirionContext) -> Self {
        Self { context }
    }

    pub fn context(&self) -> &NirionContext {
        &self.context
    }
}

impl Deref for LocalBackend {
    type Target = NirionContext;

    fn deref(&self) -> &Self::Target {
        &self.context
    }
}

impl NirionBackend for LocalBackend {
    fn projects(&self) -> Projects {
        self.context.projects.clone()
    }

    fn dispatch(
        &self,
        request: DispatchRequest,
    ) -> OperationEventStream {
        match request {
            DispatchRequest::Lifecycle(request) => compose_stream(
                self.context.clone(),
                request.target,
                lifecycle_args(request.action),
                request.concurrency,
            ),
            DispatchRequest::Pull(request) => compose_stream(
                self.context.clone(),
                request.target,
                vec!["pull".to_string()],
                request.concurrency,
            ),
            DispatchRequest::Top(request) => compose_stream(
                self.context.clone(),
                request.target,
                vec!["top".to_string()],
                ComposeConcurrency::sequential(),
            ),
            DispatchRequest::Volumes(request) => {
                let args = volumes_args(&request);
                compose_stream(
                    self.context.clone(),
                    request.target,
                    args,
                    ComposeConcurrency::sequential(),
                )
            }
        }
    }

    fn status_stream(
        &self,
        request: StatusStreamRequest,
    ) -> BoxStream<'static, anyhow::Result<ProjectStatusEvent>> {
        status_stream(&self.context, request.target, request.refresh_interval)
    }

    async fn project_status(
        &self,
        request: ProjectStatusRequest,
    ) -> anyhow::Result<ProjectStatus> {
        query_project_status(&self.context, &request.project).await
    }
}

fn lifecycle_args(action: LifecycleAction) -> Vec<String> {
    match action {
        LifecycleAction::Up => vec!["up".to_string(), "-d".to_string()],
        LifecycleAction::Down => vec!["down".to_string()],
        LifecycleAction::Start => vec!["start".to_string()],
        LifecycleAction::Stop => vec!["stop".to_string()],
        LifecycleAction::Restart => vec!["restart".to_string()],
    }
}

fn volumes_args(request: &VolumesRequest) -> Vec<String> {
    let mut args = vec![
        "volumes".to_string(),
        "--format".to_string(),
        request.format.clone(),
    ];

    if request.quiet {
        args.push("--quiet".to_string());
    }

    args
}
