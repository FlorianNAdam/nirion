use crate::TargetSelector;
use crossterm::terminal::Clear;
use crossterm::{
    cursor::{self, MoveUp},
    execute,
    style::Color,
};
use futures::StreamExt;
use nirion_lib::{
    docker::{ProjectStatus, project_status_stream},
    projects::{Projects, selected_project_names},
};
use nirion_tui_lib::status::{Status, StatusEntry};
use std::collections::BTreeMap;
use std::io::Write;
use std::io::stdout;
use tokio::time::Duration;

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

pub async fn create_status(
    statuses: &BTreeMap<String, ProjectStatus>,
    selected: &[String],
    projects: &Projects,
) -> anyhow::Result<Status> {
    let mut entries = Vec::new();

    for name in selected {
        let project_status = statuses
            .get(name)
            .cloned()
            .unwrap_or_else(|| ProjectStatus {
                services: BTreeMap::new(),
            });
        let project = &projects[name];

        let state = project_status.project_state();
        let icon = project_state_icon(&state);

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

    Ok(Status { entries })
}

pub async fn monitor(
    target: &TargetSelector,
    projects: &Projects,
    refresh_interval: Duration,
) -> anyhow::Result<()> {
    let selected = selected_project_names(target, projects);
    let mut statuses = BTreeMap::new();
    let mut stream = project_status_stream(
        target.clone(),
        projects.clone(),
        refresh_interval,
    );
    let _cursor = CursorGuard::hide()?;

    render_status(&statuses, &selected, projects, true).await?;

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            event = stream.next() => {
                let Some(event) = event else {
                    break;
                };
                let event = event?;
                statuses.insert(event.project, event.status);
                render_status(&statuses, &selected, projects, true).await?;
            }
        }
    }

    let mut stdout = stdout();
    execute!(stdout, Clear(crossterm::terminal::ClearType::CurrentLine))?;

    render_status(&statuses, &selected, projects, false).await?;

    Ok(())
}

async fn render_status(
    statuses: &BTreeMap<String, ProjectStatus>,
    selected: &[String],
    projects: &Projects,
    move_up: bool,
) -> anyhow::Result<()> {
    let mut stdout = stdout();
    let status: Status = create_status(statuses, selected, projects).await?;
    status.print()?;

    if move_up {
        execute!(stdout, MoveUp((selected.len() * 2 + 1) as u16))?;
    }
    stdout.flush()?;
    Ok(())
}
