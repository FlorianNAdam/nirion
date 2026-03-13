use std::{collections::BTreeMap, ops::Deref};

use anyhow::Context;
use crossterm::style::{Color, Stylize};
use nirion_lib::projects::ProjectName;
use serde::Deserialize;
use tokio::process::Command;

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

    let json = if output.status.success() {
        String::from_utf8_lossy(&output.stdout).to_string()
    } else {
        "[]".to_string()
    };

    ProjectStatus::from_json(&json)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceState {
    // Life Cycle
    Created,    // Container exists but not started
    Starting,   // In the process of starting up
    Running,    // Actively running
    Paused,     // Temporarily suspended
    Restarting, // Automatically restarting
    // Exited
    Succeeded, // exited with code 0
    Failed,    // exited with non-zero code
    // Health checks
    Healthy,   // Passed health checks
    Unhealthy, // Failed health checks
    //
    Unknown, // Docker cannot determine state
}

#[allow(unused)]
#[derive(Debug, Clone)]
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
    pub services: BTreeMap<String, ServiceStatus>,
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
                .split(",")
                .map(|s| s.trim())
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
                use crate::docker::ServiceState::*;
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

    pub fn segments(&self) -> Vec<Color> {
        fn order(state: &ServiceState) -> usize {
            let order = [
                // Good states
                ServiceState::Healthy,
                ServiceState::Succeeded,
                // Neutral / transitional states
                ServiceState::Running,
                ServiceState::Paused,
                ServiceState::Starting,
                ServiceState::Restarting,
                // Bad states
                ServiceState::Failed,
                ServiceState::Unhealthy,
                // Remaining / unknown
                ServiceState::Created,
                ServiceState::Unknown,
            ];
            order
                .iter()
                .position(|s| s == state)
                .unwrap_or_default()
        }

        let mut services: Vec<&ServiceStatus> =
            self.services.values().collect();

        services.sort_by_key(|s| order(&s.state));

        services
            .into_iter()
            .map(|s| s.state.color())
            .collect()
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
                if $states.iter().all(|s| matches!(s, $pat)) {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectState {
    Empty,    // No services
    Healthy,  // All services are Healthy or Succeeded
    Running,  // All services are Healthy/Succeeded/Running
    Paused,   // All services are Healthy/Succeeded/Running/Paused
    Starting, // At least one service Starting/Restarting, none failed/unhealthy
    Degraded, // At least one service Failed or Unhealthy
    Unknown,  // Cannot determine / Docker Unknown
}

impl ProjectState {
    pub fn icon(&self) -> String {
        use ProjectState::*;

        match self {
            Empty => "-".grey().to_string(),
            Healthy => "✓".green().to_string(),
            Running => "✓".yellow().to_string(),
            Paused => "=".blue().to_string(),
            Starting => "↗".cyan().to_string(),
            Degraded => "✗".red().to_string(),
            Unknown => "?".grey().to_string(),
        }
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

    pub fn color(&self) -> Color {
        match self {
            // Lifecycle
            ServiceState::Created => Color::Grey,
            ServiceState::Starting => Color::DarkGrey,
            ServiceState::Running => Color::Yellow,
            ServiceState::Paused => Color::Blue,
            ServiceState::Restarting => Color::DarkGrey,
            // Exited
            ServiceState::Succeeded => Color::Cyan,
            ServiceState::Failed => Color::Magenta,
            // Health
            ServiceState::Healthy => Color::Green,
            ServiceState::Unhealthy => Color::Red,
            // Fallback
            ServiceState::Unknown => Color::Grey,
        }
    }
}
