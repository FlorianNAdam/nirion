use std::{
    collections::BTreeMap,
    fmt::Display,
    ops::{Deref, Index},
};

use serde::{Deserialize, Serialize};

#[derive(Default, Clone)]
pub struct Projects {
    projects: BTreeMap<String, Project>,
}

impl<'de> Deserialize<'de> for Projects {
    fn deserialize<D>(deserializer: D) -> Result<Projects, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let projects = BTreeMap::<String, Project>::deserialize(deserializer)?;

        Ok(Self { projects })
    }
}

impl Serialize for Projects {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.projects.serialize(serializer)
    }
}

impl Projects {
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Project)> {
        self.projects
            .iter()
            .map(|(s, p)| (s.as_str(), p))
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.projects.contains_key(key)
    }

    pub fn get(&self, key: &str) -> Option<&Project> {
        self.projects.get(key)
    }
}

impl Index<&str> for Projects {
    type Output = Project;

    fn index(&self, index: &str) -> &Self::Output {
        &self.projects[index]
    }
}

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
    projects: &Projects,
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
    projects: &Projects,
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
    projects: &Projects,
) -> BTreeMap<String, String> {
    let mut images = BTreeMap::new();
    match target {
        TargetSelector::All => {
            for (project_name, project) in projects.iter() {
                for (service_name, service) in project.services.iter() {
                    let identifier = format!("{project_name}.{service_name}");
                    if let Some(image) = &service.image {
                        images.insert(identifier, image.to_string());
                    };
                }
            }
        }
        TargetSelector::Project(proj) => {
            let project = &projects.get(&proj.name).unwrap();
            for (service_name, service) in project.services.iter() {
                let identifier = format!("{}.{}", proj.name, service_name);
                if let Some(image) = &service.image {
                    images.insert(identifier, image.to_string());
                };
            }
        }
        TargetSelector::Service(img) => {
            let project = &projects.get(&img.project).unwrap();
            let service = &project.services[&img.service];
            let identifier = format!("{}.{}", img.project, img.service);
            if let Some(image) = &service.image {
                images.insert(identifier, image.to_string());
            };
        }
    }
    images
}
