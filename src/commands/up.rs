use anyhow::Result;
use clap::Parser;
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

use crate::docker::{
    compose_target_cmd, DockerMonitoredProcess, ProjectStatus,
};
use crate::{Project, TargetSelector};

#[derive(Parser, Debug, Clone)]
pub struct UpArgs {
    #[arg(default_value = "*", value_parser = crate::clap_parse_selector)]
    pub target: TargetSelector,

    #[arg(long)]
    pub no_monitor: bool,

    #[arg(short = 'r', long, default_value = "1")]
    pub refresh: u64,

    #[arg(short = 'm', long, default_value = "15")]
    pub max_display: usize,

    #[arg(short, long)]
    pub quiet: bool,

    #[arg(short, long)]
    pub boring: bool,
}

pub fn summary_line(
    status: &ProjectStatus,
    num_services: usize,
    show_bar: bool,
    name_width: usize,
    width: usize,
) -> String {
    let name = &status.name;

    let bar = if show_bar {
        colored_progress_bar(status, num_services, width)
    } else {
        String::new()
    };

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

pub async fn handle_up(
    args: &UpArgs,
    projects: &BTreeMap<String, Project>,
) -> Result<()> {
    if !args.boring && !matches!(args.target, TargetSelector::Image(_)) {
        fancy_up(args, projects).await?;
    } else {
        compose_target_cmd(&args.target, projects, &["up", "-d"])?;
    }
    Ok(())
}

async fn fancy_up(
    args: &UpArgs,
    projects: &BTreeMap<String, Project>,
) -> anyhow::Result<()> {
    let bar_width = 40;

    let selected: Vec<String> = match &args.target {
        TargetSelector::All => projects.keys().cloned().collect(),
        TargetSelector::Project(p) => vec![p.name.clone()],
        TargetSelector::Image(img) => vec![img.project.clone()],
    };

    let mut map: BTreeMap<String, Arc<DockerMonitoredProcess>> =
        BTreeMap::new();

    for name in &selected {
        let project = &projects[name];
        let proc = DockerMonitoredProcess::new(name.clone(), project)
            .arg("up")
            .arg("-d")
            .build()
            .await?;

        map.insert(name.clone(), proc);
    }

    if args.no_monitor {
        return Ok(());
    }

    let mut stdout = stdout();
    execute!(stdout, cursor::Hide)?;

    let project_count = selected.len();

    let max_name_width = projects
        .keys()
        .map(String::len)
        .max()
        .unwrap_or_default();

    loop {
        println!(
            "{}  ┌{}┐",
            " ".repeat(max_name_width),
            "─".repeat(bar_width + 2)
        );

        for (i, (name, proc)) in map.iter().enumerate() {
            proc.refresh_status().await?;
            let st = proc.project_status().await;
            let project = &projects[name];

            let line = summary_line(
                &st,
                project.services.len(),
                !args.quiet,
                max_name_width,
                bar_width,
            );

            if !args.quiet {
                execute!(
                    stdout,
                    Clear(ClearType::CurrentLine),
                    MoveToColumn(0)
                )?;
                println!("{}", line);
            }

            if i != map.len().saturating_sub(1) {
                println!(
                    "{}  ├{}┤",
                    " ".repeat(max_name_width),
                    "─".repeat(bar_width + 2)
                )
            }
        }

        println!(
            "{}  └{}┘",
            " ".repeat(max_name_width),
            "─".repeat(bar_width + 2)
        );

        let mut all_done = true;
        for project in map.values() {
            if !project.finished().await {
                all_done = false;
                break;
            }
        }
        if all_done {
            stdout.flush()?;
            break;
        } else {
            if !args.quiet {
                execute!(stdout, MoveUp((project_count * 2 + 1) as u16))?;
            }
            stdout.flush()?;
            sleep(Duration::from_secs(args.refresh)).await;
        }
    }

    Ok(())
}
