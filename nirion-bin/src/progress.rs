use crossterm::{
    cursor::{self, MoveUp},
    execute,
    style::{Color, Stylize},
};
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
use nirion_tui_lib::{
    spinner::Spinner,
    status::{Status, StatusEntry},
};
use std::collections::BTreeMap;
use std::io::{Write, stdout};
use std::num::NonZeroUsize;
use tokio::time::Duration;

use crate::TargetSelector;
use crate::status_display::{project_state_icon, project_status_segments};

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

trait LifecycleRenderer {
    fn needs_status_during_compose(&self) -> bool {
        false
    }

    fn start(
        &mut self,
        _context: &NirionContext,
        _selected: &[String],
        _running: &BTreeMap<String, bool>,
        _statuses: &BTreeMap<String, ProjectStatus>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn compose_event(
        &mut self,
        _event: &ComposeEvent,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn tick(
        &mut self,
        _context: &NirionContext,
        _selected: &[String],
        _running: &BTreeMap<String, bool>,
        _statuses: &BTreeMap<String, ProjectStatus>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn finish(
        &mut self,
        _context: &NirionContext,
        _selected: &[String],
        _running: &BTreeMap<String, bool>,
        _statuses: &BTreeMap<String, ProjectStatus>,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Default)]
struct ProgressRenderer {
    spinner: Spinner,
    cursor: Option<CursorGuard>,
}

impl LifecycleRenderer for ProgressRenderer {
    fn needs_status_during_compose(&self) -> bool {
        true
    }

    fn start(
        &mut self,
        context: &NirionContext,
        selected: &[String],
        running: &BTreeMap<String, bool>,
        statuses: &BTreeMap<String, ProjectStatus>,
    ) -> anyhow::Result<()> {
        self.cursor = Some(CursorGuard::hide()?);
        print_progress(
            selected,
            &self.spinner,
            running,
            statuses,
            &context.projects,
            true,
        )
    }

    fn tick(
        &mut self,
        context: &NirionContext,
        selected: &[String],
        running: &BTreeMap<String, bool>,
        statuses: &BTreeMap<String, ProjectStatus>,
    ) -> anyhow::Result<()> {
        print_progress(
            selected,
            &self.spinner,
            running,
            statuses,
            &context.projects,
            true,
        )
    }

    fn finish(
        &mut self,
        context: &NirionContext,
        selected: &[String],
        running: &BTreeMap<String, bool>,
        statuses: &BTreeMap<String, ProjectStatus>,
    ) -> anyhow::Result<()> {
        print_progress(
            selected,
            &self.spinner,
            running,
            statuses,
            &context.projects,
            false,
        )
    }
}

struct PlainRenderer;

impl LifecycleRenderer for PlainRenderer {
    fn compose_event(
        &mut self,
        event: &ComposeEvent,
    ) -> anyhow::Result<()> {
        render_compose_event(event);
        Ok(())
    }
}

struct HiddenRenderer;

impl LifecycleRenderer for HiddenRenderer {}

fn lifecycle_renderer(
    presentation: LifecyclePresentation
) -> Box<dyn LifecycleRenderer> {
    match presentation {
        LifecyclePresentation::Progress => Box::<ProgressRenderer>::default(),
        LifecyclePresentation::Plain => Box::new(PlainRenderer),
        LifecyclePresentation::Hidden => Box::new(HiddenRenderer),
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

fn render_compose_event(event: &ComposeEvent) {
    match event {
        ComposeEvent::ProjectStarted { project } => {
            println!("[{}]", project.as_str().cyan());
        }
        ComposeEvent::Process { event, .. } => render_process_event(event),
        ComposeEvent::ProjectFailed { project, error } => {
            eprintln!("Project '{}' failed: {}", project, error);
            println!();
        }
    }
}

fn render_process_event(event: &ProcessEvent) {
    match event {
        ProcessEvent::StdoutLine(line) => println!("{}", line),
        ProcessEvent::StderrLine(line) => {
            if !line.contains("the attribute `version` is obsolete") {
                eprintln!("{}", line);
            }
        }
        ProcessEvent::Exited(_) => {}
    }
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
    use nirion_lib::{
        docker::{Port, ServiceState, ServiceStatus},
        events::ExitStatus,
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
      "web": {"image": "nginx", "healthcheck": true, "restart": null},
      "db": {"image": "postgres", "healthcheck": true, "restart": null}
    }
  }
}
"#,
        )
        .unwrap()
    }

    fn service_status(
        service: &str,
        state: ServiceState,
    ) -> ServiceStatus {
        ServiceStatus {
            id: format!("{service}-id"),
            service: service.to_string(),
            container_name: service.to_string(),
            image: "image".to_string(),
            state,
            health: None,
            exit_code: None,
            running_for: None,
            status: None,
            ports: Vec::<Port>::new(),
            networks: Vec::new(),
        }
    }

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

    #[test]
    fn create_status_pads_missing_service_segments() {
        let projects = projects();
        let selected = vec!["app".to_string()];
        let running = BTreeMap::from([("app".to_string(), false)]);
        let statuses = BTreeMap::from([(
            "app".to_string(),
            ProjectStatus {
                services: BTreeMap::from([(
                    "web".to_string(),
                    service_status("web", ServiceState::Healthy),
                )]),
            },
        )]);

        let status = create_status(
            &Spinner::default(),
            &selected,
            &running,
            &statuses,
            &projects,
        );

        assert_eq!(status.entries.len(), 1);
        assert_eq!(status.entries[0].segments.len(), 2);
        assert_eq!(
            status.entries[0].segments[0],
            project_status_segments(&statuses["app"])[0]
        );
        assert_eq!(status.entries[0].segments[1], Color::Grey);
        assert_eq!(status.entries[0].suffix, "(1/2)    ");
    }
}
