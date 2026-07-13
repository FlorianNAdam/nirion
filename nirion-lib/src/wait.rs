use std::collections::BTreeMap;

use crate::{
    docker::{ProjectStatus, ServiceState},
    projects::{Projects, TargetSelector},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitTarget {
    Healthchecks,
}

pub fn wait_finished(
    target: &TargetSelector,
    projects: &Projects,
    statuses: &BTreeMap<String, ProjectStatus>,
    wait_target: WaitTarget,
) -> bool {
    match wait_target {
        WaitTarget::Healthchecks => {
            healthchecks_finished(target, projects, statuses)
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        docker::{Port, ServiceStatus},
        projects::{ProjectSelector, ServiceSelector},
    };

    fn projects() -> Projects {
        serde_json::from_str(
            r#"
{
  "myapp": {
    "name": "myapp",
    "dockerCompose": "compose.yml",
    "services": {
      "web": {"image": "nginx", "healthcheck": true, "restart": null},
      "worker": {"image": "busybox", "healthcheck": false, "restart": null}
    }
  },
  "api": {
    "name": "api",
    "dockerCompose": "compose.yml",
    "services": {
      "server": {"image": "node", "healthcheck": true, "restart": null}
    }
  }
}
"#,
        )
        .unwrap()
    }

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
            ports: Vec::<Port>::new(),
            networks: Vec::new(),
        }
    }

    fn project_status(entries: Vec<(&str, ServiceState)>) -> ProjectStatus {
        ProjectStatus {
            services: entries
                .into_iter()
                .map(|(service, state)| {
                    (service.to_string(), service_status(service, state))
                })
                .collect(),
        }
    }

    #[test]
    fn all_target_finishes_when_healthchecks_are_terminal() {
        let projects = projects();
        let statuses = BTreeMap::from([
            (
                "myapp".to_string(),
                project_status(vec![("web", ServiceState::Healthy)]),
            ),
            (
                "api".to_string(),
                project_status(vec![("server", ServiceState::Unhealthy)]),
            ),
        ]);

        assert!(healthchecks_finished(
            &TargetSelector::All,
            &projects,
            &statuses
        ));
    }

    #[test]
    fn all_target_waits_for_missing_project_status() {
        let projects = projects();
        let statuses = BTreeMap::from([(
            "myapp".to_string(),
            project_status(vec![("web", ServiceState::Healthy)]),
        )]);

        assert!(!healthchecks_finished(
            &TargetSelector::All,
            &projects,
            &statuses
        ));
    }

    #[test]
    fn project_target_waits_for_non_terminal_healthcheck() {
        let projects = projects();
        let statuses = BTreeMap::from([(
            "myapp".to_string(),
            project_status(vec![("web", ServiceState::Starting)]),
        )]);
        let target = TargetSelector::Project(ProjectSelector {
            name: "myapp".into(),
        });

        assert!(!healthchecks_finished(&target, &projects, &statuses));
    }

    #[test]
    fn project_target_waits_for_missing_healthchecked_service() {
        let projects = projects();
        let statuses = BTreeMap::from([(
            "myapp".to_string(),
            project_status(vec![("worker", ServiceState::Running)]),
        )]);
        let target = TargetSelector::Project(ProjectSelector {
            name: "myapp".into(),
        });

        assert!(!healthchecks_finished(&target, &projects, &statuses));
    }

    #[test]
    fn service_target_ignores_other_services() {
        let projects = projects();
        let statuses = BTreeMap::from([(
            "myapp".to_string(),
            project_status(vec![("web", ServiceState::Healthy)]),
        )]);
        let target = TargetSelector::Service(ServiceSelector {
            project: "myapp".into(),
            service: "worker".into(),
        });

        assert!(healthchecks_finished(&target, &projects, &statuses));
    }

    #[test]
    fn missing_project_in_config_is_treated_as_finished() {
        let projects = projects();
        let statuses = BTreeMap::new();
        let target = TargetSelector::Project(ProjectSelector {
            name: "missing".into(),
        });

        assert!(healthchecks_finished(&target, &projects, &statuses));
    }
}
