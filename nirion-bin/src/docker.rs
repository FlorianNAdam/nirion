use futures::StreamExt;
use nirion_lib::{
    backend::{
        CommandOutputEvent, CommandOutputEventStream, OperationEvent,
        OperationEventStream,
    },
    events::ProcessEvent,
};
use nirion_tui_lib::color::Colorize;

pub async fn render_operation_events(
    mut events: OperationEventStream
) -> anyhow::Result<()> {
    while let Some(event) = events.next().await {
        render_operation_event(event?);
    }

    Ok(())
}

pub async fn render_command_output_events(
    mut events: CommandOutputEventStream
) -> anyhow::Result<()> {
    while let Some(event) = events.next().await {
        render_command_output_event(event?);
    }

    Ok(())
}

fn render_operation_event(event: OperationEvent) {
    match event {
        OperationEvent::ProjectStarted { project } => {
            println!("[{}]", project.cyan());
        }
        OperationEvent::Process { event, .. } => render_process_event(event),
        OperationEvent::ProjectFailed { project, error } => {
            eprintln!("Project '{}' failed: {}", project, error);
            println!();
        }
    }
}

fn render_command_output_event(event: CommandOutputEvent) {
    match event {
        CommandOutputEvent::ProjectStarted { project } => {
            println!("[{}]", project.cyan());
        }
        CommandOutputEvent::Output { event, .. } => render_process_event(event),
        CommandOutputEvent::ProjectFailed { project, error } => {
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
        render_operation_event(OperationEvent::ProjectStarted {
            project: "app".to_string(),
        });
        render_operation_event(OperationEvent::Process {
            project: Some("app".to_string()),
            event: ProcessEvent::StdoutLine("stdout".to_string()),
        });
        render_operation_event(OperationEvent::Process {
            project: Some("app".to_string()),
            event: ProcessEvent::StderrLine("stderr".to_string()),
        });
        render_operation_event(OperationEvent::Process {
            project: Some("app".to_string()),
            event: ProcessEvent::StderrLine(
                "the attribute `version` is obsolete".to_string(),
            ),
        });
        render_operation_event(OperationEvent::Process {
            project: Some("app".to_string()),
            event: ProcessEvent::Exited(ExitStatus {
                code: Some(0),
                success: true,
            }),
        });
        render_operation_event(OperationEvent::ProjectFailed {
            project: "app".to_string(),
            error: "failed".to_string(),
        });
    }
}
