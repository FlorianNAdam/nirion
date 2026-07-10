use crossterm::{
    cursor::{self, MoveUp},
    execute,
    style::{Color, Stylize},
};
use nirion_lib::projects::Projects;
use nirion_tui_lib::{
    spinner::Spinner,
    status::{Status, StatusEntry},
};
use std::collections::BTreeMap;
use std::io::{Write, stdout};
use tokio::time::{Duration, sleep};

use crate::TargetSelector;
use crate::docker::{DockerMonitoredProcess, ProjectStatus, ServiceState};

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
            let project_state = project_status.project_state();
            project_state.icon()
        } else {
            spinner.get().yellow().to_string()
        };

        let prefix = format!("{icon} {name}");

        let progressing = project_status.progressing();
        let num_services = project.services.len();
        let mut segments = project_status.segments();

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

pub fn healthchecks_finished(
    target: &TargetSelector,
    projects: &Projects,
    statuses: &BTreeMap<String, ProjectStatus>,
) -> bool {
    let project_names: Vec<&str> = match target {
        TargetSelector::All => projects
            .iter()
            .map(|(n, _)| n)
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

            if !service.healthcheck {
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
