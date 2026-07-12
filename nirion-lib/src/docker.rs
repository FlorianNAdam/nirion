use std::{
    collections::BTreeMap, ffi::OsStr, ops::Deref, sync::Arc, time::Duration,
};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use tokio::{process::Command, sync::RwLock, task::JoinHandle};

use crate::projects::{Project, ProjectName};

pub async fn query_project_status(
    compose_file: &str,
    project_name: &ProjectName,
) -> anyhow::Result<ProjectStatus> {
    let output = Command::new("docker")
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

pub struct DockerProjectMonitor {
    project_name: ProjectName,
    compose_file: String,
    project_status: Arc<RwLock<ProjectStatus>>,
    refresh_handle: Option<JoinHandle<()>>,
}

impl DockerProjectMonitor {
    pub fn new(project: &Project, refresh_interval: Duration) -> Self {
        let mut monitor = Self {
            project_name: project.name.clone(),
            compose_file: project.docker_compose.clone(),
            project_status: Arc::new(RwLock::new(ProjectStatus {
                services: BTreeMap::new(),
            })),
            refresh_handle: None,
        };

        let handle = {
            let project_status = monitor.project_status.clone();
            let project_name = project.name.clone();
            let compose_file = monitor.compose_file.clone();
            tokio::spawn(project_refresh_thread(
                project_status,
                compose_file,
                project_name,
                refresh_interval,
            ))
        };

        monitor.refresh_handle = Some(handle);
        monitor
    }

    pub async fn refresh_status(&self) -> anyhow::Result<()> {
        let new_status =
            query_project_status(&self.compose_file, &self.project_name)
                .await?;

        let mut status = self.project_status.write().await;
        *status = new_status;
        Ok(())
    }

    pub async fn project_status(&self) -> ProjectStatus {
        self.project_status.read().await.clone()
    }
}

impl Drop for DockerProjectMonitor {
    fn drop(&mut self) {
        if let Some(handle) = &self.refresh_handle {
            handle.abort();
        }
    }
}

async fn project_refresh_thread(
    project_status: Arc<RwLock<ProjectStatus>>,
    compose_file: String,
    project_name: ProjectName,
    interval: Duration,
) {
    loop {
        match query_project_status(&compose_file, &project_name).await {
            Ok(new_status) => {
                let mut status = project_status.write().await;
                *status = new_status;
            }
            Err(_) => {}
        }
        tokio::time::sleep(interval).await;
    }
}

pub struct DockerMonitoredProcess {
    pub monitor: DockerProjectMonitor,
    finished: Arc<RwLock<bool>>,
    error: Arc<RwLock<Option<String>>>,
}

impl DockerMonitoredProcess {
    pub async fn new(
        project: Project,
        refresh_interval: Duration,
        args: impl IntoIterator<Item = impl AsRef<OsStr>>,
    ) -> Self {
        let monitor = DockerProjectMonitor::new(&project, refresh_interval);

        let finished = Arc::new(RwLock::new(false));
        let error = Arc::new(RwLock::new(None));

        let _ = {
            let compose_file = project.docker_compose.clone();
            let args: Vec<_> = args
                .into_iter()
                .map(|a| a.as_ref().to_os_string())
                .collect();
            let finished = finished.clone();
            let error = error.clone();

            tokio::spawn(async move {
                let result = Command::new("docker")
                    .arg("compose")
                    .arg("-f")
                    .arg(compose_file)
                    .arg("--project-name")
                    .arg(project.name.deref())
                    .args(args)
                    .output()
                    .await
                    .context("failed to execute docker compose")
                    .and_then(|output| {
                        if output.status.success() {
                            Ok(())
                        } else {
                            let stderr =
                                String::from_utf8_lossy(&output.stderr);
                            anyhow::bail!(
                                "docker compose exited with status {}{}{}",
                                output.status,
                                if stderr.trim().is_empty() {
                                    ""
                                } else {
                                    ": "
                                },
                                stderr.trim()
                            )
                        }
                    });

                if let Err(e) = result {
                    *error.write().await = Some(e.to_string());
                }

                *finished.write().await = true;
            })
        };

        Self {
            monitor,
            finished,
            error,
        }
    }

    pub async fn refresh_status(&self) -> anyhow::Result<()> {
        self.monitor.refresh_status().await
    }

    pub async fn project_status(&self) -> ProjectStatus {
        self.monitor.project_status().await
    }

    pub async fn finished(&self) -> bool {
        *self.finished.read().await
    }

    pub async fn error(&self) -> Option<String> {
        self.error.read().await.clone()
    }
}
