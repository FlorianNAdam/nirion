use crossterm::{
    cursor::{self, MoveToColumn, MoveUp},
    execute,
    style::{Color, Stylize},
    terminal::{Clear, ClearType},
};
use std::collections::BTreeMap;
use std::io::{stdout, Write};
use std::sync::Arc;
use tokio::time::{sleep, Duration};

use crate::docker::{DockerMonitoredProcess, ProjectStatus};
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
    for _ in 0..exited {
        segments.push(Color::Red);
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
    map: &BTreeMap<String, Arc<DockerMonitoredProcess>>,
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
    refresh: Duration,
) -> anyhow::Result<()> {
    let selected: Vec<String> = match target {
        TargetSelector::All => projects.keys().cloned().collect(),
        TargetSelector::Project(p) => vec![p.name.clone()],
        TargetSelector::Image(img) => vec![img.project.clone()],
    };

    let mut map = BTreeMap::new();

    for name in &selected {
        let project = &projects[name];
        let proc = DockerMonitoredProcess::new(name.clone(), project)
            .args(args)
            .build()
            .await?;

        map.insert(name.clone(), proc);
    }

    if no_monitor {
        return Ok(());
    }

    let mut stdout = stdout();
    execute!(stdout, cursor::Hide)?;

    let project_count = selected.len();

    let spinner = Spinner::default();

    let _ = {
        let map = map.clone();
        let refresh = refresh.clone();
        tokio::spawn(async move {
            loop {
                for proc in map.values() {
                    let _ = proc.refresh_status().await;
                    sleep(refresh).await;
                }
            }
        })
    };

    let mut finished = false;
    while !finished {
        if !quiet {
            print_progress(&map, projects, &spinner).await?;
            execute!(
                stdout,
                Clear(ClearType::CurrentLine),
                MoveUp((project_count * 2 + 1) as u16)
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
