use nirion_lib::{
    context::NirionContext,
    docker::ProjectStatus,
    events::{ComposeEvent, ProcessEvent},
    projects::Projects,
};
use nirion_tui_lib::{
    color::{Colorize, GREY},
    line_renderer::LineRenderer,
    spinner::Spinner,
    status::{Status, StatusEntry},
    terminal::{HiddenCursorGuard, terminal_width},
};
use std::collections::BTreeMap;

use crate::status_display::{project_state_icon, project_status_segments};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressPresentation {
    Progress,
    Plain,
    Hidden,
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
            segments.push(GREY);
        }

        let suffix = format!("({progressing}/{num_services})    ");

        entries.push(StatusEntry {
            prefix,
            segments,
            suffix,
        });
    }

    Status::new(entries)
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

pub(crate) struct StatusProgressRenderer {
    spinner: Option<Spinner>,
    lines: LineRenderer,
    cursor: Option<HiddenCursorGuard>,
}

impl StatusProgressRenderer {
    pub(crate) fn with_spinner() -> Self {
        Self {
            spinner: Some(Spinner::default()),
            lines: LineRenderer::default(),
            cursor: None,
        }
    }

    pub(crate) fn without_spinner() -> Self {
        Self {
            spinner: None,
            lines: LineRenderer::default(),
            cursor: None,
        }
    }

    fn spinner(&self) -> Option<&Spinner> {
        self.spinner.as_ref()
    }
}

impl ProgressRenderer for StatusProgressRenderer {
    fn needs_status_during_compose(&self) -> bool {
        self.spinner.is_some()
    }

    fn start(
        &mut self,
        context: &NirionContext,
        selected: &[String],
        running: &BTreeMap<String, bool>,
        statuses: &BTreeMap<String, ProjectStatus>,
    ) -> anyhow::Result<()> {
        self.cursor = Some(HiddenCursorGuard::hide()?);
        let progress = create_status(
            self.spinner(),
            selected,
            running,
            statuses,
            &context.projects,
        )
        .render(terminal_width());
        self.lines.start(&progress)
    }

    fn tick(
        &mut self,
        context: &NirionContext,
        selected: &[String],
        running: &BTreeMap<String, bool>,
        statuses: &BTreeMap<String, ProjectStatus>,
    ) -> anyhow::Result<()> {
        let progress = create_status(
            self.spinner(),
            selected,
            running,
            statuses,
            &context.projects,
        )
        .render(terminal_width());
        self.lines.render(&progress)
    }

    fn finish(
        &mut self,
        context: &NirionContext,
        selected: &[String],
        running: &BTreeMap<String, bool>,
        statuses: &BTreeMap<String, ProjectStatus>,
    ) -> anyhow::Result<()> {
        let progress = create_status(
            self.spinner(),
            selected,
            running,
            statuses,
            &context.projects,
        )
        .render(terminal_width());
        self.lines.finish(&progress)
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
            Box::new(StatusProgressRenderer::with_spinner())
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
        assert_eq!(status.entries[0].segments, vec![GREY, GREY]);
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
        assert_eq!(status.entries[0].segments[1], GREY);
        assert_eq!(status.entries[0].suffix, "(1/2)    ");
    }

    #[test]
    fn status_progress_renderer_needs_status_when_using_spinner() {
        assert!(
            StatusProgressRenderer::with_spinner()
                .needs_status_during_compose()
        );
    }

    #[test]
    fn status_progress_renderer_without_spinner_is_status_only() {
        assert!(
            !StatusProgressRenderer::without_spinner()
                .needs_status_during_compose()
        );
    }

    #[test]
    fn create_status_formats_status_without_spinner() {
        let projects = projects();
        let selected = vec!["app".to_string()];
        let running = BTreeMap::new();
        let statuses = BTreeMap::new();

        let output =
            create_status(None, &selected, &running, &statuses, &projects)
                .render(80);

        assert!(output.contains("app"));
        assert!(output.contains("(0/2)"));
    }
}
