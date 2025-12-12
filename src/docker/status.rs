use std::collections::BTreeMap;

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
        .await?;

    let json = if output.status.success() {
        String::from_utf8_lossy(&output.stdout).to_string()
    } else {
        "[]".to_string()
    };

    let status = ProjectStatus::from_json(project_name, &json)?;
    Ok(status)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceState {
    Running,
    Healthy,
    Exited,
    Starting,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct ServiceStatus {
    pub _name: String,
    pub state: ServiceState,
}

#[derive(Debug, Clone)]
pub struct ProjectStatus {
    pub name: String,
    pub services: BTreeMap<String, ServiceStatus>,
}

impl ProjectStatus {
    pub fn from_json(project_name: &str, json: &str) -> anyhow::Result<Self> {
        let mut status = ProjectStatus {
            name: project_name.to_string(),
            services: BTreeMap::new(),
        };

        let json = json.trim();
        if json.is_empty() || json == "[]" {
            return Ok(status);
        }
        if json.starts_with('[') {
            if let Ok(containers) =
                serde_json::from_str::<Vec<serde_json::Value>>(json)
            {
                for c in containers {
                    Self::process_container(&mut status, &c);
                }
            }
        } else {
            for line in json.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }

                if let Ok(c) = serde_json::from_str::<serde_json::Value>(line) {
                    Self::process_container(&mut status, &c);
                }
            }
        }

        Ok(status)
    }

    fn process_container(st: &mut ProjectStatus, c: &serde_json::Value) {
        let name = c["Name"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();

        let state_str = c["State"].as_str().unwrap_or("unknown");
        let health_str = c["Health"].as_str().unwrap_or("");

        let state = match state_str {
            "running" if health_str == "healthy" => ServiceState::Healthy,
            "running" => ServiceState::Running,
            "exited" => ServiceState::Exited,
            "restarting" | "created" => ServiceState::Starting,
            _ => ServiceState::Unknown,
        };

        st.services
            .insert(name.clone(), ServiceStatus { _name: name, state });
    }

    pub fn total(&self) -> usize {
        self.services.len()
    }

    pub fn running(&self) -> usize {
        self.services
            .values()
            .filter(|s| {
                matches!(s.state, ServiceState::Running | ServiceState::Healthy)
            })
            .count()
    }

    pub fn healthy(&self) -> usize {
        self.services
            .values()
            .filter(|s| matches!(s.state, ServiceState::Healthy))
            .count()
    }

    pub fn exited(&self) -> usize {
        self.services
            .values()
            .filter(|s| matches!(s.state, ServiceState::Exited))
            .count()
    }

    pub fn starting(&self) -> usize {
        self.services
            .values()
            .filter(|s| matches!(s.state, ServiceState::Starting))
            .count()
    }
}
