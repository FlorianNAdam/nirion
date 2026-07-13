use std::{collections::BTreeMap, ops::Deref, time::Duration};

use anyhow::Context;
use futures::{
    StreamExt, channel::mpsc, stream::BoxStream, stream::select_all,
};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::projects::{
    Project, ProjectName, Projects, TargetSelector, selected_project_names,
};

#[cfg(test)]
pub(crate) static TEST_DOCKER_CMD: std::sync::Mutex<Option<Vec<String>>> =
    std::sync::Mutex::new(None);

pub async fn query_project_status(
    compose_file: &str,
    project_name: &ProjectName,
) -> anyhow::Result<ProjectStatus> {
    let output = docker_command()
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
    target: TargetSelector,
    projects: Projects,
    refresh_interval: Duration,
) -> BoxStream<'static, anyhow::Result<ProjectStatusEvent>> {
    let selected = selected_project_names(&target, &projects);
    let streams = selected
        .into_iter()
        .filter_map(|name| {
            let project = projects.get(&name)?.clone();
            Some(project_status_stream(name, project, refresh_interval))
        })
        .collect::<Vec<_>>();

    select_all(streams).boxed()
}

pub fn project_status_stream(
    name: String,
    project: Project,
    refresh_interval: Duration,
) -> BoxStream<'static, anyhow::Result<ProjectStatusEvent>> {
    let (tx, rx) = mpsc::unbounded();

    tokio::spawn(async move {
        let mut first_poll = true;

        loop {
            match query_project_status(&project.docker_compose, &project.name)
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

fn docker_command() -> Command {
    #[cfg(test)]
    if let Some(cmd) = TEST_DOCKER_CMD.lock().unwrap().clone() {
        let mut command = Command::new(&cmd[0]);
        command.args(&cmd[1..]);
        return command;
    }

    Command::new("docker")
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

    #[test]
    fn parse_port_simple() {
        let ports = parse_port_mapping("80/tcp").unwrap();
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].port, 80);
        assert_eq!(ports[0].proto, "tcp");
        assert!(ports[0].external.is_none());
    }

    #[test]
    fn parse_port_with_external() {
        let ports = parse_port_mapping("0.0.0.0:8080->80/tcp").unwrap();
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].port, 80);
        assert_eq!(ports[0].external.as_ref().unwrap().port, 8080);
        assert_eq!(ports[0].external.as_ref().unwrap().ip, "0.0.0.0");
    }

    #[test]
    fn parse_port_range() {
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
    fn parse_port_no_proto_fails() {
        assert!(parse_port_mapping("80").is_err());
    }

    #[test]
    fn parse_port_descending_range_fails() {
        assert!(parse_port_mapping("82-80->82-80/tcp").is_err());
    }

    #[test]
    fn parse_port_external_missing_arrow_fails() {
        let err = parse_port_mapping("0.0.0.0:8080/tcp").unwrap_err();
        assert!(
            err.to_string()
                .contains("Failed to split port by ':'")
        );
    }

    #[test]
    fn parse_port_mismatched_ranges_fail() {
        let err = parse_port_mapping("0.0.0.0:8080-8081->80/tcp").unwrap_err();
        assert!(
            err.to_string()
                .contains("Port ranges have different lengths")
        );
    }

    #[test]
    fn parse_port_range_single() {
        let r = super::parse_port_range("80").unwrap();
        assert_eq!(r, vec![80]);
    }

    #[test]
    fn parse_port_range_expanded() {
        let r = super::parse_port_range("3-5").unwrap();
        assert_eq!(r, vec![3, 4, 5]);
    }

    #[test]
    fn parse_port_range_invalid() {
        assert!(super::parse_port_range("abc").is_err());
    }

    #[test]
    fn from_json_empty() {
        let status = ProjectStatus::from_json("").unwrap();
        assert!(status.services.is_empty());
    }

    #[test]
    fn from_json_empty_array() {
        let status = ProjectStatus::from_json("[]").unwrap();
        assert!(status.services.is_empty());
    }

    #[test]
    fn from_json_ndjson() {
        let json = r#"{"ID":"abc","Name":"web-1","Service":"web","Image":"nginx","State":"running","Health":"healthy","ExitCode":null,"RunningFor":"2 minutes","Status":null,"Ports":"0.0.0.0:8080->80/tcp","Networks":"bridge"}"#;
        let status = ProjectStatus::from_json(json).unwrap();
        assert_eq!(status.services.len(), 1);
        let svc = &status.services["web"];
        assert_eq!(svc.state, ServiceState::Healthy);
        assert_eq!(svc.ports.len(), 1);
        assert_eq!(svc.networks, vec!["bridge"]);
    }

    #[test]
    fn from_json_array() {
        let json = r#"[{"ID":"abc","Name":"web-1","Service":"web","Image":"nginx","State":"running","Health":null,"ExitCode":null,"RunningFor":"2 minutes","Status":null,"Ports":"","Networks":""}]"#;
        let status = ProjectStatus::from_json(json).unwrap();
        assert_eq!(status.services.len(), 1);
        assert_eq!(status.services["web"].state, ServiceState::Running);
    }

    #[test]
    fn from_json_array_reports_parse_errors() {
        let err = ProjectStatus::from_json("[").unwrap_err();
        assert!(
            err.to_string()
                .contains("failed to parse docker compose JSON output")
        );
    }

    #[test]
    fn from_json_reports_port_parse_errors() {
        let json = r#"{"ID":"abc","Name":"web-1","Service":"web","Image":"nginx","State":"running","Health":null,"ExitCode":null,"RunningFor":null,"Status":null,"Ports":"bad-port","Networks":""}"#;
        let err = ProjectStatus::from_json(json).unwrap_err();
        assert!(
            err.to_string()
                .contains("Failed to parse ports")
        );
    }

    #[test]
    fn from_json_multiple_ndjson() {
        let json = "{\"ID\":\"1\",\"Name\":\"a\",\"Service\":\"web\",\"Image\":\"nginx\",\"State\":\"running\",\"Health\":\"healthy\",\"ExitCode\":null,\"RunningFor\":null,\"Status\":null,\"Ports\":\"\",\"Networks\":\"\"}\n{\"ID\":\"2\",\"Name\":\"b\",\"Service\":\"db\",\"Image\":\"postgres\",\"State\":\"exited\",\"Health\":null,\"ExitCode\":0,\"RunningFor\":null,\"Status\":null,\"Ports\":\"\",\"Networks\":\"\"}";
        let status = ProjectStatus::from_json(json).unwrap();
        assert_eq!(status.services.len(), 2);
        assert_eq!(status.services["web"].state, ServiceState::Healthy);
        assert_eq!(status.services["db"].state, ServiceState::Succeeded);
    }

    #[test]
    fn from_state_running_healthy() {
        let c = ContainerInfo {
            id: "1".into(),
            name: "a".into(),
            service: "s".into(),
            image: "img".into(),
            state: "running".into(),
            health: Some("healthy".into()),
            exit_code: None,
            running_for: None,
            status: None,
            ports: None,
            networks: None,
        };
        assert_eq!(ServiceState::from_container(&c), ServiceState::Healthy);
    }

    #[test]
    fn from_state_running_unhealthy() {
        let c = ContainerInfo {
            id: "1".into(),
            name: "a".into(),
            service: "s".into(),
            image: "img".into(),
            state: "running".into(),
            health: Some("unhealthy".into()),
            exit_code: None,
            running_for: None,
            status: None,
            ports: None,
            networks: None,
        };
        assert_eq!(ServiceState::from_container(&c), ServiceState::Unhealthy);
    }

    #[test]
    fn from_state_running_no_health() {
        let c = ContainerInfo {
            id: "1".into(),
            name: "a".into(),
            service: "s".into(),
            image: "img".into(),
            state: "running".into(),
            health: None,
            exit_code: None,
            running_for: None,
            status: None,
            ports: None,
            networks: None,
        };
        assert_eq!(ServiceState::from_container(&c), ServiceState::Running);
    }

    #[test]
    fn from_state_exited_success() {
        let c = ContainerInfo {
            id: "1".into(),
            name: "a".into(),
            service: "s".into(),
            image: "img".into(),
            state: "exited".into(),
            health: None,
            exit_code: Some(0),
            running_for: None,
            status: None,
            ports: None,
            networks: None,
        };
        assert_eq!(ServiceState::from_container(&c), ServiceState::Succeeded);
    }

    #[test]
    fn from_state_exited_failure() {
        let c = ContainerInfo {
            id: "1".into(),
            name: "a".into(),
            service: "s".into(),
            image: "img".into(),
            state: "exited".into(),
            health: None,
            exit_code: Some(1),
            running_for: None,
            status: None,
            ports: None,
            networks: None,
        };
        assert_eq!(ServiceState::from_container(&c), ServiceState::Failed);
    }

    #[test]
    fn from_state_exited_without_code_is_failure() {
        let c = ContainerInfo {
            id: "1".into(),
            name: "a".into(),
            service: "s".into(),
            image: "img".into(),
            state: "exited".into(),
            health: None,
            exit_code: None,
            running_for: None,
            status: None,
            ports: None,
            networks: None,
        };
        assert_eq!(ServiceState::from_container(&c), ServiceState::Failed);
    }

    #[test]
    fn from_state_paused_and_restarting() {
        let mut c = ContainerInfo {
            id: "1".into(),
            name: "a".into(),
            service: "s".into(),
            image: "img".into(),
            state: "paused".into(),
            health: None,
            exit_code: None,
            running_for: None,
            status: None,
            ports: None,
            networks: None,
        };
        assert_eq!(ServiceState::from_container(&c), ServiceState::Paused);

        c.state = "restarting".into();
        assert_eq!(ServiceState::from_container(&c), ServiceState::Restarting);
    }

    #[test]
    fn from_state_created() {
        let c = ContainerInfo {
            id: "1".into(),
            name: "a".into(),
            service: "s".into(),
            image: "img".into(),
            state: "created".into(),
            health: None,
            exit_code: None,
            running_for: None,
            status: None,
            ports: None,
            networks: None,
        };
        assert_eq!(ServiceState::from_container(&c), ServiceState::Created);
    }

    #[test]
    fn from_state_unknown() {
        let c = ContainerInfo {
            id: "1".into(),
            name: "a".into(),
            service: "s".into(),
            image: "img".into(),
            state: "garbage".into(),
            health: None,
            exit_code: None,
            running_for: None,
            status: None,
            ports: None,
            networks: None,
        };
        assert_eq!(ServiceState::from_container(&c), ServiceState::Unknown);
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
