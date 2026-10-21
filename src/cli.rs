use crate::game::loader::LoaderKind;
use crate::net::download::DownloadSource;
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

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
#[command(
    long_about = "LauncherCore is a command-line Minecraft launcher with mod loader installation, multi-source downloads, automatic Java detection, and Microsoft/Yggdrasil authentication."
)]
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

    /// Named game instance to use (see 'mclc instance list')
    #[arg(long, value_name = "NAME", global = true, conflicts_with = "game_dir")]
    pub instance: Option<String>,

    /// Game directory containing versions, libraries, and assets (default: ./.minecraft)
    #[arg(
        long = "game-dir",
        value_name = "PATH",
        global = true,
        conflicts_with = "instance"
    )]
    pub game_dir: Option<PathBuf>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// List all available Minecraft versions
    #[command(
        long_about = "Fetch and list all official Minecraft versions from Mojang's version manifest, including release, snapshot, and older version types."
    )]
    List,

    /// List detected Java runtimes
    #[command(
        long_about = "Scan JAVA_HOME, PATH, and common installation directories, then list the detected Java runtimes with their versions and architectures."
    )]
    Java,

    /// Manage named game instances
    #[command(
        alias = "instances",
        long_about = "Manage named game instances. Each instance is an independent game directory containing versions, libraries, assets, saves, and options. Create one with 'mclc instance create <name>', then run any command with '--instance <name>'. Without --instance or --game-dir, commands use ./.minecraft."
    )]
    Instance {
        #[command(subcommand)]
        action: InstanceAction,
    },

    /// Run the persistent JSON communication server
    #[command(
        long_about = "Run a persistent NDJSON server for GUI integration. The launcher reads one JSON request per line from stdin and writes one JSON event per line to stdout. Commands: ping, shutdown, java.list, versions.list, installed.list, install, launch, login, external_login, accounts.list. Events: ready, log, progress, device_code, profiles, result, error. Each request has a client-chosen \"id\"; results and errors echo it back so concurrent work can be correlated. Requests are processed sequentially."
    )]
    Json,

    /// Start the local web backend for a browser frontend
    #[command(
        long_about = "Start a local HTTP/WebSocket bridge for a browser-based frontend. The same JSON request/event protocol as 'mclc json' is exposed at ws://HOST:PORT/ws; GET /health returns launcher status. The launcher subprocess is started automatically with the selected instance or game directory. The server binds to 127.0.0.1 by default. Browser origins on localhost, 127.0.0.1, and ::1 are allowed; use --allow-origin for other frontends. With --token, clients must pass ?token=... or an Authorization: Bearer header. A token is required when binding to a non-loopback address. The web backend does not serve frontend assets; host the frontend separately."
    )]
    Web {
        /// Port to listen on (0 chooses a free port)
        #[arg(long, default_value_t = 7420, value_name = "PORT")]
        port: u16,

        /// Address to bind (loopback only by default)
        #[arg(long, default_value = "127.0.0.1", value_name = "ADDRESS")]
        host: String,

        /// Require this token from connecting clients
        #[arg(long, value_name = "TOKEN")]
        token: Option<String>,

        /// Additional allowed Origin (repeatable)
        #[arg(long = "allow-origin", value_name = "ORIGIN")]
        allow_origins: Vec<String>,
    },

    /// Install a Minecraft version
    #[command(
        long_about = "Install a Minecraft version and optionally Fabric, Quilt, Forge, or NeoForge. The vanilla parent version is installed first. Downloads are verified and can use the official source or the BMCLAPI mirror."
    )]
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
    #[command(
        long_about = "Launch an installed Minecraft version. Java is selected automatically, and the version's inheritance chain and manifest arguments are resolved. Use 'mclc login' for Microsoft authentication or 'mclc external-login' for Yggdrasil."
    )]
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
    #[command(
        long_about = "Login to a Microsoft account via the device code flow. The access and refresh tokens are cached, and credentials are refreshed automatically before launch."
    )]
    Login,

    /// Login to an external authentication server (authlib-injector)
    #[command(
        long_about = "Login to an external Yggdrasil authentication server. The API URL is resolved through its ALI header, and the account is cached for 'launch --auth external'."
    )]
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

#[derive(Subcommand)]
pub enum InstanceAction {
    /// List registered instances
    List,

    /// Create a named instance
    Create {
        /// Instance name
        name: String,

        /// Game directory containing versions, libraries, and assets
        /// (defaults to the launcher data directory)
        #[arg(long, value_name = "PATH")]
        path: Option<PathBuf>,
    },

    /// Remove an instance from the registry
    Delete {
        /// Instance name
        name: String,

        /// Also delete the instance directory and all its files
        #[arg(long)]
        delete_files: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn json_command_parses() {
        let cli = Cli::try_parse_from(["mclc", "json"]).unwrap();
        assert!(matches!(cli.command, Commands::Json));
    }

    #[test]
    fn parses_instance_and_game_dir_globals() {
        let cli = Cli::try_parse_from(["mclc", "--instance", "fabric", "list"]).unwrap();
        assert_eq!(cli.instance.as_deref(), Some("fabric"));
        assert!(cli.game_dir.is_none());

        let cli = Cli::try_parse_from(["mclc", "list", "--game-dir", "instances/fabric"]).unwrap();
        assert_eq!(
            cli.game_dir.as_deref(),
            Some(std::path::Path::new("instances/fabric"))
        );
        assert!(cli.instance.is_none());
    }

    #[test]
    fn instance_and_game_dir_are_mutually_exclusive() {
        assert!(
            Cli::try_parse_from(["mclc", "--instance", "fabric", "--game-dir", "game", "list",])
                .is_err()
        );
    }

    #[test]
    fn instance_commands_parse() {
        let cli = Cli::try_parse_from(["mclc", "instance", "create", "fabric", "--path", "game"])
            .unwrap();
        match cli.command {
            Commands::Instance {
                action: InstanceAction::Create { name, path },
            } => {
                assert_eq!(name, "fabric");
                assert_eq!(path.as_deref(), Some(std::path::Path::new("game")));
            }
            _ => panic!("unexpected command"),
        }

        let cli = Cli::try_parse_from(["mclc", "instances", "delete", "fabric", "--delete-files"])
            .unwrap();
        assert!(matches!(
            cli.command,
            Commands::Instance {
                action: InstanceAction::Delete {
                    delete_files: true,
                    ..
                }
            }
        ));
    }

    #[test]
    fn web_command_parses_options() {
        let cli = Cli::try_parse_from([
            "mclc",
            "web",
            "--port",
            "8000",
            "--host",
            "127.0.0.1",
            "--token",
            "secret",
            "--allow-origin",
            "http://localhost:5173",
        ])
        .unwrap();

        match cli.command {
            Commands::Web {
                port,
                host,
                token,
                allow_origins,
            } => {
                assert_eq!(port, 8000);
                assert_eq!(host, "127.0.0.1");
                assert_eq!(token.as_deref(), Some("secret"));
                assert_eq!(allow_origins, vec!["http://localhost:5173"]);
            }
            _ => panic!("unexpected command"),
        }
    }

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
