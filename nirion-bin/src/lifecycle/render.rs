use crossterm::{
    cursor::{self, MoveUp},
    execute,
    style::{Color, Stylize},
};
use nirion_lib::{
    context::NirionContext,
    docker::ProjectStatus,
    events::{ComposeEvent, ProcessEvent},
    projects::Projects,
};
use nirion_tui_lib::{
    spinner::Spinner,
    status::{Status, StatusEntry},
};
use std::collections::BTreeMap;
use std::io::{Write, stdout};

use crate::lifecycle::LifecyclePresentation;
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

pub(super) trait LifecycleRenderer {
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

pub(super) fn lifecycle_renderer(
    presentation: LifecyclePresentation
) -> Box<dyn LifecycleRenderer> {
    match presentation {
        LifecyclePresentation::Progress => Box::<ProgressRenderer>::default(),
        LifecyclePresentation::Plain => Box::new(PlainRenderer),
        LifecyclePresentation::Hidden => Box::new(HiddenRenderer),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use nirion_lib::docker::{Port, ServiceState, ServiceStatus};

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
