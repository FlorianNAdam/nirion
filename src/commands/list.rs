use clap::Parser;
use std::{collections::BTreeMap, path::Path};

use crate::{Project, TargetSelector};

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
    projects: &BTreeMap<String, Project>,
    _locked_images: &BTreeMap<String, String>,
    _lock_file: &Path,
) -> anyhow::Result<()> {
    match &args.target {
        TargetSelector::All => {
            println!("Projects:");
            for project_name in projects.keys() {
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
