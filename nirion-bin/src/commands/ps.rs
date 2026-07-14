use anyhow::Result;
use clap::Parser;
use crossterm::style::Stylize;
use nirion_lib::docker::{
    query_project_status_with_docker, Port, ServiceStatus,
};
use nirion_tui_lib::table::print_table;
use std::collections::HashSet;

use crate::{
    commands::NirionContext, docker::compose_target_cmd, ClapSelector, Project,
    TargetSelector,
};

//
// ===== CLI =====
//

/// List running service containers
#[derive(Parser, Debug, Clone)]
pub struct PsArgs {
    /// Target selector: *, project, or project.service
    #[arg(
        default_value = "*",
        value_parser = TargetSelector::clap_parse,
        add = TargetSelector::clap_completer()
    )]
    pub target: TargetSelector,

    /// Disable TUI table output and use docker compose ps directly
    #[arg(long, alias = "no-tui")]
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

pub async fn handle_ps(args: &PsArgs, context: &NirionContext) -> Result<()> {
    if args.legacy {
        legacy_ps(args, context).await
    } else {
        fancy_ps(args, context).await
    }
}

async fn legacy_ps(args: &PsArgs, context: &NirionContext) -> Result<()> {
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

    compose_target_cmd(
        &context.docker_binary,
        &args.target,
        &context.projects,
        &cmd_slices,
    )
    .await
}

async fn fancy_ps(args: &PsArgs, context: &NirionContext) -> Result<()> {
    let mut rows = vec![];

    match &args.target {
        TargetSelector::All => {
            for (project_name, project) in context.projects.iter() {
                rows.extend(
                    print_project_status(
                        &context.docker_binary,
                        project_name,
                        project,
                    )
                    .await?,
                );
            }
        }

        TargetSelector::Project(sel) => {
            if let Some(project) = context.projects.get(&sel.name) {
                rows.extend(
                    print_project_status(
                        &context.docker_binary,
                        &sel.name,
                        project,
                    )
                    .await?,
                );
            }
        }

        TargetSelector::Service(sel) => {
            if let Some(project) = context.projects.get(&sel.project) {
                let project_name = &project.name;
                let status = query_project_status_with_docker(
                    &context.docker_binary,
                    &project.docker_compose,
                    &project_name,
                )
                .await?;

                if let Some(svc) = status.services.get(&sel.service) {
                    rows.push(print_header(&sel.project));
                    rows.push(print_row(svc)?);
                }
            }
        }
    }

    print_table(rows);
    Ok(())
}

async fn print_project_status(
    docker_binary: &std::path::Path,
    project_name: &str,
    project: &Project,
) -> anyhow::Result<Vec<String>> {
    let mut rows = vec![];

    rows.push(print_header(project_name));

    let project_name = &project.name;
    let status = query_project_status_with_docker(
        docker_binary,
        &project.docker_compose,
        project_name,
    )
    .await?;

    for svc in status.services.values() {
        rows.push(print_row(svc)?);
    }

    rows.push(String::new());

    Ok(rows)
}

fn print_header(project_name: &str) -> String {
    format!(
        "[{}]\t{}\t{}\t{}",
        project_name.cyan(),
        "created".blue(),
        "status".blue(),
        "ports".blue()
    )
}

fn print_row(svc: &ServiceStatus) -> anyhow::Result<String> {
    let unhealthy_token = "PS_REPLACE_TOKEN1";
    let healthy_token = "PS_REPLACE_TOKEN2";

    let running_for = svc.running_for.as_deref().unwrap_or("");
    let status = svc
        .status
        .as_deref()
        .unwrap_or("")
        .replace("unhealthy", unhealthy_token)
        .replace("healthy", healthy_token)
        .replace(healthy_token, &"healthy".green().to_string())
        .replace(unhealthy_token, &"unhealthy".red().to_string());

    let port_strs = collapsed_ports(&svc.ports)
        .into_iter()
        .collect::<HashSet<_>>();
    let mut port_strs = port_strs
        .into_iter()
        .collect::<Vec<_>>();
    port_strs.sort_unstable();
    let port_str = port_strs.join(", ");

    Ok(format!(
        " - {}\t{}\t{}\t{}",
        svc.container_name, running_for, status, port_str
    ))
}

fn collapsed_ports(ports: &[Port]) -> Vec<String> {
    let mut ports = ports.iter().collect::<Vec<_>>();
    ports.sort_by_key(|p| {
        (
            p.proto.as_str(),
            p.external.as_ref().map(|e| e.port),
            p.port,
        )
    });

    let mut collapsed = Vec::new();
    let mut i = 0;

    while i < ports.len() {
        let start = ports[i];
        let mut end = start;
        let mut next = i + 1;

        while next < ports.len()
            && ports[next].proto == start.proto
            && consecutive_external(end, ports[next])
            && ports[next].port == end.port + 1
        {
            end = ports[next];
            next += 1;
        }

        collapsed.push(format_port_range(start, end));
        i = next;
    }

    collapsed
}

fn consecutive_external(previous: &Port, next: &Port) -> bool {
    match (&previous.external, &next.external) {
        (None, None) => true,
        (Some(previous), Some(next)) => next.port == previous.port + 1,
        _ => false,
    }
}

fn format_port_range(start: &Port, end: &Port) -> String {
    let internal = if start.port == end.port {
        start.port.to_string()
    } else {
        format!("{}-{}", start.port, end.port)
    };

    let prefix = match (&start.external, &end.external) {
        (Some(start), Some(end)) if start.port == end.port => {
            format!("{}->", start.port)
        }
        (Some(start), Some(end)) => format!("{}-{}->", start.port, end.port),
        _ => String::new(),
    };

    format!("{}{} /{}", prefix, internal, start.proto).replace(" /", "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use console::strip_ansi_codes;
    use nirion_lib::docker::{ExternalPort, ServiceState};

    fn port(port: u16, proto: &str) -> Port {
        Port {
            external: None,
            port,
            proto: proto.to_string(),
        }
    }

    fn mapped_port(external: u16, internal: u16, proto: &str) -> Port {
        Port {
            external: Some(ExternalPort {
                ip: "127.0.0.1".to_string(),
                port: external,
            }),
            port: internal,
            proto: proto.to_string(),
        }
    }

    fn service_status(status: Option<&str>, ports: Vec<Port>) -> ServiceStatus {
        ServiceStatus {
            id: "id".to_string(),
            service: "web".to_string(),
            container_name: "web-1".to_string(),
            image: "image".to_string(),
            state: ServiceState::Running,
            health: None,
            exit_code: None,
            running_for: Some("2 minutes".to_string()),
            status: status.map(str::to_string),
            ports,
            networks: Vec::new(),
        }
    }

    #[test]
    fn collapsed_ports_collapses_consecutive_internal_ports() {
        assert_eq!(
            collapsed_ports(&[
                port(81, "tcp"),
                port(80, "tcp"),
                port(82, "tcp")
            ]),
            vec!["80-82/tcp"]
        );
    }

    #[test]
    fn collapsed_ports_collapses_consecutive_external_mappings() {
        assert_eq!(
            collapsed_ports(&[
                mapped_port(8081, 81, "tcp"),
                mapped_port(8080, 80, "tcp"),
            ]),
            vec!["8080-8081->80-81/tcp"]
        );
    }

    #[test]
    fn collapsed_ports_keeps_protocols_separate() {
        assert_eq!(
            collapsed_ports(&[port(80, "udp"), port(80, "tcp")]),
            vec!["80/tcp", "80/udp"]
        );
    }

    #[test]
    fn collapsed_ports_splits_internal_ranges_at_gaps() {
        assert_eq!(
            collapsed_ports(&[
                port(80, "tcp"),
                port(81, "tcp"),
                port(83, "tcp"),
                port(84, "tcp"),
            ]),
            vec!["80-81/tcp", "83-84/tcp"]
        );
    }

    #[test]
    fn collapsed_ports_does_not_merge_mapped_and_unmapped_ports() {
        assert_eq!(
            collapsed_ports(&[mapped_port(8080, 80, "tcp"), port(81, "tcp"),]),
            vec!["81/tcp", "8080->80/tcp"]
        );
    }

    #[test]
    fn collapsed_ports_requires_consecutive_external_ports() {
        assert_eq!(
            collapsed_ports(&[
                mapped_port(8080, 80, "tcp"),
                mapped_port(8082, 81, "tcp"),
            ]),
            vec!["8080->80/tcp", "8082->81/tcp"]
        );
    }

    #[test]
    fn collapsed_ports_requires_consecutive_internal_ports() {
        assert_eq!(
            collapsed_ports(&[
                mapped_port(8080, 80, "tcp"),
                mapped_port(8081, 82, "tcp"),
            ]),
            vec!["8080->80/tcp", "8081->82/tcp"]
        );
    }

    #[test]
    fn print_row_deduplicates_rendered_ports() {
        let row = print_row(&service_status(
            Some("running"),
            vec![port(80, "tcp"), port(80, "tcp")],
        ))
        .unwrap();

        assert_eq!(
            strip_ansi_codes(&row),
            " - web-1\t2 minutes\trunning\t80/tcp"
        );
    }

    #[test]
    fn print_row_colors_healthy_and_unhealthy_independently() {
        let row = print_row(&service_status(
            Some("running (healthy), running (unhealthy)"),
            vec![],
        ))
        .unwrap();

        assert_eq!(
            strip_ansi_codes(&row),
            " - web-1\t2 minutes\trunning (healthy), running (unhealthy)\t"
        );
        assert!(row.contains("healthy"));
        assert!(row.contains("unhealthy"));
    }
}
