use crossterm::{
    cursor::{self, MoveUp},
    execute,
    style::{Color, Stylize},
};
use nirion_lib::{projects::Projects, wait::healthchecks_finished};
use nirion_tui_lib::{
    spinner::Spinner,
    status::{Status, StatusEntry},
};
use std::collections::BTreeMap;
use std::io::{Write, stdout};
use tokio::time::{Duration, sleep};

use crate::TargetSelector;
use crate::docker::DockerMonitoredProcess;
use crate::status_display::{project_state_icon, project_status_segments};

async fn create_status(
    spinner: &Spinner,
    map: &BTreeMap<String, DockerMonitoredProcess>,
    projects: &Projects,
) -> anyhow::Result<Status> {
    let mut entries = Vec::new();

    for (name, proc) in map.iter() {
        let project_status = proc.project_status().await;
        let project = &projects[name];

        let icon = if proc.finished().await {
            project_state_icon(&project_status.project_state())
        } else {
            spinner.get().yellow().to_string()
        };

        let prefix = format!("{icon} {name}");

        let progressing = project_status.progressing();
        let num_services = project.services.len();
        let mut segments = project_status_segments(&project_status);

        for _ in 0..(num_services.saturating_sub(segments.len())) {
            segments.push(Color::Grey);
        }

        let suffix = format!("({progressing}/{num_services})    ");

        let entry = StatusEntry {
            prefix,
            segments,
            suffix,
        };
        entries.push(entry);
    }

    Ok(Status { entries })
}

async fn print_progress(
    map: &BTreeMap<String, DockerMonitoredProcess>,
    spinner: &Spinner,
    projects: &Projects,
) -> anyhow::Result<()> {
    let status = create_status(spinner, map, projects).await?;
    status.print()?;
    Ok(())
}

pub async fn run_command_with_progress(
    target: &TargetSelector,
    projects: &Projects,
    args: &[&str],
    no_monitor: bool,
    quiet: bool,
    refresh_interval: Duration,
    wait_for_healthchecks: bool,
) -> anyhow::Result<()> {
    let selected: Vec<String> = match target {
        TargetSelector::All => projects
            .iter()
            .map(|(n, _)| n.to_string())
            .collect(),
        TargetSelector::Project(p) => vec![p.name.clone()],
        TargetSelector::Service(img) => vec![img.project.clone()],
    };

    let mut map = BTreeMap::new();

    for name in &selected {
        let project = &projects[name];
        let proc = DockerMonitoredProcess::new(
            project.clone(),
            refresh_interval,
            args,
        )
        .await;

        map.insert(name.clone(), proc);
    }

    if no_monitor {
        wait_for_processes(&map, refresh_interval).await;
        return fail_on_process_errors(&map).await;
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

        if finished {
            fail_on_process_errors(&map).await?;
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

        if !finished {
            sleep(refresh_interval).await;
        }
    }

    for proj in map.values() {
        proj.refresh_status().await?;
    }

    if !quiet {
        print_progress(&map, &spinner, &projects).await?;
        stdout.flush()?;
    }

    fail_on_process_errors(&map).await
}

async fn wait_for_processes(
    map: &BTreeMap<String, DockerMonitoredProcess>,
    refresh_interval: Duration,
) {
    loop {
        let mut finished = true;
        for project in map.values() {
            if !project.finished().await {
                finished = false;
                break;
            }
        }

        if finished {
            break;
        }

        sleep(refresh_interval).await;
    }
}

async fn fail_on_process_errors(
    map: &BTreeMap<String, DockerMonitoredProcess>,
) -> anyhow::Result<()> {
    let mut failures = Vec::new();

    for (project_name, process) in map {
        if let Some(error) = process.error().await {
            failures.push(format!("{project_name}: {error}"));
        }
    }

    if !failures.is_empty() {
        anyhow::bail!(
            "docker compose failed for {} project(s): {}",
            failures.len(),
            failures.join("; ")
        );
    }

    Ok(())
}
