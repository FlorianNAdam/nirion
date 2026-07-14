use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Context;
use nirion_oci_lib::client::AuthConfig;
use tokio::process::Command;

use crate::{lock::LockedImages, projects::Projects};

#[cfg(test)]
static TEST_NIX_CMD: std::sync::Mutex<Option<Vec<String>>> =
    std::sync::Mutex::new(None);

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
    auth_file: Option<&Path>
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
    use nirion_oci_lib::auth::RegistryAuth;
    use std::{fs, os::unix::fs::PermissionsExt, path::Path};

    static NIX_BIN_LOCK: tokio::sync::Mutex<()> =
        tokio::sync::Mutex::const_new(());

    struct NixBinGuard;

    impl NixBinGuard {
        fn set_script(script: String) -> Self {
            *TEST_NIX_CMD.lock().unwrap() =
                Some(vec!["/bin/sh".to_string(), script]);
            Self
        }

        fn set_command(command: String) -> Self {
            *TEST_NIX_CMD.lock().unwrap() = Some(vec![command]);
            Self
        }
    }

    impl Drop for NixBinGuard {
        fn drop(&mut self) {
            *TEST_NIX_CMD.lock().unwrap() = None;
        }
    }

    fn write_fake_nix(
        dir: &Path,
        args_file: &Path,
        exit_code: i32,
        stdout: &str,
        stderr: &str,
    ) -> String {
        let nix = dir.join("nix");
        fs::write(
            &nix,
            format!(
                r#"#!/bin/sh
printf '%s\n' "$@" > '{}'
printf '%s' '{}'
printf '%s' '{}' >&2
exit {exit_code}
"#,
                args_file.display(),
                stdout,
                stderr
            ),
        )
        .unwrap();

        let mut permissions = fs::metadata(&nix)
            .unwrap()
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&nix, permissions).unwrap();

        nix.to_string_lossy().to_string()
    }

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

    #[test]
    fn load_projects_valid_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("projects.json");
        std::fs::write(
            &path,
            r#"{
                "myapp": {
                    "name": "myapp",
                    "dockerCompose": "compose.yml",
                    "services": {
                        "web": {
                            "image": "nginx",
                            "healthcheck": true,
                            "restart": null
                        }
                    }
                }
            }"#,
        )
        .unwrap();

        let result = load_projects(&path).unwrap();
        assert!(result.contains_key("myapp"));
        assert_eq!(
            result["myapp"].services["web"]
                .image
                .as_deref(),
            Some("nginx")
        );
    }

    #[test]
    fn load_projects_missing_file_errors() {
        let result = load_projects(Path::new("/nonexistent/projects.json"));
        assert!(result.is_err());
    }

    #[test]
    fn load_projects_invalid_json_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("projects.json");
        std::fs::write(&path, "not json").unwrap();
        assert!(load_projects(&path).is_err());
    }

    #[test]
    fn load_auth_config_none_returns_empty_config() {
        let result = load_auth_config(None).unwrap();
        assert!(result.sources.is_empty());
    }

    #[test]
    fn load_auth_config_valid_json_normalizes_sources() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.json");
        std::fs::write(
            &path,
            r#"{
                "docker.io/library/nginx": {
                    "type": "basic",
                    "username": "user",
                    "password": "pass"
                }
            }"#,
        )
        .unwrap();

        let result = load_auth_config(Some(&path)).unwrap();
        assert_eq!(
            result.sources["index.docker.io/library/nginx"],
            RegistryAuth::basic("user", "pass")
        );
    }

    #[test]
    fn load_auth_config_missing_file_errors() {
        let result =
            load_auth_config(Some(Path::new("/nonexistent/auth.json")));
        assert!(result.is_err());
    }

    #[test]
    fn load_auth_config_invalid_json_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.json");
        std::fs::write(&path, "not json").unwrap();
        assert!(load_auth_config(Some(&path)).is_err());
    }

    #[tokio::test]
    async fn build_nix_project_file_returns_trimmed_output_path() {
        let _nix_bin_lock = NIX_BIN_LOCK.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let nix = write_fake_nix(
            dir.path(),
            &args_file,
            0,
            "/nix/store/projects-file\n",
            "",
        );
        let _nix_bin_guard = NixBinGuard::set_script(nix);

        let result = build_nix_project_file(".#nixosConfigurations.host")
            .await
            .unwrap();

        assert_eq!(result, PathBuf::from("/nix/store/projects-file"));
        assert_eq!(
            fs::read_to_string(args_file).unwrap(),
            "build\n.#nixosConfigurations.host\n--no-link\n--print-out-paths\n"
        );
    }

    #[tokio::test]
    async fn build_nix_project_file_reports_failed_build_stderr() {
        let _nix_bin_lock = NIX_BIN_LOCK.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let nix =
            write_fake_nix(dir.path(), &args_file, 1, "", "broken flake\n");
        let _nix_bin_guard = NixBinGuard::set_script(nix);

        let err = build_nix_project_file(".#target")
            .await
            .unwrap_err();

        assert!(
            err.to_string()
                .contains("nix build failed with status")
        );
        assert!(err.to_string().contains("broken flake"));
    }

    #[tokio::test]
    async fn build_nix_project_file_reports_spawn_failure() {
        let _nix_bin_lock = NIX_BIN_LOCK.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let missing_nix = dir.path().join("missing-nix");
        let _nix_bin_guard = NixBinGuard::set_command(
            missing_nix
                .to_string_lossy()
                .to_string(),
        );

        let err = build_nix_project_file(".#target")
            .await
            .unwrap_err();

        assert!(!err.to_string().is_empty());
    }
}

pub async fn build_nix_project_file(
    nix_eval_target: &str
) -> anyhow::Result<PathBuf> {
    let output = nix_command()
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

fn nix_command() -> Command {
    #[cfg(test)]
    if let Some(cmd) = TEST_NIX_CMD.lock().unwrap().clone() {
        let mut command = Command::new(&cmd[0]);
        command.args(&cmd[1..]);
        return command;
    }

    Command::new("nix")
}
