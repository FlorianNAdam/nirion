use std::{collections::BTreeMap, fs};

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

pub static PROJECTS: Lazy<BTreeMap<String, Project>> = Lazy::new(|| {
    let path = std::env::var("NIRION_PROJECT_FILE")
        .expect("Env var NIRION_PROJECT_FILE must be set");
    let data = fs::read_to_string(path).expect("Failed to read project file");
    serde_json::from_str(&data).expect("Failed to parse project JSON")
});

#[derive(Clone, Debug)]
pub struct ProjectSelector {
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct ImageSelector {
    pub project: String,
    pub image: String,
}

#[derive(Clone, Debug)]
pub enum TargetSelector {
    All,
    Project(ProjectSelector),
    Image(ImageSelector),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    #[serde(rename = "docker-compose")]
    pub docker_compose: String,
    pub services: BTreeMap<String, String>,
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
        [project_name, image_name] => {
            if let Some(proj) = projects.get(*project_name) {
                if proj.services.contains_key(*image_name) {
                    Ok(TargetSelector::Image(ImageSelector {
                        project: project_name.to_string(),
                        image: image_name.to_string(),
                    }))
                } else {
                    anyhow::bail!(
                        "Image '{}' not found in project '{}'",
                        image_name,
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

pub fn parse_image_selector(
    s: &str,
    projects: &BTreeMap<String, Project>,
) -> anyhow::Result<ImageSelector> {
    let selector = parse_selector(s, projects)?;
    match selector {
        TargetSelector::Image(image_selector) => Ok(image_selector),
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
                for (service_name, image) in project.services.iter() {
                    let identifier = format!("{project_name}.{service_name}");
                    images.insert(identifier, image.to_string());
                }
            }
        }
        TargetSelector::Project(proj) => {
            let project = &projects[&proj.name];
            for (service_name, image) in project.services.iter() {
                let identifier = format!("{}.{}", proj.name, service_name);
                images.insert(identifier, image.to_string());
            }
        }
        TargetSelector::Image(img) => {
            let project = &projects[&img.project];
            let image = &project.services[&img.image];
            let identifier = format!("{}.{}", img.project, img.image);
            images.insert(identifier, image.to_string());
        }
    }
    images
}
