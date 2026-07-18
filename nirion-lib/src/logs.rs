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
    pub fn new(
        project: impl Into<String>,
        service: impl Into<String>,
        container_id: impl Into<String>,
        container_name: impl Into<String>,
        exit_code: Option<i64>,
        exited: bool,
    ) -> Self {
        Self {
            project: project.into(),
            service: service.into(),
            container_id: container_id.into(),
            container_name: container_name.into(),
            exit_code,
            exited,
        }
    }

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
        .map(|(_, service)| {
            LogSource::new(
                project,
                service.service.clone(),
                service.id.clone(),
                service.container_name.clone(),
                service.exit_code,
                matches!(
                    service.state,
                    ServiceState::Succeeded | ServiceState::Failed
                ),
            )
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        context::NirionContext,
        docker::{DockerCommand, ServiceStatus},
        lock::LockedImages,
        projects::{ProjectSelector, ServiceSelector},
    };
    use futures::StreamExt;
    use nirion_oci_lib::client::NirionOciClient;
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
        sync::{
            Arc,
            atomic::{AtomicU64, Ordering},
        },
        time::{SystemTime, UNIX_EPOCH},
    };
    use tokio::io::AsyncWriteExt;

    struct TempDir {
        path: PathBuf,
    }

    static NEXT_TEMP_DIR_ID: AtomicU64 = AtomicU64::new(0);

    impl TempDir {
        fn new() -> Self {
            let counter = NEXT_TEMP_DIR_ID.fetch_add(1, Ordering::Relaxed);
            let id = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "nirion-logs-test-{}-{id}-{counter}",
                std::process::id()
            ));
            fs::create_dir(&path).unwrap();
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn context(docker_command: DockerCommand) -> NirionContext {
        NirionContext {
            projects: Default::default(),
            locked_images: LockedImages::default(),
            lock_file: PathBuf::from("lock.json"),
            oci_client: Arc::new(NirionOciClient::builder().build()),
            docker_command,
        }
    }

    fn write_script(
        path: &Path,
        script: &str,
    ) {
        fs::write(path, script).unwrap();
        let mut permissions = fs::metadata(path)
            .unwrap()
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }

    fn options(follow: bool) -> LogStreamOptions {
        LogStreamOptions {
            follow,
            refresh_interval: Duration::from_millis(10),
            since: None,
            until: None,
            tail: None,
            timestamps: false,
        }
    }

    fn source() -> LogSource {
        LogSource::new("project", "web", "abc", "container", None, false)
    }

    fn coordinator() -> (
        FollowLogCoordinator,
        mpsc::UnboundedReceiver<anyhow::Result<LogEvent>>,
    ) {
        let (tx, rx) = mpsc::unbounded();
        (
            FollowLogCoordinator::new(
                context(DockerCommand::new("docker")),
                TargetSelector::All,
                options(true),
                tx,
            ),
            rx,
        )
    }

    fn service(
        name: &str,
        state: ServiceState,
        exit_code: Option<i64>,
    ) -> ServiceStatus {
        ServiceStatus {
            id: format!("{name}-id"),
            service: name.to_string(),
            container_name: format!("project-{name}-1"),
            image: "image".to_string(),
            state,
            health: None,
            exit_code,
            running_for: None,
            status: None,
            ports: Vec::new(),
            networks: Vec::new(),
        }
    }

    fn status(services: Vec<ServiceStatus>) -> ProjectStatus {
        ProjectStatus {
            services: services
                .into_iter()
                .map(|service| (service.service.clone(), service))
                .collect(),
        }
    }

    #[test]
    fn sources_from_status_uses_service_state_for_exited_flag() {
        let status = status(vec![
            service("web", ServiceState::Running, Some(0)),
            service("worker", ServiceState::Succeeded, Some(0)),
            service("job", ServiceState::Failed, Some(1)),
        ]);

        let sources =
            sources_from_status(&TargetSelector::All, "project", &status);

        assert_eq!(sources.len(), 3);
        assert!(
            !sources
                .iter()
                .find(|source| source.service == "web")
                .unwrap()
                .exited
        );
        assert!(
            sources
                .iter()
                .find(|source| source.service == "worker")
                .unwrap()
                .exited
        );
        assert!(
            sources
                .iter()
                .find(|source| source.service == "job")
                .unwrap()
                .exited
        );
    }

    #[test]
    fn sources_from_status_filters_by_target() {
        let status = status(vec![
            service("web", ServiceState::Running, None),
            service("db", ServiceState::Running, None),
        ]);

        let project_sources = sources_from_status(
            &TargetSelector::Project(ProjectSelector {
                name: "project".to_string(),
            }),
            "project",
            &status,
        );
        assert_eq!(project_sources.len(), 2);

        let service_sources = sources_from_status(
            &TargetSelector::Service(ServiceSelector {
                project: "project".to_string(),
                service: "web".to_string(),
            }),
            "project",
            &status,
        );
        assert_eq!(service_sources.len(), 1);
        assert_eq!(service_sources[0].service, "web");

        let other_project_sources = sources_from_status(
            &TargetSelector::Project(ProjectSelector {
                name: "other".to_string(),
            }),
            "project",
            &status,
        );
        assert!(other_project_sources.is_empty());
    }

    #[test]
    fn log_source_builds_lifecycle_and_line_events() {
        let running =
            LogSource::new("project", "web", "abc", "container", None, false);
        let exited =
            LogSource::new("project", "job", "def", "job-1", Some(7), true);

        assert_eq!(
            running.attached_event(),
            LogEvent::SourceAttached(running.clone())
        );
        assert_eq!(
            running.detached_event(),
            LogEvent::SourceDetached(running.clone())
        );
        assert_eq!(
            running.end_event(),
            LogEvent::SourceDetached(running.clone())
        );
        assert_eq!(exited.end_event(), LogEvent::SourceExited(exited.clone()));
        assert_eq!(
            running.log_line("hello".to_string()),
            LogLine {
                source: running,
                line: "hello".to_string(),
            }
        );
    }

    #[test]
    fn docker_logs_command_includes_optional_flags() {
        let context = context(DockerCommand::new("docker"));
        let mut options = options(true);
        options.timestamps = true;
        options.since = Some("2026-07-18T00:00:00Z".to_string());
        options.until = Some("2026-07-19T00:00:00Z".to_string());
        options.tail = Some("42".to_string());
        let source = source();

        let command = docker_logs_command(&context, &options, &source, true);
        let args = command
            .as_std()
            .get_args()
            .collect::<Vec<_>>();

        assert_eq!(
            args,
            [
                "logs",
                "--follow",
                "--timestamps",
                "--since",
                "2026-07-18T00:00:00Z",
                "--until",
                "2026-07-19T00:00:00Z",
                "--tail",
                "42",
                "abc",
            ]
        );
    }

    #[tokio::test]
    async fn container_is_running_reads_docker_inspect_output() {
        let dir = TempDir::new();
        let script = dir.path().join("docker.sh");
        write_script(
            &script,
            r#"if [ "$4" = "running" ]; then
  printf '%s\n' true
else
  printf '%s\n' false
fi
"#,
        );
        let context = context(DockerCommand::with_args("/bin/sh", [script]));

        assert!(
            container_is_running(
                &context,
                &LogSource::new(
                    "project",
                    "web",
                    "running",
                    "container",
                    None,
                    false
                ),
            )
            .await
        );
        assert!(
            !container_is_running(
                &context,
                &LogSource::new(
                    "project",
                    "web",
                    "stopped",
                    "container",
                    None,
                    false
                ),
            )
            .await
        );
    }

    #[tokio::test]
    async fn read_lines_emits_events_until_receiver_closes() {
        let (mut writer, reader) = tokio::io::duplex(64);
        let (tx, mut rx) = mpsc::unbounded();
        let source = source();

        let writer_task = tokio::spawn(async move {
            writer
                .write_all(b"one\ntwo\n")
                .await
                .unwrap();
        });
        read_lines(reader, LogEvent::StdoutLine, source.clone(), tx).await;
        writer_task.await.unwrap();

        assert_eq!(
            rx.next().await.unwrap().unwrap(),
            LogEvent::StdoutLine(LogLine {
                source: source.clone(),
                line: "one".to_string(),
            })
        );
        assert_eq!(
            rx.next().await.unwrap().unwrap(),
            LogEvent::StdoutLine(LogLine {
                source,
                line: "two".to_string(),
            })
        );
        assert!(rx.next().await.is_none());
    }

    #[tokio::test]
    async fn read_logs_snapshot_streams_output_and_returns_source() {
        let dir = TempDir::new();
        let script = dir.path().join("docker.sh");
        write_script(
            &script,
            r#"case "$1" in
  logs)
    printf '%s\n' stdout-line
    printf '%s\n' stderr-line >&2
    ;;
esac
"#,
        );
        let context = context(DockerCommand::with_args("/bin/sh", [script]));
        let source = source();
        let (tx, mut rx) = mpsc::unbounded();

        let result = read_logs(
            context,
            options(false),
            source.clone(),
            tx,
            LogReadMode::Snapshot,
        )
        .await;

        assert_eq!(result, Some(source.clone()));
        assert_eq!(rx.next().await.unwrap().unwrap(), source.attached_event());
        assert_eq!(
            rx.next().await.unwrap().unwrap(),
            LogEvent::StdoutLine(source.log_line("stdout-line".to_string()))
        );
        assert_eq!(
            rx.next().await.unwrap().unwrap(),
            LogEvent::StderrLine(source.log_line("stderr-line".to_string()))
        );
        assert_eq!(rx.next().await.unwrap().unwrap(), source.end_event());
        assert!(rx.next().await.is_none());
    }

    #[tokio::test]
    async fn read_logs_reports_spawn_failure() {
        let context =
            context(DockerCommand::new("/does/not/exist/nirion-docker"));
        let (tx, mut rx) = mpsc::unbounded();

        let result = read_logs(
            context,
            options(false),
            source(),
            tx,
            LogReadMode::Snapshot,
        )
        .await;

        assert_eq!(result, None);
        assert!(matches!(
            rx.next().await.unwrap(),
            Ok(LogEvent::SourceAttached(_))
        ));
        assert!(rx.next().await.unwrap().is_err());
        assert!(rx.next().await.is_none());
    }

    #[tokio::test]
    async fn read_logs_snapshot_reports_failed_status() {
        let dir = TempDir::new();
        let script = dir.path().join("docker.sh");
        write_script(
            &script,
            r#"case "$1" in
  logs)
    exit 42
    ;;
esac
"#,
        );
        let context = context(DockerCommand::with_args("/bin/sh", [script]));
        let (tx, mut rx) = mpsc::unbounded();

        let result = read_logs(
            context,
            options(false),
            source(),
            tx,
            LogReadMode::Snapshot,
        )
        .await;

        assert_eq!(result, None);
        assert!(matches!(
            rx.next().await.unwrap(),
            Ok(LogEvent::SourceAttached(_))
        ));
        assert!(rx.next().await.unwrap().is_err());
        assert!(rx.next().await.is_none());
    }

    #[tokio::test]
    async fn read_logs_follow_skips_non_running_container() {
        let dir = TempDir::new();
        let script = dir.path().join("docker.sh");
        write_script(
            &script,
            r#"case "$1" in
  inspect)
    printf '%s\n' false
    ;;
  logs)
    exit 99
    ;;
esac
"#,
        );
        let context = context(DockerCommand::with_args("/bin/sh", [script]));
        let source = source();
        let (tx, mut rx) = mpsc::unbounded();

        let result = read_logs(
            context,
            options(true),
            source.clone(),
            tx,
            LogReadMode::Follow,
        )
        .await;

        assert_eq!(result, Some(source));
        assert!(rx.next().await.is_none());
    }

    #[tokio::test]
    async fn read_logs_follow_treats_failed_status_as_detach() {
        let dir = TempDir::new();
        let script = dir.path().join("docker.sh");
        write_script(
            &script,
            r#"case "$1" in
  inspect)
    printf '%s\n' true
    ;;
  logs)
    exit 42
    ;;
esac
"#,
        );
        let context = context(DockerCommand::with_args("/bin/sh", [script]));
        let source = source();
        let (tx, mut rx) = mpsc::unbounded();

        let result = read_logs(
            context,
            options(true),
            source.clone(),
            tx,
            LogReadMode::Follow,
        )
        .await;

        assert_eq!(result, Some(source.clone()));
        assert_eq!(rx.next().await.unwrap().unwrap(), source.attached_event());
        assert_eq!(rx.next().await.unwrap().unwrap(), source.detached_event());
        assert!(rx.next().await.is_none());
    }

    #[test]
    fn coordinator_detaches_running_stale_source() {
        let (mut coordinator, mut rx) = coordinator();
        let source = source();
        coordinator
            .attached
            .insert(source.key(), source.clone());

        coordinator.detach_stale_sources("project", &BTreeMap::new());

        assert!(coordinator.attached.is_empty());
        assert_eq!(
            rx.try_recv().unwrap().unwrap(),
            LogEvent::SourceDetached(source)
        );
    }

    #[test]
    fn coordinator_removes_exited_stale_source_without_detach_event() {
        let (mut coordinator, mut rx) = coordinator();
        let source =
            LogSource::new("project", "web", "abc", "container", Some(0), true);
        coordinator
            .attached
            .insert(source.key(), source.clone());

        coordinator.detach_stale_sources("project", &BTreeMap::new());

        assert!(coordinator.attached.is_empty());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn coordinator_updates_existing_source_when_it_exits() {
        let (mut coordinator, mut rx) = coordinator();
        let running = source();
        let exited =
            LogSource::new("project", "web", "abc", "container", Some(0), true);
        let key = running.key();
        coordinator
            .attached
            .insert(key.clone(), running);

        assert!(coordinator.update_existing_source(&key, &exited));
        assert_eq!(coordinator.attached[&key], exited);
        assert_eq!(
            rx.try_recv().unwrap().unwrap(),
            LogEvent::SourceExited(coordinator.attached[&key].clone())
        );
    }

    #[test]
    fn coordinator_does_not_update_replaced_source() {
        let (mut coordinator, mut rx) = coordinator();
        let old = source();
        let new =
            LogSource::new("project", "web", "def", "container", None, false);
        let key = old.key();
        coordinator
            .attached
            .insert(key.clone(), old.clone());

        assert!(!coordinator.update_existing_source(&key, &new));
        assert_eq!(coordinator.attached[&key], old);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn coordinator_removes_matching_reader_when_done() {
        let (mut coordinator, _rx) = coordinator();
        let source = source();
        coordinator
            .attached
            .insert(source.key(), source.clone());

        coordinator.handle_reader_done(source);

        assert!(coordinator.attached.is_empty());
    }

    #[test]
    fn coordinator_ignores_reader_done_for_replaced_source() {
        let (mut coordinator, _rx) = coordinator();
        let old = source();
        let new =
            LogSource::new("project", "web", "def", "container", None, false);
        let key = old.key();
        coordinator
            .attached
            .insert(key.clone(), new.clone());

        coordinator.handle_reader_done(old);

        assert_eq!(coordinator.attached[&key], new);
    }
}
