use futures::{Stream, StreamExt};
use nirion_lib::{
    backend::{NirionBackend, OperationEvent, ProjectStatusRequest},
    docker::{ProjectStatus, ProjectStatusEvent},
    events::ProcessEvent,
    projects::{Projects, selected_project_names},
    wait::{WaitTarget, wait_finished},
};
use std::collections::BTreeMap;

use crate::TargetSelector;
use crate::progress_render::ProgressRenderer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProgressExit {
    Completed,
    Cancelled,
}

struct ProgressState {
    running: BTreeMap<String, bool>,
    statuses: BTreeMap<String, ProjectStatus>,
    compose_finished: bool,
    status_finished: bool,
    cancelled: bool,
    error: Option<anyhow::Error>,
}

impl ProgressState {
    fn new(selected: &[String]) -> Self {
        Self {
            running: selected
                .iter()
                .map(|name| (name.clone(), true))
                .collect(),
            statuses: BTreeMap::new(),
            compose_finished: false,
            status_finished: false,
            cancelled: false,
            error: None,
        }
    }

    fn ready(
        &self,
        target: &TargetSelector,
        projects: &Projects,
        wait: WaitTarget,
    ) -> bool {
        self.cancelled
            || self.error.is_some()
            || (wait == WaitTarget::Forever && self.status_finished)
            || (self.compose_finished
                && wait_finished(target, projects, &self.statuses, wait))
    }

    fn stop_running_projects(&mut self) {
        for value in self.running.values_mut() {
            *value = false;
        }
    }

    fn finish_compose(&mut self) {
        self.compose_finished = true;
        self.stop_running_projects();
    }

    fn fail(
        &mut self,
        error: anyhow::Error,
    ) {
        self.error = Some(error);
        self.finish_compose();
    }

    fn cancel(&mut self) {
        self.cancelled = true;
        self.stop_running_projects();
    }

    fn handle_status_event(
        &mut self,
        event: Option<anyhow::Result<ProjectStatusEvent>>,
        wait: WaitTarget,
    ) {
        match event {
            Some(Ok(event)) => {
                self.statuses
                    .insert(event.project, event.status);
            }
            Some(Err(error)) => self.fail(error),
            None => {
                self.status_finished = true;
                if wait != WaitTarget::Forever {
                    self.fail(anyhow::anyhow!(
                        "docker status stream ended before progress finished"
                    ));
                }
            }
        }
    }
}

pub(crate) async fn run_progress(
    backend: &impl NirionBackend,
    target: &TargetSelector,
    compose_stream: impl Stream<Item = anyhow::Result<OperationEvent>>,
    status_events: impl Stream<Item = anyhow::Result<ProjectStatusEvent>>,
    mut renderer: impl ProgressRenderer,
    wait: WaitTarget,
) -> anyhow::Result<ProgressExit> {
    tokio::pin!(compose_stream);
    tokio::pin!(status_events);
    let cancel = tokio::signal::ctrl_c();
    tokio::pin!(cancel);

    let projects = backend.projects();
    let selected = selected_project_names(target, &projects);
    let mut state = ProgressState::new(&selected);

    renderer.start(&projects, &selected, &state.running, &state.statuses)?;

    while !state.ready(target, &projects, wait) {
        tokio::select! {
            _ = &mut cancel => {
                state.cancel();
            }
            event = compose_stream.next(), if !state.compose_finished => {
                match event {
                    Some(Ok(event)) => {
                        handle_compose_event(&event, &mut state.running);
                        renderer.compose_event(&event)?;
                    }
                    Some(Err(error)) => state.fail(error),
                    None => state.finish_compose(),
                }
            }
            event = status_events.next(), if !state.status_finished => {
                state.handle_status_event(event, wait);
            }
        }

        renderer.tick(&projects, &selected, &state.running, &state.statuses)?;
    }

    if !state.cancelled
        && state.error.is_none()
        && renderer.needs_status_during_compose()
    {
        refresh_statuses(backend, &selected, &mut state.statuses).await?;
    }

    renderer.finish(&projects, &selected, &state.running, &state.statuses)?;

    if state.cancelled {
        return Ok(ProgressExit::Cancelled);
    }

    if let Some(error) = state.error {
        return Err(error);
    }

    Ok(ProgressExit::Completed)
}

fn handle_compose_event(
    event: &OperationEvent,
    running: &mut BTreeMap<String, bool>,
) {
    match event {
        OperationEvent::ProjectStarted { project } => {
            running.insert(project.clone(), true);
        }
        OperationEvent::ProjectFailed { project, .. } => {
            running.insert(project.clone(), false);
        }
        OperationEvent::Process {
            project: Some(project),
            event: ProcessEvent::Exited(_),
        } => {
            running.insert(project.clone(), false);
        }
        OperationEvent::Process { .. } => {}
    }
}

async fn refresh_statuses(
    backend: &impl NirionBackend,
    selected: &[String],
    statuses: &mut BTreeMap<String, ProjectStatus>,
) -> anyhow::Result<()> {
    for name in selected {
        let status = backend
            .project_status(ProjectStatusRequest {
                project: name.clone(),
            })
            .await?;
        statuses.insert(name.clone(), status);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nirion_lib::{
        docker::{Port, ServiceState, ServiceStatus},
        events::ExitStatus,
        projects::ProjectSelector,
    };
    use std::collections::BTreeMap;

    fn projects() -> Projects {
        serde_json::from_str(
            r#"
{
  "app": {
    "name": "app",
    "dockerCompose": "compose.yml",
    "services": {
      "web": {"image": "nginx", "healthcheck": true, "restart": null}
    }
  }
}
"#,
        )
        .unwrap()
    }

    fn project_status(state: ServiceState) -> ProjectStatus {
        ProjectStatus {
            services: BTreeMap::from([(
                "web".to_string(),
                ServiceStatus {
                    id: "web-id".to_string(),
                    service: "web".to_string(),
                    container_name: "web".to_string(),
                    image: "nginx".to_string(),
                    state,
                    health: None,
                    exit_code: None,
                    running_for: None,
                    status: None,
                    ports: Vec::<Port>::new(),
                    networks: Vec::new(),
                },
            )]),
        }
    }

    #[test]
    fn handle_compose_event_updates_running_state() {
        let mut running = BTreeMap::new();

        handle_compose_event(
            &OperationEvent::ProjectStarted {
                project: "app".to_string(),
            },
            &mut running,
        );
        assert_eq!(running.get("app"), Some(&true));

        handle_compose_event(
            &OperationEvent::Process {
                project: Some("app".to_string()),
                event: ProcessEvent::Exited(ExitStatus {
                    code: Some(0),
                    success: true,
                }),
            },
            &mut running,
        );
        assert_eq!(running.get("app"), Some(&false));

        handle_compose_event(
            &OperationEvent::ProjectFailed {
                project: "app".to_string(),
                error: "failed".to_string(),
            },
            &mut running,
        );
        assert_eq!(running.get("app"), Some(&false));
    }

    #[test]
    fn handle_compose_event_ignores_unscoped_process_events() {
        let mut running = BTreeMap::from([("app".to_string(), true)]);

        handle_compose_event(
            &OperationEvent::Process {
                project: None,
                event: ProcessEvent::Exited(ExitStatus {
                    code: Some(0),
                    success: true,
                }),
            },
            &mut running,
        );

        assert_eq!(running.get("app"), Some(&true));
    }

    #[test]
    fn progress_state_initializes_selected_projects_as_running() {
        let state = ProgressState::new(&["app".to_string(), "api".to_string()]);

        assert_eq!(state.running.get("app"), Some(&true));
        assert_eq!(state.running.get("api"), Some(&true));
        assert!(state.statuses.is_empty());
        assert!(!state.compose_finished);
        assert!(!state.status_finished);
        assert!(!state.cancelled);
        assert!(state.error.is_none());
    }

    #[test]
    fn progress_state_ready_when_cancelled_or_failed() {
        let projects = projects();
        let target = TargetSelector::Project(ProjectSelector {
            name: "app".to_string(),
        });
        let mut state = ProgressState::new(&["app".to_string()]);

        assert!(!state.ready(&target, &projects, WaitTarget::Healthchecks));

        state.cancel();
        assert!(state.ready(&target, &projects, WaitTarget::Healthchecks));
        assert_eq!(state.running.get("app"), Some(&false));

        let mut state = ProgressState::new(&["app".to_string()]);
        state.fail(anyhow::anyhow!("boom"));
        assert!(state.ready(&target, &projects, WaitTarget::Healthchecks));
        assert!(state.error.is_some());
        assert!(state.compose_finished);
        assert_eq!(state.running.get("app"), Some(&false));
    }

    #[test]
    fn progress_state_ready_after_compose_and_healthchecks_finish() {
        let projects = projects();
        let target = TargetSelector::Project(ProjectSelector {
            name: "app".to_string(),
        });
        let mut state = ProgressState::new(&["app".to_string()]);
        state
            .statuses
            .insert("app".to_string(), project_status(ServiceState::Healthy));

        assert!(!state.ready(&target, &projects, WaitTarget::Healthchecks));

        state.finish_compose();

        assert!(state.ready(&target, &projects, WaitTarget::Healthchecks));
        assert_eq!(state.running.get("app"), Some(&false));
    }

    #[test]
    fn progress_state_handles_status_events() {
        let mut state = ProgressState::new(&["app".to_string()]);

        state.handle_status_event(
            Some(Ok(ProjectStatusEvent {
                project: "app".to_string(),
                status: project_status(ServiceState::Starting),
            })),
            WaitTarget::Healthchecks,
        );
        assert_eq!(
            state.statuses["app"].services["web"].state,
            ServiceState::Starting
        );

        state.handle_status_event(None, WaitTarget::Healthchecks);
        assert!(state.status_finished);
        assert!(state.error.is_some());

        let mut state = ProgressState::new(&["app".to_string()]);
        state.handle_status_event(None, WaitTarget::Forever);
        assert!(state.status_finished);
        assert!(state.error.is_none());

        let projects = projects();
        let target = TargetSelector::Project(ProjectSelector {
            name: "app".to_string(),
        });
        assert!(state.ready(&target, &projects, WaitTarget::Forever));
    }

    #[test]
    fn progress_state_fails_on_status_error() {
        let mut state = ProgressState::new(&["app".to_string()]);

        state.handle_status_event(
            Some(Err(anyhow::anyhow!("status failed"))),
            WaitTarget::Forever,
        );

        assert!(state.error.is_some());
        assert!(state.compose_finished);
        assert_eq!(state.running.get("app"), Some(&false));
    }
}
