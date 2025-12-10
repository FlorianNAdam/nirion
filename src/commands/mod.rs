use std::{collections::BTreeMap, path::Path};

use clap::Subcommand;

use crate::{
    commands::{
        cat::{handle_cat, CatArgs},
        down::{handle_down, DownArgs},
        exec::{handle_exec, ExecArgs},
        list::{handle_list, ListArgs},
        lock::{handle_lock, LockArgs},
        logs::{handle_logs, LogsArgs},
        ps::{handle_ps, PsArgs},
        top::{handle_top, TopArgs},
        up::{handle_up, UpArgs},
        update::{handle_update, UpdateArgs},
    },
    Project,
};

pub mod cat;
pub mod down;
pub mod exec;
pub mod list;
pub mod lock;
pub mod logs;
pub mod ps;
pub mod top;
pub mod up;
pub mod update;

#[derive(Subcommand)]
pub enum Commands {
    Up {
        #[command(flatten)]
        args: UpArgs,
    },
    Down {
        #[command(flatten)]
        args: DownArgs,
    },
    List {
        #[command(flatten)]
        args: ListArgs,
    },
    Update {
        #[command(flatten)]
        args: UpdateArgs,
    },
    Lock {
        #[command(flatten)]
        args: LockArgs,
    },
    Exec {
        #[command(flatten)]
        args: ExecArgs,
    },
    Logs {
        #[command(flatten)]
        args: LogsArgs,
    },
    Cat {
        #[command(flatten)]
        args: CatArgs,
    },
    Ps {
        #[command(flatten)]
        args: PsArgs,
    },
    Top {
        #[command(flatten)]
        args: TopArgs,
    },
}

pub async fn handle_command(
    command: &Commands,
    projects: &BTreeMap<String, Project>,
    locked_images: &BTreeMap<String, String>,
    lock_file: &Path,
) -> anyhow::Result<()> {
    match command {
        Commands::List { args } => handle_list(&args, &projects)?,
        Commands::Up { args } => handle_up(&args, &projects)?,
        Commands::Down { args } => handle_down(&args, &projects)?,
        Commands::Update { args } => {
            handle_update(&args, &projects, &locked_images, &lock_file).await?
        }
        Commands::Lock { args } => {
            handle_lock(&args, &projects, &locked_images, &lock_file).await?
        }
        Commands::Exec { args } => handle_exec(&args, &projects)?,
        Commands::Logs { args } => handle_logs(&args, &projects)?,
        Commands::Cat { args } => handle_cat(&args, &projects)?,
        Commands::Ps { args } => handle_ps(&args, &projects)?,
        Commands::Top { args } => handle_top(&args, &projects)?,
    }

    Ok(())
}
