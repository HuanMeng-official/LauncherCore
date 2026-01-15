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
- **Multi-Threaded Downloads**: Concurrent downloading for faster installation (up to 16 parallel downloads)
- **Progress Visualization**: Real-time progress bar during downloads
- **Cross-Platform**: Supports Windows, Linux, and macOS
- **Flexible Configuration**: Specify Java runtime via command line or `JAVA_HOME` environment variable
- **Native Library Management**: Automatically extracts native libraries to version-specific directories
- **Authentication Support**: Both offline and Microsoft account authentication
- **Custom JVM Arguments**: Pass custom JVM arguments for performance tuning
- **Async & Efficient**: High-performance async downloads using Tokio + Reqwest
- **ARM64 Support**: Special handling for LWJGL natives on Linux ARM64
- **Detailed Error Handling**: Comprehensive error messages at every step using anyhow/thiserror

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
| **install <VERSION>** | Install a specific Minecraft version |
| **launch <VERSION>** | Launch a Minecraft version |
| **login** | Login to Microsoft account |
| **help** | Display help information |

### Global Options

| Option | Description |
|--------|-------------|
| `-r, --runtime <PATH>` | Specify Java runtime path |

### Launch Options

| Option | Description |
|--------|-------------|
| `-u, --username <NAME>` | Game username (required for offline mode) |
| `--access-token <TOKEN>` | Microsoft access token (for MSA authentication) |
| `-j, --jvm-args <ARGS>` | Custom JVM arguments (e.g., `-Xmx4G -XX:+UseG1GC`) |
| `--auth <TYPE>` | Authentication type: `offline` (default) or `msa` |
| `-r, --runtime <PATH>` | Specify Java runtime path |

## Examples

### Offline Mode

```bash
mclc install 1.21.3
mclc launch 1.21.3 --username PlayerName
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

Or using a cached access token:

```bash
mclc launch 1.21.3 --auth msa --access_token <your_token>
```

### List Available Versions

```bash
mclc list
```

## How It Works

1. **Version Discovery**: Calls Mojang's official [version manifest](https://launchermeta.mojang.com/mc/game/version_manifest.json) to fetch all supported versions
2. **Installation Process**:
   - Downloads the version manifest JSON
   - Downloads the client JAR file
   - Downloads all required libraries (including native libraries)
   - Downloads asset index and all asset files
   - Extracts native libraries to version-specific directories
   - Uses multi-threaded downloading with progress tracking
3. **Launch Process**:
   - Assembles the complete classpath
   - Configures native library paths
   - Builds all required JVM and game arguments
   - Launches the Minecraft client with Java

### Authentication Flow (Microsoft Account)

1. Initiates device code flow with Microsoft OAuth2
2. User completes authentication on the web browser or Microsoft Authenticator app
3. Polls for authentication token
4. Exchanges Microsoft token for Xbox Live token
5. Exchanges Xbox Live token for XSTS token
6. Authenticates with Minecraft services
7. Retrieves Minecraft profile (UUID, username)
8. Caches `access_token`, `uuid`, and `username` for future launches

## Project Structure

```
LauncherCore/
├── src/
│   ├── main.rs           # Main entry point
│   ├── cli.rs            # CLI argument definitions
│   ├── install.rs        # Installation logic and downloads
│   ├── launch.rs         # Launch argument assembly and execution
│   ├── launch_manager.rs # Launch configuration management
│   ├── auth.rs           # Microsoft account authentication
│   ├── models.rs         # Data models and JSON structures
│   └── error.rs          # Error types
├── README/
│   ├── README_en_US.md   # English documentation
│   ├── README_zh_CN.md   # Simplified Chinese documentation
│   └── README_fr_FR.md   # French documentation
├── Cargo.toml            # Project configuration and dependencies
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
- `dirs` - Cross-platform directory paths
- `url` - URL parsing and manipulation
- `futures-util` - Async utilities for parallel downloads
- `indicatif` - Progress bar visualization

## Game Directory Structure

The launcher stores game files in the following locations:

| Platform | Directory |
|----------|-----------|
| Windows | `%USERPROFILE%\.minecraft` |
| Linux | `~/.minecraft` |
| macOS | `~/Library/Application Support/minecraft` |

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

Set the `JAVA_HOME` environment variable or use `--runtime` to specify the Java path explicitly:

```bash
mclc launch 1.21.3 --username Player --runtime "C:\Program Files\Java\jdk-21\bin\java.exe"
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

### Linux ARM64 issues?

The launcher automatically downloads LWJGL native libraries for Linux ARM64. If you encounter issues, ensure your system has the necessary libraries installed:

```bash
sudo apt-get install libx11-dev libxcursor-dev libxrandr-dev libxinerama-dev
```

## Contributing

Issues and Pull Requests are welcome! Please open an issue to discuss your ideas before submitting a Pull Request.

## License

MIT
