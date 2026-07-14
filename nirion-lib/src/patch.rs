use std::{collections::BTreeMap, fs, process::Stdio};

use anyhow::{Context, Result};
use serde::Deserialize;
use tokio::process::Command;

use crate::projects::{Projects, TargetSelector};

#[cfg(test)]
static TEST_SUDO_CMD: std::sync::Mutex<Option<Vec<String>>> =
    std::sync::Mutex::new(None);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatchTarget {
    EnvFile,
    Compose,
}

pub async fn patch_target(
    target: &TargetSelector,
    projects: &Projects,
    patch_target: &PatchTarget,
) -> Result<()> {
    match target {
        TargetSelector::All => {
            anyhow::bail!("Only individual projects can be patched");
        }

        TargetSelector::Project(proj) => {
            if patch_target == &PatchTarget::EnvFile {
                anyhow::bail!(
                    "Only individual service env files can be patched"
                );
            }

            let project = &projects[&proj.name];
            patch(&project.docker_compose).await?;
        }

        TargetSelector::Service(img) => {
            let project = &projects[&img.project];

            match patch_target {
                PatchTarget::Compose => {
                    patch(&project.docker_compose).await?;
                }

                PatchTarget::EnvFile => {
                    let compose =
                        load_compose_env_files(&project.docker_compose)?;

                    let service = compose.services.get(&img.service).with_context(|| {
                        format!(
                            "Service `{}` not found in compose file for project `{}`",
                            img.service, img.project
                        )
                    })?;

                    let env_file =
                        service
                            .env_file
                            .first()
                            .with_context(|| {
                                format!(
                                    "No env_file found for `{}.{}`",
                                    img.project, img.service
                                )
                            })?;

                    patch(env_file).await?;
                }
            }
        }
    }

    Ok(())
}

fn load_compose_env_files(path: &str) -> anyhow::Result<ComposeFile> {
    let data = fs::read_to_string(path)
        .with_context(|| format!("Failed reading {}", path))?;

    serde_yaml_ng::from_str::<ComposeFile>(&data)
        .with_context(|| format!("Compose file parse error in {}", path))
}

#[derive(Debug, Deserialize)]
struct ComposeFile {
    services: BTreeMap<String, Service>,
}

#[derive(Debug, Deserialize)]
struct Service {
    #[serde(default)]
    env_file: Vec<String>,
}

pub async fn patch(file: &str) -> Result<()> {
    let status = sudo_command()
        .arg("mirage-patch")
        .arg(file)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .context("failed to spawn mirage-patch")?;

    if !status.success() {
        anyhow::bail!("mirage-patch exited with status: {}", status);
    }

    Ok(())
}

fn sudo_command() -> Command {
    #[cfg(test)]
    if let Some(cmd) = TEST_SUDO_CMD.lock().unwrap().clone() {
        let mut command = Command::new(&cmd[0]);
        command.args(&cmd[1..]);
        return command;
    }

    Command::new("sudo")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, io::Write, os::unix::fs::PermissionsExt, path::Path};

    static SUDO_BIN_LOCK: tokio::sync::Mutex<()> =
        tokio::sync::Mutex::const_new(());

    struct SudoBinGuard;

    impl SudoBinGuard {
        fn set_script(script: String) -> Self {
            *TEST_SUDO_CMD.lock().unwrap() =
                Some(vec!["/bin/sh".to_string(), script]);
            Self
        }

        fn set_command(command: String) -> Self {
            *TEST_SUDO_CMD.lock().unwrap() = Some(vec![command]);
            Self
        }
    }

    impl Drop for SudoBinGuard {
        fn drop(&mut self) {
            *TEST_SUDO_CMD.lock().unwrap() = None;
        }
    }

    fn write_fake_sudo(
        dir: &Path,
        args_file: &Path,
        exit_code: i32,
    ) -> String {
        let sudo = dir.join("sudo");
        let tmp = dir.join("sudo.tmp");
        let mut file = fs::File::create(&tmp).unwrap();
        file.write_all(
            format!(
                r#"#!/bin/sh
printf '%s\n' "$@" > '{}'
exit {exit_code}
"#,
                args_file.display()
            )
            .as_bytes(),
        )
        .unwrap();
        file.sync_all().unwrap();
        drop(file);

        let mut permissions = fs::metadata(&tmp)
            .unwrap()
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&tmp, permissions).unwrap();
        fs::rename(&tmp, &sudo).unwrap();

        sudo.to_string_lossy().to_string()
    }

    fn projects(compose_path: &str) -> Projects {
        serde_json::from_value(serde_json::json!({
            "myapp": {
                "name": "myapp",
                "dockerCompose": compose_path,
                "services": {
                    "web": {
                        "image": "nginx",
                        "healthcheck": false,
                        "restart": null
                    }
                }
            }
        }))
        .unwrap()
    }

    fn service_target(service: &str) -> TargetSelector {
        TargetSelector::Service(crate::projects::ServiceSelector {
            project: "myapp".into(),
            service: service.into(),
        })
    }

    #[tokio::test]
    async fn patch_target_rejects_all_projects() {
        let projects = Projects::default();
        let err = patch_target(
            &TargetSelector::All,
            &projects,
            &PatchTarget::Compose,
        )
        .await
        .unwrap_err();

        assert_eq!(err.to_string(), "Only individual projects can be patched");
    }

    #[tokio::test]
    async fn patch_target_rejects_project_env_file() {
        let target =
            TargetSelector::Project(crate::projects::ProjectSelector {
                name: "myapp".into(),
            });
        let projects = projects("compose.yml");
        let err = patch_target(&target, &projects, &PatchTarget::EnvFile)
            .await
            .unwrap_err();

        assert_eq!(
            err.to_string(),
            "Only individual service env files can be patched"
        );
    }

    #[test]
    fn load_compose_env_files_reads_env_files() {
        let dir = tempfile::tempdir().unwrap();
        let compose_path = dir.path().join("compose.yml");
        std::fs::write(
            &compose_path,
            r#"
services:
  web:
    env_file:
      - web.env
      - common.env
  db:
    image: postgres
"#,
        )
        .unwrap();

        let compose =
            load_compose_env_files(compose_path.to_str().unwrap()).unwrap();

        assert_eq!(
            compose.services["web"].env_file,
            vec!["web.env", "common.env"]
        );
        assert!(
            compose.services["db"]
                .env_file
                .is_empty()
        );
    }

    #[test]
    fn load_compose_env_files_reports_missing_file() {
        let err =
            load_compose_env_files("/nonexistent/compose.yml").unwrap_err();

        assert!(
            err.to_string()
                .contains("Failed reading /nonexistent/compose.yml")
        );
    }

    #[test]
    fn load_compose_env_files_reports_invalid_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let compose_path = dir.path().join("compose.yml");
        std::fs::write(&compose_path, "services: [").unwrap();

        let err =
            load_compose_env_files(compose_path.to_str().unwrap()).unwrap_err();

        assert!(
            err.to_string()
                .contains("Compose file parse error")
        );
    }

    #[tokio::test]
    async fn patch_target_reports_missing_service_for_env_file() {
        let dir = tempfile::tempdir().unwrap();
        let compose_path = dir.path().join("compose.yml");
        std::fs::write(
            &compose_path,
            r#"
services:
  web:
    env_file:
      - web.env
"#,
        )
        .unwrap();
        let projects = projects(compose_path.to_str().unwrap());

        let err = patch_target(
            &service_target("worker"),
            &projects,
            &PatchTarget::EnvFile,
        )
        .await
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "Service `worker` not found in compose file for project `myapp`"
        );
    }

    #[tokio::test]
    async fn patch_target_reports_missing_env_file() {
        let dir = tempfile::tempdir().unwrap();
        let compose_path = dir.path().join("compose.yml");
        std::fs::write(
            &compose_path,
            r#"
services:
  web:
    image: nginx
"#,
        )
        .unwrap();
        let projects = projects(compose_path.to_str().unwrap());

        let err = patch_target(
            &service_target("web"),
            &projects,
            &PatchTarget::EnvFile,
        )
        .await
        .unwrap_err();

        assert_eq!(err.to_string(), "No env_file found for `myapp.web`");
    }

    #[tokio::test]
    async fn patch_invokes_mirage_patch_through_sudo() {
        let _sudo_bin_lock = SUDO_BIN_LOCK.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let sudo = write_fake_sudo(dir.path(), &args_file, 0);
        let _sudo_bin_guard = SudoBinGuard::set_script(sudo);

        patch("compose.yml").await.unwrap();

        assert_eq!(
            fs::read_to_string(args_file).unwrap(),
            "mirage-patch\ncompose.yml\n"
        );
    }

    #[tokio::test]
    async fn patch_reports_failed_status() {
        let _sudo_bin_lock = SUDO_BIN_LOCK.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let sudo = write_fake_sudo(dir.path(), &args_file, 3);
        let _sudo_bin_guard = SudoBinGuard::set_script(sudo);

        let err = patch("compose.yml").await.unwrap_err();

        assert!(
            err.to_string()
                .contains("mirage-patch exited with status")
        );
    }

    #[tokio::test]
    async fn patch_reports_spawn_failure() {
        let _sudo_bin_lock = SUDO_BIN_LOCK.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let missing_sudo = dir.path().join("missing-sudo");
        let _sudo_bin_guard = SudoBinGuard::set_command(
            missing_sudo
                .to_string_lossy()
                .to_string(),
        );

        let err = patch("compose.yml").await.unwrap_err();

        assert!(
            err.to_string()
                .contains("failed to spawn mirage-patch")
        );
    }

    #[tokio::test]
    async fn patch_target_patches_project_compose_file() {
        let _sudo_bin_lock = SUDO_BIN_LOCK.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let sudo = write_fake_sudo(dir.path(), &args_file, 0);
        let _sudo_bin_guard = SudoBinGuard::set_script(sudo);
        let compose_path = dir.path().join("compose.yml");
        let projects = projects(compose_path.to_str().unwrap());

        patch_target(
            &TargetSelector::Project(crate::projects::ProjectSelector {
                name: "myapp".into(),
            }),
            &projects,
            &PatchTarget::Compose,
        )
        .await
        .unwrap();

        assert_eq!(
            fs::read_to_string(args_file).unwrap(),
            format!("mirage-patch\n{}\n", compose_path.display())
        );
    }

    #[tokio::test]
    async fn patch_target_patches_first_service_env_file() {
        let _sudo_bin_lock = SUDO_BIN_LOCK.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let sudo = write_fake_sudo(dir.path(), &args_file, 0);
        let _sudo_bin_guard = SudoBinGuard::set_script(sudo);
        let compose_path = dir.path().join("compose.yml");
        std::fs::write(
            &compose_path,
            r#"
services:
  web:
    env_file:
      - web.env
      - common.env
"#,
        )
        .unwrap();
        let projects = projects(compose_path.to_str().unwrap());

        patch_target(&service_target("web"), &projects, &PatchTarget::EnvFile)
            .await
            .unwrap();

        assert_eq!(
            fs::read_to_string(args_file).unwrap(),
            "mirage-patch\nweb.env\n"
        );
    }
}
