use anyhow::Result;
use clap::Args;
use nirion_lib::{
    compose_file::{compose_to_string, full_compose, service_compose},
    context::NirionContext,
    projects::TargetSelector,
};

use crate::ClapSelector;

/// Print the docker compose file
#[derive(Args, Debug, Clone)]
pub struct CatArgs {
    /// Target selector: *, project, or project.service
    #[arg(
        default_value = "*",
        value_parser = TargetSelector::clap_parse,
        add = TargetSelector::clap_completer()
    )]
    pub target: TargetSelector,
}

pub async fn handle_cat(
    args: &CatArgs,
    context: &NirionContext,
) -> Result<()> {
    match &args.target {
        TargetSelector::All => {
            for (project_name, project) in context.projects.iter() {
                println!("Project {}:", project_name);
                print_compose(&full_compose(project)?)?;
            }
        }
        TargetSelector::Project(proj) => {
            let project = &context.projects[&proj.name];
            print_compose(&full_compose(project)?)?;
        }
        TargetSelector::Service(img) => {
            let project = &context.projects[&img.project];
            print_compose(&service_compose(
                &img.project,
                project,
                &img.service,
            )?)?;
        }
    }

    Ok(())
}

fn print_compose(compose: &serde_yaml_ng::Value) -> Result<()> {
    let pretty = compose_to_string(compose)?;
    println!("{}", pretty);
    println!();
    Ok(())
}
