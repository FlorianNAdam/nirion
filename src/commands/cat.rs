use anyhow::Result;
use clap::Parser;
use serde_yml as serde_yaml;
use serde_yml::{Mapping, Value};
use std::path::Path;
use std::{collections::BTreeMap, fs};

use crate::{Project, TargetSelector};

/// Print the docker compose file as yaml
#[derive(Parser, Debug, Clone)]
pub struct CatArgs {
    /// Target selector: *, project, or project.service
    #[arg(default_value = "*", value_parser = crate::clap_parse_selector)]
    pub target: TargetSelector,
}

pub async fn handle_cat(
    args: &CatArgs,
    projects: &BTreeMap<String, Project>,
    _locked_images: &BTreeMap<String, String>,
    _lock_file: &Path,
) -> Result<()> {
    match &args.target {
        TargetSelector::All => {
            for (project_name, project) in projects {
                println!("Project {}:", project_name);
                print_full_yaml(project_name, project)?;
            }
        }
        TargetSelector::Project(proj) => {
            let project = &projects[&proj.name];
            print_full_yaml(&proj.name, project)?;
        }
        TargetSelector::Service(img) => {
            let project = &projects[&img.project];
            print_service_section(&img.project, project, &img.service)?;
        }
    }

    Ok(())
}

fn load_yaml(path: &str) -> Result<Value> {
    let data = fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("Failed reading {}: {}", path, e))?;

    serde_yaml::from_str::<Value>(&data)
        .map_err(|e| anyhow::anyhow!("YAML parse error in {}: {}", path, e))
}

fn print_full_yaml(_project_name: &str, project: &Project) -> Result<()> {
    let path = &project.docker_compose;

    let yaml = load_yaml(path)?;
    let pretty = serde_yaml::to_string(&yaml)
        .map_err(|e| anyhow::anyhow!("Failed to pretty-print YAML: {}", e))?;

    println!("{}", pretty);
    println!();

    Ok(())
}

fn print_service_section(
    project_name: &str,
    project: &Project,
    service_name: &str,
) -> Result<()> {
    let path = &project.docker_compose;

    let yaml = load_yaml(path)?;

    let services = yaml
        .get("services")
        .ok_or_else(|| anyhow::anyhow!("No `services:` section in YAML"))?;

    let services_map = services
        .as_mapping()
        .ok_or_else(|| anyhow::anyhow!("`services:` is not a YAML mapping"))?;

    let service_value = services_map
        .get(&Value::String(service_name.to_string()))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Service `{}` not found in docker-compose for project `{}`",
                service_name,
                project_name
            )
        })?;

    let mut root_map = Mapping::new();
    let mut svc_map = Mapping::new();
    svc_map.insert(
        Value::String(service_name.to_string()),
        service_value.clone(),
    );
    root_map.insert(Value::String("services".into()), Value::Mapping(svc_map));

    let pretty = serde_yaml::to_string(&Value::Mapping(root_map))
        .map_err(|e| anyhow::anyhow!("Failed to pretty-print YAML: {}", e))?;

    println!("{}", pretty);
    println!();

    Ok(())
}
