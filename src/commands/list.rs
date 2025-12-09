use clap::Parser;
use std::collections::BTreeMap;

use crate::{Project, TargetSelector};

#[derive(Parser, Debug, Clone)]
pub struct ListArgs {
    /// Target selector: *, project, or project.service
    #[arg(default_value = "*", value_parser = crate::clap_parse_selector)]
    pub target: TargetSelector,
}

pub fn handle_list(
    args: &ListArgs,
    projects: &BTreeMap<String, Project>,
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

        TargetSelector::Image(img) => {
            println!(
                "Selector '{}' refers to a specific image. Printing that one only:",
                format!("{}.{}", img.project, img.image)
            );
            println!("- {}", img.image);
        }
    }
    Ok(())
}
