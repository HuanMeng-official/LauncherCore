# LauncherCore

> LauncherCore is a Rust-based command-line Minecraft launcher that handles the core Minecraft launch process, including version management, dependency processing, resource downloading, argument assembly, and process launching. It is designed to be scriptable and can serve as the backend for a GUI launcher or as reference code for learning Minecraft launch workflows.

[![EN](https://img.shields.io/badge/English-Current-blue)](README.md)
[![CN](https://img.shields.io/badge/简体中文-Language-green)](README/README_zh_CN.md)
![MIT](https://img.shields.io/badge/License-MIT-green)
![Rust](https://img.shields.io/badge/Rust-100%25-orange)

## Features

- **Command-Line Interface**: Built with Clap for easy and intuitive usage
- **All Official Versions**: Fetches the complete version list from Mojang's version manifest
- **Verified Downloads**: Validates HTTP status, file size, and SHA-1 metadata where available; writes through a temporary file and atomically replaces the destination
- **Reliable Installs**: Download, extraction, and asset-index errors abort the install instead of reporting success
- **Download Sources & Retry**: Supports the official Mojang source, the BMCLAPI mirror, or automatic fallback; failed requests are retried with exponential backoff
- **Concurrent Downloads**: Up to 16 parallel downloads with a live progress bar
- **Java Runtime Detection**: Scans `JAVA_HOME`, `PATH`, and common install locations, then selects the Java major version required by the target Minecraft version; `--runtime` overrides the choice
- **Version Inheritance**: Resolves `inheritsFrom` chains, merges libraries, arguments, assets, downloads, and Java requirements
- **Manifest-Driven Arguments**: Evaluates `arguments` with OS/architecture rules and placeholders, preserves repeated JVM flags such as `--add-opens` and `--add-exports`, and falls back to legacy `minecraftArguments`
- **Mod Loader Support**: Installs Fabric, Quilt, Forge, and NeoForge
- **Authentication Support**: Offline, Microsoft account, and external Yggdrasil (authlib-injector) authentication
- **Automatic MSA Refresh**: Stores the Microsoft refresh token and Minecraft token expiry, and renews credentials before launch when needed
- **Standard Offline UUID**: Derives the Java-compatible `OfflinePlayer:<name>` UUID for offline mode
- **Multi-Account Support**: Stores multiple external accounts and refreshes expired tokens
- **Credential Protection**: Access tokens are redacted from launch logs; on Unix, credential files are written with `0600` permissions
- **Custom JVM Arguments**: Pass custom JVM arguments for performance tuning
- **JSON Communication Mode**: A persistent NDJSON server exposes install, launch, login, and account operations for GUI frontends
- **Web Frontend Backend**: `mclc web` exposes the same protocol over a local WebSocket with Origin and optional token checks
- **Cross-Platform**: Supports Windows, Linux, and macOS, including Linux ARM64 native handling
- **Detailed Error Handling**: Comprehensive error messages at every step using anyhow/thiserror

## Requirements

- Rust 1.88 or newer to build (edition 2024; tested with 1.99.1)
- A Java runtime compatible with the target Minecraft version. The launcher selects the exact major version declared by the version manifest (commonly Java 8, 16, 17, 21, or newer) and scans `JAVA_HOME`, `PATH`, and common vendor installation directories. Use `--runtime` to override the selection.
- Network access for the first install. Forge and NeoForge also require Java to run their official installers.

## Building

```bash
git clone https://github.com/HuanMeng-official/LauncherCore.git
cd LauncherCore
cargo build --release
```

The executable is written to `target/release/mclc` (`target/release/mclc.exe` on Windows).

## Usage

```
mclc <COMMAND> [OPTIONS]
```

### Commands

| Command | Description |
|---------|-------------|
| **list** | List all available Minecraft versions |
| **java** | List detected Java runtimes |
| **install <VERSION>** | Install a Minecraft version, optionally with a mod loader |
| **launch <VERSION>** | Launch an installed Minecraft version |
| **login** | Login to a Microsoft account |
| **external-login <IDENTIFIER>** | Login to an external Yggdrasil server (authlib-injector) |
| **json** | Start the persistent NDJSON server for GUI integration |
| **web** | Start a local WebSocket backend for a browser frontend |
| **help** | Display help information |

### Global Options

| Option | Description |
|--------|-------------|
| `-r, --runtime <PATH>` | Path to the Java executable (overrides automatic detection) |
| `--download-source <SOURCE>` | Download source: `auto` (default), `official`, or `bmclapi` |

### Install Options

| Option | Description |
|--------|-------------|
| `--loader <LOADER>` | Install a mod loader: `fabric`, `quilt`, `forge`, or `neoforge` |
| `--loader-version <VERSION>` | Pin a loader version; the default is the loader's stable/recommended release |

### Launch Options

| Option | Description |
|--------|-------------|
| `-u, --username <NAME>` | Game username (offline mode) or account identifier (external auth) |
| `--access-token <TOKEN>` | Existing Minecraft access token (MSA authentication) |
| `-j, --jvm-args <ARGS>` | Custom JVM arguments (e.g., `-Xmx4G -XX:+UseG1GC`) |
| `--auth <TYPE>` | Authentication type: `offline` (default), `msa`, or `external` |
| `--api-url <URL>` | Yggdrasil API URL (external auth) |
| `--authlib-jar <PATH>` | Use a local authlib-injector.jar instead of downloading one |
| `-r, --runtime <PATH>` | Path to the Java executable (overrides automatic detection) |

### External Login Options

| Option | Description |
|--------|-------------|
| `-p, --password <PASSWORD>` | Password. Omit it to be prompted without echo; scripts can use the `MCLC_PASSWORD` environment variable instead |
| `-a, --api-url <URL>` | API URL of the authentication server |

## Examples

### Check Detected Java Runtimes

```bash
mclc java
```

### Offline Mode

```bash
mclc install 1.21.1
mclc launch 1.21.1 --username PlayerName
```

Use the BMCLAPI mirror (useful in mainland China):

```bash
mclc install 1.21.1 --download-source bmclapi
```

With a custom Java path and JVM arguments:

```bash
mclc launch 1.21.1 --username PlayerName --runtime "C:\Program Files\Java\jdk-21\bin\java.exe" --jvm-args "-Xmx4G -XX:+UseG1GC"
```

### Mod Loaders

```bash
mclc install 1.21.1 --loader fabric
mclc install 1.21.1 --loader quilt
mclc install 1.21.1 --loader forge
mclc install 1.21.1 --loader neoforge

# Pin a specific loader version
mclc install 1.21.1 --loader fabric --loader-version <loader-version>
```

The install command prints the generated loader version id. Launch it like any other version:

```bash
mclc launch <loader-version-id>
```

Loader version selection:

- **Fabric**: newest stable loader from the Fabric metadata API
- **Quilt**: newest stable (non-beta) loader, falling back to the newest available
- **Forge**: the `recommended` promotion, falling back to `latest`
- **NeoForge**: newest stable version compatible with the target Minecraft version (1.20.1 uses the legacy `net.neoforged:forge` artifact)

The install command ensures the vanilla parent version is installed first. Fabric and Quilt profiles are built from their metadata APIs. Forge and NeoForge are installed by downloading their official installer, verifying its SHA-1, and running it headlessly with `--installClient` using the detected Java runtime. Use `--runtime` if the required Java version is not detected automatically.

### Online Mode (Microsoft Account)

```bash
mclc login
# Follow the device code authentication flow
mclc install 1.21.1
mclc launch 1.21.1 --auth msa
```

Cached credentials are refreshed automatically before launch. To launch with an existing Minecraft access token instead:

```bash
mclc launch 1.21.1 --auth msa --access-token <minecraft_access_token>
```

### External Authentication (authlib-injector)

```bash
mclc external-login player@example.com --api-url https://littleskin.cn
# The password is prompted without echo. For scripts, set MCLC_PASSWORD.
mclc install 1.21.1
mclc launch 1.21.1 --auth external --api-url https://littleskin.cn --username player@example.com
```

The API URL is resolved through the server's ALI header, so the same URL you passed to `external-login` can be used for `launch`. Use `--authlib-jar` if you already have a local authlib-injector.jar.

### List Available Versions

```bash
mclc list
```

### JSON Communication Mode

`mclc json` starts a persistent NDJSON server for GUI integration. It reads one JSON request per line from stdin and writes one JSON event per line to stdout. Human-readable output is converted to `log` events, so the protocol stream never contains unstructured text.

For the complete protocol reference and a framework-agnostic client guide, see [INTEGRATION.md](INTEGRATION.md).

```bash
mclc json
```

Startup emits `{"type":"ready","protocol":1,"version":"..."}`. A request carries a client-chosen `id`, which is echoed by the matching result or error; events emitted while that request is running also carry its `id`. Every protocol line is a complete JSON object on stdout, so clients can ignore stderr except for diagnostics.

```json
{"id":1,"command":"install","params":{"version":"1.21.1","loader":"fabric"}}
{"type":"progress","id":1,"phase":"downloads","current":42,"total":317,"message":"..."}
{"type":"result","id":1,"ok":true,"data":{"version_id":"fabric-loader-0.19.5-1.21.1"}}
```

Errors keep the same envelope with `ok: false`:

```json
{"type":"error","id":2,"ok":false,"error":"Unknown command: foo"}
```

Commands:

| Command | Parameters | Result data |
|---------|------------|-------------|
| `ping` | none | `{"pong":true}` |
| `shutdown` | none | `{"shutdown":true}`, then the server exits |
| `java.list` | none | `{"runtimes":[...]}` |
| `versions.list` | none | `{"versions":[...]}` |
| `installed.list` | none | `{"versions":[{"id":"..."}]}` |
| `install` | `version`, optional `loader`, `loader_version`, `download_source`, `runtime` | `{"version_id":"..."}` |
| `launch` | `version`, optional `username`, `access_token`, `jvm_args`, `auth`, `api_url`, `authlib_jar`, `runtime` | `{"launched":true,"version":"..."}` |
| `login` | none | `{"username":"...","uuid":"..."}` |
| `external_login` | `identifier`, `password`, `api_url`, optional `profile` | account details |
| `accounts.list` | none | `{"accounts":[...],"msa":{...}}` (`msa` is `null` when no Microsoft account is cached) |

Events other than results and errors:

- `log` — `level` (`info`/`warn`) and `message`
- `progress` — `phase`, `current`, `total`, and `message`
- `device_code` — `user_code`, `verification_uri`, `expires_in`, and `message` during Microsoft login
- `profiles` — available Yggdrasil characters when a selection is required

Notes:

- Requests are processed sequentially. `launch` returns after the game process has been spawned; the launcher does not wait for the game to exit.
- If `external_login` has multiple profiles and no `profile` is given, the server emits a `profiles` event and returns `profile selection required`. Retry with the chosen profile id or name.
- Passwords are only accepted as request parameters in JSON mode; they are never read from the terminal.
- The global `--runtime` and `--download-source` options provide defaults that requests can override.

### Web Backend for Browser Frontends

`mclc web` exposes the same JSON protocol over a local WebSocket so a separately hosted browser frontend can drive the launcher. It starts an internal `mclc json` process and does not serve frontend assets.

```bash
mclc web
mclc web --port 7420 --token <token>
```

- WebSocket endpoint: `ws://127.0.0.1:7420/ws`
- Health endpoint: `http://127.0.0.1:7420/health`
- The launcher core does not provide or serve a frontend. Host your own frontend separately; a Vite dev server on localhost is allowed by default.
- The server binds to `127.0.0.1` by default (`--host` changes the address). Loopback origins such as `http://localhost:5173` are allowed; add `--allow-origin <ORIGIN>` for other frontends.
- With `--token`, connect to `ws://127.0.0.1:7420/ws?token=<token>`, or pass `Authorization: Bearer <token>` from a non-browser client.
- Requests, results, and events keep the shapes documented above. The bridge remaps request ids internally so multiple browser tabs can share one launcher; requests are still processed sequentially.
- New WebSocket clients receive the current `ready` event. Events are not replayed, so re-query `accounts.list`, `java.list`, `installed.list`, and `versions.list` after reconnecting.
- Run `mclc web` from the desired game root; `.minecraft` is created there. Global `--runtime` and `--download-source` options are passed to the launcher subprocess.
- Do not bind to `0.0.0.0` unless you understand the risk. A non-loopback bind requires `--token`; also provide explicit `--allow-origin` values for the frontend.

## How It Works

### Version Discovery and Installation

1. Fetches Mojang's [version manifest](https://launchermeta.mojang.com/mc/game/version_manifest.json)
2. Downloads the client JAR and version JSON
3. Applies library rules and downloads the required libraries, natives, asset index, and asset objects
4. Verifies HTTP status, size, and SHA-1 where available; writes to a temporary file and atomically replaces the destination
5. Retries failed requests with exponential backoff; the `auto` source falls back to BMCLAPI
6. Extracts the current platform's native libraries and keeps the native JARs required by LWJGL3 on the classpath
7. Uses concurrent downloads with progress tracking

### Version Inheritance and Argument Assembly

- The launcher follows the `inheritsFrom` chain, detects cycles, and merges parent and child version JSONs
- Scalar fields such as `mainClass`, `assetIndex`, `downloads`, and `javaVersion` are inherited or overridden by the child
- Libraries are merged; a child library with the same coordinate replaces the parent entry
- Arguments are appended in parent-then-child order **without deduplicating**, so repeated JVM flags such as `--add-opens` and `--add-exports` keep their flag/value pairing
- The client JAR is resolved from the child `jar` field, the parent `jar`, or the parent version id
- Placeholders are substituted, including `${natives_directory}`, `${library_directory}`, `${classpath_separator}`, `${auth_player_name}`, and `${auth_access_token}`
- In JVM arguments, `${version_name}` resolves to the game JAR name (for example `1.18.2`); in game arguments it resolves to the launched version id
- OS and architecture rules are evaluated; legacy versions fall back to `minecraftArguments`

### Launch Process

- Selects a Java runtime matching the version's `javaVersion.majorVersion`, unless `--runtime` is provided
- Assembles the classpath from the resolved game JAR and merged libraries
- Configures native library and LWJGL extraction paths
- Evaluates JVM and game arguments, including OS/architecture rules and placeholders
- Runs the game with its working directory set to `.minecraft`, so logs, crash reports, and options stay inside the game directory
- Refreshes cached Microsoft credentials when they are close to expiring
- Derives the standard offline UUID from the username in offline mode
- Redacts secrets such as the access token from the printed command

### Mod Loader Installation

- Fabric and Quilt profiles are constructed from the loader metadata (`launcherMeta`) and saved as local version JSONs
- Forge and NeoForge installers are downloaded from their official Maven repositories, verified against the published SHA-1, and executed headlessly with `--installClient`
- The version id is read from the installer's embedded `version.json` or `install_profile.json`
- After the installer finishes, any missing loader libraries and parent game files are verified and downloaded

### Authentication Flow (Microsoft Account)

1. Initiates the device code flow with Microsoft OAuth2
2. The user completes authentication in a browser or the Microsoft Authenticator app
3. Polls for an authentication token
4. Exchanges the Microsoft token for an Xbox Live token
5. Exchanges the Xbox Live token for an XSTS token
6. Authenticates with Minecraft services
7. Retrieves the Minecraft profile (UUID, username)
8. Caches `access_token`, `uuid`, `username`, the Microsoft `refresh_token`, and the token expiry time
9. Renews credentials automatically before launch when the cached token is close to expiry; run `mclc login` again if renewal fails

### Authentication Flow (External Yggdrasil)

1. Resolves the server API URL through the `X-Authlib-Injector-API-Location` header
2. Fetches server metadata (server name, signature key)
3. Authenticates the account and receives the access token
4. If the account has multiple profiles and no profile is selected, asks the user to choose one and binds it through `/authserver/refresh`
5. Stores the account in `accounts.json`
6. On launch, validates the token and refreshes it if necessary, then starts the game with authlib-injector as a Java agent

## Tested Configurations

The following configurations were tested end-to-end during development:

| Configuration | Result |
|---------------|--------|
| Minecraft 1.21.1 (vanilla) | Installed (launch path exercised by the loader runs) |
| Fabric 1.21.1 | Installed and launched (`fabric-loader-0.19.5-1.21.1`) |
| Forge 1.12.2 | Installed and launched (`1.12.2-forge-14.23.5.2859`) |
| Forge 1.21.1 | Installed and launched (`1.21.1-forge-52.1.0`) |
| NeoForge 1.21.1 | Installed and launched (`neoforge-21.1.264`) |
| Forge 1.16.5 / 1.18.2 | Installer JSON and argument layouts verified against the official installers |

This list is not a compatibility guarantee; Minecraft, loader, and Java versions change frequently.

## Project Structure

```
LauncherCore/
├── src/
│   ├── main.rs           # Main entry point
│   ├── cli.rs            # CLI argument definitions
│   ├── commands.rs       # Shared install and launch command logic
│   ├── output.rs         # Human/JSON output abstraction
│   ├── json_server.rs    # Persistent NDJSON communication server
│   ├── web_server.rs     # Local WebSocket backend for browser frontends
│   ├── install.rs        # Version installation and file downloads
│   ├── download.rs       # Download sources, URL mapping, and retry logic
│   ├── version.rs        # Version JSON inheritance and merging
│   ├── loader.rs         # Fabric/Quilt/Forge/NeoForge installation
│   ├── launch.rs         # Launch argument assembly and process execution
│   ├── launch_manager.rs # Launcher configuration and account storage
│   ├── java.rs           # Java runtime detection and selection
│   ├── auth.rs           # Microsoft account authentication
│   ├── yggdrasil.rs      # External Yggdrasil auth and authlib-injector
│   ├── models.rs         # Data models and JSON structures
│   ├── error.rs          # Error types
│   └── test_http.rs      # HTTP test helper (test builds only)
├── README/
│   ├── README_zh_CN.md        # Simplified Chinese documentation
│   └── INTEGRATION_zh_CN.md   # Simplified Chinese integration guide
├── INTEGRATION.md        # JSON integration guide
├── Cargo.toml            # Project configuration and dependencies
├── Cargo.lock            # Locked dependency versions
└── README.md             # This file
```

## Dependencies

- `clap` - CLI argument parsing
- `tokio` - Async runtime
- `reqwest` - HTTP client with streaming and form support
- `axum` - Local HTTP and WebSocket server for browser frontends
- `serde` / `serde_json` - JSON serialization/deserialization
- `anyhow` - Convenient error handling
- `thiserror` - Error derive macros
- `zip` - ZIP archive extraction and installer inspection
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
- `md-5` - Java-compatible offline UUID derivation

## Game and Configuration Directories

Game files are stored in `./.minecraft`, relative to the directory where `mclc` is executed (not the user's home directory).

| Data | Location |
|------|----------|
| Game files | `./.minecraft` (current working directory) |
| Logs | `./.minecraft/logs` |
| Crash reports | `./.minecraft/crash-reports` |
| Config (Windows) | `%APPDATA%\mclc` |
| Config (Linux) | `$XDG_CONFIG_HOME/mclc` or `~/.config/mclc` |
| Config (macOS) | `~/Library/Application Support/mclc` |

The configuration directory contains `auth_cache.json`, `accounts.json`, and the downloaded `cache/authlib-injector.jar`. On Unix, the directory is created with `0700` and credential files with `0600` permissions.

Inside the game directory:

```
.minecraft/
├── versions/
│   └── <version_id>/
│       ├── <version_id>.json    # Version JSON (may contain inheritsFrom)
│       ├── <version_id>.jar     # Client JAR for vanilla versions
│       └── natives/             # Extracted native libraries
├── libraries/                   # Shared library files
├── assets/
│   ├── indexes/                 # Asset index JSON files
│   └── objects/                 # Downloaded asset files
├── logs/                        # Game logs
└── crash-reports/               # Crash reports
```

Loader versions normally do not have their own client JAR; they use the parent version's JAR through `inheritsFrom`. Forge and NeoForge installers may also create `launcher_profiles.json` in the game directory.

## Troubleshooting

### Java not found?

Run `mclc java` to see which Java runtimes were detected. Install the Java major version required by your Minecraft version, or specify the path explicitly:

```bash
mclc launch 1.21.1 --username Player --runtime "C:\Program Files\Java\jdk-21\bin\java.exe"
```

### Downloads fail or stall?

Downloads are retried automatically. If Mojang's servers are slow or unreachable, use the BMCLAPI mirror explicitly:

```bash
mclc install 1.21.1 --download-source bmclapi
```

Forge and NeoForge installers may still download some files from official sources during installation.

### Mod loader installation issues?

1. Use `mclc java` to verify that a matching Java runtime is installed; Forge and NeoForge installers need it
2. Pass `--runtime <path>` if the automatically selected Java cannot run the installer
3. Use `--loader-version <version>` to pin a specific loader release
4. Forge and NeoForge installation requires network access to their Maven repositories
5. Loader version ids are printed by the install command; use that exact id with `launch`

### Launch fails with errors?

1. Verify all resources and dependencies are fully downloaded
2. Check `.minecraft/logs/latest.log` and `.minecraft/crash-reports/`
3. Ensure your Java version is compatible with the target Minecraft version
4. Try reinstalling the version: `mclc install <VERSION>`
5. If a mod loader fails with a `ResolutionException` or a missing-module error, make sure you are using the latest build; the launcher preserves repeated JVM flags and resolves `${version_name}` correctly for Forge/NeoForge

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

The launcher selects the appropriate LWJGL native for Linux ARM64 and falls back to downloading it from Maven when the version manifest does not include one. If you encounter issues, ensure your system has the necessary libraries installed:

```bash
sudo apt-get install libx11-dev libxcursor-dev libxrandr-dev libxinerama-dev
```

### Multiple instances

Running multiple game instances from the same `.minecraft` directory shares `logs/latest.log`. Use separate game directories if you need isolated logs or saves.

## Contributing

Issues and Pull Requests are welcome! Please open an issue to discuss your ideas before submitting a Pull Request.

## License

MIT
