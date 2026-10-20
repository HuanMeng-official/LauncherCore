use crate::download::DownloadSource;
use crate::loader::LoaderKind;
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
#[command(about = "Command-line Minecraft launcher")]
#[command(long_about = "LauncherCore is a command-line Minecraft launcher with mod loader installation, multi-source downloads, automatic Java detection, and Microsoft/Yggdrasil authentication.")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Path to the Java executable (overrides automatic detection)
    #[arg(long = "runtime", short = 'r', value_name = "PATH", global = true)]
    pub java_runtime_path: Option<String>,

    /// Download source: auto, official, or bmclapi
    #[arg(
        long = "download-source",
        value_enum,
        default_value = "auto",
        global = true
    )]
    pub download_source: DownloadSource,
}

#[derive(Subcommand)]
pub enum Commands {
    /// List all available Minecraft versions
    #[command(long_about = "Fetch and list all official Minecraft versions from Mojang's version manifest, including release, snapshot, and older version types.")]
    List,

    /// List detected Java runtimes
    #[command(long_about = "Scan JAVA_HOME, PATH, and common installation directories, then list the detected Java runtimes with their versions and architectures.")]
    Java,

    /// Install a Minecraft version
    #[command(long_about = "Install a Minecraft version and optionally Fabric, Quilt, Forge, or NeoForge. The vanilla parent version is installed first. Downloads are verified and can use the official source or the BMCLAPI mirror.")]
    Install {
        /// Minecraft version to install (e.g., 1.21.1)
        version: String,

        /// Mod loader to install (fabric, quilt, forge, neoforge)
        #[arg(long, value_enum)]
        loader: Option<LoaderKind>,

        /// Loader version to install (defaults to the stable/recommended release)
        #[arg(long = "loader-version")]
        loader_version: Option<String>,
    },

    /// Launch an installed Minecraft version
    #[command(long_about = "Launch an installed Minecraft version. Java is selected automatically, and the version's inheritance chain and manifest arguments are resolved. Use 'mclc login' for Microsoft authentication or 'mclc external-login' for Yggdrasil.")]
    Launch {
        /// Installed version ID to launch
        version: String,

        /// Game username (offline) or account identifier (external auth)
        #[arg(short = 'u', long)]
        username: Option<String>,

        /// Existing Minecraft access token (MSA authentication)
        #[arg(long)]
        access_token: Option<String>,

        /// Custom JVM arguments (e.g., -Xmx4G -XX:+UseG1GC)
        #[arg(short = 'j', long, value_name = "ARGS")]
        jvm_args: Option<String>,

        /// Authentication type (offline, msa, external)
        #[arg(long = "auth", value_enum, default_value_t = AuthType::Offline)]
        auth_type: AuthType,

        /// Yggdrasil API URL (required for external auth)
        #[arg(long = "api-url")]
        api_url: Option<String>,

        /// Path to authlib-injector.jar (external auth; skips auto-download)
        #[arg(long = "authlib-jar")]
        authlib_jar: Option<String>,
    },

    /// Login to a Microsoft account
    #[command(long_about = "Login to a Microsoft account via the device code flow. The access and refresh tokens are cached, and credentials are refreshed automatically before launch.")]
    Login,

    /// Login to an external authentication server (authlib-injector)
    #[command(long_about = "Login to an external Yggdrasil authentication server. The API URL is resolved through its ALI header, and the account is cached for 'launch --auth external'.")]
    ExternalLogin {
        /// Account identifier (email or username)
        identifier: String,

        /// Password (omit to be prompted; can also be set via MCLC_PASSWORD)
        #[arg(short = 'p', long)]
        password: Option<String>,

        /// API URL of the Yggdrasil authentication server
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
