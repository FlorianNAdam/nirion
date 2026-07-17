use futures::{StreamExt, stream};
use nirion_lib::{
    compose::{ComposeConcurrency, compose_target},
    context::NirionContext,
    docker::{
        ProjectStatus, ProjectStatusEvent, query_project_status, status_stream,
    },
    events::{ComposeEvent, ProcessEvent},
    projects::{Projects, selected_project_names},
    wait::{WaitTarget, wait_finished},
};
use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use tokio::time::Duration;

use crate::TargetSelector;

mod render;
use render::lifecycle_renderer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecyclePresentation {
    Progress,
    Plain,
    Hidden,
}

#[derive(Debug, Clone, Copy)]
pub struct LifecycleOptions {
    pub presentation: LifecyclePresentation,
    pub jobs: usize,
    pub refresh_interval: Duration,
    pub wait_for_healthchecks: bool,
}

pub fn lifecycle_options(
    plain: bool,
    quiet: bool,
    jobs: Option<NonZeroUsize>,
    refresh_interval: Duration,
    wait_for_healthchecks: bool,
) -> LifecycleOptions {
    let presentation = if quiet {
        LifecyclePresentation::Hidden
    } else if plain {
        LifecyclePresentation::Plain
    } else {
        LifecyclePresentation::Progress
    };

    LifecycleOptions {
        presentation,
        jobs: jobs
            .map(usize::from)
            .unwrap_or(usize::MAX),
        refresh_interval,
        wait_for_healthchecks,
    }
}

struct LifecycleState {
    running: BTreeMap<String, bool>,
    statuses: BTreeMap<String, ProjectStatus>,
    compose_finished: bool,
    status_finished: bool,
    error: Option<anyhow::Error>,
}

impl LifecycleState {
    fn new(selected: &[String]) -> Self {
        Self {
            running: selected
                .iter()
                .map(|name| (name.clone(), true))
                .collect(),
            statuses: BTreeMap::new(),
            compose_finished: false,
            status_finished: false,
            error: None,
        }
    }

    fn ready(
        &self,
        target: &TargetSelector,
        projects: &Projects,
        wait_for_healthchecks: bool,
    ) -> bool {
        self.error.is_some()
            || (self.compose_finished
                && (!wait_for_healthchecks
                    || wait_finished(
                        target,
                        projects,
                        &self.statuses,
                        WaitTarget::Healthchecks,
                    )))
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

    fn handle_status_event(
        &mut self,
        event: Option<anyhow::Result<ProjectStatusEvent>>,
    ) {
        match event {
            Some(Ok(event)) => {
                self.statuses
                    .insert(event.project, event.status);
            }
            Some(Err(error)) => self.fail(error),
            None => {
                self.status_finished = true;
                self.fail(anyhow::anyhow!(
                    "docker status stream ended before progress finished"
                ));
            }
        }
    }
}

pub async fn run_lifecycle_command(
    context: &NirionContext,
    target: &TargetSelector,
    args: &[&str],
    options: LifecycleOptions,
) -> anyhow::Result<()> {
    let args = args
        .iter()
        .map(|arg| arg.to_string())
        .collect::<Vec<_>>();
    let mut compose_stream = compose_target(
        context.clone(),
        target.clone(),
        args,
        ComposeConcurrency::Jobs(options.jobs),
    );

    let selected = selected_project_names(target, &context.projects);
    let mut state = LifecycleState::new(&selected);

    let mut renderer = lifecycle_renderer(options.presentation);

    let needs_status = renderer.needs_status_during_compose()
        || (options.wait_for_healthchecks
            && !wait_finished(
                target,
                &context.projects,
                &BTreeMap::new(),
                WaitTarget::Healthchecks,
            ));
    let mut status_events = if needs_status {
        status_stream(context, target.clone(), options.refresh_interval)
    } else {
        stream::pending().boxed()
    };

    renderer.start(context, &selected, &state.running, &state.statuses)?;

    while !state.ready(target, &context.projects, options.wait_for_healthchecks)
    {
        tokio::select! {
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
                state.handle_status_event(event);
            }
        }

        renderer.tick(context, &selected, &state.running, &state.statuses)?;
    }

    if state.error.is_none() && renderer.needs_status_during_compose() {
        refresh_statuses(context, &selected, &mut state.statuses).await?;
    }

    renderer.finish(context, &selected, &state.running, &state.statuses)?;

    if let Some(error) = state.error {
        return Err(error);
    }

    Ok(())
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
