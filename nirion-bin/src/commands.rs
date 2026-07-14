use paste::paste;

use clap::Subcommand;
use nirion_lib::context::NirionContext;

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
