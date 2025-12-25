use crossterm::{
    cursor::{self, MoveToColumn, MoveUp},
    execute,
    style::{Color, Stylize},
};
use std::collections::BTreeMap;
use std::io::{stdout, Write};
use tokio::time::Duration;

use crate::docker::{DockerMonitoredProcess, ProjectStatus, ServiceState};
use crate::spinner::Spinner;
use crate::util::{ansi_len, lpad_ansi};
use crate::{Project, TargetSelector};

struct Status {
    entries: Vec<StatusEntry>,
}

struct StatusEntry {
    prefix: String,
    segments: Vec<Color>,
    suffix: String,
}

fn print_status(status: Status) -> anyhow::Result<()> {
    let bar_width = 40;

    let max_prefix_length = status
        .entries
        .iter()
        .map(|e| ansi_len(&e.prefix))
        .max()
        .unwrap_or_default();

    let mut stdout = stdout();

    println!(
        "{} ┌{}┐",
        " ".repeat(max_prefix_length),
        "─".repeat(bar_width + 2)
    );

    let num_entries = status.entries.len();
    for (i, entry) in status.entries.into_iter().enumerate() {
        let line = render_status_line(entry, max_prefix_length, bar_width);

        execute!(stdout, MoveToColumn(0))?;
        println!("{}", line);

        if i != num_entries.saturating_sub(1) {
            println!(
                "{} ├{}┤",
                " ".repeat(max_prefix_length),
                "─".repeat(bar_width + 2)
            )
        }
    }

    println!(
        "{} └{}┘",
        " ".repeat(max_prefix_length),
        "─".repeat(bar_width + 2)
    );
    Ok(())
}

fn render_status_line(
    entry: StatusEntry,
    max_prefix_width: usize,
    bar_width: usize,
) -> String {
    let prefix = lpad_ansi(&entry.prefix, max_prefix_width);
    let bar = render_status_bar(entry.segments, bar_width);
    let suffix = entry.suffix;

    format!("{prefix} │ {bar} │ {suffix}")
}

fn render_status_bar(segments: Vec<Color>, width: usize) -> String {
    if segments.len() == 0 {
        return " ".repeat(width);
    }

    let mut out = String::new();
    for (i, color) in segments.iter().enumerate() {
        let width = optimal_sublist_length(width, segments.len(), i);

        out.push_str(
            "█"
                .repeat(width.saturating_sub(1))
                .with(*color)
                .to_string()
                .as_str(),
        );
        out.push_str("▊".with(*color).to_string().as_str());
    }
    out
}

fn create_segments(status: &ProjectStatus, num_services: usize) -> Vec<Color> {
    let total = num_services.max(status.total());

    let healthy = status.healthy();
    let running = status.running() - status.healthy();
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

fn optimal_sublist_length(width: usize, n: usize, i: usize) -> usize {
    if n == 0 {
        return 0;
    }
    let base = width / n;
    let extra = width % n;

    if i < extra {
        base + 1
    } else {
        base
    }
}

async fn create_status(
    spinner: &Spinner,
    map: &BTreeMap<String, DockerMonitoredProcess>,
    projects: &BTreeMap<String, Project>,
) -> anyhow::Result<Status> {
    let mut entries = Vec::new();

    for (name, proc) in map.iter() {
        let project_status = proc.project_status().await;
        let project = &projects[name];

        let icon = if proc.finished().await {
            "✓".green().to_string()
        } else {
            spinner.get().yellow().to_string()
        };

        let name = &project_status.name;

        let prefix = format!("{icon} {name}");

        let num_running = project_status.running();
        let num_services = project.services.len();
        let segments = create_segments(&project_status, num_services);

        let suffix = format!("({num_running}/{num_services})  ");

        let entry = StatusEntry {
            prefix,
            segments,
            suffix,
        };
        entries.push(entry);
    }

    return Ok(Status { entries });
}

async fn print_progress(
    map: &BTreeMap<String, DockerMonitoredProcess>,
    spinner: &Spinner,
    projects: &BTreeMap<String, Project>,
) -> anyhow::Result<()> {
    let status = create_status(spinner, map, projects).await?;
    print_status(status)?;
    Ok(())
}

pub async fn run_command_with_progress(
    target: &TargetSelector,
    projects: &BTreeMap<String, Project>,
    args: &[&str],
    no_monitor: bool,
    quiet: bool,
    refresh_interval: Duration,
    wait_for_healthchecks: bool,
) -> anyhow::Result<()> {
    let selected: Vec<String> = match target {
        TargetSelector::All => projects.keys().cloned().collect(),
        TargetSelector::Project(p) => vec![p.name.clone()],
        TargetSelector::Service(img) => vec![img.project.clone()],
    };

    let mut map = BTreeMap::new();

    for name in &selected {
        let project = &projects[name];
        let proc = DockerMonitoredProcess::new(
            name.clone(),
            project.clone(),
            refresh_interval,
            args,
        )
        .await;

        map.insert(name.clone(), proc);
    }

    if no_monitor {
        return Ok(());
    }

    let mut stdout = stdout();
    execute!(stdout, cursor::Hide)?;

    let spinner = Spinner::default();

    let mut finished = false;
    while !finished {
        if !quiet {
            print_progress(&map, &spinner, &projects).await?;
            execute!(stdout, MoveUp((selected.len() * 2 + 1) as u16))?;
            stdout.flush()?;
        }

        finished = true;
        for project in map.values() {
            if !project.finished().await {
                finished = false;
                break;
            }
        }

        let mut statuses = BTreeMap::new();
        for (project_name, status) in map.iter() {
            statuses.insert(
                project_name.to_string(),
                status.project_status().await.clone(),
            );
        }
        if wait_for_healthchecks
            && !healthchecks_finished(target, projects, &statuses)
        {
            finished = false;
        }
    }

    for proj in map.values() {
        proj.refresh_status().await?;
    }

    if !quiet {
        print_progress(&map, &spinner, &projects).await?;
        stdout.flush()?;
    }

    Ok(())
}

pub fn healthchecks_finished(
    target: &TargetSelector,
    projects: &BTreeMap<String, Project>,
    statuses: &BTreeMap<String, ProjectStatus>,
) -> bool {
    let project_names: Vec<&str> = match target {
        TargetSelector::All => projects
            .keys()
            .map(String::as_str)
            .collect(),
        TargetSelector::Project(p) => vec![p.name.as_str()],
        TargetSelector::Service(s) => vec![s.project.as_str()],
    };

    for project_name in project_names {
        let project = match projects.get(project_name) {
            Some(p) => p,
            None => continue,
        };

        let status = match statuses.get(project_name) {
            Some(s) => s,
            None => return false,
        };

        for (service_name, service) in &project.services {
            if let TargetSelector::Service(sel) = target {
                if sel.project == project_name && sel.service != *service_name {
                    continue;
                }
            }

            if service.healthcheck.is_none() {
                continue;
            }

            let Some(service_status) = status.services.get(service_name) else {
                return false;
            };

            match service_status.state {
                ServiceState::Healthy | ServiceState::Unhealthy => {}
                _ => return false,
            }
        }
    }

    true
}
