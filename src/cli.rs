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

    /// Override automatic Java detection
    #[arg(long = "runtime", short = 'r', value_name = "PATH", global = true)]
    pub java_runtime_path: Option<String>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// List all available Minecraft versions
    #[command(long_about = "Show official version list, including version type (release/snapshot/etc)")]
    List,

    /// List detected Java runtimes
    #[command(long_about = "Scan JAVA_HOME, PATH, and common install directories for Java runtimes")]
    Java,

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

        /// Path to authlib-injector.jar (for external auth, skips auto-download)
        #[arg(long = "authlib-jar")]
        authlib_jar: Option<String>,
    },

    /// Login to Microsoft account
    #[command(long_about = "Login to Microsoft account via device code flow to get access token for launching game")]
    Login,

    /// Login to external authentication server (authlib-injector)
    #[command(long_about = "Login to external Yggdrasil authentication server")]
    ExternalLogin {
        /// Account identifier (email or username)
        identifier: String,

        /// Password (omit to be prompted; can also be set via MCLC_PASSWORD)
        #[arg(short = 'p', long)]
        password: Option<String>,

        /// API URL of the auth server
        #[arg(short = 'a', long = "api-url")]
        api_url: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn external_login_password_is_optional() {
        let cli = Cli::try_parse_from([
            "mclc",
            "external-login",
            "alice",
            "--api-url",
            "https://example.invalid",
        ])
        .unwrap();

        match cli.command {
            Commands::ExternalLogin { password, .. } => assert!(password.is_none()),
            _ => panic!("unexpected command"),
        }
    }
}
