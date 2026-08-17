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
pub type CommandOutputEventStream =
    BoxStream<'static, anyhow::Result<CommandOutputEvent>>;
pub type StatusEventStream =
    BoxStream<'static, anyhow::Result<ProjectStatusEvent>>;
pub type LogEventStream = BoxStream<'static, anyhow::Result<LogEvent>>;
pub type HealthLogEventStream =
    BoxStream<'static, anyhow::Result<HealthLogEvent>>;
pub type LockUpdateEventStream =
    BoxStream<'static, anyhow::Result<LockUpdateEvent>>;

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

#[derive(Debug, Clone)]
pub enum CommandOutputEvent {
    ProjectStarted {
        project: String,
    },
    Output {
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

impl From<ComposeEvent> for CommandOutputEvent {
    fn from(event: ComposeEvent) -> Self {
        match event {
            ComposeEvent::ProjectStarted { project } => {
                Self::ProjectStarted { project }
            }
            ComposeEvent::Process { project, event } => {
                Self::Output { project, event }
            }
            ComposeEvent::ProjectFailed { project, error } => {
                Self::ProjectFailed { project, error }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct LifecycleOperation {
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
pub struct PullOperation {
    pub target: TargetSelector,
    pub concurrency: ComposeConcurrency,
}

#[derive(Debug, Clone)]
pub struct TopOperation {
    pub target: TargetSelector,
}

#[derive(Debug, Clone)]
pub struct VolumesOperation {
    pub target: TargetSelector,
    pub format: String,
    pub quiet: bool,
}

#[derive(Debug, Clone)]
pub struct ComposePassthroughOperation {
    pub target: TargetSelector,
    pub args: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct StatusStreamRequest {
    pub target: TargetSelector,
    pub refresh_interval: Duration,
}

#[derive(Debug, Clone)]
pub struct ProjectStatusQuery {
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
pub struct InspectQuery {
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
pub struct LockUpdateOperation {
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

    async fn lifecycle(
        &self,
        operation: LifecycleOperation,
    ) -> OperationEventStream;

    async fn pull(
        &self,
        operation: PullOperation,
    ) -> OperationEventStream;

    async fn top(
        &self,
        operation: TopOperation,
    ) -> CommandOutputEventStream;

    async fn volumes(
        &self,
        operation: VolumesOperation,
    ) -> CommandOutputEventStream;

    async fn compose_passthrough(
        &self,
        operation: ComposePassthroughOperation,
    ) -> CommandOutputEventStream;

    async fn status_stream(
        &self,
        request: StatusStreamRequest,
    ) -> StatusEventStream;

    async fn project_status(
        &self,
        query: ProjectStatusQuery,
    ) -> anyhow::Result<ProjectStatus>;

    async fn log_stream(
        &self,
        request: LogsRequest,
    ) -> LogEventStream;

    async fn health_log_stream(
        &self,
        request: HealthLogsRequest,
    ) -> HealthLogEventStream;

    async fn exec(
        &self,
        request: ExecRequest,
        io: ExecIo,
    ) -> anyhow::Result<()>;

    async fn inspect(
        &self,
        query: InspectQuery,
    ) -> anyhow::Result<Vec<String>>;

    async fn lock_updates(
        &self,
        request: LockUpdateOperation,
    ) -> LockUpdateEventStream;
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

    async fn lifecycle(
        &self,
        operation: LifecycleOperation,
    ) -> OperationEventStream {
        operation_events(compose_stream(
            self.context.clone(),
            operation.target,
            lifecycle_args(operation.action),
            operation.concurrency,
        ))
    }

    async fn pull(
        &self,
        operation: PullOperation,
    ) -> OperationEventStream {
        operation_events(compose_stream(
            self.context.clone(),
            operation.target,
            vec!["pull".to_string()],
            operation.concurrency,
        ))
    }

    async fn top(
        &self,
        operation: TopOperation,
    ) -> CommandOutputEventStream {
        command_output_events(compose_stream(
            self.context.clone(),
            operation.target,
            vec!["top".to_string()],
            ComposeConcurrency::sequential(),
        ))
    }

    async fn volumes(
        &self,
        operation: VolumesOperation,
    ) -> CommandOutputEventStream {
        let args = volumes_args(&operation);
        command_output_events(compose_stream(
            self.context.clone(),
            operation.target,
            args,
            ComposeConcurrency::sequential(),
        ))
    }

    async fn compose_passthrough(
        &self,
        operation: ComposePassthroughOperation,
    ) -> CommandOutputEventStream {
        command_output_events(compose_stream(
            self.context.clone(),
            operation.target,
            operation.args,
            ComposeConcurrency::sequential(),
        ))
    }

    async fn status_stream(
        &self,
        request: StatusStreamRequest,
    ) -> StatusEventStream {
        status_stream(&self.context, request.target, request.refresh_interval)
    }

    async fn project_status(
        &self,
        query: ProjectStatusQuery,
    ) -> anyhow::Result<ProjectStatus> {
        query_project_status(&self.context, &query.project).await
    }

    async fn log_stream(
        &self,
        request: LogsRequest,
    ) -> LogEventStream {
        logs_stream(self.context.clone(), request.target, request.options)
    }

    async fn health_log_stream(
        &self,
        request: HealthLogsRequest,
    ) -> HealthLogEventStream {
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
        query: InspectQuery,
    ) -> anyhow::Result<Vec<String>> {
        inspect_targets(&self.context, query).await
    }

    async fn lock_updates(
        &self,
        request: LockUpdateOperation,
    ) -> LockUpdateEventStream {
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

fn operation_events(
    events: BoxStream<'static, anyhow::Result<ComposeEvent>>
) -> OperationEventStream {
    events
        .map(|event| event.map(OperationEvent::from))
        .boxed()
}

fn command_output_events(
    events: BoxStream<'static, anyhow::Result<ComposeEvent>>
) -> CommandOutputEventStream {
    events
        .map(|event| event.map(CommandOutputEvent::from))
        .boxed()
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
    query: InspectQuery,
) -> anyhow::Result<Vec<String>> {
    match query.target {
        TargetSelector::All => {
            let mut outputs = Vec::new();
            for (project_name, _) in context.projects.iter() {
                outputs.extend(
                    inspect_project(
                        context,
                        query.kind,
                        &ProjectSelector {
                            name: project_name.to_string(),
                        },
                        &query.format,
                        query.raw,
                    )
                    .await?,
                );
            }
            Ok(outputs)
        }
        TargetSelector::Project(project) => {
            inspect_project(
                context,
                query.kind,
                &project,
                &query.format,
                query.raw,
            )
            .await
        }
        TargetSelector::Service(service) => {
            let output = match query.kind {
                InspectKind::Container => {
                    inspect_container(
                        context,
                        &service,
                        &query.format,
                        query.raw,
                    )
                    .await?
                }
                InspectKind::Image => {
                    inspect_image(context, &service, &query.format, query.raw)
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

fn volumes_args(request: &VolumesOperation) -> Vec<String> {
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
