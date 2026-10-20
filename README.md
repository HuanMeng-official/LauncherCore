# LauncherCore

> LauncherCore is a Rust-based command-line Minecraft launcher that handles the core Minecraft launch process, including version management, dependency processing, resource downloading, argument assembly, and process launching. Suitable as a backend module for CLI launchers or as reference code for learning Minecraft launch workflows.

[![EN](https://img.shields.io/badge/English-Current-blue)](README.md)
[![CN](https://img.shields.io/badge/简体中文-Language-green)](README/README_zh_CN.md)
![MIT](https://img.shields.io/badge/License-MIT-green)
![Rust](https://img.shields.io/badge/Rust-100%25-orange)

## Features

- **Command-Line Interface**: Built with Clap for easy and intuitive usage
- **All Official Versions**: Automatically fetches the complete version list from Mojang's official servers
- **Automatic Resource Downloads**: Downloads client JAR, libraries (including natives), and asset files automatically
- **Verified Downloads**: Validates HTTP status, file size, and SHA-1 metadata where available, writes through a temporary file, and atomically replaces the destination
- **Reliable Installs**: Download, extraction, and asset-index errors abort the install instead of reporting success
- **Download Sources & Retry**: Supports official Mojang, the BMCLAPI mirror, or automatic fallback; failed requests are retried with backoff
- **Concurrent Downloads**: Concurrent downloading for faster installation (up to 16 parallel downloads)
- **Progress Visualization**: Real-time progress bar during downloads
- **Cross-Platform**: Supports Windows, Linux, and macOS
- **Java Runtime Detection**: Scans `JAVA_HOME`, `PATH`, and common install directories, then selects the Java major version required by the target Minecraft version; `--runtime` overrides the choice
- **Native Library Management**: Applies version rules and downloads/extracts only the natives for the current OS and architecture
- **Manifest-Driven Arguments**: Uses each version's `arguments` (with OS/architecture rules) and falls back to legacy `minecraftArguments`
- **Authentication Support**: Offline, Microsoft account, and external Yggdrasil (authlib-injector) authentication
- **Automatic MSA Refresh**: Stores the Microsoft refresh token and Minecraft token expiry, and renews credentials before launch when needed
- **Standard Offline UUID**: Derives the Java-compatible `OfflinePlayer:<name>` UUID for offline mode
- **Multi-Account Support**: Stores multiple external accounts and refreshes expired tokens
- **Credential Protection**: Access tokens are redacted from launch logs; on Unix, credential files are written with `0600` permissions
- **Custom JVM Arguments**: Pass custom JVM arguments for performance tuning
- **Async & Efficient**: High-performance async downloads using Tokio + Reqwest
- **ARM64 Support**: Special handling for LWJGL natives on Linux ARM64
- **Detailed Error Handling**: Comprehensive error messages at every step using anyhow/thiserror

## Requirements

- Rust 1.88 or newer to build (edition 2024; tested with 1.99.1)
- A Java runtime compatible with the target Minecraft version. Java is detected automatically from `JAVA_HOME`, `PATH`, and common install directories; use `--runtime` to override. Depending on the Minecraft version, Java 8, 16, 17, or 21 may be required.

## Building

```bash
git clone https://github.com/HuanMeng-official/LauncherCore.git
cd LauncherCore
cargo build --release
```

After building, the executable will be located at `target/release/mclc` (or `target/release/mclc.exe` on Windows).

## Usage

```
mclc <COMMAND> [OPTIONS]
```

### Commands

| Command | Description |
|---------|-------------|
| **list** | List all available Minecraft versions |
| **java** | List detected Java runtimes |
| **install <VERSION>** | Install a specific Minecraft version |
| **launch <VERSION>** | Launch a Minecraft version |
| **login** | Login to a Microsoft account |
| **external-login <IDENTIFIER>** | Login to an external Yggdrasil server (authlib-injector) |
| **help** | Display help information |

### Global Options

| Option | Description |
|--------|-------------|
| `-r, --runtime <PATH>` | Override automatic Java detection |
| `--download-source <SOURCE>` | Download source: `auto` (default), `official`, or `bmclapi` |

### Launch Options

| Option | Description |
|--------|-------------|
| `-u, --username <NAME>` | Game username (offline mode) or account identifier (external auth) |
| `--access-token <TOKEN>` | Existing Minecraft access token (MSA authentication) |
| `-j, --jvm-args <ARGS>` | Custom JVM arguments (e.g., `-Xmx4G -XX:+UseG1GC`) |
| `--auth <TYPE>` | Authentication type: `offline` (default), `msa`, or `external` |
| `--api-url <URL>` | Yggdrasil API URL (external auth) |
| `--authlib-jar <PATH>` | Use a local authlib-injector.jar instead of downloading one |
| `-r, --runtime <PATH>` | Override automatic Java detection |

### External Login Options

| Option | Description |
|--------|-------------|
| `-p, --password <PASSWORD>` | Password. Omit it to be prompted without echo; scripts can use the `MCLC_PASSWORD` environment variable instead |
| `-a, --api-url <URL>` | API URL of the authentication server |

## Examples

### Offline Mode

```bash
mclc install 1.21.3
mclc launch 1.21.3 --username PlayerName
```

Use the BMCLAPI mirror (useful in mainland China):

```bash
mclc install 1.21.3 --download-source bmclapi
```

With custom Java path and JVM arguments:

```bash
mclc launch 1.21.3 --username PlayerName --runtime "C:\Program Files\Java\jdk-21\bin\java.exe" --jvm-args "-Xmx4G -XX:+UseG1GC"
```

### Online Mode (Microsoft Account)

```bash
mclc login
# Follow the device code authentication flow
mclc install 1.21.3
mclc launch 1.21.3 --auth msa
```

To launch with an existing Minecraft access token:

```bash
mclc launch 1.21.3 --auth msa --access-token <minecraft_access_token>
```

### External Authentication (authlib-injector)

```bash
mclc external-login player@example.com --api-url https://littleskin.cn
# The password is prompted without echo. For scripts, set MCLC_PASSWORD.
mclc install 1.21.3
mclc launch 1.21.3 --auth external --api-url https://littleskin.cn --username player@example.com
```

The API URL is resolved through the server's ALI header, so the same URL you passed to `external-login` can be used for `launch`. Use `--authlib-jar` if you already have a local authlib-injector.jar.

### List Available Versions

```bash
mclc list
```

## How It Works

1. **Version Discovery**: Calls Mojang's official [version manifest](https://launchermeta.mojang.com/mc/game/version_manifest.json) to fetch all supported versions
2. **Installation Process**:
   - Downloads the version manifest JSON
   - Downloads the client JAR file
   - Applies library rules and downloads only libraries/natives valid for the current OS and architecture
   - Downloads the asset index and all asset files
   - Verifies HTTP status, size, and SHA-1 where available; writes to a temporary file and atomically replaces the destination
   - Retries failed requests with exponential backoff; `auto` source falls back to the BMCLAPI mirror
   - Extracts native libraries to version-specific directories
   - Uses concurrent downloading with progress tracking
3. **Launch Process**:
   - Selects a Java runtime matching the version's `javaVersion.majorVersion` (or uses `--runtime`)
   - Assembles the complete classpath
   - Configures native library paths
   - Evaluates the version manifest's JVM and game arguments, including OS/architecture rules and placeholders
   - Falls back to legacy `minecraftArguments` for older versions
   - Refreshes cached Microsoft credentials automatically when they are close to expiring
   - Derives the standard offline UUID from the username in offline mode
   - Redacts secrets such as the access token from the printed command
   - Launches the Minecraft client with Java

### Authentication Flow (Microsoft Account)

1. Initiates device code flow with Microsoft OAuth2
2. User completes authentication on the web browser or Microsoft Authenticator app
3. Polls for authentication token
4. Exchanges Microsoft token for Xbox Live token
5. Exchanges Xbox Live token for XSTS token
6. Authenticates with Minecraft services
7. Retrieves Minecraft profile (UUID, username)
8. Caches `access_token`, `uuid`, `username`, the Microsoft `refresh_token`, and the token expiry time
9. Before a later launch, renews the Microsoft token and Minecraft credentials automatically when the cached token is close to expiring; if renewal fails, run `mclc login` again

### Authentication Flow (External Yggdrasil)

1. Resolves the server API URL through the `X-Authlib-Injector-API-Location` header
2. Fetches server metadata (server name, signature key)
3. Authenticates the account and receives the access token
4. If the account has multiple profiles and no profile is selected, asks the user to choose one and binds it through `/authserver/refresh`
5. Stores the account in `accounts.json` for future launches
6. On launch, validates the token and refreshes it if necessary, then starts the game with authlib-injector as a Java agent

## Project Structure

```
LauncherCore/
├── src/
│   ├── main.rs           # Main entry point
│   ├── cli.rs            # CLI argument definitions
│   ├── install.rs        # Installation logic and downloads
│   ├── download.rs       # Download sources and retry logic
│   ├── launch.rs         # Launch argument assembly and execution
│   ├── launch_manager.rs # Launcher configuration and account storage
│   ├── java.rs           # Java runtime detection and selection
│   ├── auth.rs           # Microsoft account authentication
│   ├── yggdrasil.rs      # External Yggdrasil auth and authlib-injector
│   ├── models.rs         # Data models and JSON structures
│   ├── error.rs          # Error types
│   └── test_http.rs      # HTTP test helper (test builds only)
├── README/
│   └── README_zh_CN.md   # Simplified Chinese documentation
├── Cargo.toml            # Project configuration and dependencies
├── Cargo.lock            # Locked dependency versions
└── README.md             # This file
```

## Dependencies

- `clap` - CLI argument parsing
- `tokio` - Async runtime
- `reqwest` - HTTP client with streaming support
- `serde` / `serde_json` - JSON serialization/deserialization
- `anyhow` - Convenient error handling
- `thiserror` - Error derive macros
- `zip` - ZIP archive extraction (for native libraries)
- `dirs` - Cross-platform configuration directory paths
- `url` - URL parsing and manipulation
- `futures-util` - Async utilities for parallel downloads
- `indicatif` - Progress bar visualization
- `base64` - Encoding authlib-injector prefetched metadata
- `uuid` - Client token and UUID generation
- `tempfile` - Temporary files for verified atomic writes
- `sha1` - Download integrity verification
- `sha2` - authlib-injector integrity verification
- `rpassword` - Hidden password input

## Game and Configuration Directories

Game files are stored in `./.minecraft`, relative to the directory where `mclc` is executed (not the user's home directory).

| Data | Location |
|------|----------|
| Game files | `./.minecraft` (current working directory) |
| Config (Windows) | `%APPDATA%\mclc` |
| Config (Linux) | `$XDG_CONFIG_HOME/mclc` or `~/.config/mclc` |
| Config (macOS) | `~/Library/Application Support/mclc` |

The configuration directory contains `auth_cache.json`, `accounts.json`, and the downloaded `cache/authlib-injector.jar`. On Unix, the directory is created with `0700` and credential files with `0600` permissions.

Inside the game directory:
```
.minecraft/
├── versions/
│   └── <version_id>/
│       ├── <version_id>.json    # Version manifest
│       ├── <version_id>.jar     # Client JAR
│       └── natives/             # Extracted native libraries
├── libraries/                   # Shared library files
└── assets/
    ├── indexes/                 # Asset index JSON files
    └── objects/                 # Downloaded asset files
```

## Troubleshooting

### Java not found?

Run `mclc java` to see which Java runtimes were detected. Install the Java major version required by your Minecraft version, or specify the path explicitly:

```bash
mclc launch 1.21.3 --username Player --runtime "C:\Program Files\Java\jdk-21\bin\java.exe"
```

### Downloads fail or stall?

Downloads are retried automatically. If Mojang's servers are slow or unreachable, use the BMCLAPI mirror explicitly:

```bash
mclc install 1.21.3 --download-source bmclapi
```

### Launch fails with errors?

1. Verify all resources and dependencies are fully downloaded
2. Check the detailed error output in the terminal
3. Ensure your Java version is compatible with the target Minecraft version
4. Try reinstalling the version: `mclc install <VERSION>`

### Microsoft account authentication issues?

1. Ensure you have an Xbox account linked to your Microsoft account
2. Child accounts may need to be added to a family by an adult
3. Your account must be in a region where Xbox Live is available
4. If the cached token can no longer be refreshed, run `mclc login` again

### External login issues?

1. Make sure the API URL is reachable and serves the `X-Authlib-Injector-API-Location` header or a valid Yggdrasil API
2. Use the same `--api-url` value for `external-login` and `launch`
3. If your token has expired, run `external-login` again or let `launch` refresh it automatically
4. Use `--authlib-jar` to bypass downloading authlib-injector

### Linux ARM64 issues?

The launcher automatically downloads LWJGL native libraries for Linux ARM64. If you encounter issues, ensure your system has the necessary libraries installed:

```bash
sudo apt-get install libx11-dev libxcursor-dev libxrandr-dev libxinerama-dev
```

## Contributing

Issues and Pull Requests are welcome! Please open an issue to discuss your ideas before submitting a Pull Request.

## License

MIT
