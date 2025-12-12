use anyhow::Result;
use std::{
    collections::BTreeMap, ffi::OsStr, process::Command as ProcCommand,
    sync::Arc, time::Duration,
};
use tokio::{process::Command, sync::RwLock, task::JoinHandle};

use crate::{Project, TargetSelector};

pub fn compose_target_cmd(
    target: &TargetSelector,
    projects: &BTreeMap<String, Project>,
    args: &[&str],
) -> Result<()> {
    match target {
        TargetSelector::All => {
            for (project_name, project) in projects {
                let mut cmd_args = vec![
                    "--file",
                    &project.docker_compose,
                    "--project-name",
                    &project_name,
                ];
                cmd_args.extend_from_slice(args);

                println!("Running: docker compose {:?}", cmd_args.join(" "));

                let status = ProcCommand::new("docker-compose")
                    .args(&cmd_args)
                    .status()?;
                if status.success() {
                    println!(
                        "Project '{}' completed successfully.",
                        project_name
                    );
                } else {
                    println!(
                        "Project '{}' failed. Status: {}",
                        project_name, status
                    );
                }
            }
        }
        TargetSelector::Project(proj) => {
            let compose_file = &projects[&proj.name].docker_compose;
            let mut cmd_args =
                vec!["--file", compose_file, "--project-name", &proj.name];
            cmd_args.extend_from_slice(args);

            println!("Running: docker-compose {:?}", cmd_args);

            let status = ProcCommand::new("docker-compose")
                .args(&cmd_args)
                .status()?;
            if status.success() {
                println!("Project '{}' completed successfully.", proj.name);
            } else {
                println!("Project '{}' failed. Status: {}", proj.name, status);
            }
        }
        TargetSelector::Image(img) => {
            let compose_file = &projects[&img.project].docker_compose;
            let mut cmd_args =
                vec!["--file", compose_file, "--project-name", &img.project];
            cmd_args.extend_from_slice(args);
            cmd_args.push(&img.image);

            println!(
                "Running: docker-compose {:?} (image {})",
                cmd_args.join(" "),
                img.image
            );

            let status = ProcCommand::new("docker-compose")
                .args(&cmd_args)
                .status()?;
            if status.success() {
                println!(
                    "Image '{}' in project '{}' completed successfully.",
                    img.image, img.project
                );
            } else {
                println!(
                    "Image '{}' in project '{}' failed. Status: {}",
                    img.image, img.project, status
                );
            }
        }
    }
    Ok(())
}

pub fn fetch_digest(image: &str) -> anyhow::Result<String> {
    let output = ProcCommand::new("skopeo")
        .args([
            "inspect",
            "--format",
            "{{.Digest}}",
            &format!("docker://{}", image),
        ])
        .output()?;

    if !output.status.success() {
        anyhow::bail!("Failed to fetch digest")
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_string())
}

pub struct DockerProjectMonitor {
    pub project_name: String,
    pub compose_file: String,
    project_status: Arc<RwLock<ProjectStatus>>,
    refresh_handle: Option<JoinHandle<()>>,
}

impl DockerProjectMonitor {
    pub fn new(
        project_name: impl Into<String>,
        project: &Project,
        refresh_interval: Duration,
    ) -> Self {
        let name: String = project_name.into();
        let mut monitor = Self {
            project_name: name.clone(),
            compose_file: project.docker_compose.clone(),
            project_status: Arc::new(RwLock::new(ProjectStatus {
                name: name.clone(),
                services: BTreeMap::new(),
            })),
            refresh_handle: None,
        };

        let handle = {
            let project_status = monitor.project_status.clone();
            let project_name = name;
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

    pub async fn refresh_status(&self) -> Result<()> {
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

async fn project_refresh_thread(
    project_status: Arc<RwLock<ProjectStatus>>,
    compose_file: String,
    project_name: String,
    interval: Duration,
) {
    loop {
        match query_project_status(&compose_file, &project_name).await {
            Ok(new_status) => {
                let mut status = project_status.write().await;
                *status = new_status;
            }
            Err(e) => {
                eprintln!(
                    "Failed to refresh status for {}: {:?}",
                    project_name, e
                );
            }
        }
        tokio::time::sleep(interval).await;
    }
}

pub async fn query_project_status(
    compose_file: &str,
    project_name: &str,
) -> Result<ProjectStatus> {
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
    pub fn from_json(project_name: &str, json: &str) -> Result<Self> {
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

pub struct DockerMonitoredProcess {
    pub monitor: DockerProjectMonitor,
    finished: Arc<RwLock<bool>>,
}

impl DockerMonitoredProcess {
    pub async fn new(
        project_name: impl Into<String>,
        project: Project,
        refresh_interval: Duration,
        args: impl IntoIterator<Item = impl AsRef<OsStr>>,
    ) -> Self {
        let name = project_name.into();
        let monitor =
            DockerProjectMonitor::new(&name, &project, refresh_interval);

        let finished = Arc::new(RwLock::new(false));

        let _ = {
            let project_name = name.clone();
            let compose_file = project.docker_compose.clone();
            let args: Vec<_> = args
                .into_iter()
                .map(|a| a.as_ref().to_os_string())
                .collect();
            let finished = finished.clone();

            tokio::spawn(async move {
                let _ = Command::new("docker")
                    .arg("compose")
                    .arg("-f")
                    .arg(compose_file)
                    .arg("--project-name")
                    .arg(project_name)
                    .args(args)
                    .output()
                    .await;

                *finished.write().await = true;
            })
        };

        Self { monitor, finished }
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
}
