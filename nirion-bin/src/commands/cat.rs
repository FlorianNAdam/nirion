use anyhow::Result;
use clap::Parser;
use nirion_lib::{
    compose_file::{compose_to_string, full_compose, service_compose},
    lock::LockedImages,
    projects::{Projects, TargetSelector},
};
use nirion_oci_lib::client::AuthConfig;
use std::path::Path;

use crate::ClapSelector;

/// Print the docker compose file
#[derive(Parser, Debug, Clone)]
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
    projects: &Projects,
    _locked_images: &LockedImages,
    _lock_file: &Path,
    _auth: &AuthConfig,
) -> Result<()> {
    match &args.target {
        TargetSelector::All => {
            for (project_name, project) in projects.iter() {
                println!("Project {}:", project_name);
                print_compose(&full_compose(project)?)?;
            }
        }
        TargetSelector::Project(proj) => {
            let project = &projects[&proj.name];
            print_compose(&full_compose(project)?)?;
        }
        TargetSelector::Service(img) => {
            let project = &projects[&img.project];
            print_compose(&service_compose(
                &img.project,
                project,
                &img.service,
            )?)?;
        }
    }

    Ok(())
}

fn print_compose(compose: &serde_yml::Value) -> Result<()> {
    let pretty = compose_to_string(compose)?;
    println!("{}", pretty);
    println!();
    Ok(())
}
