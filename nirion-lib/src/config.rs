use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Context;
use nirion_oci_lib::client::AuthConfig;
use tokio::process::Command;

use crate::{lock::LockedImages, projects::Projects};

pub fn load_locked_images(lock_file: &Path) -> anyhow::Result<LockedImages> {
    let locked_images = if lock_file.exists() {
        let lock_file_data = fs::read_to_string(lock_file)
            .context("Failed to read lock file")?;
        serde_json::from_str(&lock_file_data)
            .context("Failed to parse lock file")?
    } else {
        LockedImages::default()
    };

    Ok(locked_images)
}

pub fn load_projects(project_file: &Path) -> anyhow::Result<Projects> {
    let project_data = fs::read_to_string(project_file)
        .context("Failed to read projects file")?;
    let projects = serde_json::from_str(&project_data)
        .context("Failed to parse projects file")?;

    Ok(projects)
}

pub fn load_auth_config(
    auth_file: Option<&Path>,
) -> anyhow::Result<AuthConfig> {
    let Some(auth_file) = auth_file else {
        return Ok(AuthConfig::default());
    };

    let auth_data =
        fs::read_to_string(auth_file).context("Failed to read auth file")?;
    let auth = serde_json::from_str(&auth_data)
        .context("Failed to parse auth file")?;

    Ok(auth)
}

pub fn nix_config_target(target: &str) -> String {
    format!(
        "{}.{}",
        target,
        [
            "config",
            "virtualisation",
            "nirion",
            "out",
            "projectsFileStatic"
        ]
        .join(".")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nix_config_target_basic() {
        let result = nix_config_target("nixosConfigurations.myhost");
        assert_eq!(
            result,
            "nixosConfigurations.myhost.config.virtualisation.nirion.out.projectsFileStatic"
        );
    }

    #[test]
    fn nix_config_target_simple() {
        let result = nix_config_target("foo");
        assert_eq!(
            result,
            "foo.config.virtualisation.nirion.out.projectsFileStatic"
        );
    }

    #[test]
    fn load_locked_images_missing_file() {
        let result =
            load_locked_images(Path::new("/nonexistent/path/lock.json"))
                .unwrap();
        assert!(result.iter().next().is_none());
    }

    #[test]
    fn load_locked_images_valid_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lock.json");
        std::fs::write(
            &path,
            r#"{"myapp.web":{"image":"nginx","version":"1.0","digest":"sha256:aaa"}}"#,
        )
        .unwrap();
        let result = load_locked_images(&path).unwrap();
        assert!(result.contains_key("myapp.web"));
    }

    #[test]
    fn load_locked_images_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lock.json");
        std::fs::write(&path, "not json").unwrap();
        assert!(load_locked_images(&path).is_err());
    }
}

pub async fn build_nix_project_file(
    nix_eval_target: &str,
) -> anyhow::Result<PathBuf> {
    let output = Command::new("nix")
        .args(["build", nix_eval_target, "--no-link", "--print-out-paths"])
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "nix build failed with status {}{}{}",
            output.status,
            if stderr.trim().is_empty() { "" } else { ": " },
            stderr.trim()
        );
    }

    let raw_path = str::from_utf8(&output.stdout)?
        .trim()
        .to_string();

    Ok(PathBuf::from(raw_path))
}
