use crate::docker::{DockerProjectMonitor, ProjectStatus};
use crate::{Project, TargetSelector};
use crossterm::terminal::Clear;
use crossterm::{
    cursor::{self, MoveUp},
    execute,
    style::{Color, Stylize},
};
use nirion_tui_lib::status::{Status, StatusEntry};
use std::collections::BTreeMap;
use std::io::stdout;
use std::io::Write;
use tokio::time::{sleep, Duration};

fn create_segments(status: &ProjectStatus, num_services: usize) -> Vec<Color> {
    let total = num_services.max(status.total());

    let healthy = status.healthy();
    let running = status.running() - healthy;
    let starting = status.starting();
    let exited = status.exited();
    let unhealthy = status.unhealthy();

    let mut segments = Vec::new();
    for _ in 0..healthy {
        segments.push(Color::Green);
    }
    for _ in 0..running {
        segments.push(Color::Yellow);
    }
    for _ in 0..starting {
        segments.push(Color::Cyan);
    }
    for _ in 0..unhealthy {
        segments.push(Color::Red);
    }
    for _ in 0..exited {
        segments.push(Color::DarkGrey);
    }
    for _ in segments.len()..total {
        segments.push(Color::Grey);
    }

    segments
}

pub async fn create_status(
    monitors: &BTreeMap<String, DockerProjectMonitor>,
    projects: &BTreeMap<String, Project>,
) -> anyhow::Result<Status> {
    let mut entries = Vec::new();

    for (name, monitor) in monitors {
        let project_status = monitor.project_status().await;
        let project = &projects[name];

        let running = project_status.running();
        let total = project_status.total();
        let healthy = project_status.healthy();

        let icon = if running == total && total > 0 && healthy == total {
            "✓".green().to_string()
        } else if running == total {
            "✓".yellow().to_string()
        } else if running > 0 {
            "↗".cyan().to_string()
        } else {
            "✗".red().to_string()
        };

        let prefix = format!("{icon} {name}");

        let num_services = project.services.len();
        let segments = create_segments(&project_status, num_services);
        let suffix = format!("({running}/{num_services})");

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
    projects: &BTreeMap<String, Project>,
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
    projects: &BTreeMap<String, Project>,
    refresh_interval: Duration,
) -> BTreeMap<String, DockerProjectMonitor> {
    let selected: Vec<String> = match target {
        TargetSelector::All => projects.keys().cloned().collect(),
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
