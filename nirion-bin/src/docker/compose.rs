use anyhow::Context;
use crossterm::style::Stylize;
use nirion_lib::{compose::compose_cmd, projects::Projects};

use crate::TargetSelector;

pub async fn compose_target_cmd(
    target: &TargetSelector,
    projects: &Projects,
    args: &[&str],
) -> anyhow::Result<()> {
    let mut failures = Vec::new();

    match target {
        TargetSelector::All => {
            for (name, project) in projects.iter() {
                println!("[{}]", name.to_string().cyan());

                let compose_file = &project.docker_compose;
                let project_name = &project.name;

                if let Err(e) =
                    compose_cmd(compose_file, project_name, &args).await
                {
                    eprintln!("Project '{}' failed: {}", name, e);
                    failures.push(format!("{name}: {e}"));
                }

                println!()
            }
        }

        TargetSelector::Project(proj) => {
            let project = &projects[&proj.name];

            let compose_file = &project.docker_compose;
            let project_name = &project.name;

            compose_cmd(compose_file, project_name, &args)
                .await
                .with_context(|| format!("Project '{}' failed", proj.name))?;
        }

        TargetSelector::Service(img) => {
            let project = &projects[&img.project];

            let compose_file = &project.docker_compose;
            let project_name = &project.name;

            let mut cmd_args = args.to_vec();
            cmd_args.push(&img.service);

            if let Err(e) =
                compose_cmd(compose_file, project_name, &cmd_args).await
            {
                anyhow::bail!(
                    "Service '{}.{}' failed: {}",
                    img.project,
                    img.service,
                    e
                );
            }
        }
    }

    if !failures.is_empty() {
        anyhow::bail!(
            "docker compose failed for {} project(s): {}",
            failures.len(),
            failures.join("; ")
        );
    }

    Ok(())
}
