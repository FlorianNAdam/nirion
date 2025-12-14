use std::collections::BTreeMap;

use anyhow::Context;
use serde::Deserialize;
use tokio::process::Command;

pub async fn query_project_status(
    compose_file: &str,
    project_name: &str,
) -> anyhow::Result<ProjectStatus> {
    let output = Command::new("docker")
        .arg("compose")
        .arg("-f")
        .arg(compose_file)
        .arg("--project-name")
        .arg(project_name)
        .arg("ps")
        .arg("--format")
        .arg("json")
        .output()
        .await
        .context("failed to execute docker compose ps")?;

    let json = if output.status.success() {
        String::from_utf8_lossy(&output.stdout).to_string()
    } else {
        "[]".to_string()
    };

    ProjectStatus::from_json(project_name, &json)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceState {
    Healthy,
    Running,
    Starting,
    Exited,
    Unhealthy,
    Unknown,
}

#[allow(unused)]
#[derive(Debug, Clone)]
pub struct ServiceStatus {
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

#[derive(Debug, Clone)]
pub struct Port {
    pub external: Option<ExternalPort>,
    pub port: u16,
    pub proto: String,
}

#[allow(unused)]
#[derive(Debug, Clone)]
pub struct ExternalPort {
    pub ip: String,
    pub port: u16,
}

#[derive(Debug, Clone)]
pub struct ProjectStatus {
    pub name: String,
    pub services: BTreeMap<String, ServiceStatus>,
}

#[derive(Debug, Deserialize)]
struct ContainerInfo {
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
    pub fn from_json(project_name: &str, json: &str) -> anyhow::Result<Self> {
        let mut project = ProjectStatus {
            name: project_name.to_string(),
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
                .split(",")
                .filter(|s| !s.is_empty())
                .map(|port_str| {
                    let (port_str, proto) = port_str
                        .split_once("/")
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                "Failed to split port by '/': {port_str}"
                            )
                        })?;

                    if let Some((ip, port_str)) = port_str.rsplit_once(":") {
                        let (external, internal) = port_str
                            .split_once("->")
                            .ok_or_else(|| {
                                anyhow::anyhow!(
                                    "Failed to split port by ':': {port_str}"
                                )
                            })?;

                        Ok(Port {
                            external: Some(ExternalPort {
                                ip: ip.to_string(),
                                port: external.parse()?,
                            }),
                            port: internal.parse()?,
                            proto: proto.to_string(),
                        })
                    } else {
                        Ok(Port {
                            port: port_str.parse()?,
                            external: None,
                            proto: proto.to_string(),
                        })
                    }
                })
                .collect::<anyhow::Result<Vec<_>>>()
                .context(format!("Failed to parse ports: {:?}", ports_c))?;

            project.services.insert(
                c.service.clone(),
                ServiceStatus {
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

    pub fn total(&self) -> usize {
        self.services.len()
    }

    pub fn healthy(&self) -> usize {
        self.services
            .values()
            .filter(|s| s.state == ServiceState::Healthy)
            .count()
    }

    pub fn unhealthy(&self) -> usize {
        self.services
            .values()
            .filter(|s| s.state == ServiceState::Unhealthy)
            .count()
    }

    pub fn running(&self) -> usize {
        self.services
            .values()
            .filter(|s| {
                matches!(s.state, ServiceState::Running | ServiceState::Healthy)
            })
            .count()
    }

    pub fn exited(&self) -> usize {
        self.services
            .values()
            .filter(|s| s.state == ServiceState::Exited)
            .count()
    }

    pub fn starting(&self) -> usize {
        self.services
            .values()
            .filter(|s| s.state == ServiceState::Starting)
            .count()
    }
}

impl ServiceState {
    fn from_container(c: &ContainerInfo) -> Self {
        match (c.state.as_str(), c.health.as_deref(), c.exit_code) {
            ("running", Some("healthy"), _) => ServiceState::Healthy,
            ("running", Some("unhealthy"), _) => ServiceState::Unhealthy,
            ("running", _, _) => ServiceState::Running,
            ("created" | "restarting", _, _) => ServiceState::Starting,
            ("exited", _, Some(0)) => ServiceState::Exited,
            ("exited", _, _) => ServiceState::Exited,
            _ => ServiceState::Unknown,
        }
    }
}
