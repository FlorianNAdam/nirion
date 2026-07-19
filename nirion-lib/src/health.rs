use anyhow::Context;
use chrono::DateTime;
use futures::{StreamExt, channel::mpsc, future::join_all, stream::BoxStream};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    time::{Duration, SystemTime},
};

use crate::{
    context::NirionContext,
    docker::{ProjectStatus, query_project_status, status_stream},
    projects::{TargetSelector, selected_project_names},
};

#[derive(Debug, Clone)]
pub struct HealthLogStreamOptions {
    pub follow: bool,
    pub refresh_interval: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthLogEvent {
    LogEntry(HealthLogRecord),
    NoEntries(HealthLogSnapshot),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthLogSource {
    pub project: String,
    pub service: String,
    pub container_id: String,
    pub container_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthLogSnapshot {
    pub source: HealthLogSource,
    pub status: Option<String>,
    pub entries: Vec<HealthLogEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthLogRecord {
    pub source: HealthLogSource,
    pub status: Option<String>,
    pub entry: HealthLogEntry,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthLogEntry {
    pub start: SystemTime,
    pub end: SystemTime,
    pub exit_code: i64,
    pub output: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct HealthLogSourceKey {
    project: String,
    service: String,
}

#[derive(Debug, Clone, Default)]
struct HealthLogProgress {
    container_id: Option<String>,
    last_entry: Option<HealthLogEntryKey>,
    last_empty_status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct HealthLogEntryKey {
    start: SystemTime,
    end: SystemTime,
    exit_code: i64,
    output: String,
}

struct HealthLogSnapshotResult {
    current_sources: Vec<HealthLogSourceKey>,
    snapshots: Vec<HealthLogSnapshot>,
    failures: Vec<String>,
}

struct FollowHealthLogCoordinator {
    context: NirionContext,
    target: TargetSelector,
    options: HealthLogStreamOptions,
    tx: HealthLogEventTx,
    progress: BTreeMap<HealthLogSourceKey, HealthLogProgress>,
    last_failures: BTreeMap<String, Vec<String>>,
}

impl FollowHealthLogCoordinator {
    fn new(
        context: NirionContext,
        target: TargetSelector,
        options: HealthLogStreamOptions,
        tx: HealthLogEventTx,
    ) -> Self {
        Self {
            context,
            target,
            options,
            tx,
            progress: BTreeMap::new(),
            last_failures: BTreeMap::new(),
        }
    }

    async fn reconcile_project(
        &mut self,
        project: &str,
        status: &ProjectStatus,
    ) -> bool {
        let result = health_log_snapshots_from_status(
            &self.context,
            &self.target,
            project,
            status,
        )
        .await;
        self.emit_snapshot_result(project, result)
    }

    fn emit_snapshot_result(
        &mut self,
        project: &str,
        result: HealthLogSnapshotResult,
    ) -> bool {
        self.prune_project(project, &result.current_sources);

        if !self.emit_snapshots(result.snapshots) {
            return false;
        }

        self.emit_failures(project, &result.failures)
    }

    fn emit_snapshots(
        &mut self,
        snapshots: Vec<HealthLogSnapshot>,
    ) -> bool {
        for snapshot in snapshots {
            if !self.emit_snapshot(snapshot) {
                return false;
            }
        }
        true
    }

    fn emit_snapshot(
        &mut self,
        snapshot: HealthLogSnapshot,
    ) -> bool {
        if snapshot.entries.is_empty() {
            let event = HealthLogEvent::NoEntries(snapshot);
            return !self.should_emit_event(&event) || self.emit_event(event);
        }

        let source = snapshot.source;
        let status = snapshot.status;
        for entry in snapshot.entries {
            let event = HealthLogEvent::LogEntry(HealthLogRecord {
                source: source.clone(),
                status: status.clone(),
                entry,
            });
            if self.should_emit_event(&event) && !self.emit_event(event) {
                return false;
            }
        }

        true
    }

    fn emit_event(
        &self,
        event: HealthLogEvent,
    ) -> bool {
        self.tx
            .unbounded_send(Ok(event))
            .is_ok()
    }

    fn emit_error(
        &self,
        error: anyhow::Error,
    ) -> bool {
        self.tx
            .unbounded_send(Err(error))
            .is_ok()
    }

    fn should_emit_event(
        &mut self,
        event: &HealthLogEvent,
    ) -> bool {
        match event {
            HealthLogEvent::LogEntry(record) => {
                self.should_emit_entry(&record.source, &record.entry)
            }
            HealthLogEvent::NoEntries(snapshot) => {
                self.should_emit_empty_snapshot(snapshot)
            }
        }
    }

    fn should_emit_empty_snapshot(
        &mut self,
        snapshot: &HealthLogSnapshot,
    ) -> bool {
        let state = self.progress_for_source(&snapshot.source);
        if state.last_empty_status == snapshot.status {
            return false;
        }
        state.last_empty_status = snapshot.status.clone();
        true
    }

    fn should_emit_entry(
        &mut self,
        source: &HealthLogSource,
        entry: &HealthLogEntry,
    ) -> bool {
        let state = self.progress_for_source(source);
        let key = entry.key();
        if state
            .last_entry
            .as_ref()
            .map(|last| &key <= last)
            .unwrap_or(false)
        {
            return false;
        }
        state.last_entry = Some(key);
        state.last_empty_status = None;
        true
    }

    fn progress_for_source(
        &mut self,
        source: &HealthLogSource,
    ) -> &mut HealthLogProgress {
        let state = self
            .progress
            .entry(source.key())
            .or_default();
        if state.container_id.as_deref() != Some(&source.container_id) {
            state.container_id = Some(source.container_id.clone());
            state.last_entry = None;
            state.last_empty_status = None;
        }
        state
    }

    fn prune_project(
        &mut self,
        project: &str,
        current: &[HealthLogSourceKey],
    ) {
        self.progress.retain(|key, _| {
            key.project != project
                || current
                    .iter()
                    .any(|current| current == key)
        });
    }

    fn emit_failures(
        &mut self,
        project: &str,
        failures: &[String],
    ) -> bool {
        if failures.is_empty() {
            self.last_failures.remove(project);
            return true;
        }

        if self
            .last_failures
            .get(project)
            .map(|last| last.as_slice() == failures)
            .unwrap_or(false)
        {
            return true;
        }

        self.last_failures
            .insert(project.to_string(), failures.to_vec());
        self.emit_error(anyhow::anyhow!(health_log_failures_message(failures)))
    }
}

type HealthLogEventTx = mpsc::UnboundedSender<anyhow::Result<HealthLogEvent>>;

#[derive(Debug, Deserialize)]
struct DockerHealth {
    #[serde(rename = "Status")]
    status: Option<String>,
    #[serde(rename = "Log", default)]
    log: Vec<DockerHealthLogEntry>,
}

#[derive(Debug, Deserialize)]
struct DockerHealthLogEntry {
    #[serde(rename = "Start")]
    start: String,
    #[serde(rename = "End")]
    end: String,
    #[serde(rename = "ExitCode")]
    exit_code: i64,
    #[serde(rename = "Output")]
    output: String,
}

pub fn health_logs_stream(
    context: NirionContext,
    target: TargetSelector,
    options: HealthLogStreamOptions,
) -> BoxStream<'static, anyhow::Result<HealthLogEvent>> {
    let (tx, rx) = mpsc::unbounded();

    tokio::spawn(async move {
        if options.follow {
            follow_health_logs(context, target, options, tx).await;
        } else {
            bounded_health_logs(context, target, tx).await;
        }
    });

    rx.boxed()
}

async fn bounded_health_logs(
    context: NirionContext,
    target: TargetSelector,
    tx: HealthLogEventTx,
) {
    for project in selected_project_names(&target, &context.projects) {
        let status = match query_project_status(&context, &project).await {
            Ok(status) => status,
            Err(error) => {
                let _ = tx.unbounded_send(Err(error));
                continue;
            }
        };

        let result = health_log_snapshots_from_status(
            &context, &target, &project, &status,
        )
        .await;
        if !emit_snapshots(result.snapshots, &tx) {
            return;
        }
        if !result.failures.is_empty()
            && tx
                .unbounded_send(Err(anyhow::anyhow!(
                    health_log_failures_message(&result.failures)
                )))
                .is_err()
        {
            return;
        }
    }
}

async fn follow_health_logs(
    context: NirionContext,
    target: TargetSelector,
    options: HealthLogStreamOptions,
    tx: HealthLogEventTx,
) {
    let mut coordinator =
        FollowHealthLogCoordinator::new(context, target, options, tx);
    let mut status_events = status_stream(
        &coordinator.context,
        coordinator.target.clone(),
        coordinator.options.refresh_interval,
    );

    while let Some(event) = status_events.next().await {
        match event {
            Ok(event) => {
                if !coordinator
                    .reconcile_project(&event.project, &event.status)
                    .await
                {
                    return;
                }
            }
            Err(error) => {
                if !coordinator.emit_error(error) {
                    return;
                }
            }
        }

        if coordinator.tx.is_closed() {
            return;
        }
    }
}

fn emit_snapshots(
    snapshots: Vec<HealthLogSnapshot>,
    tx: &HealthLogEventTx,
) -> bool {
    for snapshot in snapshots {
        if !emit_snapshot(snapshot, tx) {
            return false;
        }
    }

    true
}

fn emit_snapshot(
    snapshot: HealthLogSnapshot,
    tx: &HealthLogEventTx,
) -> bool {
    if snapshot.entries.is_empty() {
        return tx
            .unbounded_send(Ok(HealthLogEvent::NoEntries(snapshot)))
            .is_ok();
    }

    let source = snapshot.source;
    let status = snapshot.status;
    for entry in snapshot.entries {
        let event = HealthLogEvent::LogEntry(HealthLogRecord {
            source: source.clone(),
            status: status.clone(),
            entry,
        });
        if tx.unbounded_send(Ok(event)).is_err() {
            return false;
        }
    }

    true
}

impl HealthLogSource {
    fn new(
        project: impl Into<String>,
        service: impl Into<String>,
        container_id: impl Into<String>,
        container_name: impl Into<String>,
    ) -> Self {
        Self {
            project: project.into(),
            service: service.into(),
            container_id: container_id.into(),
            container_name: container_name.into(),
        }
    }

    fn key(&self) -> HealthLogSourceKey {
        HealthLogSourceKey {
            project: self.project.clone(),
            service: self.service.clone(),
        }
    }
}

impl HealthLogEntry {
    fn key(&self) -> HealthLogEntryKey {
        HealthLogEntryKey {
            start: self.start,
            end: self.end,
            exit_code: self.exit_code,
            output: self.output.clone(),
        }
    }
}

fn health_log_failures_message(failures: &[String]) -> String {
    format!(
        "failed to read health logs for {} service(s): {}",
        failures.len(),
        failures.join("; ")
    )
}

async fn health_log_snapshots_from_status(
    context: &NirionContext,
    target: &TargetSelector,
    project: &str,
    status: &ProjectStatus,
) -> HealthLogSnapshotResult {
    let sources = sources_from_status(context, target, project, status);
    read_health_snapshots(context, sources).await
}

async fn read_health_snapshots(
    context: &NirionContext,
    sources: Vec<HealthLogSource>,
) -> HealthLogSnapshotResult {
    let source_keys = sources
        .iter()
        .map(HealthLogSource::key)
        .collect::<Vec<_>>();
    let results = join_all(
        sources
            .into_iter()
            .map(|source| async move {
                let result =
                    inspect_health_log_entries(context, &source.container_id)
                        .await;
                (source, result)
            }),
    )
    .await;

    let mut result = HealthLogSnapshotResult {
        current_sources: source_keys,
        snapshots: Vec::new(),
        failures: Vec::new(),
    };

    for (source, inspect_result) in results {
        match inspect_result {
            Ok((health_status, entries)) => {
                result
                    .snapshots
                    .push(HealthLogSnapshot {
                        source,
                        status: health_status,
                        entries,
                    });
            }
            Err(error) => {
                result.failures.push(format!(
                    "{}.{}: {error}",
                    source.project, source.service
                ));
            }
        }
    }

    result
}

fn sources_from_status(
    context: &NirionContext,
    target: &TargetSelector,
    project: &str,
    status: &ProjectStatus,
) -> Vec<HealthLogSource> {
    let Some(project_config) = context.projects.get(project) else {
        return Vec::new();
    };

    status
        .services
        .iter()
        .filter(|(service, _)| service_selected(target, project, service))
        .filter(|(service, _)| {
            project_config
                .services
                .get(*service)
                .map(|service| service.healthcheck)
                .unwrap_or(false)
        })
        .map(|(service_name, service)| {
            HealthLogSource::new(
                project,
                service_name.clone(),
                service.id.clone(),
                service.container_name.clone(),
            )
        })
        .collect()
}

async fn inspect_health_log_entries(
    context: &NirionContext,
    container_id: &str,
) -> anyhow::Result<(Option<String>, Vec<HealthLogEntry>)> {
    if let Some(health) = inspect_health(context, container_id).await? {
        let entries = health
            .log
            .into_iter()
            .map(parse_health_log_entry)
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok((health.status, entries))
    } else {
        Ok((None, Vec::new()))
    }
}

fn service_selected(
    target: &TargetSelector,
    project: &str,
    service: &str,
) -> bool {
    match target {
        TargetSelector::All => true,
        TargetSelector::Project(sel) => sel.name == project,
        TargetSelector::Service(sel) => {
            sel.project == project && sel.service == service
        }
    }
}

fn parse_health_log_entry(
    entry: DockerHealthLogEntry
) -> anyhow::Result<HealthLogEntry> {
    Ok(HealthLogEntry {
        start: parse_docker_timestamp(&entry.start).with_context(|| {
            format!("failed to parse health log start time {}", entry.start)
        })?,
        end: parse_docker_timestamp(&entry.end).with_context(|| {
            format!("failed to parse health log end time {}", entry.end)
        })?,
        exit_code: entry.exit_code,
        output: entry.output,
    })
}

fn parse_docker_timestamp(timestamp: &str) -> anyhow::Result<SystemTime> {
    Ok(DateTime::parse_from_rfc3339(timestamp)?.into())
}

async fn inspect_health(
    context: &NirionContext,
    container_id: &str,
) -> anyhow::Result<Option<DockerHealth>> {
    let output = context
        .docker_command
        .command()
        .arg("inspect")
        .arg("--format")
        .arg("{{json .State.Health}}")
        .arg(container_id)
        .output()
        .await
        .context("failed to execute docker inspect")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "docker inspect failed with status {}{}{}",
            output.status,
            if stderr.trim().is_empty() { "" } else { ": " },
            stderr.trim()
        );
    }

    let stdout = String::from_utf8(output.stdout)?;
    let stdout = stdout.trim();
    if stdout.is_empty() || stdout == "null" {
        return Ok(None);
    }

    serde_json::from_str(stdout).context("failed to parse docker health JSON")
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        docker::DockerCommand,
        lock::LockedImages,
        projects::{Projects, ServiceSelector, TargetSelector},
    };
    use futures::StreamExt;
    use nirion_oci_lib::client::NirionOciClient;
    use std::{
        fs,
        io::Write,
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
        sync::Arc,
        time::Duration,
    };

    fn projects() -> Projects {
        serde_json::from_value(serde_json::json!({
            "myapp": {
                "name": "myapp",
                "dockerCompose": "compose.yml",
                "services": {
                    "web": {
                        "image": "nginx:latest",
                        "healthcheck": true,
                        "restart": null
                    }
                }
            }
        }))
        .unwrap()
    }

    fn context(docker_command: DockerCommand) -> NirionContext {
        context_with_projects(projects(), docker_command)
    }

    fn context_with_projects(
        projects: Projects,
        docker_command: DockerCommand,
    ) -> NirionContext {
        NirionContext {
            projects,
            locked_images: LockedImages::default(),
            lock_file: PathBuf::from("lock.json"),
            oci_client: Arc::new(NirionOciClient::builder().build()),
            docker_command,
        }
    }

    fn write_fake_docker(
        dir: &Path,
        args_file: &Path,
        health_json: &str,
    ) -> String {
        let docker = dir.join("docker-health");
        let tmp = dir.join("docker-health.tmp");
        let compose_json = serde_json::json!([{
            "ID": "abc",
            "Name": "myapp-web-1",
            "Service": "web",
            "Image": "nginx:latest",
            "State": "running",
            "Health": "healthy",
            "ExitCode": 0,
            "RunningFor": "1 minute",
            "Status": "Up 1 minute (healthy)",
            "Ports": "",
            "Networks": "default"
        }]);

        let mut file = fs::File::create(&tmp).unwrap();
        file.write_all(
            format!(
                r#"#!/bin/sh
printf '%s\n' "$@" >> '{}'
if [ "$1" = "compose" ]; then
  printf '%s\n' '{}'
  exit 0
fi
printf '%s\n' '{}'
"#,
                args_file.display(),
                compose_json,
                health_json,
            )
            .as_bytes(),
        )
        .unwrap();
        file.sync_all().unwrap();
        drop(file);

        let mut permissions = fs::metadata(&tmp)
            .unwrap()
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&tmp, permissions).unwrap();
        fs::rename(&tmp, &docker).unwrap();

        docker.to_string_lossy().to_string()
    }

    fn write_fake_partial_failure_docker(
        dir: &Path,
        args_file: &Path,
    ) -> String {
        let docker = dir.join("docker-health-partial-failure");
        let tmp = dir.join("docker-health-partial-failure.tmp");
        let compose_json = serde_json::json!([
            {
                "ID": "abc",
                "Name": "myapp-web-1",
                "Service": "web",
                "Image": "nginx:latest",
                "State": "running",
                "Health": "healthy",
                "ExitCode": 0,
                "RunningFor": "1 minute",
                "Status": "Up 1 minute (healthy)",
                "Ports": "",
                "Networks": "default"
            },
            {
                "ID": "def",
                "Name": "myapp-db-1",
                "Service": "db",
                "Image": "postgres:16",
                "State": "running",
                "Health": "unhealthy",
                "ExitCode": 0,
                "RunningFor": "1 minute",
                "Status": "Up 1 minute (unhealthy)",
                "Ports": "",
                "Networks": "default"
            }
        ]);
        let health_json = r#"{"Status":"healthy","Log":[{"Start":"2026-07-19T10:00:00Z","End":"2026-07-19T10:00:01Z","ExitCode":0,"Output":"OK\n"}]}"#;

        let mut file = fs::File::create(&tmp).unwrap();
        file.write_all(
            format!(
                r#"#!/bin/sh
printf '%s\n' "$@" >> '{}'
if [ "$1" = "compose" ]; then
  printf '%s\n' '{}'
  exit 0
fi
if [ "$4" = "def" ]; then
  printf '%s\n' 'inspect failed' >&2
  exit 1
fi
printf '%s\n' '{}'
"#,
                args_file.display(),
                compose_json,
                health_json,
            )
            .as_bytes(),
        )
        .unwrap();
        file.sync_all().unwrap();
        drop(file);

        let mut permissions = fs::metadata(&tmp)
            .unwrap()
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&tmp, permissions).unwrap();
        fs::rename(&tmp, &docker).unwrap();

        docker.to_string_lossy().to_string()
    }

    fn write_fake_follow_docker(
        dir: &Path,
        args_file: &Path,
        count_file: &Path,
    ) -> String {
        let docker = dir.join("docker-health-follow");
        let tmp = dir.join("docker-health-follow.tmp");
        let compose_json = serde_json::json!([{
            "ID": "abc",
            "Name": "myapp-web-1",
            "Service": "web",
            "Image": "nginx:latest",
            "State": "running",
            "Health": "unhealthy",
            "ExitCode": 0,
            "RunningFor": "1 minute",
            "Status": "Up 1 minute (unhealthy)",
            "Ports": "",
            "Networks": "default"
        }]);
        let first_health = r#"{"Status":"unhealthy","Log":[{"Start":"2026-07-19T10:00:00Z","End":"2026-07-19T10:00:01Z","ExitCode":1,"Output":"first\n"}]}"#;
        let second_health = r#"{"Status":"healthy","Log":[{"Start":"2026-07-19T10:00:00Z","End":"2026-07-19T10:00:01Z","ExitCode":1,"Output":"first\n"},{"Start":"2026-07-19T10:00:02Z","End":"2026-07-19T10:00:03Z","ExitCode":0,"Output":"second\n"}]}"#;

        let mut file = fs::File::create(&tmp).unwrap();
        file.write_all(
            format!(
                r#"#!/bin/sh
printf '%s\n' "$@" >> '{}'
if [ "$1" = "compose" ]; then
  printf '%s\n' '{}'
  exit 0
fi
count=0
if [ -f '{}' ]; then
  count=$(cat '{}')
fi
count=$((count + 1))
printf '%s\n' "$count" > '{}'
if [ "$count" = "1" ]; then
  printf '%s\n' '{}'
else
  printf '%s\n' '{}'
fi
"#,
                args_file.display(),
                compose_json,
                count_file.display(),
                count_file.display(),
                count_file.display(),
                first_health,
                second_health,
            )
            .as_bytes(),
        )
        .unwrap();
        file.sync_all().unwrap();
        drop(file);

        let mut permissions = fs::metadata(&tmp)
            .unwrap()
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&tmp, permissions).unwrap();
        fs::rename(&tmp, &docker).unwrap();

        docker.to_string_lossy().to_string()
    }

    async fn health_log_events(
        context: NirionContext,
        target: TargetSelector,
    ) -> Vec<HealthLogEvent> {
        health_logs_stream(
            context,
            target,
            HealthLogStreamOptions {
                follow: false,
                refresh_interval: Duration::from_millis(10),
            },
        )
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<anyhow::Result<Vec<_>>>()
        .unwrap()
    }

    fn health_entry(output: &str) -> HealthLogEntry {
        HealthLogEntry {
            start: parse_docker_timestamp("2026-07-19T10:00:00Z").unwrap(),
            end: parse_docker_timestamp("2026-07-19T10:00:01Z").unwrap(),
            exit_code: 0,
            output: output.to_string(),
        }
    }

    fn health_snapshot_result(
        snapshots: Vec<HealthLogSnapshot>,
        failures: Vec<String>,
    ) -> HealthLogSnapshotResult {
        HealthLogSnapshotResult {
            current_sources: snapshots
                .iter()
                .map(|snapshot| snapshot.source.key())
                .collect(),
            snapshots,
            failures,
        }
    }

    fn health_source(container_id: &str) -> HealthLogSource {
        HealthLogSource::new("myapp", "web", container_id, "myapp-web-1")
    }

    async fn assert_no_health_event(
        rx: &mut mpsc::UnboundedReceiver<anyhow::Result<HealthLogEvent>>
    ) {
        assert!(
            tokio::time::timeout(Duration::from_millis(50), rx.next())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn health_logs_reads_docker_health_log_for_service_target() {
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(
            dir.path(),
            &args_file,
            r#"{"Status":"healthy","Log":[{"Start":"2026-07-19T10:00:00Z","End":"2026-07-19T10:00:01Z","ExitCode":0,"Output":"OK\n"}]}"#,
        );

        let events = health_log_events(
            context(DockerCommand::with_args("/bin/sh", [docker])),
            TargetSelector::Service(ServiceSelector {
                project: "myapp".into(),
                service: "web".into(),
            }),
        )
        .await;

        assert_eq!(events.len(), 1);
        let HealthLogEvent::LogEntry(record) = &events[0] else {
            panic!("expected health log entry");
        };
        assert_eq!(record.status.as_deref(), Some("healthy"));
        assert_eq!(record.entry.exit_code, 0);
        assert_eq!(record.entry.output, "OK\n");
        assert!(
            fs::read_to_string(args_file)
                .unwrap()
                .contains("inspect\n--format\n{{json .State.Health}}\nabc\n")
        );
    }

    #[tokio::test]
    async fn health_logs_supports_all_target() {
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(
            dir.path(),
            &args_file,
            r#"{"Status":"starting","Log":[]}"#,
        );

        let events = health_log_events(
            context(DockerCommand::with_args("/bin/sh", [docker])),
            TargetSelector::All,
        )
        .await;

        assert_eq!(events.len(), 1);
        let HealthLogEvent::NoEntries(snapshot) = &events[0] else {
            panic!("expected empty health log snapshot");
        };
        assert_eq!(snapshot.status.as_deref(), Some("starting"));
    }

    #[tokio::test]
    async fn health_logs_skips_services_without_configured_healthchecks() {
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(
            dir.path(),
            &args_file,
            r#"{"Status":"healthy","Log":[]}"#,
        );
        let projects: Projects = serde_json::from_value(serde_json::json!({
            "myapp": {
                "name": "myapp",
                "dockerCompose": "compose.yml",
                "services": {
                    "web": {
                        "image": "nginx:latest",
                        "healthcheck": false,
                        "restart": null
                    }
                }
            }
        }))
        .unwrap();

        let events = health_log_events(
            context_with_projects(
                projects,
                DockerCommand::with_args("/bin/sh", [docker]),
            ),
            TargetSelector::All,
        )
        .await;

        assert!(events.is_empty());
        assert!(
            !fs::read_to_string(args_file)
                .unwrap()
                .contains("inspect\n--format\n{{json .State.Health}}\nabc\n")
        );
    }

    #[tokio::test]
    async fn health_logs_stream_follow_emits_only_new_entries() {
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let count_file = dir.path().join("count");
        let docker =
            write_fake_follow_docker(dir.path(), &args_file, &count_file);

        let mut stream = health_logs_stream(
            context(DockerCommand::with_args("/bin/sh", [docker])),
            TargetSelector::Service(ServiceSelector {
                project: "myapp".into(),
                service: "web".into(),
            }),
            HealthLogStreamOptions {
                follow: true,
                refresh_interval: Duration::from_millis(10),
            },
        );

        let first = tokio::time::timeout(Duration::from_secs(1), stream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let second =
            tokio::time::timeout(Duration::from_secs(1), stream.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();

        assert!(matches!(
            first,
            HealthLogEvent::LogEntry(HealthLogRecord { entry, .. })
                if entry.output == "first\n"
        ));
        assert!(matches!(
            second,
            HealthLogEvent::LogEntry(HealthLogRecord { entry, .. })
                if entry.output == "second\n"
        ));
    }

    #[tokio::test]
    async fn follow_coordinator_dedupes_failures_per_project() {
        let (tx, mut rx) = mpsc::unbounded();
        let mut coordinator = FollowHealthLogCoordinator::new(
            context(DockerCommand::new("docker")),
            TargetSelector::All,
            HealthLogStreamOptions {
                follow: true,
                refresh_interval: Duration::from_millis(10),
            },
            tx,
        );

        assert!(coordinator.emit_snapshot_result(
            "myapp",
            health_snapshot_result(
                Vec::new(),
                vec!["myapp.web: inspect failed".to_string()],
            ),
        ));
        let first = rx.next().await.unwrap().unwrap_err();
        assert!(first.to_string().contains("myapp.web"));

        assert!(coordinator.emit_snapshot_result(
            "other",
            health_snapshot_result(Vec::new(), Vec::new()),
        ));
        assert!(coordinator.emit_snapshot_result(
            "myapp",
            health_snapshot_result(
                Vec::new(),
                vec!["myapp.web: inspect failed".to_string()],
            ),
        ));

        assert_no_health_event(&mut rx).await;
    }

    #[tokio::test]
    async fn follow_coordinator_resets_progress_when_container_changes() {
        let (tx, mut rx) = mpsc::unbounded();
        let mut coordinator = FollowHealthLogCoordinator::new(
            context(DockerCommand::new("docker")),
            TargetSelector::All,
            HealthLogStreamOptions {
                follow: true,
                refresh_interval: Duration::from_millis(10),
            },
            tx,
        );
        let entry = health_entry("OK\n");

        assert!(coordinator.emit_snapshot_result(
            "myapp",
            health_snapshot_result(
                vec![HealthLogSnapshot {
                    source: health_source("abc"),
                    status: Some("healthy".to_string()),
                    entries: vec![entry.clone()],
                }],
                Vec::new(),
            ),
        ));
        let first = rx.next().await.unwrap().unwrap();
        assert!(matches!(
            first,
            HealthLogEvent::LogEntry(HealthLogRecord { entry, .. })
                if entry.output == "OK\n"
        ));

        assert!(coordinator.emit_snapshot_result(
            "myapp",
            health_snapshot_result(
                vec![HealthLogSnapshot {
                    source: health_source("abc"),
                    status: Some("healthy".to_string()),
                    entries: vec![entry.clone()],
                }],
                Vec::new(),
            ),
        ));
        assert_no_health_event(&mut rx).await;

        assert!(coordinator.emit_snapshot_result(
            "myapp",
            health_snapshot_result(
                vec![HealthLogSnapshot {
                    source: health_source("def"),
                    status: Some("healthy".to_string()),
                    entries: vec![entry],
                }],
                Vec::new(),
            ),
        ));
        let after_replacement = rx.next().await.unwrap().unwrap();
        assert!(matches!(
            after_replacement,
            HealthLogEvent::LogEntry(HealthLogRecord { entry, .. })
                if entry.output == "OK\n"
        ));
    }

    #[tokio::test]
    async fn health_logs_stream_emits_successful_entries_before_failures() {
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_partial_failure_docker(dir.path(), &args_file);
        let projects: Projects = serde_json::from_value(serde_json::json!({
            "myapp": {
                "name": "myapp",
                "dockerCompose": "compose.yml",
                "services": {
                    "web": {
                        "image": "nginx:latest",
                        "healthcheck": true,
                        "restart": null
                    },
                    "db": {
                        "image": "postgres:16",
                        "healthcheck": true,
                        "restart": null
                    }
                }
            }
        }))
        .unwrap();

        let stream = health_logs_stream(
            context_with_projects(
                projects,
                DockerCommand::with_args("/bin/sh", [docker]),
            ),
            TargetSelector::All,
            HealthLogStreamOptions {
                follow: false,
                refresh_interval: Duration::from_millis(10),
            },
        );
        let events = stream.collect::<Vec<_>>().await;

        assert!(events.iter().any(|event| matches!(
            event,
            Ok(HealthLogEvent::LogEntry(HealthLogRecord { entry, .. }))
                if entry.output == "OK\n"
        )));
        assert!(events.iter().any(|event| {
            event
                .as_ref()
                .err()
                .map(|error| error.to_string().contains("myapp.db"))
                .unwrap_or(false)
        }));
    }

    #[test]
    fn parse_docker_timestamp_accepts_offset_and_nanoseconds() {
        let parsed =
            parse_docker_timestamp("2026-07-19T13:32:15.494067631+02:00")
                .unwrap();
        let expected =
            DateTime::parse_from_rfc3339("2026-07-19T11:32:15.494067631Z")
                .unwrap()
                .into();

        assert_eq!(parsed, expected);
    }
}
