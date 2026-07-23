use std::{
    collections::BTreeMap, ffi::OsString, ops::Deref, path::PathBuf,
    time::Duration,
};

use anyhow::Context;
use futures::{
    StreamExt, channel::mpsc, stream::BoxStream, stream::select_all,
};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::context::NirionContext;
use crate::projects::{
    Project, ProjectName, Projects, TargetSelector, selected_project_names,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DockerCommand {
    pub program: PathBuf,
    pub args: Vec<OsString>,
}

impl DockerCommand {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
        }
    }

    pub fn with_args(
        program: impl Into<PathBuf>,
        args: impl IntoIterator<Item = impl Into<OsString>>,
    ) -> Self {
        Self {
            program: program.into(),
            args: args
                .into_iter()
                .map(Into::into)
                .collect(),
        }
    }

    pub fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command.args(&self.args);
        command
    }
}

impl Default for DockerCommand {
    fn default() -> Self {
        Self::new("docker")
    }
}

pub async fn query_project_status(
    context: &NirionContext,
    project_name: &str,
) -> anyhow::Result<ProjectStatus> {
    let project = &context.projects[project_name];
    query_project_status_for_command(
        &context.docker_command,
        &project.docker_compose,
        &project.name,
    )
    .await
}

async fn query_project_status_for_command(
    docker_command: &DockerCommand,
    compose_file: &str,
    project_name: &ProjectName,
) -> anyhow::Result<ProjectStatus> {
    let output = docker_command
        .command()
        .arg("compose")
        .arg("-f")
        .arg(compose_file)
        .arg("--project-name")
        .arg(project_name.deref())
        .arg("ps")
        .arg("-a")
        .arg("--format")
        .arg("json")
        .output()
        .await
        .context("failed to execute docker compose ps")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "docker compose ps failed with status {}{}{}",
            output.status,
            if stderr.trim().is_empty() { "" } else { ": " },
            stderr.trim()
        );
    }

    let json = String::from_utf8_lossy(&output.stdout).to_string();

    ProjectStatus::from_json(&json)
}

#[derive(Debug, Clone)]
pub struct ProjectStatusEvent {
    pub project: String,
    pub status: ProjectStatus,
}

pub fn status_stream(
    context: &NirionContext,
    target: TargetSelector,
    refresh_interval: Duration,
) -> BoxStream<'static, anyhow::Result<ProjectStatusEvent>> {
    status_stream_for_command(
        context.docker_command.clone(),
        target,
        context.projects.clone(),
        refresh_interval,
    )
}

fn status_stream_for_command(
    docker_command: DockerCommand,
    target: TargetSelector,
    projects: Projects,
    refresh_interval: Duration,
) -> BoxStream<'static, anyhow::Result<ProjectStatusEvent>> {
    let selected = selected_project_names(&target, &projects);
    let streams = selected
        .into_iter()
        .filter_map(|name| {
            let project = projects.get(&name)?.clone();
            Some(project_status_stream_for_command(
                docker_command.clone(),
                name,
                project,
                refresh_interval,
            ))
        })
        .collect::<Vec<_>>();

    select_all(streams).boxed()
}

fn project_status_stream_for_command(
    docker_command: DockerCommand,
    name: String,
    project: Project,
    refresh_interval: Duration,
) -> BoxStream<'static, anyhow::Result<ProjectStatusEvent>> {
    let (tx, rx) = mpsc::unbounded();

    tokio::spawn(async move {
        let mut first_poll = true;

        loop {
            match query_project_status_for_command(
                &docker_command,
                &project.docker_compose,
                &project.name,
            )
            .await
            {
                Ok(status) => {
                    if tx
                        .unbounded_send(Ok(ProjectStatusEvent {
                            project: name.clone(),
                            status,
                        }))
                        .is_err()
                    {
                        return;
                    }
                }
                Err(error) if first_poll => {
                    if tx.unbounded_send(Err(error)).is_err() {
                        return;
                    }
                }
                Err(_) => {
                    if tx.is_closed() {
                        return;
                    }
                }
            }

            first_poll = false;
            tokio::time::sleep(refresh_interval).await;
        }
    });

    rx.boxed()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServiceState {
    Created,
    Starting,
    Running,
    Paused,
    Restarting,
    Succeeded,
    Failed,
    Healthy,
    Unhealthy,
    Unknown,
}

#[allow(unused)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceStatus {
    pub id: String,
    pub service: String,
    pub container_name: String,
    pub image: String,
    pub state: ServiceState,
    pub health: Option<String>,
    pub exit_code: Option<i64>,
    pub running_for: Option<String>,
    pub status: Option<String>,
    pub ports: Vec<Port>,
    pub networks: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Port {
    pub external: Option<ExternalPort>,
    pub port: u16,
    pub proto: String,
}

#[allow(unused)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalPort {
    pub ip: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectStatus {
    pub services: BTreeMap<String, ServiceStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectState {
    Empty,
    Healthy,
    Running,
    Paused,
    Starting,
    Degraded,
    Unknown,
}

#[derive(Debug, Deserialize)]
struct ContainerInfo {
    #[serde(rename = "ID")]
    id: String,
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Service")]
    service: String,
    #[serde(rename = "Image")]
    image: String,
    #[serde(rename = "State")]
    state: String,
    #[serde(rename = "Health")]
    health: Option<String>,
    #[serde(rename = "ExitCode")]
    exit_code: Option<i64>,
    #[serde(rename = "RunningFor")]
    running_for: Option<String>,
    #[serde(rename = "Status")]
    status: Option<String>,
    #[serde(rename = "Ports")]
    ports: Option<String>,
    #[serde(rename = "Networks")]
    networks: Option<String>,
}

impl ProjectStatus {
    pub fn from_json(json: &str) -> anyhow::Result<Self> {
        let mut project = ProjectStatus {
            services: BTreeMap::new(),
        };

        let json = json.trim();
        if json.is_empty() || json == "[]" {
            return Ok(project);
        }

        let containers: Vec<ContainerInfo> = if json.starts_with('[') {
            serde_json::from_str(json)
                .context("failed to parse docker compose JSON output")?
        } else {
            json.lines()
                .filter_map(|line| serde_json::from_str(line).ok())
                .collect()
        };

        for c in containers {
            let ports_c = c.ports.clone();
            let state = ServiceState::from_container(&c);
            let networks = c
                .networks
                .unwrap_or_default()
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>();

            let ports = c
                .ports
                .unwrap_or_default()
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(parse_port_mapping)
                .collect::<anyhow::Result<Vec<_>>>()
                .map(|ports| ports.into_iter().flatten().collect())
                .context(format!("Failed to parse ports: {:?}", ports_c))?;

            project.services.insert(
                c.service.clone(),
                ServiceStatus {
                    id: c.id,
                    service: c.service,
                    container_name: c.name,
                    image: c.image,
                    state,
                    health: c.health,
                    exit_code: c.exit_code,
                    running_for: c.running_for,
                    status: c.status,
                    ports,
                    networks,
                },
            );
        }

        Ok(project)
    }

    pub fn progressing(&self) -> usize {
        self.services
            .values()
            .filter(|s| {
                use ServiceState::*;
                matches!(
                    s.state,
                    Healthy
                        | Succeeded
                        | Running
                        | Paused
                        | Starting
                        | Restarting
                )
            })
            .count()
    }

    pub fn project_state(&self) -> ProjectState {
        if self.services.is_empty() {
            return ProjectState::Empty;
        }

        use ServiceState::*;

        let states: Vec<&ServiceState> = self
            .services
            .values()
            .map(|s| &s.state)
            .collect();

        macro_rules! project_states {
            ($states:expr, { $($predicate:ident [$pat:pat] => $result:expr),* $(,)? }) => {(|| {
                $({project_states!(@inner $predicate, $states, $pat, $result)})*
                ProjectState::Unknown
            })()};
            (@inner $predicate:ident, $states:expr, $pat:pat,$result:expr) => {
                if $states.iter().$predicate(|s| matches!(s, $pat)) {
                    return $result;
                }
            };
        }

        project_states!(states, {
            all [Healthy | Succeeded] => ProjectState::Healthy,
            any [Failed | Unhealthy] => ProjectState::Degraded,
            any [Starting | Restarting] => ProjectState::Starting,
            all [Healthy | Succeeded | Running] => ProjectState::Running,
            all [Healthy | Succeeded | Running | Paused] => ProjectState::Paused,
        })
    }
}

fn parse_port_mapping(port_str: &str) -> anyhow::Result<Vec<Port>> {
    let (port_str, proto) = port_str
        .split_once('/')
        .ok_or_else(|| {
            anyhow::anyhow!("Failed to split port by '/': {port_str}")
        })?;

    if let Some((ip, port_str)) = port_str.rsplit_once(':') {
        let (external, internal) =
            port_str
                .split_once("->")
                .ok_or_else(|| {
                    anyhow::anyhow!("Failed to split port by ':': {port_str}")
                })?;

        let external = parse_port_range(external)?;
        let internal = parse_port_range(internal)?;

        if external.len() != internal.len() {
            anyhow::bail!(
                "Port ranges have different lengths: {external:?}->{internal:?}"
            );
        }

        Ok(external
            .into_iter()
            .zip(internal)
            .map(|(external, internal)| Port {
                external: Some(ExternalPort {
                    ip: ip.to_string(),
                    port: external,
                }),
                port: internal,
                proto: proto.to_string(),
            })
            .collect())
    } else {
        parse_port_range(port_str).map(|ports| {
            ports
                .into_iter()
                .map(|port| Port {
                    port,
                    external: None,
                    proto: proto.to_string(),
                })
                .collect()
        })
    }
}

fn parse_port_range(port_str: &str) -> anyhow::Result<Vec<u16>> {
    if let Some((start, end)) = port_str.split_once('-') {
        let start = start.parse::<u16>()?;
        let end = end.parse::<u16>()?;

        if start > end {
            anyhow::bail!("Invalid descending port range: {port_str}");
        }

        Ok((start..=end).collect())
    } else {
        Ok(vec![port_str.parse()?])
    }
}

impl ServiceState {
    fn from_container(c: &ContainerInfo) -> Self {
        match c.state.as_str() {
            "created" => ServiceState::Created,
            "running" => match c.health.as_deref() {
                Some("healthy") => ServiceState::Healthy,
                Some("unhealthy") => ServiceState::Unhealthy,
                _ => ServiceState::Running,
            },
            "paused" => ServiceState::Paused,
            "restarting" => ServiceState::Restarting,
            "exited" => match c.exit_code {
                Some(0) => ServiceState::Succeeded,
                Some(_) => ServiceState::Failed,
                None => ServiceState::Failed,
            },
            _ => ServiceState::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        context::NirionContext, lock::LockedImages, projects::ProjectSelector,
    };
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
                        "resolvedImage": "nginx:latest@sha256:abc",
                        "healthcheck": false,
                        "restart": null
                    }
                }
            }
        }))
        .unwrap()
    }

    fn context(docker_command: DockerCommand) -> NirionContext {
        NirionContext {
            projects: projects(),
            locked_images: LockedImages::default(),
            lock_file: PathBuf::from("lock.json"),
            oci_client: Arc::new(NirionOciClient::builder().build()),
            docker_command,
        }
    }

    fn fake_docker_command(script: &str) -> DockerCommand {
        DockerCommand::with_args("/bin/sh", [script])
    }

    fn write_fake_docker(
        dir: &Path,
        args_file: &Path,
        stdout: &str,
        stderr: &str,
        exit_code: i32,
    ) -> String {
        let docker = dir.join("docker");
        let tmp = dir.join("docker.tmp");
        let mut file = fs::File::create(&tmp).unwrap();
        file.write_all(
            format!(
                r#"#!/bin/sh
printf '%s\n' "$@" > '{}'
printf '%s\n' '{}'
printf '%s\n' '{}' >&2
exit {exit_code}
"#,
                args_file.display(),
                stdout,
                stderr,
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

    fn compose_ps_service(
        service: &str,
        id: &str,
    ) -> String {
        serde_json::json!({
            "ID": id,
            "Name": format!("myapp-{service}-1"),
            "Service": service,
            "Image": "nginx:latest",
            "State": "running",
            "Health": "healthy",
            "ExitCode": null,
            "RunningFor": "1 minute",
            "Status": "Up 1 minute",
            "Ports": "",
            "Networks": "default"
        })
        .to_string()
    }

    #[tokio::test]
    async fn query_project_status_runs_configured_docker_command() {
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(
            dir.path(),
            &args_file,
            &compose_ps_service("web", "container-123"),
            "",
            0,
        );
        let context = context(fake_docker_command(&docker));

        let status = query_project_status(&context, "myapp")
            .await
            .unwrap();

        assert_eq!(status.services["web"].id, "container-123");
        assert_eq!(status.services["web"].state, ServiceState::Healthy);
        assert_eq!(
            fs::read_to_string(args_file).unwrap(),
            "compose\n-f\ncompose.yml\n--project-name\nmyapp\nps\n-a\n--format\njson\n"
        );
    }

    #[tokio::test]
    async fn status_stream_emits_initial_status() {
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(
            dir.path(),
            &args_file,
            &compose_ps_service("web", "container-abc"),
            "",
            0,
        );
        let context = context(fake_docker_command(&docker));
        let mut stream = status_stream(
            &context,
            TargetSelector::Project(ProjectSelector {
                name: "myapp".into(),
            }),
            Duration::from_secs(60),
        );

        let event = stream.next().await.unwrap().unwrap();

        assert_eq!(event.project, "myapp");
        assert_eq!(event.status.services["web"].id, "container-abc");
    }

    #[tokio::test]
    async fn status_stream_emits_first_poll_error() {
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(dir.path(), &args_file, "", "boom", 7);
        let context = context(fake_docker_command(&docker));
        let mut stream = status_stream(
            &context,
            TargetSelector::Project(ProjectSelector {
                name: "myapp".into(),
            }),
            Duration::from_secs(60),
        );

        let err = stream
            .next()
            .await
            .unwrap()
            .unwrap_err();

        assert!(
            err.to_string()
                .contains("docker compose ps failed with status")
        );
        assert!(err.to_string().contains("boom"));
    }

    fn service(state: ServiceState) -> ServiceStatus {
        ServiceStatus {
            id: "abc".into(),
            service: "web".into(),
            container_name: "web-1".into(),
            image: "nginx:latest".into(),
            state,
            health: None,
            exit_code: None,
            running_for: None,
            status: None,
            ports: vec![],
            networks: vec![],
        }
    }

    fn container_info(
        state: &str,
        health: Option<&str>,
        exit_code: Option<i64>,
    ) -> ContainerInfo {
        ContainerInfo {
            id: "1".into(),
            name: "a".into(),
            service: "s".into(),
            image: "img".into(),
            state: state.into(),
            health: health.map(str::to_string),
            exit_code,
            running_for: None,
            status: None,
            ports: None,
            networks: None,
        }
    }

    #[test]
    fn parse_port_mapping_parses_unmapped_port() {
        let ports = parse_port_mapping("80/tcp").unwrap();
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].port, 80);
        assert_eq!(ports[0].proto, "tcp");
        assert!(ports[0].external.is_none());
    }

    #[test]
    fn parse_port_mapping_parses_external_port() {
        let ports = parse_port_mapping("0.0.0.0:8080->80/tcp").unwrap();
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].port, 80);
        assert_eq!(ports[0].external.as_ref().unwrap().port, 8080);
        assert_eq!(ports[0].external.as_ref().unwrap().ip, "0.0.0.0");
    }

    #[test]
    fn parse_port_mapping_expands_matching_port_ranges() {
        let ports = parse_port_mapping("0.0.0.0:8080-8082->80-82/tcp").unwrap();
        assert_eq!(ports.len(), 3);
        assert_eq!(ports[0].port, 80);
        assert_eq!(ports[0].external.as_ref().unwrap().port, 8080);
        assert_eq!(ports[1].port, 81);
        assert_eq!(ports[1].external.as_ref().unwrap().port, 8081);
        assert_eq!(ports[2].port, 82);
        assert_eq!(ports[2].external.as_ref().unwrap().port, 8082);
    }

    #[test]
    fn parse_port_mapping_rejects_missing_protocol() {
        assert!(parse_port_mapping("80").is_err());
    }

    #[test]
    fn parse_port_mapping_rejects_descending_range() {
        assert!(parse_port_mapping("82-80->82-80/tcp").is_err());
    }

    #[test]
    fn parse_port_mapping_rejects_external_port_without_arrow() {
        let err = parse_port_mapping("0.0.0.0:8080/tcp").unwrap_err();
        assert!(
            err.to_string()
                .contains("Failed to split port by ':'")
        );
    }

    #[test]
    fn parse_port_mapping_rejects_mismatched_ranges() {
        let err = parse_port_mapping("0.0.0.0:8080-8081->80/tcp").unwrap_err();
        assert!(
            err.to_string()
                .contains("Port ranges have different lengths")
        );
    }

    #[test]
    fn parse_port_range_parses_single_port() {
        let ports = super::parse_port_range("80").unwrap();
        assert_eq!(ports, vec![80]);
    }

    #[test]
    fn parse_port_range_expands_range() {
        let ports = super::parse_port_range("3-5").unwrap();
        assert_eq!(ports, vec![3, 4, 5]);
    }

    #[test]
    fn parse_port_range_rejects_invalid_port() {
        assert!(super::parse_port_range("abc").is_err());
    }

    #[test]
    fn project_status_from_json_treats_empty_output_as_empty_status() {
        let status = ProjectStatus::from_json("").unwrap();
        assert!(status.services.is_empty());
    }

    #[test]
    fn project_status_from_json_treats_empty_array_as_empty_status() {
        let status = ProjectStatus::from_json("[]").unwrap();
        assert!(status.services.is_empty());
    }

    #[test]
    fn project_status_from_json_parses_ndjson_output() {
        let json = r#"{"ID":"abc","Name":"web-1","Service":"web","Image":"nginx","State":"running","Health":"healthy","ExitCode":null,"RunningFor":"2 minutes","Status":null,"Ports":"0.0.0.0:8080->80/tcp","Networks":"bridge"}"#;
        let status = ProjectStatus::from_json(json).unwrap();
        assert_eq!(status.services.len(), 1);
        let svc = &status.services["web"];
        assert_eq!(svc.state, ServiceState::Healthy);
        assert_eq!(svc.ports.len(), 1);
        assert_eq!(svc.networks, vec!["bridge"]);
    }

    #[test]
    fn project_status_from_json_parses_json_array_output() {
        let json = r#"[{"ID":"abc","Name":"web-1","Service":"web","Image":"nginx","State":"running","Health":null,"ExitCode":null,"RunningFor":"2 minutes","Status":null,"Ports":"","Networks":""}]"#;
        let status = ProjectStatus::from_json(json).unwrap();
        assert_eq!(status.services.len(), 1);
        assert_eq!(status.services["web"].state, ServiceState::Running);
    }

    #[test]
    fn project_status_from_json_reports_json_array_parse_errors() {
        let err = ProjectStatus::from_json("[").unwrap_err();
        assert!(
            err.to_string()
                .contains("failed to parse docker compose JSON output")
        );
    }

    #[test]
    fn project_status_from_json_reports_port_parse_errors() {
        let json = r#"{"ID":"abc","Name":"web-1","Service":"web","Image":"nginx","State":"running","Health":null,"ExitCode":null,"RunningFor":null,"Status":null,"Ports":"bad-port","Networks":""}"#;
        let err = ProjectStatus::from_json(json).unwrap_err();
        assert!(
            err.to_string()
                .contains("Failed to parse ports")
        );
    }

    #[test]
    fn project_status_from_json_parses_multiple_ndjson_lines() {
        let json = "{\"ID\":\"1\",\"Name\":\"a\",\"Service\":\"web\",\"Image\":\"nginx\",\"State\":\"running\",\"Health\":\"healthy\",\"ExitCode\":null,\"RunningFor\":null,\"Status\":null,\"Ports\":\"\",\"Networks\":\"\"}\n{\"ID\":\"2\",\"Name\":\"b\",\"Service\":\"db\",\"Image\":\"postgres\",\"State\":\"exited\",\"Health\":null,\"ExitCode\":0,\"RunningFor\":null,\"Status\":null,\"Ports\":\"\",\"Networks\":\"\"}";
        let status = ProjectStatus::from_json(json).unwrap();
        assert_eq!(status.services.len(), 2);
        assert_eq!(status.services["web"].state, ServiceState::Healthy);
        assert_eq!(status.services["db"].state, ServiceState::Succeeded);
    }

    #[test]
    fn service_state_from_container_treats_running_healthy_as_healthy() {
        let container = container_info("running", Some("healthy"), None);
        assert_eq!(
            ServiceState::from_container(&container),
            ServiceState::Healthy
        );
    }

    #[test]
    fn service_state_from_container_treats_running_unhealthy_as_unhealthy() {
        let container = container_info("running", Some("unhealthy"), None);
        assert_eq!(
            ServiceState::from_container(&container),
            ServiceState::Unhealthy
        );
    }

    #[test]
    fn service_state_from_container_treats_running_without_health_as_running() {
        let container = container_info("running", None, None);
        assert_eq!(
            ServiceState::from_container(&container),
            ServiceState::Running
        );
    }

    #[test]
    fn service_state_from_container_treats_zero_exit_as_succeeded() {
        let container = container_info("exited", None, Some(0));
        assert_eq!(
            ServiceState::from_container(&container),
            ServiceState::Succeeded
        );
    }

    #[test]
    fn service_state_from_container_treats_nonzero_exit_as_failed() {
        let container = container_info("exited", None, Some(1));
        assert_eq!(
            ServiceState::from_container(&container),
            ServiceState::Failed
        );
    }

    #[test]
    fn service_state_from_container_treats_missing_exit_code_as_failed() {
        let container = container_info("exited", None, None);
        assert_eq!(
            ServiceState::from_container(&container),
            ServiceState::Failed
        );
    }

    #[test]
    fn service_state_from_container_preserves_paused_and_restarting_states() {
        let mut container = container_info("paused", None, None);
        assert_eq!(
            ServiceState::from_container(&container),
            ServiceState::Paused
        );

        container.state = "restarting".into();
        assert_eq!(
            ServiceState::from_container(&container),
            ServiceState::Restarting
        );
    }

    #[test]
    fn service_state_from_container_preserves_created_state() {
        let container = container_info("created", None, None);
        assert_eq!(
            ServiceState::from_container(&container),
            ServiceState::Created
        );
    }

    #[test]
    fn service_state_from_container_maps_unknown_state_to_unknown() {
        let container = container_info("garbage", None, None);
        assert_eq!(
            ServiceState::from_container(&container),
            ServiceState::Unknown
        );
    }

    #[test]
    fn project_state_empty() {
        let status = ProjectStatus {
            services: BTreeMap::new(),
        };
        assert_eq!(status.project_state(), ProjectState::Empty);
    }

    #[test]
    fn project_state_healthy() {
        let mut services = BTreeMap::new();
        services.insert(
            "a".into(),
            ServiceStatus {
                state: ServiceState::Healthy,
                ..service(ServiceState::Healthy)
            },
        );
        services.insert(
            "b".into(),
            ServiceStatus {
                state: ServiceState::Succeeded,
                ..service(ServiceState::Succeeded)
            },
        );
        let status = ProjectStatus { services };
        assert_eq!(status.project_state(), ProjectState::Healthy);
    }

    #[test]
    fn project_state_degraded() {
        let mut services = BTreeMap::new();
        services.insert(
            "a".into(),
            ServiceStatus {
                state: ServiceState::Healthy,
                ..service(ServiceState::Healthy)
            },
        );
        services.insert(
            "b".into(),
            ServiceStatus {
                state: ServiceState::Failed,
                ..service(ServiceState::Failed)
            },
        );
        let status = ProjectStatus { services };
        assert_eq!(status.project_state(), ProjectState::Degraded);
    }

    #[test]
    fn project_state_starting() {
        let mut services = BTreeMap::new();
        services.insert(
            "a".into(),
            ServiceStatus {
                state: ServiceState::Starting,
                ..service(ServiceState::Starting)
            },
        );
        let status = ProjectStatus { services };
        assert_eq!(status.project_state(), ProjectState::Starting);
    }

    #[test]
    fn project_state_running() {
        let mut services = BTreeMap::new();
        services.insert(
            "a".into(),
            ServiceStatus {
                state: ServiceState::Running,
                ..service(ServiceState::Running)
            },
        );
        let status = ProjectStatus { services };
        assert_eq!(status.project_state(), ProjectState::Running);
    }

    #[test]
    fn project_state_paused() {
        let mut services = BTreeMap::new();
        services.insert(
            "a".into(),
            ServiceStatus {
                state: ServiceState::Paused,
                ..service(ServiceState::Paused)
            },
        );
        let status = ProjectStatus { services };
        assert_eq!(status.project_state(), ProjectState::Paused);
    }

    #[test]
    fn project_state_unknown_when_no_rule_matches() {
        let mut services = BTreeMap::new();
        services.insert(
            "a".into(),
            ServiceStatus {
                state: ServiceState::Created,
                ..service(ServiceState::Created)
            },
        );
        let status = ProjectStatus { services };
        assert_eq!(status.project_state(), ProjectState::Unknown);
    }

    #[test]
    fn progressing_counts_active() {
        let mut services = BTreeMap::new();
        services.insert(
            "a".into(),
            ServiceStatus {
                state: ServiceState::Running,
                ..service(ServiceState::Running)
            },
        );
        services.insert(
            "b".into(),
            ServiceStatus {
                state: ServiceState::Healthy,
                ..service(ServiceState::Healthy)
            },
        );
        services.insert(
            "c".into(),
            ServiceStatus {
                state: ServiceState::Failed,
                ..service(ServiceState::Failed)
            },
        );
        let status = ProjectStatus { services };
        assert_eq!(status.progressing(), 2);
    }

    #[test]
    fn progressing_zero_when_empty() {
        let status = ProjectStatus {
            services: BTreeMap::new(),
        };
        assert_eq!(status.progressing(), 0);
    }
}
