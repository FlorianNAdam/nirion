use crate::docker::DockerProjectMonitor;
use crate::TargetSelector;
use crossterm::terminal::Clear;
use crossterm::{
    cursor::{self, MoveUp},
    execute,
    style::Color,
};
use nirion_lib::projects::Projects;
use nirion_tui_lib::status::{Status, StatusEntry};
use std::collections::BTreeMap;
use std::io::stdout;
use std::io::Write;
use tokio::time::{sleep, Duration};

pub async fn create_status(
    monitors: &BTreeMap<String, DockerProjectMonitor>,
    projects: &Projects,
) -> anyhow::Result<Status> {
    let mut entries = Vec::new();

    for (name, monitor) in monitors {
        let project_status = monitor.project_status().await;
        let project = &projects[name];

        let state = project_status.project_state();
        let icon = state.icon();

        let prefix = format!("{icon} {name}");

        let progressing = project_status.progressing();
        let num_services = project.services.len();
        let mut segments = project_status.segments();

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
    monitors: &BTreeMap<String, DockerProjectMonitor>,
    projects: &Projects,
) -> anyhow::Result<()> {
    let mut stdout = stdout();
    execute!(stdout, cursor::Hide)?;

    let ctrl_c = tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
    });

    loop {
        let status: Status = create_status(monitors, projects).await?;
        status.print()?;

        execute!(stdout, MoveUp((monitors.len() * 2 + 1) as u16))?;
        stdout.flush()?;

        if ctrl_c.is_finished() {
            break;
        }

        sleep(Duration::from_millis(100)).await;
    }
    execute!(stdout, Clear(crossterm::terminal::ClearType::CurrentLine))?;

    let status: Status = create_status(monitors, projects).await?;
    status.print()?;

    execute!(stdout, cursor::Show)?;
    stdout.flush()?;

    Ok(())
}

pub async fn create_monitors(
    target: &TargetSelector,
    projects: &Projects,
    refresh_interval: Duration,
) -> BTreeMap<String, DockerProjectMonitor> {
    let selected: Vec<String> = match target {
        TargetSelector::All => projects
            .iter()
            .map(|(n, _)| n.to_string())
            .collect(),
        TargetSelector::Project(p) => vec![p.name.clone()],
        TargetSelector::Service(s) => vec![s.project.clone()],
    };

    let mut monitors = BTreeMap::new();

    for name in selected {
        if let Some(project) = projects.get(&name) {
            let monitor = DockerProjectMonitor::new(project, refresh_interval);
            monitors.insert(name, monitor);
        }
    }

    monitors
}
