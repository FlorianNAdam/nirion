use std::{collections::BTreeMap, time::Duration};

use async_trait::async_trait;
use futures::{StreamExt, stream::BoxStream};

use crate::{
    compose::{ComposeConcurrency, compose_stream},
    context::NirionContext,
    docker::{
        ProjectStatus, ProjectStatusEvent, query_project_status, status_stream,
    },
    events::{ComposeEvent, LockUpdateEvent, ProcessEvent},
    exec::{ExecIo, ExecRequest, exec as run_exec},
    health::{HealthLogEvent, HealthLogStreamOptions, health_logs_stream},
    inspect::{
        inspect_container, inspect_image, inspect_project_containers,
        inspect_project_images,
    },
    lock::LockedImages,
    lock_update::image_update_stream,
    logs::{LogEvent, LogStreamOptions, logs_stream},
    projects::{ProjectSelector, Projects, TargetSelector, get_images},
};

pub type OperationEventStream =
    BoxStream<'static, anyhow::Result<OperationEvent>>;

#[derive(Debug, Clone)]
pub enum OperationEvent {
    ProjectStarted {
        project: String,
    },
    Process {
        project: Option<String>,
        event: ProcessEvent,
    },
    ProjectFailed {
        project: String,
        error: String,
    },
}

impl From<ComposeEvent> for OperationEvent {
    fn from(event: ComposeEvent) -> Self {
        match event {
            ComposeEvent::ProjectStarted { project } => {
                Self::ProjectStarted { project }
            }
            ComposeEvent::Process { project, event } => {
                Self::Process { project, event }
            }
            ComposeEvent::ProjectFailed { project, error } => {
                Self::ProjectFailed { project, error }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum DispatchRequest {
    Lifecycle(LifecycleRequest),
    Pull(PullRequest),
    Top(TopRequest),
    Volumes(VolumesRequest),
    ComposePassthrough(ComposePassthroughRequest),
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
pub struct ComposePassthroughRequest {
    pub target: TargetSelector,
    pub args: Vec<String>,
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

#[derive(Debug, Clone)]
pub struct LogsRequest {
    pub target: TargetSelector,
    pub options: LogStreamOptions,
}

#[derive(Debug, Clone)]
pub struct HealthLogsRequest {
    pub target: TargetSelector,
    pub options: HealthLogStreamOptions,
}

#[derive(Debug, Clone)]
pub struct InspectRequest {
    pub target: TargetSelector,
    pub kind: InspectKind,
    pub format: String,
    pub raw: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InspectKind {
    Container,
    Image,
}

#[derive(Debug, Clone)]
pub struct LockUpdateRequest {
    pub target: TargetSelector,
    pub mode: LockUpdateMode,
    pub jobs: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockUpdateMode {
    MissingOnly,
    UpdateAll,
}

#[async_trait]
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

    async fn project_status(
        &self,
        request: ProjectStatusRequest,
    ) -> anyhow::Result<ProjectStatus>;

    fn logs(
        &self,
        request: LogsRequest,
    ) -> BoxStream<'static, anyhow::Result<LogEvent>>;

    fn health_logs(
        &self,
        request: HealthLogsRequest,
    ) -> BoxStream<'static, anyhow::Result<HealthLogEvent>>;

    async fn exec(
        &self,
        request: ExecRequest,
        io: ExecIo,
    ) -> anyhow::Result<()>;

    async fn inspect(
        &self,
        request: InspectRequest,
    ) -> anyhow::Result<Vec<String>>;

    fn lock_updates(
        &self,
        request: LockUpdateRequest,
    ) -> BoxStream<'static, anyhow::Result<LockUpdateEvent>>;
}

#[derive(Clone)]
pub struct LocalBackend {
    context: NirionContext,
}

impl LocalBackend {
    pub fn new(context: NirionContext) -> Self {
        Self { context }
    }
}

#[async_trait]
impl NirionBackend for LocalBackend {
    fn projects(&self) -> Projects {
        self.context.projects.clone()
    }

    fn dispatch(
        &self,
        request: DispatchRequest,
    ) -> OperationEventStream {
        let events = match request {
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
            DispatchRequest::ComposePassthrough(request) => compose_stream(
                self.context.clone(),
                request.target,
                request.args,
                ComposeConcurrency::sequential(),
            ),
        };

        events
            .map(|event| event.map(OperationEvent::from))
            .boxed()
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

    fn logs(
        &self,
        request: LogsRequest,
    ) -> BoxStream<'static, anyhow::Result<LogEvent>> {
        logs_stream(self.context.clone(), request.target, request.options)
    }

    fn health_logs(
        &self,
        request: HealthLogsRequest,
    ) -> BoxStream<'static, anyhow::Result<HealthLogEvent>> {
        health_logs_stream(
            self.context.clone(),
            request.target,
            request.options,
        )
    }

    async fn exec(
        &self,
        request: ExecRequest,
        io: ExecIo,
    ) -> anyhow::Result<()> {
        run_exec(&self.context, &request, io).await
    }

    async fn inspect(
        &self,
        request: InspectRequest,
    ) -> anyhow::Result<Vec<String>> {
        inspect_targets(&self.context, request).await
    }

    fn lock_updates(
        &self,
        request: LockUpdateRequest,
    ) -> BoxStream<'static, anyhow::Result<LockUpdateEvent>> {
        let mut images = get_images(&request.target, &self.context.projects);
        if request.mode == LockUpdateMode::MissingOnly {
            retain_images_missing_lock_entries(
                &mut images,
                &self.context.locked_images,
            );
        }

        image_update_stream(&self.context, images, request.jobs)
    }
}

fn retain_images_missing_lock_entries(
    images: &mut BTreeMap<String, String>,
    locked_images: &LockedImages,
) {
    images.retain(|name, image| {
        locked_images
            .get(name)
            .map(|locked| locked.image != *image)
            .unwrap_or(true)
    });
}

async fn inspect_targets(
    context: &NirionContext,
    request: InspectRequest,
) -> anyhow::Result<Vec<String>> {
    match request.target {
        TargetSelector::All => {
            let mut outputs = Vec::new();
            for (project_name, _) in context.projects.iter() {
                outputs.extend(
                    inspect_project(
                        context,
                        request.kind,
                        &ProjectSelector {
                            name: project_name.to_string(),
                        },
                        &request.format,
                        request.raw,
                    )
                    .await?,
                );
            }
            Ok(outputs)
        }
        TargetSelector::Project(project) => {
            inspect_project(
                context,
                request.kind,
                &project,
                &request.format,
                request.raw,
            )
            .await
        }
        TargetSelector::Service(service) => {
            let output = match request.kind {
                InspectKind::Container => {
                    inspect_container(
                        context,
                        &service,
                        &request.format,
                        request.raw,
                    )
                    .await?
                }
                InspectKind::Image => {
                    inspect_image(
                        context,
                        &service,
                        &request.format,
                        request.raw,
                    )
                    .await?
                }
            };
            Ok(vec![output])
        }
    }
}

async fn inspect_project(
    context: &NirionContext,
    kind: InspectKind,
    project: &ProjectSelector,
    format: &str,
    raw: bool,
) -> anyhow::Result<Vec<String>> {
    match kind {
        InspectKind::Container => {
            inspect_project_containers(context, project, format, raw).await
        }
        InspectKind::Image => {
            inspect_project_images(context, project, format, raw).await
        }
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
