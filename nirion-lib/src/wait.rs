use std::collections::BTreeMap;

use crate::{
    docker::{ProjectStatus, ServiceState},
    projects::{Projects, TargetSelector},
};

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
