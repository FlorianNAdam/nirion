use crossterm::style::Stylize;
use futures::StreamExt;
use nirion_lib::{
    compose::{ComposeConcurrency, compose_target},
    context::NirionContext,
    events::{ComposeEvent, ProcessEvent},
    projects::TargetSelector,
};

pub async fn compose_target_cmd(
    context: &NirionContext,
    target: &TargetSelector,
    args: &[&str],
) -> anyhow::Result<()> {
    let mut stream = compose_target(
        context.clone(),
        target.clone(),
        args.iter()
            .map(|arg| arg.to_string())
            .collect(),
        ComposeConcurrency::Sequential,
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
