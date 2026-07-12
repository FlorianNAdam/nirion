use std::fs;

use serde_yaml_ng::{Mapping, Value};

use crate::projects::Project;

pub fn load_compose(path: &str) -> anyhow::Result<Value> {
    let data = fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("Failed reading {}: {}", path, e))?;

    serde_yaml_ng::from_str::<Value>(&data).map_err(|e| {
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
    serde_yaml_ng::to_string(compose).map_err(|e| {
        anyhow::anyhow!("Failed to pretty-print compose file: {}", e)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::{Project, ProjectName};
    use std::collections::BTreeMap;

    fn write_compose(contents: &str) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("compose.yml");
        std::fs::write(&path, contents).unwrap();
        (dir, path.to_string_lossy().to_string())
    }

    fn project(path: String) -> Project {
        Project {
            name: ProjectName("myapp".into()),
            docker_compose: path,
            services: BTreeMap::new(),
        }
    }

    #[test]
    fn load_compose_reads_valid_yaml() {
        let (_dir, path) = write_compose(
            r#"
services:
  web:
    image: nginx
"#,
        );

        let compose = load_compose(&path).unwrap();
        assert_eq!(compose["services"]["web"]["image"], "nginx");
    }

    #[test]
    fn load_compose_reports_missing_file() {
        let err = load_compose("/nonexistent/compose.yml").unwrap_err();
        assert!(
            err.to_string()
                .contains("Failed reading")
        );
    }

    #[test]
    fn load_compose_reports_invalid_yaml() {
        let (_dir, path) = write_compose("services: [");
        let err = load_compose(&path).unwrap_err();
        assert!(
            err.to_string()
                .contains("Compose file parse error")
        );
    }

    #[test]
    fn full_compose_loads_project_compose_file() {
        let (_dir, path) = write_compose("services: {}");
        let compose = full_compose(&project(path)).unwrap();
        assert!(compose.get("services").is_some());
    }

    #[test]
    fn service_compose_returns_only_selected_service() {
        let (_dir, path) = write_compose(
            r#"
services:
  web:
    image: nginx
  db:
    image: postgres
"#,
        );

        let compose = service_compose("myapp", &project(path), "web").unwrap();
        assert_eq!(compose["services"]["web"]["image"], "nginx");
        assert!(compose["services"].get("db").is_none());
    }

    #[test]
    fn service_compose_requires_services_section() {
        let (_dir, path) = write_compose("name: myapp");
        let err = service_compose("myapp", &project(path), "web").unwrap_err();
        assert!(
            err.to_string()
                .contains("No `services:` section")
        );
    }

    #[test]
    fn service_compose_requires_services_mapping() {
        let (_dir, path) = write_compose("services: []");
        let err = service_compose("myapp", &project(path), "web").unwrap_err();
        assert!(
            err.to_string()
                .contains("`services:` is not a mapping")
        );
    }

    #[test]
    fn service_compose_reports_missing_service() {
        let (_dir, path) = write_compose("services: {db: {image: postgres}}");
        let err = service_compose("myapp", &project(path), "web").unwrap_err();
        assert!(
            err.to_string()
                .contains("Service `web` not found")
        );
    }

    #[test]
    fn compose_to_string_serializes_yaml() {
        let compose = serde_yaml_ng::from_str::<Value>("services: {}").unwrap();
        let rendered = compose_to_string(&compose).unwrap();
        assert!(rendered.contains("services:"));
    }
}
