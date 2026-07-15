use std::{
    env, fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> Self {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir()
            .join(format!("nirion-cli-test-{}-{id}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn write_projects(path: &Path) {
    write_projects_with_compose(path, "compose.yml");
}

fn write_projects_with_compose(
    path: &Path,
    compose: &str,
) {
    let contents = r#"{
  "myapp": {
    "name": "myapp",
    "dockerCompose": "__COMPOSE__",
    "services": {
      "web": {
        "image": "nginx:latest",
        "healthcheck": false,
        "restart": null
      }
    }
  }
}"#
    .replace("__COMPOSE__", compose);

    fs::write(path, contents).unwrap();
}

fn write_empty_projects(path: &Path) {
    fs::write(
        path,
        r#"{
  "empty": {
    "name": "empty",
    "dockerCompose": "compose.yml",
    "services": {}
  }
}"#,
    )
    .unwrap();
}

fn write_fake_docker(
    path: &Path,
    args_file: &Path,
    stdout: &str,
    stderr: &str,
    exit_code: i32,
) {
    fs::write(
        path,
        format!(
            r#"printf '%s\n' "$@" > '{}'
printf '%s\n' '{}'
printf '%s\n' '{}' >&2
exit {exit_code}
"#,
            args_file.display(),
            stdout,
            stderr,
        ),
    )
    .unwrap();
    let mut permissions = fs::metadata(path)
        .unwrap()
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

fn write_fake_docker_append(
    path: &Path,
    args_file: &Path,
    stdout: &str,
    stderr: &str,
    exit_code: i32,
) {
    fs::write(
        path,
        format!(
            r#"printf '%s\n' '---' >> '{}'
printf '%s\n' "$@" >> '{}'
printf '%s\n' '{}'
printf '%s\n' '{}' >&2
exit {exit_code}
"#,
            args_file.display(),
            args_file.display(),
            stdout,
            stderr,
        ),
    )
    .unwrap();
}

fn write_fake_sudo(
    path: &Path,
    args_file: &Path,
) {
    fs::write(
        path,
        format!(
            r#"#!/bin/sh
printf '%s\n' "$@" > '{}'
exit 0
"#,
            args_file.display(),
        ),
    )
    .unwrap();
    let mut permissions = fs::metadata(path)
        .unwrap()
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

fn nirion_command(
    project_file: &Path,
    lock_file: &Path,
    docker_script: &Path,
) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_nirion"));
    command
        .arg("--project-file")
        .arg(project_file)
        .arg("--lock-file")
        .arg(lock_file)
        .arg("--docker-command")
        .arg("/bin/sh")
        .arg("--docker-command-arg")
        .arg(docker_script);
    command
}

fn assert_success(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_failure(output: &std::process::Output) {
    assert!(
        !output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn up_no_tui_uses_configured_docker_command() {
    let dir = TempDir::new();
    let project_file = dir.path().join("projects.json");
    let lock_file = dir.path().join("nirion.lock");
    let docker_script = dir.path().join("fake-docker.sh");
    let args_file = dir.path().join("docker-args");
    write_projects(&project_file);
    write_fake_docker(&docker_script, &args_file, "", "", 0);

    let output = nirion_command(&project_file, &lock_file, &docker_script)
        .arg("up")
        .arg("--no-tui")
        .output()
        .unwrap();

    assert_success(&output);
    assert_eq!(
        fs::read_to_string(args_file).unwrap(),
        "compose\n--file\ncompose.yml\n--project-name\nmyapp\nup\n-d\n"
    );
}

#[test]
fn up_service_target_appends_service_name() {
    let dir = TempDir::new();
    let project_file = dir.path().join("projects.json");
    let lock_file = dir.path().join("nirion.lock");
    let docker_script = dir.path().join("fake-docker.sh");
    let args_file = dir.path().join("docker-args");
    write_projects(&project_file);
    write_fake_docker(&docker_script, &args_file, "", "", 0);

    let output = nirion_command(&project_file, &lock_file, &docker_script)
        .arg("up")
        .arg("myapp.web")
        .arg("--no-tui")
        .output()
        .unwrap();

    assert_success(&output);
    assert_eq!(
        fs::read_to_string(args_file).unwrap(),
        "compose\n--file\ncompose.yml\n--project-name\nmyapp\nup\n-d\nweb\n"
    );
}

#[test]
fn ps_no_tui_forwards_legacy_flags() {
    let dir = TempDir::new();
    let project_file = dir.path().join("projects.json");
    let lock_file = dir.path().join("nirion.lock");
    let docker_script = dir.path().join("fake-docker.sh");
    let args_file = dir.path().join("docker-args");
    write_projects(&project_file);
    write_fake_docker(&docker_script, &args_file, "container-list", "", 0);

    let output = nirion_command(&project_file, &lock_file, &docker_script)
        .arg("ps")
        .arg("--no-tui")
        .arg("--all")
        .arg("--format")
        .arg("json")
        .output()
        .unwrap();

    assert_success(&output);
    assert!(String::from_utf8_lossy(&output.stdout).contains("container-list"));
    assert_eq!(
        fs::read_to_string(args_file).unwrap(),
        "compose\n--file\ncompose.yml\n--project-name\nmyapp\nps\n--all\n--format\njson\n"
    );
}

#[test]
fn inspect_image_raw_prints_docker_output() {
    let dir = TempDir::new();
    let project_file = dir.path().join("projects.json");
    let lock_file = dir.path().join("nirion.lock");
    let docker_script = dir.path().join("fake-docker.sh");
    let args_file = dir.path().join("docker-args");
    write_projects(&project_file);
    write_fake_docker(
        &docker_script,
        &args_file,
        r#"{"Id":"image-id"}"#,
        "",
        0,
    );

    let output = nirion_command(&project_file, &lock_file, &docker_script)
        .arg("inspect")
        .arg("myapp.web")
        .arg("--inspect-target")
        .arg("image")
        .arg("--raw")
        .output()
        .unwrap();

    assert_success(&output);
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains(r#"{"Id":"image-id"}"#)
    );
    assert_eq!(
        fs::read_to_string(args_file).unwrap(),
        "image\ninspect\n--format\njson\nnginx:latest\n"
    );
}

#[test]
fn compose_passthrough_commands_are_wired() {
    let cases: &[(&[&str], &str)] = &[
        (&["down", "--no-tui"], "down\n"),
        (&["start", "--no-tui"], "start\n"),
        (&["stop", "--no-tui"], "stop\n"),
        (&["restart", "--no-tui"], "restart\n"),
        (&["logs"], "logs\n"),
        (&["top"], "top\n"),
        (&["volumes"], "volumes\n--format\ntable\n"),
        (&["compose-exec", "*", "pull"], "pull\n"),
    ];

    for (args, expected_command_args) in cases {
        let dir = TempDir::new();
        let project_file = dir.path().join("projects.json");
        let lock_file = dir.path().join("nirion.lock");
        let docker_script = dir.path().join("fake-docker.sh");
        let args_file = dir.path().join("docker-args");
        write_projects(&project_file);
        write_fake_docker(&docker_script, &args_file, "", "", 0);

        let output = nirion_command(&project_file, &lock_file, &docker_script)
            .args(*args)
            .output()
            .unwrap();

        assert_success(&output);
        assert_eq!(
            fs::read_to_string(args_file).unwrap(),
            format!(
                "compose\n--file\ncompose.yml\n--project-name\nmyapp\n{}",
                expected_command_args
            ),
            "failed command case: {args:?}"
        );
    }
}

#[test]
fn list_prints_projects_and_services() {
    let dir = TempDir::new();
    let project_file = dir.path().join("projects.json");
    let lock_file = dir.path().join("nirion.lock");
    let docker_script = dir.path().join("fake-docker.sh");
    let args_file = dir.path().join("docker-args");
    write_projects(&project_file);
    fs::write(&lock_file, "{}").unwrap();
    write_fake_docker(&docker_script, &args_file, "", "", 0);

    let projects_output =
        nirion_command(&project_file, &lock_file, &docker_script)
            .arg("list")
            .output()
            .unwrap();
    assert_success(&projects_output);
    assert!(
        String::from_utf8_lossy(&projects_output.stdout).contains("- myapp")
    );

    let services_output =
        nirion_command(&project_file, &lock_file, &docker_script)
            .arg("list")
            .arg("myapp")
            .output()
            .unwrap();
    assert_success(&services_output);
    assert!(String::from_utf8_lossy(&services_output.stdout).contains("- web"));
}

#[test]
fn cat_prints_project_and_service_compose() {
    let dir = TempDir::new();
    let project_file = dir.path().join("projects.json");
    let lock_file = dir.path().join("nirion.lock");
    let compose_file = dir.path().join("compose.yml");
    let docker_script = dir.path().join("fake-docker.sh");
    let args_file = dir.path().join("docker-args");
    fs::write(
        &compose_file,
        r#"
services:
  web:
    image: nginx:latest
  db:
    image: postgres:latest
"#,
    )
    .unwrap();
    write_projects_with_compose(&project_file, &compose_file.to_string_lossy());
    fs::write(&lock_file, "{}").unwrap();
    write_fake_docker(&docker_script, &args_file, "", "", 0);

    let project_output =
        nirion_command(&project_file, &lock_file, &docker_script)
            .arg("cat")
            .arg("myapp")
            .output()
            .unwrap();
    assert_success(&project_output);
    let stdout = String::from_utf8_lossy(&project_output.stdout);
    assert!(stdout.contains("web:"));
    assert!(stdout.contains("db:"));

    let service_output =
        nirion_command(&project_file, &lock_file, &docker_script)
            .arg("cat")
            .arg("myapp.web")
            .output()
            .unwrap();
    assert_success(&service_output);
    let stdout = String::from_utf8_lossy(&service_output.stdout);
    assert!(stdout.contains("web:"));
    assert!(!stdout.contains("db:"));
}

#[test]
fn exec_forwards_options_and_command() {
    let dir = TempDir::new();
    let project_file = dir.path().join("projects.json");
    let lock_file = dir.path().join("nirion.lock");
    let docker_script = dir.path().join("fake-docker.sh");
    let args_file = dir.path().join("docker-args");
    write_projects(&project_file);
    fs::write(&lock_file, "{}").unwrap();
    write_fake_docker(&docker_script, &args_file, "", "", 0);

    let output = nirion_command(&project_file, &lock_file, &docker_script)
        .arg("exec")
        .arg("myapp.web")
        .arg("-T")
        .arg("-u")
        .arg("1000:1000")
        .arg("-w")
        .arg("/srv")
        .arg("--index")
        .arg("2")
        .arg("-e")
        .arg("FOO=bar")
        .arg("--privileged")
        .arg("printenv")
        .output()
        .unwrap();

    assert_success(&output);
    assert_eq!(
        fs::read_to_string(args_file).unwrap(),
        "compose\n--file\ncompose.yml\n--project-name\nmyapp\nexec\n-T\n-u\n1000:1000\n-w\n/srv\n--index\n2\n-e\nFOO=bar\n--privileged\nweb\nprintenv\n"
    );
}

#[test]
fn exec_requires_command() {
    let dir = TempDir::new();
    let project_file = dir.path().join("projects.json");
    let lock_file = dir.path().join("nirion.lock");
    let docker_script = dir.path().join("fake-docker.sh");
    let args_file = dir.path().join("docker-args");
    write_projects(&project_file);
    fs::write(&lock_file, "{}").unwrap();
    write_fake_docker(&docker_script, &args_file, "", "", 0);

    let output = nirion_command(&project_file, &lock_file, &docker_script)
        .arg("exec")
        .arg("myapp.web")
        .output()
        .unwrap();

    assert_failure(&output);
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("No command specified for exec")
    );
}

#[test]
fn reload_legacy_runs_down_then_up() {
    let dir = TempDir::new();
    let project_file = dir.path().join("projects.json");
    let lock_file = dir.path().join("nirion.lock");
    let docker_script = dir.path().join("fake-docker.sh");
    let args_file = dir.path().join("docker-args");
    write_projects(&project_file);
    fs::write(&lock_file, "{}").unwrap();
    write_fake_docker_append(&docker_script, &args_file, "", "", 0);

    let output = nirion_command(&project_file, &lock_file, &docker_script)
        .arg("reload")
        .arg("myapp")
        .arg("--no-tui")
        .output()
        .unwrap();

    assert_success(&output);
    assert_eq!(
        fs::read_to_string(args_file).unwrap(),
        "---\ncompose\n--file\ncompose.yml\n--project-name\nmyapp\ndown\n---\ncompose\n--file\ncompose.yml\n--project-name\nmyapp\nup\n-d\n"
    );
}

#[test]
fn lock_and_update_report_no_images() {
    let dir = TempDir::new();
    let project_file = dir.path().join("projects.json");
    let lock_file = dir.path().join("nirion.lock");
    let docker_script = dir.path().join("fake-docker.sh");
    let args_file = dir.path().join("docker-args");
    write_empty_projects(&project_file);
    fs::write(&lock_file, "{}").unwrap();
    write_fake_docker(&docker_script, &args_file, "", "", 0);

    for command in ["lock", "update"] {
        let output = nirion_command(&project_file, &lock_file, &docker_script)
            .arg(command)
            .arg("empty")
            .output()
            .unwrap();

        assert_success(&output);
        assert!(
            String::from_utf8_lossy(&output.stdout)
                .contains("No images found to update"),
            "failed command: {command}"
        );
    }
}

#[test]
fn patch_invokes_sudo_mirage_patch_for_project_compose() {
    let dir = TempDir::new();
    let project_file = dir.path().join("projects.json");
    let lock_file = dir.path().join("nirion.lock");
    let docker_script = dir.path().join("fake-docker.sh");
    let docker_args_file = dir.path().join("docker-args");
    let sudo_script = dir.path().join("sudo");
    let sudo_args_file = dir.path().join("sudo-args");
    write_projects(&project_file);
    fs::write(&lock_file, "{}").unwrap();
    write_fake_docker(&docker_script, &docker_args_file, "", "", 0);
    write_fake_sudo(&sudo_script, &sudo_args_file);

    let mut command = nirion_command(&project_file, &lock_file, &docker_script);
    command
        .env(
            "PATH",
            format!(
                "{}:{}",
                dir.path().display(),
                env::var("PATH").unwrap_or_default()
            ),
        )
        .arg("patch")
        .arg("myapp");

    let output = command.output().unwrap();

    assert_success(&output);
    assert_eq!(
        fs::read_to_string(sudo_args_file).unwrap(),
        "mirage-patch\ncompose.yml\n"
    );
}
