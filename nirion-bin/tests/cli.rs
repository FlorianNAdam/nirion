use std::{
    fs,
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
    fs::write(
        path,
        r#"{
  "myapp": {
    "name": "myapp",
    "dockerCompose": "compose.yml",
    "services": {
      "web": {
        "image": "nginx:latest",
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

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
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

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
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

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
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

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains(r#"{"Id":"image-id"}"#)
    );
    assert_eq!(
        fs::read_to_string(args_file).unwrap(),
        "image\ninspect\n--format\njson\nnginx:latest\n"
    );
}
