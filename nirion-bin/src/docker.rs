use futures::StreamExt;
use nirion_lib::{
    backend::OperationEventStream,
    compose::{ComposeConcurrency, compose_stream},
    context::NirionContext,
    events::{ComposeEvent, ProcessEvent},
    projects::TargetSelector,
};
use nirion_tui_lib::color::Colorize;

pub async fn compose_target_cmd(
    context: &NirionContext,
    target: &TargetSelector,
    args: &[&str],
) -> anyhow::Result<()> {
    let mut stream = compose_stream(
        context.clone(),
        target.clone(),
        args.iter()
            .map(|arg| arg.to_string())
            .collect(),
        ComposeConcurrency::sequential(),
    );

    while let Some(event) = stream.next().await {
        render_compose_event(event?);
    }

    Ok(())
}

pub async fn render_operation_events(
    mut events: OperationEventStream
) -> anyhow::Result<()> {
    while let Some(event) = events.next().await {
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

#[cfg(test)]
mod tests {
    use super::*;
    use nirion_lib::events::ExitStatus;

    #[test]
    fn render_compose_event_accepts_all_event_types() {
        render_compose_event(ComposeEvent::ProjectStarted {
            project: "app".to_string(),
        });
        render_compose_event(ComposeEvent::Process {
            project: Some("app".to_string()),
            event: ProcessEvent::StdoutLine("stdout".to_string()),
        });
        render_compose_event(ComposeEvent::Process {
            project: Some("app".to_string()),
            event: ProcessEvent::StderrLine("stderr".to_string()),
        });
        render_compose_event(ComposeEvent::Process {
            project: Some("app".to_string()),
            event: ProcessEvent::StderrLine(
                "the attribute `version` is obsolete".to_string(),
            ),
        });
        render_compose_event(ComposeEvent::Process {
            project: Some("app".to_string()),
            event: ProcessEvent::Exited(ExitStatus {
                code: Some(0),
                success: true,
            }),
        });
        render_compose_event(ComposeEvent::ProjectFailed {
            project: "app".to_string(),
            error: "failed".to_string(),
        });
    }
}
