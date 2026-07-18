use futures::{Stream, StreamExt};
use nirion_lib::{
    context::NirionContext,
    docker::{ProjectStatus, ProjectStatusEvent, query_project_status},
    events::{ComposeEvent, ProcessEvent},
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
    context: &NirionContext,
    target: &TargetSelector,
    compose_stream: impl Stream<Item = anyhow::Result<ComposeEvent>>,
    status_events: impl Stream<Item = anyhow::Result<ProjectStatusEvent>>,
    mut renderer: impl ProgressRenderer,
    wait: WaitTarget,
) -> anyhow::Result<ProgressExit> {
    tokio::pin!(compose_stream);
    tokio::pin!(status_events);
    let cancel = tokio::signal::ctrl_c();
    tokio::pin!(cancel);

    let selected = selected_project_names(target, &context.projects);
    let mut state = ProgressState::new(&selected);

    renderer.start(context, &selected, &state.running, &state.statuses)?;

    while !state.ready(target, &context.projects, wait) {
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

        renderer.tick(context, &selected, &state.running, &state.statuses)?;
    }

    if !state.cancelled
        && state.error.is_none()
        && renderer.needs_status_during_compose()
    {
        refresh_statuses(context, &selected, &mut state.statuses).await?;
    }

    renderer.finish(context, &selected, &state.running, &state.statuses)?;

    if state.cancelled {
        return Ok(ProgressExit::Cancelled);
    }

    if let Some(error) = state.error {
        return Err(error);
    }

    Ok(ProgressExit::Completed)
}

fn handle_compose_event(
    event: &ComposeEvent,
    running: &mut BTreeMap<String, bool>,
) {
    match event {
        ComposeEvent::ProjectStarted { project } => {
            running.insert(project.clone(), true);
        }
        ComposeEvent::ProjectFailed { project, .. } => {
            running.insert(project.clone(), false);
        }
        ComposeEvent::Process {
            project: Some(project),
            event: ProcessEvent::Exited(_),
        } => {
            running.insert(project.clone(), false);
        }
        ComposeEvent::Process { .. } => {}
    }
}

async fn refresh_statuses(
    context: &NirionContext,
    selected: &[String],
    statuses: &mut BTreeMap<String, ProjectStatus>,
) -> anyhow::Result<()> {
    for name in selected {
        let status = query_project_status(context, name).await?;
        statuses.insert(name.clone(), status);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nirion_lib::events::ExitStatus;
    use std::collections::BTreeMap;

    #[test]
    fn handle_compose_event_updates_running_state() {
        let mut running = BTreeMap::new();

        handle_compose_event(
            &ComposeEvent::ProjectStarted {
                project: "app".to_string(),
            },
            &mut running,
        );
        assert_eq!(running.get("app"), Some(&true));

        handle_compose_event(
            &ComposeEvent::Process {
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
            &ComposeEvent::ProjectFailed {
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
            &ComposeEvent::Process {
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
}
