use anyhow::Result;
use std::{collections::BTreeMap, process::Command as ProcCommand, sync::Arc};
use tokio::{process::Command, sync::RwLock};

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

pub struct DockerUpProcess {
    pub project_name: String,
    pub compose_file: String,

    project_status: Arc<RwLock<ProjectStatus>>,
    finished: Arc<RwLock<bool>>,
}

impl DockerUpProcess {
    pub async fn create(
        project_name: String,
        project: &Project,
    ) -> Result<Arc<Self>> {
        let proc = Arc::new(Self {
            compose_file: project.docker_compose.clone(),
            project_name: project_name.clone(),
            project_status: Arc::new(RwLock::new(ProjectStatus {
                name: project_name.clone(),
                services: BTreeMap::new(),
            })),
            finished: Arc::new(RwLock::new(false)),
        });

        proc.spawn_background().await?;
        proc.refresh_status().await?;

        Ok(proc)
    }

    async fn spawn_background(self: &Arc<Self>) -> Result<()> {
        let compose_file = self.compose_file.clone();
        let project_name = self.project_name.clone();
        let finished = self.finished.clone();

        tokio::spawn(async move {
            let _out = tokio::process::Command::new("docker")
                .arg("compose")
                .arg("-f")
                .arg(&compose_file)
                .arg("--project-name")
                .arg(&project_name)
                .arg("up")
                .arg("-d")
                .output()
                .await;

            {
                let mut finished = finished.write().await;
                *finished = true;
            }

            // match out {
            //     Ok(o) if o.status.success() => {
            //         eprintln!("[{project_name}] docker up -d finished");
            //     }
            //     Ok(o) => {
            //         eprintln!(
            //             "[{project_name}] docker up -d failed: {}",
            //             String::from_utf8_lossy(&o.stderr)
            //         );
            //     }
            //     Err(e) => {
            //         eprintln!("[{project_name}] docker up -d error: {e}");
            //     }
            // }
        });

        Ok(())
    }

    pub async fn refresh_status(self: &Arc<Self>) -> Result<()> {
        let output = Command::new("docker")
            .arg("compose")
            .arg("-f")
            .arg(&self.compose_file)
            .arg("--project-name")
            .arg(&self.project_name)
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

        let parsed = ProjectStatus::from_json(&self.project_name, &json)?;
        *self.project_status.write().await = parsed;

        Ok(())
    }

    pub async fn project_status(&self) -> ProjectStatus {
        self.project_status.read().await.clone()
    }

    pub async fn finished(&self) -> bool {
        *self.finished.read().await
    }
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
