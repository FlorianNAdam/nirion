use nirion_lib::lock::LockedImages;
use nirion_lib::projects::Projects;
use nirion_oci_lib::client::AuthConfig;
use paste::paste;
use std::path::PathBuf;

use clap::Subcommand;

pub struct NirionContext {
    pub projects: Projects,
    pub locked_images: LockedImages,
    pub lock_file: PathBuf,
    pub auth: AuthConfig,
}

macro_rules! define_commands {
    (
        [ $( $modname:ident ),* $(,)? ]
    ) => {
        paste! {
            $(
                pub mod $modname;
                use crate::commands::$modname::{ [<handle_ $modname>], [<$modname:camel Args>] };
            )*

            #[derive(Subcommand)]
            pub enum Commands {
                $(
                    [<$modname:camel>] {
                        #[command(flatten)]
                        args: [<$modname:camel Args>],
                    },
                )*
            }

            pub async fn handle_command(
                command: &Commands,
                context: &NirionContext
            ) -> anyhow::Result<()> {
                match command {
                    $(
                        Commands::[<$modname:camel>] { args } =>
                            [<handle_ $modname>](args, context).await?,
                    )*
                }
                Ok(())
            }
        }
    };
}

define_commands!([
    up,
    down,
    reload,
    start,
    stop,
    list,
    update,
    lock,
    exec,
    logs,
    cat,
    ps,
    top,
    volumes,
    restart,
    compose_exec,
    monitor,
    patch,
    inspect
]);
