use clap::{Parser, Subcommand, ValueEnum};

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
pub enum AuthType {
    Offline,
    Msa,
}

#[derive(Parser)]
#[command(name = "mclc")]
#[command(override_usage = "mclc <COMMAND> <OPTIONS>")]
#[command(disable_version_flag = true)]
#[command(about = "A simple Minecraft launcher core.")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
    #[arg(long = "runtime", short = 'r', value_name = "PATH", global = true)]
    pub java_runtime_path: Option<String>,
}

#[derive(Subcommand)]
pub enum Commands {
    List,
    Install {
        version: String,
    },
    #[command(about = "Minecraft Launcher Cli", long_about = "A Simple Minecraft Launcher")]
    Launch {
        version: String,
        #[arg(short, long)]
        username: Option<String>,
        #[arg(long)]
        access_token: Option<String>,
        #[arg(short, long)]
        jvm_args: Option<String>,
        #[arg(long, value_enum, default_value_t = AuthType::Offline)]
        auth_type: AuthType,
    },
    Login,
}
