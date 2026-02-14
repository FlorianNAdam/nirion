use std::{collections::BTreeMap, fmt::Display, ops::Deref};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
pub struct ProjectSelector {
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct ServiceSelector {
    pub project: String,
    pub service: String,
}

#[derive(Clone, Debug)]
pub enum TargetSelector {
    All,
    Project(ProjectSelector),
    Service(ServiceSelector),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectName(pub String);

impl Display for ProjectName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl Into<String> for ProjectName {
    fn into(self) -> String {
        self.0
    }
}

impl Deref for ProjectName {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    pub name: ProjectName,
    #[serde(rename = "docker-compose")]
    pub docker_compose: String,
    pub services: BTreeMap<String, Service>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Service {
    pub image: Option<String>,
    pub healthcheck: Option<serde_json::Value>,
    pub restart: Option<String>,
}

pub fn parse_selector(
    s: &str,
    projects: &BTreeMap<String, Project>,
) -> anyhow::Result<TargetSelector> {
    let s = s.trim();
    if s == "*" {
        return Ok(TargetSelector::All);
    }

    let parts: Vec<&str> = s.splitn(2, '.').collect();
    match parts.as_slice() {
        [project_name] => {
            if projects.contains_key(*project_name) {
                Ok(TargetSelector::Project(ProjectSelector {
                    name: project_name.to_string(),
                }))
            } else {
                anyhow::bail!("Project '{}' not found", project_name);
            }
        }
        [project_name, service_name] => {
            if let Some(proj) = projects.get(*project_name) {
                if proj
                    .services
                    .contains_key(*service_name)
                {
                    Ok(TargetSelector::Service(ServiceSelector {
                        project: project_name.to_string(),
                        service: service_name.to_string(),
                    }))
                } else {
                    anyhow::bail!(
                        "Service '{}' not found in project '{}'",
                        service_name,
                        project_name
                    );
                }
            } else {
                anyhow::bail!("Project '{}' not found", project_name);
            }
        }
        _ => anyhow::bail!("Invalid target selector: {}", s),
    }
}

pub fn parse_service_selector(
    s: &str,
    projects: &BTreeMap<String, Project>,
) -> anyhow::Result<ServiceSelector> {
    let selector = parse_selector(s, projects)?;
    match selector {
        TargetSelector::Service(service_selector) => Ok(service_selector),
        _ => anyhow::bail!(
            "Expected image selector like <project>.<image> but got {}",
            s
        ),
    }
}

pub fn get_images(
    target: &TargetSelector,
    projects: &BTreeMap<String, Project>,
) -> BTreeMap<String, String> {
    let mut images = BTreeMap::new();
    match target {
        TargetSelector::All => {
            for (project_name, project) in projects {
                for (service_name, service) in project.services.iter() {
                    let identifier = format!("{project_name}.{service_name}");
                    if let Some(image) = &service.image {
                        images.insert(identifier, image.to_string());
                    };
                }
            }
        }
        TargetSelector::Project(proj) => {
            let project = &projects[&proj.name];
            for (service_name, service) in project.services.iter() {
                let identifier = format!("{}.{}", proj.name, service_name);
                if let Some(image) = &service.image {
                    images.insert(identifier, image.to_string());
                };
            }
        }
        TargetSelector::Service(img) => {
            let project = &projects[&img.project];
            let service = &project.services[&img.service];
            let identifier = format!("{}.{}", img.project, img.service);
            if let Some(image) = &service.image {
                images.insert(identifier, image.to_string());
            };
        }
    }
    images
}
