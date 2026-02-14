use std::{
    collections::BTreeMap, ffi::OsStr, ops::Deref, sync::Arc, time::Duration,
};
use tokio::{process::Command, sync::RwLock, task::JoinHandle};

use crate::{
    docker::{query_project_status, ProjectStatus},
    Project, ProjectName,
};

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

pub struct DockerMonitoredProcess {
    pub monitor: DockerProjectMonitor,
    finished: Arc<RwLock<bool>>,
}

impl DockerMonitoredProcess {
    pub async fn new(
        project: Project,
        refresh_interval: Duration,
        args: impl IntoIterator<Item = impl AsRef<OsStr>>,
    ) -> Self {
        let monitor = DockerProjectMonitor::new(&project, refresh_interval);

        let finished = Arc::new(RwLock::new(false));

        let _ = {
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
                    .arg(project.name.deref())
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
