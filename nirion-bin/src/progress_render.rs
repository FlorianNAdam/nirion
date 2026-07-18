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

use crate::status_display::{project_state_icon, project_status_segments};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressPresentation {
    Progress,
    Plain,
    Hidden,
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
    spinner: Option<&Spinner>,
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

        let icon = if let Some(spinner) = spinner
            && *running.get(name).unwrap_or(&false)
        {
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
    spinner: Option<&Spinner>,
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

pub(crate) trait ProgressRenderer {
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

impl<T> ProgressRenderer for Box<T>
where
    T: ProgressRenderer + ?Sized,
{
    fn needs_status_during_compose(&self) -> bool {
        (**self).needs_status_during_compose()
    }

    fn start(
        &mut self,
        context: &NirionContext,
        selected: &[String],
        running: &BTreeMap<String, bool>,
        statuses: &BTreeMap<String, ProjectStatus>,
    ) -> anyhow::Result<()> {
        (**self).start(context, selected, running, statuses)
    }

    fn compose_event(
        &mut self,
        event: &ComposeEvent,
    ) -> anyhow::Result<()> {
        (**self).compose_event(event)
    }

    fn tick(
        &mut self,
        context: &NirionContext,
        selected: &[String],
        running: &BTreeMap<String, bool>,
        statuses: &BTreeMap<String, ProjectStatus>,
    ) -> anyhow::Result<()> {
        (**self).tick(context, selected, running, statuses)
    }

    fn finish(
        &mut self,
        context: &NirionContext,
        selected: &[String],
        running: &BTreeMap<String, bool>,
        statuses: &BTreeMap<String, ProjectStatus>,
    ) -> anyhow::Result<()> {
        (**self).finish(context, selected, running, statuses)
    }
}

#[derive(Default)]
struct ProgressStatusRenderer {
    spinner: Spinner,
    cursor: Option<CursorGuard>,
}

impl ProgressRenderer for ProgressStatusRenderer {
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
            Some(&self.spinner),
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
            Some(&self.spinner),
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
            Some(&self.spinner),
            running,
            statuses,
            &context.projects,
            false,
        )
    }
}

#[derive(Default)]
pub(crate) struct StaticStatusRenderer {
    cursor: Option<CursorGuard>,
}

impl ProgressRenderer for StaticStatusRenderer {
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
            None,
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
            None,
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
            None,
            running,
            statuses,
            &context.projects,
            false,
        )
    }
}

struct PlainRenderer;

impl ProgressRenderer for PlainRenderer {
    fn compose_event(
        &mut self,
        event: &ComposeEvent,
    ) -> anyhow::Result<()> {
        render_compose_event(event);
        Ok(())
    }
}

struct HiddenRenderer;

impl ProgressRenderer for HiddenRenderer {}

pub(crate) fn progress_renderer(
    presentation: ProgressPresentation
) -> Box<dyn ProgressRenderer> {
    match presentation {
        ProgressPresentation::Progress => {
            Box::<ProgressStatusRenderer>::default()
        }
        ProgressPresentation::Plain => Box::new(PlainRenderer),
        ProgressPresentation::Hidden => Box::new(HiddenRenderer),
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
    fn create_status_uses_empty_status_when_project_has_no_status() {
        let projects = projects();
        let selected = vec!["app".to_string()];
        let running = BTreeMap::new();
        let statuses = BTreeMap::new();

        let status =
            create_status(None, &selected, &running, &statuses, &projects);

        assert_eq!(status.entries.len(), 1);
        assert_eq!(status.entries[0].segments, vec![Color::Grey, Color::Grey]);
        assert_eq!(status.entries[0].suffix, "(0/2)    ");
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

        let status =
            create_status(None, &selected, &running, &statuses, &projects);

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
