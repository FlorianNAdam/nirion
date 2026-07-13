use crossterm::{
    cursor::{self, MoveUp},
    execute,
    style::{Color, Stylize},
};
use futures::StreamExt;
use nirion_lib::{
    compose::{ComposeConcurrency, compose_target_with_concurrency},
    docker::{ProjectStatus, query_project_status, status_stream},
    events::{ComposeEvent, ProcessEvent},
    projects::{Projects, selected_project_names},
    wait::{WaitTarget, wait_finished},
};
use nirion_tui_lib::{
    spinner::Spinner,
    status::{Status, StatusEntry},
};
use std::collections::BTreeMap;
use std::io::{Write, stdout};
use tokio::time::Duration;

use crate::TargetSelector;
use crate::status_display::{project_state_icon, project_status_segments};

struct CursorGuard;

impl CursorGuard {
    fn hide() -> anyhow::Result<Self> {
        let mut stdout = stdout();
        execute!(stdout, cursor::Hide)?;
        Ok(Self)
    }
}

impl Drop for CursorGuard {
    fn drop(&mut self) {
        let mut stdout = stdout();
        let _ = execute!(stdout, cursor::Show);
        let _ = stdout.flush();
    }
}

fn empty_status() -> ProjectStatus {
    ProjectStatus {
        services: BTreeMap::new(),
    }
}

fn create_status(
    spinner: &Spinner,
    selected: &[String],
    running: &BTreeMap<String, bool>,
    statuses: &BTreeMap<String, ProjectStatus>,
    projects: &Projects,
) -> Status {
    let mut entries = Vec::new();

    for name in selected {
        let project_status = statuses
            .get(name)
            .cloned()
            .unwrap_or_else(empty_status);
        let project = &projects[name];

        let icon = if *running.get(name).unwrap_or(&false) {
            spinner.get().yellow().to_string()
        } else {
            project_state_icon(&project_status.project_state())
        };

        let prefix = format!("{icon} {name}");

        let progressing = project_status.progressing();
        let num_services = project.services.len();
        let mut segments = project_status_segments(&project_status);

        for _ in 0..(num_services.saturating_sub(segments.len())) {
            segments.push(Color::Grey);
        }

        let suffix = format!("({progressing}/{num_services})    ");

        entries.push(StatusEntry {
            prefix,
            segments,
            suffix,
        });
    }

    Status { entries }
}

fn print_progress(
    selected: &[String],
    spinner: &Spinner,
    running: &BTreeMap<String, bool>,
    statuses: &BTreeMap<String, ProjectStatus>,
    projects: &Projects,
    move_up: bool,
) -> anyhow::Result<()> {
    let mut stdout = stdout();
    let status = create_status(spinner, selected, running, statuses, projects);
    status.print()?;

    if move_up {
        execute!(stdout, MoveUp((selected.len() * 2 + 1) as u16))?;
    }

    stdout.flush()?;
    Ok(())
}

pub async fn run_command_with_progress(
    target: &TargetSelector,
    projects: &Projects,
    args: &[&str],
    no_monitor: bool,
    quiet: bool,
    refresh_interval: Duration,
    wait_for_healthchecks: bool,
) -> anyhow::Result<()> {
    if no_monitor {
        return run_compose_hidden(target, projects, args).await;
    }

    let selected = selected_project_names(target, projects);
    let args = args
        .iter()
        .map(|arg| arg.to_string())
        .collect::<Vec<_>>();
    let mut compose_stream = compose_target_with_concurrency(
        target.clone(),
        projects.clone(),
        args,
        ComposeConcurrency::Parallel,
    );
    let mut status_stream =
        status_stream(target.clone(), projects.clone(), refresh_interval);
    let spinner = Spinner::default();
    let mut running = selected
        .iter()
        .map(|name| (name.clone(), true))
        .collect::<BTreeMap<_, _>>();
    let mut statuses = BTreeMap::new();
    let mut compose_finished = false;
    let mut compose_error = None;

    let _cursor = CursorGuard::hide()?;

    if !quiet {
        print_progress(
            &selected, &spinner, &running, &statuses, projects, true,
        )?;
    }

    loop {
        let ready = compose_error.is_some()
            || (compose_finished
                && (!wait_for_healthchecks
                    || wait_finished(
                        target,
                        projects,
                        &statuses,
                        WaitTarget::Healthchecks,
                    )));

        if ready {
            break;
        }

        tokio::select! {
            event = compose_stream.next(), if !compose_finished => {
                match event {
                    Some(Ok(event)) => handle_compose_event(event, &mut running),
                    Some(Err(error)) => {
                        compose_error = Some(error);
                        compose_finished = true;
                        for value in running.values_mut() {
                            *value = false;
                        }
                    }
                    None => {
                        compose_finished = true;
                        for value in running.values_mut() {
                            *value = false;
                        }
                    }
                }
            }
            event = status_stream.next() => {
                match event {
                    Some(Ok(event)) => {
                        statuses.insert(event.project, event.status);
                    }
                    Some(Err(error)) => {
                        compose_error = Some(error);
                        compose_finished = true;
                        for value in running.values_mut() {
                            *value = false;
                        }
                    }
                    None => {
                        compose_error = Some(anyhow::anyhow!("docker status stream ended before progress finished"));
                        compose_finished = true;
                        for value in running.values_mut() {
                            *value = false;
                        }
                    }
                }
            }
        }

        if !quiet {
            print_progress(
                &selected, &spinner, &running, &statuses, projects, true,
            )?;
        }
    }

    if compose_error.is_none() {
        refresh_statuses(&selected, projects, &mut statuses).await?;
    }

    if !quiet {
        print_progress(
            &selected, &spinner, &running, &statuses, projects, false,
        )?;
    }

    if let Some(error) = compose_error {
        return Err(error);
    }

    Ok(())
}

async fn run_compose_hidden(
    target: &TargetSelector,
    projects: &Projects,
    args: &[&str],
) -> anyhow::Result<()> {
    let args = args
        .iter()
        .map(|arg| arg.to_string())
        .collect::<Vec<_>>();
    let mut stream = compose_target_with_concurrency(
        target.clone(),
        projects.clone(),
        args,
        ComposeConcurrency::Parallel,
    );

    while let Some(event) = stream.next().await {
        event?;
    }

    Ok(())
}

fn handle_compose_event(
    event: ComposeEvent,
    running: &mut BTreeMap<String, bool>,
) {
    match event {
        ComposeEvent::ProjectStarted { project } => {
            running.insert(project, true);
        }
        ComposeEvent::ProjectFailed { project, .. } => {
            running.insert(project, false);
        }
        ComposeEvent::Process {
            project: Some(project),
            event: ProcessEvent::Exited(_),
        } => {
            running.insert(project, false);
        }
        ComposeEvent::Process { .. } => {}
    }
}

async fn refresh_statuses(
    selected: &[String],
    projects: &Projects,
    statuses: &mut BTreeMap<String, ProjectStatus>,
) -> anyhow::Result<()> {
    for name in selected {
        let project = &projects[name];
        let status =
            query_project_status(&project.docker_compose, &project.name)
                .await?;
        statuses.insert(name.clone(), status);
    }

    Ok(())
}
