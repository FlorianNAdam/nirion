use std::fs;

use serde_yml::{Mapping, Value};

use crate::projects::Project;

pub fn load_compose(path: &str) -> anyhow::Result<Value> {
    let data = fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("Failed reading {}: {}", path, e))?;

    serde_yml::from_str::<Value>(&data).map_err(|e| {
        anyhow::anyhow!("Compose file parse error in {}: {}", path, e)
    })
}

pub fn full_compose(project: &Project) -> anyhow::Result<Value> {
    load_compose(&project.docker_compose)
}

pub fn service_compose(
    project_name: &str,
    project: &Project,
    service_name: &str,
) -> anyhow::Result<Value> {
    let compose = load_compose(&project.docker_compose)?;

    let services = compose.get("services").ok_or_else(|| {
        anyhow::anyhow!("No `services:` section in compose file")
    })?;

    let services_map = services
        .as_mapping()
        .ok_or_else(|| anyhow::anyhow!("`services:` is not a mapping"))?;

    let service_value = services_map
        .get(&Value::String(service_name.to_string()))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Service `{}` not found in compose file for project `{}`",
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

    Ok(Value::Mapping(root_map))
}

pub fn compose_to_string(compose: &Value) -> anyhow::Result<String> {
    serde_yml::to_string(compose).map_err(|e| {
        anyhow::anyhow!("Failed to pretty-print compose file: {}", e)
    })
}
