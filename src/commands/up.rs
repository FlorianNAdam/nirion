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

use crate::docker::{compose_target_cmd, DockerUpProcess, ProjectStatus};
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

impl ProjectStatus {
    pub fn summary_line(
        &self,
        num_services: usize,
        show_bar: bool,
        name_width: usize,
        width: usize,
    ) -> String {
        let bar = if show_bar {
            self.colored_progress_bar(num_services, width)
        } else {
            String::new()
        };

        let status_icon = if self.exited() > 0 {
            "✗".red().to_string()
        } else if self.running() == self.total()
            && self.healthy() == self.running()
        {
            "✓".green().to_string()
        } else if self.running() == self.total() {
            "✓".yellow().to_string()
        } else if self.starting() > 0 {
            "↗".cyan().to_string()
        } else {
            "⚠".yellow().to_string()
        };

        let healthy_str =
            if self.healthy() > 0 && self.healthy() < self.running() {
                format!(" ({} healthy)", self.healthy())
            } else if self.healthy() == self.running() && self.healthy() > 0 {
                " (all healthy)".to_string()
            } else {
                String::new()
            };

        format!(
            "{:name_width$}  {} {:2}/{}{} {}",
            self.name,
            bar,
            self.running(),
            num_services.max(self.total()),
            healthy_str,
            status_icon
        )
    }

    fn colored_progress_bar(
        &self,
        num_services: usize,
        width: usize,
    ) -> String {
        let display_width = width.max(10);
        let bar_width = display_width - 2;

        let total = num_services.max(self.total());
        if total == 0 {
            return format!("[{:^width$}]", "N/A", width = bar_width);
        }

        let healthy = self.healthy();
        let running = self.running() - self.healthy();
        let starting = self.starting();
        let exited = self.exited();

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
            let width = optimal_sublist_length(bar_width, total, i);

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
    let selected: Vec<String> = match &args.target {
        TargetSelector::All => projects.keys().cloned().collect(),
        TargetSelector::Project(p) => vec![p.name.clone()],
        TargetSelector::Image(img) => vec![img.project.clone()],
    };

    let mut map: BTreeMap<String, Arc<DockerUpProcess>> = BTreeMap::new();

    for name in &selected {
        let project = &projects[name];
        let proc = DockerUpProcess::create(name.clone(), project).await?;
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
        println!("{}  ┌{}┐", " ".repeat(max_name_width), "─".repeat(40));

        for (i, (name, proc)) in map.iter().enumerate() {
            proc.refresh_status().await?;
            let st = proc.project_status().await;
            let project = &projects[name];

            let line = st.summary_line(
                project.services.len(),
                !args.quiet,
                max_name_width,
                40,
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
                println!("{}  ├{}┤", " ".repeat(max_name_width), "─".repeat(40))
            }
        }

        println!("{}  └{}┘", " ".repeat(max_name_width), "─".repeat(40));

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
