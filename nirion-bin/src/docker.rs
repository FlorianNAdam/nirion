use crossterm::style::Stylize;
use futures::StreamExt;
use nirion_lib::{
    events::{ComposeEvent, ProcessEvent},
    projects::{Projects, TargetSelector},
};
use std::path::Path;

pub async fn compose_target_cmd(
    docker_binary: &Path,
    target: &TargetSelector,
    projects: &Projects,
    args: &[&str],
) -> anyhow::Result<()> {
    let mut stream = nirion_lib::compose::compose_target_with_docker(
        docker_binary.to_path_buf(),
        target.clone(),
        projects.clone(),
        args.iter()
            .map(|arg| arg.to_string())
            .collect(),
    );

    while let Some(event) = stream.next().await {
        render_compose_event(event?);
    }

    Ok(())
}

fn render_compose_event(event: ComposeEvent) {
    match event {
        ComposeEvent::ProjectStarted { project } => {
            println!("[{}]", project.cyan());
        }
        ComposeEvent::Process { event, .. } => render_process_event(event),
        ComposeEvent::ProjectFailed { project, error } => {
            eprintln!("Project '{}' failed: {}", project, error);
            println!();
        }
    }
}

fn render_process_event(event: ProcessEvent) {
    match event {
        ProcessEvent::StdoutLine(line) => println!("{}", line),
        ProcessEvent::StderrLine(line) => {
            if !line.contains("the attribute `version` is obsolete") {
                println!("{}", line);
            }
        }
        ProcessEvent::Exited(_) => {}
    }
}
