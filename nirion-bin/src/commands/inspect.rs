use anyhow::Result;
use clap::{Parser, ValueEnum};
use nirion_lib::{
    context::NirionContext,
    inspect::{
        inspect_project, inspect_service, InspectTarget as LibInspectTarget,
    },
    projects::{ProjectSelector, TargetSelector},
};

use crate::ClapSelector;

/// Patch service files using mirage-patch
#[derive(Parser, Debug, Clone)]
pub struct InspectArgs {
    /// Target selector: *, project, or project.service
    #[arg(
        default_value = "*",
        value_parser = TargetSelector::clap_parse,
        add = TargetSelector::clap_completer()
    )]
    pub target: TargetSelector,

    /// What to inspect
    #[arg(short, long, value_enum, default_value = "container")]
    inspect_target: InspectTarget,

    /// The inspect format
    #[arg(short, long, default_value = "json")]
    format: String,

    /// Print json without pretty printing
    #[arg(short, long)]
    raw: bool,
}

#[derive(Clone, Debug, ValueEnum, PartialEq, Eq)]
enum InspectTarget {
    Image,
    Container,
}

impl From<&InspectTarget> for LibInspectTarget {
    fn from(value: &InspectTarget) -> Self {
        match value {
            InspectTarget::Image => Self::Image,
            InspectTarget::Container => Self::Container,
        }
    }
}

pub async fn handle_inspect(
    args: &InspectArgs,
    context: &NirionContext,
) -> Result<()> {
    let inspect_target = LibInspectTarget::from(&args.inspect_target);

    match &args.target {
        TargetSelector::All => {
            for (project_name, _) in context.projects.iter() {
                let project_selector = ProjectSelector {
                    name: project_name.to_string(),
                };
                for output in inspect_project(
                    context,
                    &project_selector,
                    &inspect_target,
                    &args.format,
                    args.raw,
                )
                .await?
                {
                    println!("{output}");
                }
            }
        }
        TargetSelector::Project(proj) => {
            for output in inspect_project(
                context,
                proj,
                &inspect_target,
                &args.format,
                args.raw,
            )
            .await?
            {
                println!("{output}");
            }
        }
        TargetSelector::Service(img) => {
            let output = inspect_service(
                context,
                img,
                &inspect_target,
                &args.format,
                args.raw,
            )
            .await?;
            println!("{output}");
        }
    }
    Ok(())
}
