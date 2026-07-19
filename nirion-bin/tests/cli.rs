use std::{
    env, fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::Duration,
};

use nirion_tui_lib::ansi::strip_ansi_codes;

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

fn write_completion_projects(path: &Path) {
    fs::write(
        path,
        r#"{
  "app": {
    "name": "app",
    "dockerCompose": "app.yml",
    "services": {
      "web": {
        "image": "nginx:latest",
        "healthcheck": false,
        "restart": null
      },
      "worker": {
        "image": "alpine:latest",
        "healthcheck": false,
        "restart": null
      }
    }
  },
  "app2": {
    "name": "app2",
    "dockerCompose": "app2.yml",
    "services": {
      "web": {
        "image": "nginx:latest",
        "healthcheck": false,
        "restart": null
      }
    }
  },
  "auth": {
    "name": "auth",
    "dockerCompose": "auth.yml",
    "services": {
      "server": {
        "image": "authelia/authelia:latest",
        "healthcheck": false,
        "restart": null
      }
    }
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

fn write_fake_inspect_container_docker(
    path: &Path,
    args_file: &Path,
    inspect_stdout: &str,
) {
    fs::write(
        path,
        format!(
            r#"printf '%s\n' '---' >> '{}'
printf '%s\n' "$@" >> '{}'
if [ "$1" = "compose" ]; then
  printf '%s\n' '{}'
  exit 0
fi
printf '%s\n' '{}'
"#,
            args_file.display(),
            args_file.display(),
            ps_status_json(),
            inspect_stdout,
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

fn fish_completion(
    project_file: &Path,
    words: &[&str],
) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_nirion"))
        .env("COMPLETE", "fish")
        .env("NIRION_PROJECT_FILE", project_file)
        .arg("--")
        .args(words)
        .output()
        .unwrap();

    assert_success(&output);
    String::from_utf8(output.stdout).unwrap()
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

fn stop_child(child: &mut std::process::Child) {
    let _ = child.kill();
    child.wait().unwrap();
}

fn ps_status_json() -> &'static str {
    r#"[{"ID":"abc","Name":"myapp-web-1","Service":"web","Image":"nginx:latest","State":"running","Health":"healthy","ExitCode":0,"RunningFor":"2 minutes","Status":"Up 2 minutes (healthy)","Ports":"127.0.0.1:8080-8081->80-81/tcp","Networks":"default"}]"#
}

#[test]
fn version_does_not_require_files() {
    let output = Command::new(env!("CARGO_BIN_EXE_nirion"))
        .arg("--version")
        .output()
        .unwrap();

    assert_success(&output);
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("nirion {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn fish_completion_suggests_subcommands() {
    let output = Command::new(env!("CARGO_BIN_EXE_nirion"))
        .env("COMPLETE", "fish")
        .arg("--")
        .args(["nirion", "u"])
        .output()
        .unwrap();

    assert_success(&output);
    let output = String::from_utf8(output.stdout).unwrap();

    assert!(output.contains("up\tCreate and start service containers\n"));
    assert!(output.contains("update\tUpdate lock file entries\n"));
}

#[test]
fn fish_completion_suggests_projects_and_services_for_target() {
    let dir = tempfile::tempdir().unwrap();
    let project_file = dir.path().join("projects.json");
    write_completion_projects(&project_file);

    let output = fish_completion(&project_file, &["nirion", "up", "a"]);

    assert!(output.contains("app\n"));
    assert!(output.contains("app.web\n"));
    assert!(output.contains("app.worker\n"));
    assert!(output.contains("app2\n"));
    assert!(output.contains("app2.web\n"));
    assert!(output.contains("auth\n"));
    assert!(output.contains("auth.server\n"));
}

#[test]
fn fish_completion_after_dot_requires_exact_project_match() {
    let dir = tempfile::tempdir().unwrap();
    let project_file = dir.path().join("projects.json");
    write_completion_projects(&project_file);

    let output = fish_completion(&project_file, &["nirion", "up", "app."]);

    assert!(output.contains("app.web\n"));
    assert!(output.contains("app.worker\n"));
    assert!(!output.contains("app2.web\n"));
}

#[test]
fn fish_completion_for_service_selector_suggests_services_only() {
    let dir = tempfile::tempdir().unwrap();
    let project_file = dir.path().join("projects.json");
    write_completion_projects(&project_file);

    let output = fish_completion(&project_file, &["nirion", "exec", "a"]);

    assert!(output.contains("app.web\n"));
    assert!(output.contains("app.worker\n"));
    assert!(output.contains("app2.web\n"));
    assert!(output.contains("auth.server\n"));
    assert!(!output.contains("app\n"));
    assert!(!output.contains("app2\n"));
    assert!(!output.contains("*\n"));
}

#[test]
fn fish_completion_for_service_selector_after_dot_requires_exact_project_match()
{
    let dir = tempfile::tempdir().unwrap();
    let project_file = dir.path().join("projects.json");
    write_completion_projects(&project_file);

    let output = fish_completion(&project_file, &["nirion", "exec", "app."]);

    assert!(output.contains("app.web\n"));
    assert!(output.contains("app.worker\n"));
    assert!(!output.contains("app2.web\n"));
}

#[test]
fn up_plain_uses_configured_docker_command() {
    let dir = tempfile::tempdir().unwrap();
    let project_file = dir.path().join("projects.json");
    let lock_file = dir.path().join("nirion.lock");
    let docker_script = dir.path().join("fake-docker.sh");
    let args_file = dir.path().join("docker-args");
    write_projects(&project_file);
    write_fake_docker(
        &docker_script,
        &args_file,
        "compose-out",
        "compose-err",
        0,
    );

    let output = nirion_command(&project_file, &lock_file, &docker_script)
        .arg("up")
        .arg("--plain")
        .arg("--jobs")
        .arg("1")
        .output()
        .unwrap();

    assert_success(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.contains("compose-out"));
    assert!(stderr.contains("compose-err"));
    assert_eq!(
        fs::read_to_string(args_file).unwrap(),
        "compose\n--file\ncompose.yml\n--project-name\nmyapp\nup\n-d\n"
    );
}

#[test]
fn up_service_target_appends_service_name() {
    let dir = tempfile::tempdir().unwrap();
    let project_file = dir.path().join("projects.json");
    let lock_file = dir.path().join("nirion.lock");
    let docker_script = dir.path().join("fake-docker.sh");
    let args_file = dir.path().join("docker-args");
    write_projects(&project_file);
    write_fake_docker(&docker_script, &args_file, "", "", 0);

    let output = nirion_command(&project_file, &lock_file, &docker_script)
        .arg("up")
        .arg("myapp.web")
        .arg("--plain")
        .output()
        .unwrap();

    assert_success(&output);
    assert_eq!(
        fs::read_to_string(args_file).unwrap(),
        "compose\n--file\ncompose.yml\n--project-name\nmyapp\nup\n-d\nweb\n"
    );
}

#[test]
fn up_quiet_suppresses_compose_output() {
    let dir = tempfile::tempdir().unwrap();
    let project_file = dir.path().join("projects.json");
    let lock_file = dir.path().join("nirion.lock");
    let docker_script = dir.path().join("fake-docker.sh");
    let args_file = dir.path().join("docker-args");
    write_projects(&project_file);
    write_fake_docker(
        &docker_script,
        &args_file,
        "compose-out",
        "compose-err",
        0,
    );

    let output = nirion_command(&project_file, &lock_file, &docker_script)
        .arg("up")
        .arg("--quiet")
        .output()
        .unwrap();

    assert_success(&output);
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    assert_eq!(
        fs::read_to_string(args_file).unwrap(),
        "compose\n--file\ncompose.yml\n--project-name\nmyapp\nup\n-d\n"
    );
}

#[test]
fn inspect_image_raw_prints_docker_output() {
    let dir = tempfile::tempdir().unwrap();
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
        .arg("image")
        .arg("myapp.web")
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
fn inspect_container_raw_prints_docker_output() {
    let dir = tempfile::tempdir().unwrap();
    let project_file = dir.path().join("projects.json");
    let lock_file = dir.path().join("nirion.lock");
    let docker_script = dir.path().join("fake-docker.sh");
    let args_file = dir.path().join("docker-args");
    write_projects(&project_file);
    write_fake_inspect_container_docker(
        &docker_script,
        &args_file,
        r#"{"Id":"container-id"}"#,
    );

    let output = nirion_command(&project_file, &lock_file, &docker_script)
        .arg("inspect")
        .arg("container")
        .arg("myapp.web")
        .arg("--raw")
        .output()
        .unwrap();

    assert_success(&output);
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains(r#"{"Id":"container-id"}"#)
    );
    let args = fs::read_to_string(args_file).unwrap();
    assert!(args.contains(
        "compose\n-f\ncompose.yml\n--project-name\nmyapp\nps\n-a\n--format\njson\n"
    ));
    assert!(args.contains("inspect\n--format\njson\nabc\n"));
}

#[test]
fn inspect_container_pretty_prints_docker_output_by_default() {
    let dir = tempfile::tempdir().unwrap();
    let project_file = dir.path().join("projects.json");
    let lock_file = dir.path().join("nirion.lock");
    let docker_script = dir.path().join("fake-docker.sh");
    let args_file = dir.path().join("docker-args");
    write_projects(&project_file);
    write_fake_inspect_container_docker(
        &docker_script,
        &args_file,
        r#"{"Id":"container-id"}"#,
    );

    let output = nirion_command(&project_file, &lock_file, &docker_script)
        .arg("inspect")
        .arg("container")
        .arg("myapp.web")
        .output()
        .unwrap();

    assert_success(&output);
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("\"Id\": \"container-id\"")
    );
    assert!(
        fs::read_to_string(args_file)
            .unwrap()
            .contains("inspect\n--format\njson\nabc\n")
    );
}

#[test]
fn inspect_project_target_prints_service_outputs() {
    let dir = tempfile::tempdir().unwrap();
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
        .arg("image")
        .arg("myapp")
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
fn inspect_container_project_target_prints_service_outputs() {
    let dir = tempfile::tempdir().unwrap();
    let project_file = dir.path().join("projects.json");
    let lock_file = dir.path().join("nirion.lock");
    let docker_script = dir.path().join("fake-docker.sh");
    let args_file = dir.path().join("docker-args");
    write_projects(&project_file);
    write_fake_inspect_container_docker(
        &docker_script,
        &args_file,
        r#"{"Id":"container-id"}"#,
    );

    let output = nirion_command(&project_file, &lock_file, &docker_script)
        .arg("inspect")
        .arg("container")
        .arg("myapp")
        .arg("--raw")
        .output()
        .unwrap();

    assert_success(&output);
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains(r#"{"Id":"container-id"}"#)
    );
    let args = fs::read_to_string(args_file).unwrap();
    assert!(args.contains(
        "compose\n-f\ncompose.yml\n--project-name\nmyapp\nps\n-a\n--format\njson\n"
    ));
    assert!(args.contains("inspect\n--format\njson\nabc\n"));
}

#[test]
fn inspect_all_target_prints_project_outputs() {
    let dir = tempfile::tempdir().unwrap();
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
        .arg("image")
        .arg("*")
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
fn inspect_container_all_target_prints_project_outputs() {
    let dir = tempfile::tempdir().unwrap();
    let project_file = dir.path().join("projects.json");
    let lock_file = dir.path().join("nirion.lock");
    let docker_script = dir.path().join("fake-docker.sh");
    let args_file = dir.path().join("docker-args");
    write_projects(&project_file);
    write_fake_inspect_container_docker(
        &docker_script,
        &args_file,
        r#"{"Id":"container-id"}"#,
    );

    let output = nirion_command(&project_file, &lock_file, &docker_script)
        .arg("inspect")
        .arg("container")
        .arg("*")
        .arg("--raw")
        .output()
        .unwrap();

    assert_success(&output);
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains(r#"{"Id":"container-id"}"#)
    );
    let args = fs::read_to_string(args_file).unwrap();
    assert!(args.contains(
        "compose\n-f\ncompose.yml\n--project-name\nmyapp\nps\n-a\n--format\njson\n"
    ));
    assert!(args.contains("inspect\n--format\njson\nabc\n"));
}

#[test]
fn compose_passthrough_commands_are_wired() {
    let cases: &[(&[&str], &str)] = &[
        (&["down", "--plain"], "down\n"),
        (&["start", "--plain"], "start\n"),
        (&["stop", "--plain"], "stop\n"),
        (&["restart", "--plain"], "restart\n"),
        (&["pull"], "pull\n"),
        (&["top"], "top\n"),
        (&["volumes"], "volumes\n--format\ntable\n"),
        (&["compose-exec", "*", "pull"], "pull\n"),
    ];

    for (args, expected_command_args) in cases {
        let dir = tempfile::tempdir().unwrap();
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

fn write_fake_logs_docker(
    path: &Path,
    args_file: &Path,
    stdout: &str,
    stderr: &str,
) {
    fs::write(
        path,
        format!(
            r#"printf '%s\n' '---' >> '{}'
printf '%s\n' "$@" >> '{}'
case "$1" in
  compose)
    printf '%s\n' '{}'
    ;;
  inspect)
    printf '%s\n' true
    ;;
  logs)
    printf '%s\n' '{}'
    printf '%s\n' '{}' >&2
    ;;
esac
"#,
            args_file.display(),
            args_file.display(),
            ps_status_json(),
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

#[test]
fn logs_discovers_container_and_streams_docker_logs() {
    let dir = tempfile::tempdir().unwrap();
    let project_file = dir.path().join("projects.json");
    let lock_file = dir.path().join("nirion.lock");
    let docker_script = dir.path().join("fake-docker.sh");
    let args_file = dir.path().join("docker-args");
    write_projects(&project_file);
    write_fake_logs_docker(
        &docker_script,
        &args_file,
        "stdout-line",
        "stderr-line",
    );

    let output = nirion_command(&project_file, &lock_file, &docker_script)
        .arg("logs")
        .arg("myapp.web")
        .output()
        .unwrap();

    assert_success(&output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "[myapp.web] stdout-line\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "[myapp.web] stderr-line\n"
    );
    let args = fs::read_to_string(args_file).unwrap();
    assert!(args.contains(
        "compose\n-f\ncompose.yml\n--project-name\nmyapp\nps\n-a\n--format\njson\n"
    ));
    assert!(args.contains("logs\nabc\n"));
}

#[test]
fn logs_reports_compose_ps_failure() {
    let dir = tempfile::tempdir().unwrap();
    let project_file = dir.path().join("projects.json");
    let lock_file = dir.path().join("nirion.lock");
    let docker_script = dir.path().join("fake-docker.sh");
    let args_file = dir.path().join("docker-args");
    write_projects(&project_file);
    write_fake_docker(&docker_script, &args_file, "", "compose ps failed", 17);

    let output = nirion_command(&project_file, &lock_file, &docker_script)
        .arg("logs")
        .arg("myapp.web")
        .output()
        .unwrap();

    assert_failure(&output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("docker compose ps failed with status"));
    assert!(stderr.contains("compose ps failed"));
    assert_eq!(
        fs::read_to_string(args_file).unwrap(),
        "compose\n-f\ncompose.yml\n--project-name\nmyapp\nps\n-a\n--format\njson\n"
    );
}

#[test]
fn logs_label_none_suppresses_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let project_file = dir.path().join("projects.json");
    let lock_file = dir.path().join("nirion.lock");
    let docker_script = dir.path().join("fake-docker.sh");
    let args_file = dir.path().join("docker-args");
    write_projects(&project_file);
    write_fake_logs_docker(&docker_script, &args_file, "stdout-line", "");

    let output = nirion_command(&project_file, &lock_file, &docker_script)
        .arg("logs")
        .arg("myapp.web")
        .arg("--label")
        .arg("none")
        .output()
        .unwrap();

    assert_success(&output);
    assert_eq!(String::from_utf8_lossy(&output.stdout), "stdout-line\n");
}

#[test]
fn logs_until_is_passed_to_docker_logs() {
    let dir = tempfile::tempdir().unwrap();
    let project_file = dir.path().join("projects.json");
    let lock_file = dir.path().join("nirion.lock");
    let docker_script = dir.path().join("fake-docker.sh");
    let args_file = dir.path().join("docker-args");
    write_projects(&project_file);
    write_fake_logs_docker(&docker_script, &args_file, "", "");

    let output = nirion_command(&project_file, &lock_file, &docker_script)
        .arg("logs")
        .arg("myapp.web")
        .arg("--until")
        .arg("2026-07-18T12:00:00Z")
        .output()
        .unwrap();

    assert_success(&output);
    assert!(
        fs::read_to_string(args_file)
            .unwrap()
            .contains("logs\n--until\n2026-07-18T12:00:00Z\nabc\n")
    );
}

#[test]
fn logs_until_conflicts_with_follow() {
    let dir = tempfile::tempdir().unwrap();
    let project_file = dir.path().join("projects.json");
    let lock_file = dir.path().join("nirion.lock");
    let docker_script = dir.path().join("fake-docker.sh");
    let args_file = dir.path().join("docker-args");
    write_projects(&project_file);
    write_fake_docker(&docker_script, &args_file, "", "", 0);

    let output = nirion_command(&project_file, &lock_file, &docker_script)
        .arg("logs")
        .arg("--follow")
        .arg("--until")
        .arg("2026-07-18T12:00:00Z")
        .output()
        .unwrap();

    assert_failure(&output);
    assert!(String::from_utf8_lossy(&output.stderr).contains("--follow"));
}

#[test]
fn logs_follow_reattaches_when_reader_exits_for_same_container() {
    let dir = tempfile::tempdir().unwrap();
    let project_file = dir.path().join("projects.json");
    let lock_file = dir.path().join("nirion.lock");
    let docker_script = dir.path().join("fake-docker.sh");
    let args_file = dir.path().join("docker-args");
    write_projects(&project_file);
    write_fake_logs_docker(&docker_script, &args_file, "stdout-line", "");

    let mut child = nirion_command(&project_file, &lock_file, &docker_script)
        .arg("logs")
        .arg("myapp.web")
        .arg("--follow")
        .arg("--refresh")
        .arg("10ms")
        .arg("--events")
        .arg("never")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("nirion logs exited early with status {status}");
        }

        let args = fs::read_to_string(&args_file).unwrap_or_default();
        if args
            .matches("logs\n--follow\nabc\n")
            .count()
            >= 2
        {
            stop_child(&mut child);
            return;
        }

        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for reattach; args:\n{args}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn logs_follow_does_not_reattach_to_exited_container() {
    let dir = tempfile::tempdir().unwrap();
    let project_file = dir.path().join("projects.json");
    let lock_file = dir.path().join("nirion.lock");
    let docker_script = dir.path().join("fake-docker.sh");
    let args_file = dir.path().join("docker-args");
    let ps_count_file = dir.path().join("ps-count");
    let logs_count_file = dir.path().join("logs-count");
    write_projects(&project_file);

    fs::write(
        &docker_script,
        format!(
            r#"printf '%s\n' '---' >> '{}'
printf '%s\n' "$@" >> '{}'
case "$1" in
  compose)
    count=$(cat '{}' 2>/dev/null || printf 0)
    count=$((count + 1))
    printf '%s' "$count" > '{}'
    if [ "$count" -eq 1 ]; then
      printf '%s\n' '{}'
    else
      printf '%s\n' '{}'
    fi
    ;;
  inspect)
    printf '%s\n' true
    ;;
  logs)
    count=$(cat '{}' 2>/dev/null || printf 0)
    count=$((count + 1))
    printf '%s' "$count" > '{}'
    if [ "$count" -eq 1 ]; then
      printf '%s\n' 'stdout-line'
    else
      printf '%s\n' 'historical-exited-line'
    fi
    ;;
esac
"#,
            args_file.display(),
            args_file.display(),
            ps_count_file.display(),
            ps_count_file.display(),
            ps_status_json(),
            r#"[{"ID":"abc","Name":"myapp-web-1","Service":"web","Image":"nginx:latest","State":"exited","Health":null,"ExitCode":0,"RunningFor":"","Status":"Exited (0)","Ports":"","Networks":"default"}]"#,
            logs_count_file.display(),
            logs_count_file.display(),
        ),
    )
    .unwrap();
    let mut permissions = fs::metadata(&docker_script)
        .unwrap()
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&docker_script, permissions).unwrap();

    let mut child = nirion_command(&project_file, &lock_file, &docker_script)
        .arg("logs")
        .arg("myapp.web")
        .arg("--follow")
        .arg("--refresh")
        .arg("10ms")
        .arg("--events")
        .arg("never")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("nirion logs exited early with status {status}");
        }

        let ps_count = fs::read_to_string(&ps_count_file)
            .ok()
            .and_then(|count| count.parse::<usize>().ok())
            .unwrap_or(0);
        let logs_count = fs::read_to_string(&logs_count_file)
            .ok()
            .and_then(|count| count.parse::<usize>().ok())
            .unwrap_or(0);
        if ps_count >= 2 && logs_count >= 2 {
            stop_child(&mut child);
            break;
        }

        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for exited status"
        );
        thread::sleep(Duration::from_millis(10));
    }

    assert_eq!(fs::read_to_string(logs_count_file).unwrap(), "2");
    let args = fs::read_to_string(args_file).unwrap();
    assert!(args.contains("logs\n--follow\nabc\n"));
    assert!(args.contains("logs\nabc\n"));
}

#[test]
fn logs_follow_treats_missing_container_as_transient_detach() {
    let dir = tempfile::tempdir().unwrap();
    let project_file = dir.path().join("projects.json");
    let lock_file = dir.path().join("nirion.lock");
    let docker_script = dir.path().join("fake-docker.sh");
    let args_file = dir.path().join("docker-args");
    let logs_count_file = dir.path().join("logs-count");
    write_projects(&project_file);

    fs::write(
        &docker_script,
        format!(
            r#"printf '%s\n' '---' >> '{}'
printf '%s\n' "$@" >> '{}'
case "$1" in
  compose)
    printf '%s\n' '{}'
    ;;
  inspect)
    count=$(cat '{}' 2>/dev/null || printf 0)
    if [ "$count" -eq 0 ]; then
      printf '%s\n' true
    else
      printf '%s\n' false
    fi
    ;;
  logs)
    count=$(cat '{}' 2>/dev/null || printf 0)
    count=$((count + 1))
    printf '%s' "$count" > '{}'
    if [ "$count" -eq 1 ]; then
      printf '%s\n' 'stdout-line'
    else
      printf '%s\n' 'Error response from daemon: No such container: abc' >&2
      exit 1
    fi
    ;;
esac
"#,
            args_file.display(),
            args_file.display(),
            ps_status_json(),
            logs_count_file.display(),
            logs_count_file.display(),
            logs_count_file.display(),
        ),
    )
    .unwrap();
    let mut permissions = fs::metadata(&docker_script)
        .unwrap()
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&docker_script, permissions).unwrap();

    let mut child = nirion_command(&project_file, &lock_file, &docker_script)
        .arg("logs")
        .arg("myapp.web")
        .arg("--follow")
        .arg("--refresh")
        .arg("10ms")
        .arg("--events")
        .arg("never")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("nirion logs exited early with status {status}");
        }

        let args = fs::read_to_string(&args_file).unwrap_or_default();
        if args
            .matches("inspect\n--format\n{{.State.Running}}\nabc\n")
            .count()
            >= 2
        {
            stop_child(&mut child);
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for second inspect; args:\n{args}"
        );
        thread::sleep(Duration::from_millis(10));
    }

    assert_eq!(fs::read_to_string(logs_count_file).unwrap(), "1");
}

#[test]
fn logs_rejects_removed_reconnect_option() {
    let dir = tempfile::tempdir().unwrap();
    let project_file = dir.path().join("projects.json");
    let lock_file = dir.path().join("nirion.lock");
    let docker_script = dir.path().join("fake-docker.sh");
    let args_file = dir.path().join("docker-args");
    write_projects(&project_file);
    write_fake_docker(&docker_script, &args_file, "", "", 0);

    let output = nirion_command(&project_file, &lock_file, &docker_script)
        .arg("logs")
        .arg("--reconnect")
        .output()
        .unwrap();

    assert_failure(&output);
    assert!(String::from_utf8_lossy(&output.stderr).contains("--reconnect"));
}

#[test]
fn pull_service_target_appends_service_name() {
    let dir = tempfile::tempdir().unwrap();
    let project_file = dir.path().join("projects.json");
    let lock_file = dir.path().join("nirion.lock");
    let docker_script = dir.path().join("fake-docker.sh");
    let args_file = dir.path().join("docker-args");
    write_projects(&project_file);
    write_fake_docker(&docker_script, &args_file, "", "", 0);

    let output = nirion_command(&project_file, &lock_file, &docker_script)
        .arg("pull")
        .arg("myapp.web")
        .output()
        .unwrap();

    assert_success(&output);
    assert_eq!(
        fs::read_to_string(args_file).unwrap(),
        "compose\n--file\ncompose.yml\n--project-name\nmyapp\npull\nweb\n"
    );
}

#[test]
fn list_prints_projects_and_services() {
    let dir = tempfile::tempdir().unwrap();
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
fn list_service_target_prints_only_selected_service() {
    let dir = tempfile::tempdir().unwrap();
    let project_file = dir.path().join("projects.json");
    let lock_file = dir.path().join("nirion.lock");
    let docker_script = dir.path().join("fake-docker.sh");
    let args_file = dir.path().join("docker-args");
    write_completion_projects(&project_file);
    fs::write(&lock_file, "{}").unwrap();
    write_fake_docker(&docker_script, &args_file, "", "", 0);

    let output = nirion_command(&project_file, &lock_file, &docker_script)
        .arg("list")
        .arg("app.worker")
        .output()
        .unwrap();

    assert_success(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Selector 'app.worker'"));
    assert!(stdout.contains("- worker"));
    assert!(!stdout.contains("- web"));
}

#[test]
fn cat_prints_project_and_service_compose() {
    let dir = tempfile::tempdir().unwrap();
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
fn cat_all_prints_each_project_with_heading() {
    let dir = tempfile::tempdir().unwrap();
    let project_file = dir.path().join("projects.json");
    let lock_file = dir.path().join("nirion.lock");
    let app_compose = dir.path().join("app.yml");
    let auth_compose = dir.path().join("auth.yml");
    let docker_script = dir.path().join("fake-docker.sh");
    let args_file = dir.path().join("docker-args");

    fs::write(
        &app_compose,
        r#"
services:
  web:
    image: nginx:latest
"#,
    )
    .unwrap();
    fs::write(
        &auth_compose,
        r#"
services:
  server:
    image: authelia/authelia:latest
"#,
    )
    .unwrap();
    fs::write(
        &project_file,
        format!(
            r#"{{
  "app": {{
    "name": "app",
    "dockerCompose": "{}",
    "services": {{
      "web": {{"image": "nginx:latest", "healthcheck": false, "restart": null}}
    }}
  }},
  "auth": {{
    "name": "auth",
    "dockerCompose": "{}",
    "services": {{
      "server": {{"image": "authelia/authelia:latest", "healthcheck": false, "restart": null}}
    }}
  }}
}}"#,
            app_compose.display(),
            auth_compose.display()
        ),
    )
    .unwrap();
    fs::write(&lock_file, "{}").unwrap();
    write_fake_docker(&docker_script, &args_file, "", "", 0);

    let output = nirion_command(&project_file, &lock_file, &docker_script)
        .arg("cat")
        .output()
        .unwrap();

    assert_success(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Project app:"));
    assert!(stdout.contains("Project auth:"));
    assert!(stdout.contains("web:"));
    assert!(stdout.contains("server:"));
}

#[test]
fn ps_prints_status_and_collapsed_ports_from_docker_json() {
    let dir = tempfile::tempdir().unwrap();
    let project_file = dir.path().join("projects.json");
    let lock_file = dir.path().join("nirion.lock");
    let docker_script = dir.path().join("fake-docker.sh");
    let args_file = dir.path().join("docker-args");
    write_projects(&project_file);
    fs::write(&lock_file, "{}").unwrap();
    write_fake_docker(&docker_script, &args_file, ps_status_json(), "", 0);

    let output = nirion_command(&project_file, &lock_file, &docker_script)
        .arg("ps")
        .arg("myapp")
        .output()
        .unwrap();

    assert_success(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stdout = strip_ansi_codes(&stdout);
    assert!(stdout.contains("[myapp]"));
    assert!(stdout.contains("myapp-web-1"));
    assert!(stdout.contains("2 minutes"));
    assert!(stdout.contains("healthy"));
    assert!(stdout.contains("8080-8081->80-81/tcp"));
    assert_eq!(
        fs::read_to_string(args_file).unwrap(),
        "compose\n-f\ncompose.yml\n--project-name\nmyapp\nps\n-a\n--format\njson\n"
    );
}

#[test]
fn ps_all_prints_status_for_all_projects() {
    let dir = tempfile::tempdir().unwrap();
    let project_file = dir.path().join("projects.json");
    let lock_file = dir.path().join("nirion.lock");
    let docker_script = dir.path().join("fake-docker.sh");
    let args_file = dir.path().join("docker-args");
    write_completion_projects(&project_file);
    fs::write(&lock_file, "{}").unwrap();
    write_fake_docker_append(
        &docker_script,
        &args_file,
        ps_status_json(),
        "",
        0,
    );

    let output = nirion_command(&project_file, &lock_file, &docker_script)
        .arg("ps")
        .arg("*")
        .output()
        .unwrap();

    assert_success(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stdout = strip_ansi_codes(&stdout);
    assert!(stdout.contains("[app]"));
    assert!(stdout.contains("[app2]"));
    assert!(stdout.contains("[auth]"));
    assert_eq!(stdout.matches("myapp-web-1").count(), 3);

    let args = fs::read_to_string(args_file).unwrap();
    assert!(args.contains(
        "compose\n-f\napp.yml\n--project-name\napp\nps\n-a\n--format\njson\n"
    ));
    assert!(args.contains(
        "compose\n-f\napp2.yml\n--project-name\napp2\nps\n-a\n--format\njson\n"
    ));
    assert!(args.contains(
        "compose\n-f\nauth.yml\n--project-name\nauth\nps\n-a\n--format\njson\n"
    ));
}

#[test]
fn ps_service_prints_only_selected_service() {
    let dir = tempfile::tempdir().unwrap();
    let project_file = dir.path().join("projects.json");
    let lock_file = dir.path().join("nirion.lock");
    let docker_script = dir.path().join("fake-docker.sh");
    let args_file = dir.path().join("docker-args");
    write_projects(&project_file);
    fs::write(&lock_file, "{}").unwrap();
    write_fake_docker(
        &docker_script,
        &args_file,
        r#"[{"ID":"abc","Name":"myapp-web-1","Service":"web","Image":"nginx:latest","State":"running","Health":"healthy","ExitCode":0,"RunningFor":"2 minutes","Status":"Up 2 minutes (healthy)","Ports":"","Networks":"default"},{"ID":"def","Name":"myapp-db-1","Service":"db","Image":"postgres:16","State":"running","Health":null,"ExitCode":0,"RunningFor":"3 minutes","Status":"Up 3 minutes","Ports":"","Networks":"default"}]"#,
        "",
        0,
    );

    let output = nirion_command(&project_file, &lock_file, &docker_script)
        .arg("ps")
        .arg("myapp.web")
        .output()
        .unwrap();

    assert_success(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stdout = strip_ansi_codes(&stdout);
    assert!(stdout.contains("[myapp]"));
    assert!(stdout.contains("myapp-web-1"));
    assert!(!stdout.contains("myapp-db-1"));
    assert_eq!(
        fs::read_to_string(args_file).unwrap(),
        "compose\n-f\ncompose.yml\n--project-name\nmyapp\nps\n-a\n--format\njson\n"
    );
}

#[test]
fn exec_forwards_options_and_command() {
    let dir = tempfile::tempdir().unwrap();
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
    let dir = tempfile::tempdir().unwrap();
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
fn reload_plain_runs_down_then_up() {
    let dir = tempfile::tempdir().unwrap();
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
        .arg("--plain")
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
    let dir = tempfile::tempdir().unwrap();
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
fn lock_skips_services_that_already_have_lock_entries() {
    let dir = tempfile::tempdir().unwrap();
    let project_file = dir.path().join("projects.json");
    let lock_file = dir.path().join("nirion.lock");
    let docker_script = dir.path().join("fake-docker.sh");
    let args_file = dir.path().join("docker-args");
    write_projects_with_compose(&project_file, "compose.yml");
    fs::write(
        &lock_file,
        r#"{
  "myapp.web": {
    "image": "nginx:latest",
    "version": "1.25.0",
    "digest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
  }
}"#,
    )
    .unwrap();
    write_fake_docker(&docker_script, &args_file, "", "", 0);

    let original_lock = fs::read_to_string(&lock_file).unwrap();
    let output = nirion_command(&project_file, &lock_file, &docker_script)
        .arg("lock")
        .arg("myapp.web")
        .output()
        .unwrap();

    assert_success(&output);
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("No images found to update")
    );
    assert_eq!(fs::read_to_string(lock_file).unwrap(), original_lock);
}

#[test]
fn update_invalid_image_reference_does_not_rewrite_lock_file() {
    let dir = tempfile::tempdir().unwrap();
    let project_file = dir.path().join("projects.json");
    let lock_file = dir.path().join("nirion.lock");
    let docker_script = dir.path().join("fake-docker.sh");
    let args_file = dir.path().join("docker-args");
    fs::write(
        &project_file,
        r#"{
  "myapp": {
    "name": "myapp",
    "dockerCompose": "compose.yml",
    "services": {
      "web": {
        "image": "not a valid image",
        "healthcheck": false,
        "restart": null
      }
    }
  }
}"#,
    )
    .unwrap();
    fs::write(&lock_file, "{}").unwrap();
    write_fake_docker(&docker_script, &args_file, "", "", 0);

    let output = nirion_command(&project_file, &lock_file, &docker_script)
        .arg("update")
        .arg("myapp.web")
        .output()
        .unwrap();

    assert_failure(&output);
    let stdout = String::from_utf8_lossy(&output.stdout).to_lowercase();
    assert!(stdout.contains("image"));
    assert!(
        stdout.contains("invalid")
            || (stdout.contains("not") && stdout.contains("valid"))
    );
    assert!(!stdout.contains("lock file updated successfully"));
    assert_eq!(fs::read_to_string(lock_file).unwrap(), "{}");
}
