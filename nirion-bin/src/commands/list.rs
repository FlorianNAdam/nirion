use clap::Parser;
use nirion_lib::{lock::LockedImages, projects::Projects};
use std::path::Path;

use crate::{ClapSelector, TargetSelector};

/// List projects or services
#[derive(Parser, Debug, Clone)]
pub struct ListArgs {
    /// Target selector: *, project, or project.service
    #[arg(
        default_value = "*",
        value_parser = TargetSelector::clap_parse,
        add = TargetSelector::clap_completer()
    )]
    pub target: TargetSelector,
}

pub async fn handle_list(
    args: &ListArgs,
    projects: &Projects,
    _locked_images: &LockedImages,
    _lock_file: &Path,
) -> anyhow::Result<()> {
    match &args.target {
        TargetSelector::All => {
            println!("Projects:");
            for (project_name, _) in projects.iter() {
                println!("- {}", project_name);
            }
        }

        TargetSelector::Project(proj) => {
            let project = &projects[&proj.name];
            println!("Images in project '{}':", proj.name);
            for image in project.services.keys() {
                println!("- {}", image);
            }
        }

        TargetSelector::Service(img) => {
            println!(
                "Selector '{}' refers to a specific service. Printing that one only:",
                format!("{}.{}", img.project, img.service)
            );
            println!("- {}", img.service);
        }
    }
    Ok(())
}
