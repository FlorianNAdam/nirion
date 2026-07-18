use std::{collections::BTreeMap, time::Duration};

use anyhow::Context;
use futures::{StreamExt, channel::mpsc, stream::BoxStream};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, BufReader},
    process::Command,
    task::JoinSet,
};

use crate::{
    context::NirionContext,
    docker::{
        ProjectStatus, ServiceState, query_project_status, status_stream,
    },
    projects::{TargetSelector, selected_project_names},
};

#[derive(Debug, Clone)]
pub struct LogStreamOptions {
    pub follow: bool,
    pub refresh_interval: Duration,
    pub since: Option<String>,
    pub until: Option<String>,
    pub tail: Option<String>,
    pub timestamps: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogEvent {
    SourceAttached(LogSource),
    StdoutLine(LogLine),
    StderrLine(LogLine),
    SourceExited(LogSource),
    SourceDetached(LogSource),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SourceKey {
    project: String,
    service: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogSource {
    pub project: String,
    pub service: String,
    pub container_id: String,
    pub container_name: String,
    pub exit_code: Option<i64>,
    exited: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    pub source: LogSource,
    pub line: String,
}

impl LogSource {
    fn key(&self) -> SourceKey {
        SourceKey {
            project: self.project.clone(),
            service: self.service.clone(),
        }
    }

    fn attached_event(&self) -> LogEvent {
        LogEvent::SourceAttached(self.clone())
    }

    fn detached_event(&self) -> LogEvent {
        LogEvent::SourceDetached(self.clone())
    }

    fn end_event(&self) -> LogEvent {
        if self.exited {
            LogEvent::SourceExited(self.clone())
        } else {
            self.detached_event()
        }
    }

    fn log_line(
        &self,
        line: String,
    ) -> LogLine {
        LogLine {
            source: self.clone(),
            line,
        }
    }
}

type LogEventTx = mpsc::UnboundedSender<anyhow::Result<LogEvent>>;

#[derive(Clone, Copy)]
enum LogReadMode {
    Follow,
    Snapshot,
}

struct FollowLogCoordinator {
    context: NirionContext,
    target: TargetSelector,
    options: LogStreamOptions,
    tx: LogEventTx,
    attached: BTreeMap<SourceKey, LogSource>,
}

impl FollowLogCoordinator {
    fn new(
        context: NirionContext,
        target: TargetSelector,
        options: LogStreamOptions,
        tx: LogEventTx,
    ) -> Self {
        Self {
            context,
            target,
            options,
            tx,
            attached: BTreeMap::new(),
        }
    }

    fn reconcile_project(
        &mut self,
        project: &str,
        status: &ProjectStatus,
        readers: &mut JoinSet<Option<LogSource>>,
    ) {
        let sources = sources_from_status(&self.target, project, status);
        let current = sources
            .iter()
            .map(|source| (source.key(), source.clone()))
            .collect::<BTreeMap<_, _>>();

        self.detach_stale_sources(project, &current);
        self.attach_new_sources(sources, readers);
    }

    fn detach_stale_sources(
        &mut self,
        project: &str,
        current: &BTreeMap<SourceKey, LogSource>,
    ) {
        let stale = self
            .attached
            .iter()
            .filter(|(key, _)| key.project == project)
            .filter(|(key, old)| {
                current
                    .get(key)
                    .map(|new| new.container_id != old.container_id)
                    .unwrap_or(true)
            })
            .map(|(_, source)| source.clone())
            .collect::<Vec<_>>();

        for source in stale {
            if !source.exited {
                self.emit_event(source.detached_event());
            }
            self.attached.remove(&source.key());
        }
    }

    fn attach_new_sources(
        &mut self,
        sources: Vec<LogSource>,
        readers: &mut JoinSet<Option<LogSource>>,
    ) {
        for source in sources {
            let key = source.key();
            if self.update_existing_source(&key, &source) {
                continue;
            }

            if source.exited {
                self.attached
                    .insert(key, source.clone());
                tokio::spawn(read_logs(
                    self.context.clone(),
                    self.options.clone(),
                    source,
                    self.tx.clone(),
                    LogReadMode::Snapshot,
                ));
                continue;
            }

            self.attached
                .insert(key, source.clone());
            readers.spawn(read_logs(
                self.context.clone(),
                self.options.clone(),
                source,
                self.tx.clone(),
                LogReadMode::Follow,
            ));
        }
    }

    fn update_existing_source(
        &mut self,
        key: &SourceKey,
        source: &LogSource,
    ) -> bool {
        let Some(old) = self.attached.get(key) else {
            return false;
        };
        if old.container_id != source.container_id {
            return false;
        }

        if !old.exited && source.exited {
            self.emit_event(source.end_event());
            self.attached
                .insert(key.clone(), source.clone());
        }
        true
    }

    fn handle_reader_done(
        &mut self,
        source: LogSource,
    ) {
        let key = source.key();
        if self
            .attached
            .get(&key)
            .map(|attached| attached.container_id == source.container_id)
            .unwrap_or(false)
        {
            self.attached.remove(&key);
        }
    }

    fn emit_event(
        &self,
        event: LogEvent,
    ) -> bool {
        self.tx
            .unbounded_send(Ok(event))
            .is_ok()
    }
}

pub fn logs_stream(
    context: NirionContext,
    target: TargetSelector,
    options: LogStreamOptions,
) -> BoxStream<'static, anyhow::Result<LogEvent>> {
    let (tx, rx) = mpsc::unbounded();

    tokio::spawn(async move {
        if options.follow {
            follow_logs(context, target, options, tx).await;
        } else {
            bounded_logs(context, target, options, tx).await;
        }
    });

    rx.boxed()
}

async fn bounded_logs(
    context: NirionContext,
    target: TargetSelector,
    options: LogStreamOptions,
    tx: LogEventTx,
) {
    match query_sources(&context, &target).await {
        Ok(sources) => {
            for source in sources {
                tokio::spawn(read_logs(
                    context.clone(),
                    options.clone(),
                    source,
                    tx.clone(),
                    LogReadMode::Snapshot,
                ));
            }
        }
        Err(error) => {
            let _ = tx.unbounded_send(Err(error));
        }
    }
}

async fn follow_logs(
    context: NirionContext,
    target: TargetSelector,
    options: LogStreamOptions,
    tx: LogEventTx,
) {
    let mut coordinator =
        FollowLogCoordinator::new(context, target, options, tx);
    let mut readers = JoinSet::new();
    let mut status_events = status_stream(
        &coordinator.context,
        coordinator.target.clone(),
        coordinator.options.refresh_interval,
    );

    loop {
        tokio::select! {
            event = status_events.next() => {
                let Some(event) = event else {
                    return;
                };

                match event {
                    Ok(event) => {
                        coordinator.reconcile_project(
                            &event.project,
                            &event.status,
                            &mut readers,
                        );
                    }
                    Err(error) => {
                        if coordinator.tx.unbounded_send(Err(error)).is_err() {
                            return;
                        }
                    }
                }
            }

            result = readers.join_next(), if !readers.is_empty() => {
                let Some(result) = result else {
                    continue;
                };
                match result {
                    Ok(Some(source)) => coordinator.handle_reader_done(source),
                    Ok(None) => {}
                    Err(error) => {
                        let _ = coordinator.tx.unbounded_send(Err(error.into()));
                    }
                }
            }
        }

        if coordinator.tx.is_closed() {
            return;
        }
    }
}

async fn query_sources(
    context: &NirionContext,
    target: &TargetSelector,
) -> anyhow::Result<Vec<LogSource>> {
    let mut sources = Vec::new();

    for project in selected_project_names(target, &context.projects) {
        let status = query_project_status(context, &project).await?;
        sources.extend(sources_from_status(target, &project, &status));
    }

    Ok(sources)
}

fn sources_from_status(
    target: &TargetSelector,
    project: &str,
    status: &ProjectStatus,
) -> Vec<LogSource> {
    status
        .services
        .iter()
        .filter(|(service, _)| service_selected(target, project, service))
        .map(|(_, service)| LogSource {
            project: project.to_string(),
            service: service.service.clone(),
            container_id: service.id.clone(),
            container_name: service.container_name.clone(),
            exit_code: service.exit_code,
            exited: matches!(
                service.state,
                ServiceState::Succeeded | ServiceState::Failed
            ),
        })
        .collect()
}

fn service_selected(
    target: &TargetSelector,
    project: &str,
    service: &str,
) -> bool {
    match target {
        TargetSelector::All => true,
        TargetSelector::Project(selected) => selected.name == project,
        TargetSelector::Service(selected) => {
            selected.project == project && selected.service == service
        }
    }
}

async fn read_logs(
    context: NirionContext,
    options: LogStreamOptions,
    source: LogSource,
    tx: LogEventTx,
    mode: LogReadMode,
) -> Option<LogSource> {
    if matches!(mode, LogReadMode::Follow)
        && !container_is_running(&context, &source).await
    {
        return Some(source);
    }

    if tx
        .unbounded_send(Ok(source.attached_event()))
        .is_err()
    {
        return None;
    }

    let mut child = match docker_logs_command(
        &context,
        &options,
        &source,
        matches!(mode, LogReadMode::Follow),
    )
    .spawn()
    .context("failed to execute docker logs")
    {
        Ok(child) => child,
        Err(error) => {
            let _ = tx.unbounded_send(Err(error));
            return None;
        }
    };

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_reader = async {
        if let Some(stdout) = stdout {
            read_lines(
                stdout,
                LogEvent::StdoutLine,
                source.clone(),
                tx.clone(),
            )
            .await;
        }
    };
    let stderr_reader = async {
        if let Some(stderr) = stderr {
            read_lines(
                stderr,
                LogEvent::StderrLine,
                source.clone(),
                tx.clone(),
            )
            .await;
        }
    };

    let (_, _, status) =
        tokio::join!(stdout_reader, stderr_reader, child.wait());

    match status {
        Ok(status) if status.success() => {
            let _ = tx.unbounded_send(Ok(source.end_event()));
            Some(source)
        }
        Ok(status) => {
            if matches!(mode, LogReadMode::Follow) {
                let _ = tx.unbounded_send(Ok(source.detached_event()));
                return Some(source);
            }

            let _ = tx.unbounded_send(Err(anyhow::anyhow!(
                "docker logs for {}.{} exited with status {}",
                source.project,
                source.service,
                status
            )));
            None
        }
        Err(error) => {
            let _ = tx.unbounded_send(Err(error.into()));
            None
        }
    }
}

async fn container_is_running(
    context: &NirionContext,
    source: &LogSource,
) -> bool {
    let output = match context
        .docker_command
        .command()
        .arg("inspect")
        .arg("--format")
        .arg("{{.State.Running}}")
        .arg(&source.container_id)
        .output()
        .await
    {
        Ok(output) => output,
        Err(_) => return false,
    };

    output.status.success()
        && String::from_utf8_lossy(&output.stdout).trim() == "true"
}

fn docker_logs_command(
    context: &NirionContext,
    options: &LogStreamOptions,
    source: &LogSource,
    follow: bool,
) -> Command {
    let mut command = context.docker_command.command();
    command.arg("logs");
    if follow {
        command.arg("--follow");
    }
    if options.timestamps {
        command.arg("--timestamps");
    }
    if let Some(since) = &options.since {
        command.arg("--since").arg(since);
    }
    if let Some(until) = &options.until {
        command.arg("--until").arg(until);
    }
    if let Some(tail) = &options.tail {
        command.arg("--tail").arg(tail);
    }
    command
        .arg(&source.container_id)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    command
}

async fn read_lines(
    stream: impl AsyncRead + Unpin + Send + 'static,
    event: fn(LogLine) -> LogEvent,
    source: LogSource,
    tx: LogEventTx,
) {
    let reader = BufReader::new(stream);
    let mut lines = reader.lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if tx
            .unbounded_send(Ok(event(source.log_line(line))))
            .is_err()
        {
            return;
        }
    }
}
