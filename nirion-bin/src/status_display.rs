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

#[cfg(test)]
mod tests {
    use super::*;
    use console::strip_ansi_codes;
    use std::collections::BTreeMap;

    fn service_status(service: &str, state: ServiceState) -> ServiceStatus {
        ServiceStatus {
            id: format!("{service}-id"),
            service: service.to_string(),
            container_name: service.to_string(),
            image: "image".to_string(),
            state,
            health: None,
            exit_code: None,
            running_for: None,
            status: None,
            ports: Vec::new(),
            networks: Vec::new(),
        }
    }

    #[test]
    fn project_state_icon_renders_expected_visible_symbols() {
        let cases = [
            (ProjectState::Empty, "-"),
            (ProjectState::Healthy, "✓"),
            (ProjectState::Running, "✓"),
            (ProjectState::Paused, "="),
            (ProjectState::Starting, "↗"),
            (ProjectState::Degraded, "✗"),
            (ProjectState::Unknown, "?"),
        ];

        for (state, expected) in cases {
            assert_eq!(strip_ansi_codes(&project_state_icon(&state)), expected);
        }
    }

    #[test]
    fn project_status_segments_sorts_services_by_state_priority() {
        let status = ProjectStatus {
            services: BTreeMap::from([
                (
                    "failed".to_string(),
                    service_status("failed", ServiceState::Failed),
                ),
                (
                    "healthy".to_string(),
                    service_status("healthy", ServiceState::Healthy),
                ),
                (
                    "running".to_string(),
                    service_status("running", ServiceState::Running),
                ),
            ]),
        };

        assert_eq!(
            project_status_segments(&status),
            vec![
                service_state_color(&ServiceState::Healthy),
                service_state_color(&ServiceState::Running),
                service_state_color(&ServiceState::Failed),
            ]
        );
    }

    #[test]
    fn service_state_color_keeps_semantic_groups_distinct() {
        let healthy = service_state_color(&ServiceState::Healthy);
        let unhealthy = service_state_color(&ServiceState::Unhealthy);
        let failed = service_state_color(&ServiceState::Failed);
        let running = service_state_color(&ServiceState::Running);
        let paused = service_state_color(&ServiceState::Paused);
        let succeeded = service_state_color(&ServiceState::Succeeded);
        let neutral = service_state_color(&ServiceState::Created);
        let active = service_state_color(&ServiceState::Starting);

        assert_eq!(neutral, service_state_color(&ServiceState::Unknown));
        assert_eq!(active, service_state_color(&ServiceState::Restarting));

        let primary_colors =
            [healthy, unhealthy, failed, running, paused, succeeded];
        for (i, left) in primary_colors.iter().enumerate() {
            for right in primary_colors.iter().skip(i + 1) {
                assert_ne!(left, right);
            }
        }

        assert_ne!(neutral, active);
    }
}
