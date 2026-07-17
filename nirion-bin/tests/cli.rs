use std::{
    env, fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use console::strip_ansi_codes;

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
    let dir = TempDir::new();
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
    let dir = TempDir::new();
    let project_file = dir.path().join("projects.json");
    write_completion_projects(&project_file);

    let output = fish_completion(&project_file, &["nirion", "up", "app."]);

    assert!(output.contains("app.web\n"));
    assert!(output.contains("app.worker\n"));
    assert!(!output.contains("app2.web\n"));
}

#[test]
fn fish_completion_for_service_selector_suggests_services_only() {
    let dir = TempDir::new();
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
    let dir = TempDir::new();
    let project_file = dir.path().join("projects.json");
    write_completion_projects(&project_file);

    let output = fish_completion(&project_file, &["nirion", "exec", "app."]);

    assert!(output.contains("app.web\n"));
    assert!(output.contains("app.worker\n"));
    assert!(!output.contains("app2.web\n"));
}

#[test]
fn up_plain_uses_configured_docker_command() {
    let dir = TempDir::new();
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
    let dir = TempDir::new();
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
        (&["down", "--plain"], "down\n"),
        (&["start", "--plain"], "start\n"),
        (&["stop", "--plain"], "stop\n"),
        (&["restart", "--plain"], "restart\n"),
        (&["pull"], "pull\n"),
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
fn logs_reconnect_re_runs_following_logs() {
    let dir = TempDir::new();
    let project_file = dir.path().join("projects.json");
    let lock_file = dir.path().join("nirion.lock");
    let docker_script = dir.path().join("fake-docker.sh");
    let args_file = dir.path().join("docker-args");
    write_projects(&project_file);
    write_fake_docker_append(&docker_script, &args_file, "", "", 0);

    let mut child = nirion_command(&project_file, &lock_file, &docker_script)
        .arg("logs")
        .arg("--follow")
        .arg("--reconnect")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("nirion logs exited early with status {status}");
        }

        let args = fs::read_to_string(&args_file).unwrap_or_default();
        if args.matches("---\n").count() >= 2 {
            child.kill().unwrap();
            child.wait().unwrap();
            assert!(args.contains("logs\n--follow\n"));
            return;
        }

        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for reconnect; args:\n{args}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn logs_reconnect_reports_successful_exits() {
    let dir = TempDir::new();
    let project_file = dir.path().join("projects.json");
    let lock_file = dir.path().join("nirion.lock");
    let docker_script = dir.path().join("fake-docker.sh");
    let args_file = dir.path().join("docker-args");
    write_projects(&project_file);
    write_fake_docker_append(&docker_script, &args_file, "", "", 0);

    let mut child = nirion_command(&project_file, &lock_file, &docker_script)
        .arg("logs")
        .arg("--follow")
        .arg("--reconnect")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        let args = fs::read_to_string(&args_file).unwrap_or_default();
        if args.matches("---\n").count() >= 2 {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(stderr.contains("logs exited; reconnecting"));
            return;
        }

        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for reconnect; args:\n{args}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn logs_reconnect_requires_follow() {
    let dir = TempDir::new();
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
    assert!(String::from_utf8_lossy(&output.stderr).contains("--follow"));
}

#[test]
fn pull_service_target_appends_service_name() {
    let dir = TempDir::new();
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
fn list_service_target_prints_only_selected_service() {
    let dir = TempDir::new();
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
fn cat_all_prints_each_project_with_heading() {
    let dir = TempDir::new();
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
    let dir = TempDir::new();
    let project_file = dir.path().join("projects.json");
    let lock_file = dir.path().join("nirion.lock");
    let docker_script = dir.path().join("fake-docker.sh");
    let args_file = dir.path().join("docker-args");
    write_projects(&project_file);
    fs::write(&lock_file, "{}").unwrap();
    write_fake_docker(
        &docker_script,
        &args_file,
        r#"[{"ID":"abc","Name":"myapp-web-1","Service":"web","Image":"nginx:latest","State":"running","Health":"healthy","ExitCode":0,"RunningFor":"2 minutes","Status":"Up 2 minutes (healthy)","Ports":"127.0.0.1:8080-8081->80-81/tcp","Networks":"default"}]"#,
        "",
        0,
    );

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
fn reload_plain_runs_down_then_up() {
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
fn lock_skips_services_that_already_have_lock_entries() {
    let dir = TempDir::new();
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
    let dir = TempDir::new();
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
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("Checking myapp.web: not a valid image")
    );
    assert!(
        !String::from_utf8_lossy(&output.stdout)
            .contains("Lock file updated successfully")
    );
    assert_eq!(fs::read_to_string(lock_file).unwrap(), "{}");
}
