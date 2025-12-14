use anyhow::Result;
use clap::Parser;
use std::{
    collections::{BTreeMap, HashSet},
    path::Path,
};

use crate::{
    docker::compose_target_cmd, docker::query_project_status, Project,
    TargetSelector,
};

//
// ===== CLI =====
//

#[derive(Parser, Debug, Clone)]
pub struct PsArgs {
    /// Target selector: *, project, or project.service
    #[arg(default_value = "*", value_parser = crate::clap_parse_selector)]
    pub target: TargetSelector,

    /// Use legacy docker compose ps implementation
    #[arg(long)]
    pub legacy: bool,

    /// Show all containers (including stopped ones)
    #[arg(short = 'a', long)]
    pub all: bool,

    /// Filter services by a property (currently only 'status')
    #[arg(long)]
    pub filter: Option<String>,

    /// Format output (table, json, Go template)
    #[arg(short = 'f', long)]
    pub format: Option<String>,

    /// Short format
    #[arg(short = 's', long, conflicts_with = "format")]
    pub short: bool,

    /// Don't truncate output
    #[arg(long)]
    pub no_trunc: bool,

    /// Include orphaned services
    #[arg(long)]
    pub orphans: Option<bool>,

    /// Only display container IDs
    #[arg(short = 'q', long)]
    pub quiet: bool,

    /// Display services
    #[arg(long)]
    pub services: bool,

    /// Filter by status (can be repeated)
    #[arg(long)]
    pub status: Vec<String>,
}

pub async fn handle_ps(
    args: &PsArgs,
    projects: &BTreeMap<String, Project>,
    locked_images: &BTreeMap<String, String>,
    lock_file: &Path,
) -> Result<()> {
    if args.legacy {
        legacy_ps(args, projects, locked_images, lock_file).await
    } else {
        fancy_ps(args, projects).await
    }
}

async fn legacy_ps(
    args: &PsArgs,
    projects: &BTreeMap<String, Project>,
    _locked_images: &BTreeMap<String, String>,
    _lock_file: &Path,
) -> Result<()> {
    let mut cmd_args: Vec<String> = vec!["ps".into()];

    if args.all {
        cmd_args.push("--all".into());
    }

    if let Some(filter) = &args.filter {
        cmd_args.push("--filter".into());
        cmd_args.push(filter.clone());
    }

    if let Some(format) = &args.format {
        cmd_args.push("--format".into());
        cmd_args.push(format.clone());
    } else if args.short {
        cmd_args.push("--format".into());
        cmd_args.push(
            "table{{.Name}}\t{{.RunningFor}}\t{{.Status}}\t{{.Ports}}"
                .to_string(),
        );
    }

    if args.no_trunc {
        cmd_args.push("--no-trunc".into());
    }

    if let Some(orphans) = args.orphans {
        cmd_args.push(format!("--orphans={orphans}"));
    }

    if args.quiet {
        cmd_args.push("--quiet".into());
    }

    if args.services {
        cmd_args.push("--services".into());
    }

    for s in &args.status {
        cmd_args.push("--status".into());
        cmd_args.push(s.clone());
    }

    let cmd_slices: Vec<&str> = cmd_args
        .iter()
        .map(|s| s.as_str())
        .collect();

    compose_target_cmd(&args.target, projects, &cmd_slices).await
}

async fn fancy_ps(
    _args: &PsArgs,
    projects: &BTreeMap<String, Project>,
) -> Result<()> {
    print_header();

    for (project_name, project) in projects {
        let status =
            query_project_status(&project.docker_compose, project_name).await?;

        for svc in status.services.values() {
            print_row(svc);
        }
    }

    Ok(())
}

fn print_header() {
    println!(
        "{:<32} {:<14} {:<28} {}",
        "NAME", "RUNNING FOR", "STATUS", "PORTS"
    );
}

fn print_row(svc: &crate::docker::ServiceStatus) {
    let running_for = svc.running_for.as_deref().unwrap_or("");
    let status = svc.status.as_deref().unwrap_or("");

    let port_strs = svc
        .ports
        .iter()
        .map(|p| {
            let prefix = if let Some(external) = &p.external {
                format!("{}->", external.port)
            } else {
                String::new()
            };

            format!("{}{}/{}", prefix, p.port, p.proto)
        })
        .collect::<HashSet<_>>();
    let mut port_strs = port_strs
        .into_iter()
        .collect::<Vec<_>>();
    port_strs.sort_unstable();
    let port_str = port_strs.join(", ");

    println!(
        "{:<32} {:<14} {:<28} {}",
        svc.container_name,
        truncate(running_for, 14),
        truncate(status, 28),
        port_str
    );
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max.saturating_sub(1)])
    }
}
