use anyhow::Result;
use clap::{Args, Subcommand};
use nirion_lib::{
    context::NirionContext,
    inspect::{
        inspect_container, inspect_image, inspect_project_containers,
        inspect_project_images,
    },
    projects::{ProjectSelector, TargetSelector},
};

use crate::ClapSelector;

/// Inspect images and services
#[derive(Args, Debug, Clone)]
pub struct InspectArgs {
    #[command(subcommand)]
    command: InspectCommand,
}

#[derive(Subcommand, Debug, Clone)]
enum InspectCommand {
    /// Inspect service containers
    Container(InspectTargetArgs),

    /// Inspect service images
    Image(InspectTargetArgs),
}

#[derive(Args, Debug, Clone)]
struct InspectTargetArgs {
    /// Target selector: *, project, or project.service
    #[arg(
        default_value = "*",
        value_parser = TargetSelector::clap_parse,
        add = TargetSelector::clap_completer()
    )]
    pub target: TargetSelector,

    /// The inspect format
    #[arg(short, long, default_value = "json")]
    format: String,

    /// Print json without pretty printing
    #[arg(short, long)]
    raw: bool,
}

pub async fn handle_inspect(
    args: &InspectArgs,
    context: &NirionContext,
) -> Result<()> {
    match &args.command {
        InspectCommand::Container(args) => {
            inspect_containers(args, context).await?
        }
        InspectCommand::Image(args) => inspect_images(args, context).await?,
    }

    Ok(())
}

async fn inspect_containers(
    args: &InspectTargetArgs,
    context: &NirionContext,
) -> Result<()> {
    match &args.target {
        TargetSelector::All => {
            for (project_name, _) in context.projects.iter() {
                let project_selector = ProjectSelector {
                    name: project_name.to_string(),
                };
                for output in inspect_project_containers(
                    context,
                    &project_selector,
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
            for output in inspect_project_containers(
                context,
                proj,
                &args.format,
                args.raw,
            )
            .await?
            {
                println!("{output}");
            }
        }
        TargetSelector::Service(img) => {
            let output =
                inspect_container(context, img, &args.format, args.raw).await?;
            println!("{output}");
        }
    }
    Ok(())
}

async fn inspect_images(
    args: &InspectTargetArgs,
    context: &NirionContext,
) -> Result<()> {
    match &args.target {
        TargetSelector::All => {
            for (project_name, _) in context.projects.iter() {
                let project_selector = ProjectSelector {
                    name: project_name.to_string(),
                };
                for output in inspect_project_images(
                    context,
                    &project_selector,
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
            for output in
                inspect_project_images(context, proj, &args.format, args.raw)
                    .await?
            {
                println!("{output}");
            }
        }
        TargetSelector::Service(img) => {
            let output =
                inspect_image(context, img, &args.format, args.raw).await?;
            println!("{output}");
        }
    }
    Ok(())
}
