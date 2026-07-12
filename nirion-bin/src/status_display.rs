use crossterm::style::{Color, Stylize};
use nirion_lib::docker::{
    ProjectState, ProjectStatus, ServiceState, ServiceStatus,
};

pub fn project_state_icon(state: &ProjectState) -> String {
    use ProjectState::*;

    match state {
        Empty => "-".grey().to_string(),
        Healthy => "✓".green().to_string(),
        Running => "✓".yellow().to_string(),
        Paused => "=".blue().to_string(),
        Starting => "↗".cyan().to_string(),
        Degraded => "✗".red().to_string(),
        Unknown => "?".grey().to_string(),
    }
}

pub fn project_status_segments(status: &ProjectStatus) -> Vec<Color> {
    let mut services: Vec<&ServiceStatus> = status.services.values().collect();

    services.sort_by_key(|s| service_state_order(&s.state));

    services
        .into_iter()
        .map(|s| service_state_color(&s.state))
        .collect()
}

fn service_state_order(state: &ServiceState) -> usize {
    let order = [
        ServiceState::Healthy,
        ServiceState::Succeeded,
        ServiceState::Running,
        ServiceState::Paused,
        ServiceState::Starting,
        ServiceState::Restarting,
        ServiceState::Failed,
        ServiceState::Unhealthy,
        ServiceState::Created,
        ServiceState::Unknown,
    ];

    order
        .iter()
        .position(|s| s == state)
        .unwrap_or_default()
}

fn service_state_color(state: &ServiceState) -> Color {
    match state {
        ServiceState::Created => Color::Grey,
        ServiceState::Starting => Color::DarkGrey,
        ServiceState::Running => Color::Yellow,
        ServiceState::Paused => Color::Blue,
        ServiceState::Restarting => Color::DarkGrey,
        ServiceState::Succeeded => Color::Cyan,
        ServiceState::Failed => Color::Magenta,
        ServiceState::Healthy => Color::Green,
        ServiceState::Unhealthy => Color::Red,
        ServiceState::Unknown => Color::Grey,
    }
}
