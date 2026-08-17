use anyhow::Result;
use clap::{Args, Subcommand};
use nirion_lib::{
    backend::NirionBackend,
    inspect::{InspectKind, InspectQuery},
    projects::TargetSelector,
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
    backend: &dyn NirionBackend,
) -> Result<()> {
    match &args.command {
        InspectCommand::Container(args) => {
            inspect_targets(args, backend, InspectKind::Container).await?
        }
        InspectCommand::Image(args) => {
            inspect_targets(args, backend, InspectKind::Image).await?
        }
    }

    Ok(())
}

async fn inspect_targets(
    args: &InspectTargetArgs,
    backend: &dyn NirionBackend,
    kind: InspectKind,
) -> Result<()> {
    for output in backend
        .inspect(InspectQuery {
            target: args.target.clone(),
            kind,
            format: args.format.clone(),
            raw: args.raw,
        })
        .await?
    {
        println!("{output}");
    }

    Ok(())
}
