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
    fn serialize<S>(
        &self,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
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

    pub fn contains_key(
        &self,
        key: &str,
    ) -> bool {
        self.projects.contains_key(key)
    }

    pub fn get(
        &self,
        key: &str,
    ) -> Option<&Project> {
        self.projects.get(key)
    }
}

impl Index<&str> for Projects {
    type Output = Project;

    fn index(
        &self,
        index: &str,
    ) -> &Self::Output {
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
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
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
    #[serde(rename = "dockerCompose")]
    pub docker_compose: String,
    pub services: BTreeMap<String, Service>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Service {
    pub image: Option<String>,
    #[serde(default)]
    pub healthcheck: bool,
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

pub fn selected_project_names(
    target: &TargetSelector,
    projects: &Projects,
) -> Vec<String> {
    match target {
        TargetSelector::All => projects
            .iter()
            .map(|(name, _)| name.to_string())
            .collect(),
        TargetSelector::Project(project) => vec![project.name.clone()],
        TargetSelector::Service(service) => vec![service.project.clone()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_projects() -> Projects {
        let mut projects = Projects::default();
        projects.projects.insert(
            "myapp".into(),
            Project {
                name: ProjectName("myapp".into()),
                docker_compose: "docker-compose.yml".into(),
                services: [
                    (
                        "web".into(),
                        Service {
                            image: Some("nginx:latest".into()),
                            healthcheck: true,
                            restart: None,
                        },
                    ),
                    (
                        "db".into(),
                        Service {
                            image: Some("postgres:16".into()),
                            healthcheck: false,
                            restart: None,
                        },
                    ),
                ]
                .into(),
            },
        );
        projects.projects.insert(
            "api".into(),
            Project {
                name: ProjectName("api".into()),
                docker_compose: "docker-compose.yml".into(),
                services: [(
                    "server".into(),
                    Service {
                        image: Some("node:20".into()),
                        healthcheck: true,
                        restart: Some("always".into()),
                    },
                )]
                .into(),
            },
        );
        projects
    }

    #[test]
    fn parse_selector_wildcard() {
        let projects = test_projects();
        let sel = parse_selector("*", &projects).unwrap();
        assert!(matches!(sel, TargetSelector::All));
    }

    #[test]
    fn parse_selector_project() {
        let projects = test_projects();
        let sel = parse_selector("myapp", &projects).unwrap();
        match sel {
            TargetSelector::Project(p) => assert_eq!(p.name, "myapp"),
            _ => panic!("expected Project"),
        }
    }

    #[test]
    fn parse_selector_service() {
        let projects = test_projects();
        let sel = parse_selector("myapp.web", &projects).unwrap();
        match sel {
            TargetSelector::Service(s) => {
                assert_eq!(s.project, "myapp");
                assert_eq!(s.service, "web");
            }
            _ => panic!("expected Service"),
        }
    }

    #[test]
    fn parse_selector_project_not_found() {
        let projects = test_projects();
        assert!(parse_selector("nonexistent", &projects).is_err());
    }

    #[test]
    fn parse_selector_service_not_found() {
        let projects = test_projects();
        assert!(parse_selector("myapp.nonexistent", &projects).is_err());
    }

    #[test]
    fn parse_selector_project_not_found_for_service() {
        let projects = test_projects();
        assert!(parse_selector("nonexistent.web", &projects).is_err());
    }

    #[test]
    fn parse_selector_trims_whitespace() {
        let projects = test_projects();
        let sel = parse_selector("  myapp  ", &projects).unwrap();
        assert!(matches!(sel, TargetSelector::Project(_)));
    }

    #[test]
    fn parse_service_selector_valid() {
        let projects = test_projects();
        let sel = parse_service_selector("myapp.web", &projects).unwrap();
        assert_eq!(sel.project, "myapp");
        assert_eq!(sel.service, "web");
    }

    #[test]
    fn parse_service_selector_project_rejected() {
        let projects = test_projects();
        assert!(parse_service_selector("myapp", &projects).is_err());
    }

    #[test]
    fn parse_service_selector_wildcard_rejected() {
        let projects = test_projects();
        assert!(parse_service_selector("*", &projects).is_err());
    }

    #[test]
    fn get_images_all() {
        let projects = test_projects();
        let images = get_images(&TargetSelector::All, &projects);
        assert_eq!(images.len(), 3);
        assert_eq!(images["api.server"], "node:20");
        assert_eq!(images["myapp.web"], "nginx:latest");
        assert_eq!(images["myapp.db"], "postgres:16");
    }

    #[test]
    fn get_images_project() {
        let projects = test_projects();
        let sel = TargetSelector::Project(ProjectSelector {
            name: "myapp".into(),
        });
        let images = get_images(&sel, &projects);
        assert_eq!(images.len(), 2);
        assert!(images.contains_key("myapp.web"));
        assert!(images.contains_key("myapp.db"));
    }

    #[test]
    fn get_images_service() {
        let projects = test_projects();
        let sel = TargetSelector::Service(ServiceSelector {
            project: "myapp".into(),
            service: "web".into(),
        });
        let images = get_images(&sel, &projects);
        assert_eq!(images.len(), 1);
        assert_eq!(images["myapp.web"], "nginx:latest");
    }

    #[test]
    fn get_images_skips_none() {
        let mut projects = test_projects();
        projects
            .projects
            .get_mut("myapp")
            .unwrap()
            .services
            .insert(
                "worker".into(),
                Service {
                    image: None,
                    healthcheck: false,
                    restart: None,
                },
            );
        let images = get_images(&TargetSelector::All, &projects);
        assert_eq!(images.len(), 3);
        assert!(!images.contains_key("myapp.worker"));
    }

    #[test]
    fn selected_project_names_all() {
        let projects = test_projects();
        let names = selected_project_names(&TargetSelector::All, &projects);

        assert_eq!(names, vec!["api", "myapp"]);
    }

    #[test]
    fn selected_project_names_project() {
        let projects = test_projects();
        let sel = TargetSelector::Project(ProjectSelector {
            name: "myapp".into(),
        });
        let names = selected_project_names(&sel, &projects);

        assert_eq!(names, vec!["myapp"]);
    }

    #[test]
    fn selected_project_names_service() {
        let projects = test_projects();
        let sel = TargetSelector::Service(ServiceSelector {
            project: "myapp".into(),
            service: "web".into(),
        });
        let names = selected_project_names(&sel, &projects);

        assert_eq!(names, vec!["myapp"]);
    }

    #[test]
    fn projects_serialize_as_project_map() {
        let projects = test_projects();
        let json = serde_json::to_value(&projects).unwrap();

        assert_eq!(json["myapp"]["name"], "myapp");
        assert_eq!(json["myapp"]["dockerCompose"], "docker-compose.yml");
        assert_eq!(json["myapp"]["services"]["web"]["image"], "nginx:latest");
    }

    #[test]
    fn project_name_behaves_like_string() {
        let name = ProjectName("myapp".into());

        assert_eq!(name.to_string(), "myapp");
        assert_eq!(&*name, "myapp");

        let string: String = name.into();
        assert_eq!(string, "myapp");
    }
}
