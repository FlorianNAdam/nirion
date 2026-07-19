use clap::Args;
use nirion_lib::context::NirionContext;

use crate::{ClapSelector, TargetSelector};

/// List projects or services
#[derive(Args, Debug, Clone)]
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
    context: &NirionContext,
) -> anyhow::Result<()> {
    match &args.target {
        TargetSelector::All => {
            println!("Projects:");
            for (project_name, _) in context.projects.iter() {
                println!("- {}", project_name);
            }
        }

        TargetSelector::Project(proj) => {
            let project = &context.projects[&proj.name];
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
