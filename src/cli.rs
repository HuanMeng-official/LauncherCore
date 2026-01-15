use clap::{Parser, Subcommand, ValueEnum};

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
pub enum AuthType {
    #[value(name = "offline")]
    Offline,
    #[value(name = "msa")]
    Msa,
    #[value(name = "external")]
    External,
}

#[derive(Parser)]
#[command(name = "mclc")]
#[command(override_usage = "mclc <COMMAND> [OPTIONS]")]
#[command(disable_version_flag = true)]
#[command(about = "Minecraft Launcher Core")]
#[command(long_about = "Minecraft Launcher Core is a simple cli launcher")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Specify Java runtime path
    #[arg(long = "runtime", short = 'r', value_name = "PATH", global = true)]
    pub java_runtime_path: Option<String>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// List all available Minecraft versions
    #[command(long_about = "Show official version list, including version type (release/snapshot/etc)")]
    List,

    /// Install specified Minecraft version
    #[command(long_about = "Download all necessary files for the specified version, including client JAR, library files, and asset files")]
    Install {
        /// Version to install (e.g., 1.21.11, etc.)
        version: String,
    },

    /// Launch specified Minecraft version
    #[command(long_about = "Run installed Minecraft version. If using Microsoft authentication, run the login command first")]
    Launch {
        /// Version to launch
        version: String,

        /// Game username (required for offline mode)
        #[arg(short = 'u', long)]
        username: Option<String>,

        /// Microsoft access token (used for MSA authentication)
        #[arg(long)]
        access_token: Option<String>,

        /// Custom JVM arguments (e.g., -Xmx4G -XX:+UseG1GC)
        #[arg(short = 'j', long, value_name = "ARGS")]
        jvm_args: Option<String>,

        /// Authentication type
        #[arg(long = "auth", value_enum, default_value_t = AuthType::Offline)]
        auth_type: AuthType,

        /// Yggdrasil API URL (for external auth)
        #[arg(long = "api-url")]
        api_url: Option<String>,
    },

    /// Login to Microsoft account
    #[command(long_about = "Login to Microsoft account via device code flow to get access token for launching game")]
    Login,

    /// Login to external authentication server (authlib-injector)
    #[command(long_about = "Login to external Yggdrasil authentication server")]
    ExternalLogin {
        /// Account identifier (email or username)
        identifier: String,

        /// Password
        #[arg(short = 'p', long)]
        password: String,

        /// API URL of the auth server
        #[arg(short = 'a', long = "api-url")]
        api_url: String,
    },
}
