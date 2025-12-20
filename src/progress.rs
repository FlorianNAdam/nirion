use crossterm::{
    cursor::{self, MoveToColumn, MoveUp},
    execute,
    style::{Color, Stylize},
    terminal::{Clear, ClearType},
};
use std::collections::BTreeMap;
use std::io::{stdout, Write};
use tokio::time::Duration;

use crate::docker::{DockerMonitoredProcess, ProjectStatus, ServiceState};
use crate::spinner::Spinner;
use crate::{Project, TargetSelector};

pub fn summary_line(
    status: &ProjectStatus,
    num_services: usize,
    name_width: usize,
    width: usize,
) -> String {
    let name = &status.name;

    let bar = colored_progress_bar(status, num_services, width);

    let status_icon = status_icon(status);

    let health_str = health_str(status);

    let running_services = status.running();
    let total_num_services = num_services.max(status.total());

    format!(
        "{name:name_width$}  {bar} {running_services:2}/{total_num_services}{health_str} {status_icon}",
    )
}

fn status_icon(status: &ProjectStatus) -> String {
    let healthy = status.healthy();
    let running = status.running();
    let total = status.total();

    if status.exited() > 0 {
        "✗".red().to_string()
    } else if running == total && healthy == running {
        "✓".green().to_string()
    } else if running == total {
        "✓".yellow().to_string()
    } else if status.starting() > 0 {
        "↗".cyan().to_string()
    } else {
        "⚠".yellow().to_string()
    }
}

fn health_str(status: &ProjectStatus) -> String {
    let healthy = status.healthy();
    let running = status.running();

    if healthy > 0 && healthy < running {
        format!(" ({} healthy)", healthy)
    } else if healthy == running && healthy > 0 {
        " (all healthy)".to_string()
    } else {
        String::new()
    }
}

fn colored_progress_bar(
    status: &ProjectStatus,
    num_services: usize,
    width: usize,
) -> String {
    let total = num_services.max(status.total());
    if total == 0 {
        return format!("[{:^width$}]", "N/A");
    }

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

    let mut out = String::from("│ ");
    for (i, color) in segments.iter().enumerate() {
        let width = optimal_sublist_length(width, total, i);

        out.push_str(
            "█"
                .repeat(width.saturating_sub(1))
                .with(*color)
                .to_string()
                .as_str(),
        );
        out.push_str("▊".with(*color).to_string().as_str());
    }
    out.push_str(" │");
    out
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

async fn print_progress(
    map: &BTreeMap<String, DockerMonitoredProcess>,
    projects: &BTreeMap<String, Project>,
    spinner: &Spinner,
) -> anyhow::Result<()> {
    let bar_width = 40;

    let max_name_width = projects
        .keys()
        .map(String::len)
        .max()
        .unwrap_or_default();

    let mut stdout = stdout();

    println!(
        "  {}  ┌{}┐",
        " ".repeat(max_name_width),
        "─".repeat(bar_width + 2)
    );

    for (i, (name, proc)) in map.iter().enumerate() {
        let st = proc.project_status().await;
        let project = &projects[name];

        let line = summary_line(
            &st,
            project.services.len(),
            max_name_width,
            bar_width,
        );

        let icon = if proc.finished().await {
            "✓".green().to_string()
        } else {
            spinner.get().yellow().to_string()
        };

        execute!(stdout, MoveToColumn(0))?;
        println!("{} {}", icon, line);

        if i != map.len().saturating_sub(1) {
            println!(
                "  {}  ├{}┤",
                " ".repeat(max_name_width),
                "─".repeat(bar_width + 2)
            )
        }
    }

    println!(
        "  {}  └{}┘",
        " ".repeat(max_name_width),
        "─".repeat(bar_width + 2)
    );

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
            print_progress(&map, projects, &spinner).await?;
            execute!(
                stdout,
                Clear(ClearType::CurrentLine),
                MoveUp((selected.len() * 2 + 1) as u16)
            )?;
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
        print_progress(&map, projects, &spinner).await?;
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
