## LauncherCore
> This launcher, written in Rust, handles most of the key processes in the Minecraft launch core, including version management, dependency handling, resource downloading, launch parameter assembly, and process launching. It is suitable for use as a backend module for a CLI launcher or as reference code for learning the Minecraft launch process.  

[![EN](https://img.shields.io/badge/English-Click-blue)](./README.md)
[![CN](https://img.shields.io/badge/简体中文-Click-blue)](./README/README_zh_CN.md)
[![FR](https://img.shields.io/badge/Français-Click-yellow)](./README/README_fr_FR.md)
![MIT](https://img.shields.io/badge/License-MIT-green)
![Rust](https://img.shields.io/badge/Rust-100%25-orange) 

### Features:
- **Command Line Interface**: Built on Clap, simple and user-friendly.
- **Supports All Official Versions**: Automatically fetches the complete list of Minecraft versions and supports installation and launch of any official version.
- **Automatic Resource Downloads**: Automatically downloads client JARs, dependencies (including natives), and resource files (assets) without manual intervention.
- **Platform Compatibility**: Supports the three major platforms: Windows, Linux, and macOS.
- **Flexible Java Path Configuration**: Allows specifying the Java runtime environment via command-line arguments or the `JAVA_HOME` environment variable.
- **Local Natives Management**: Automatically extracts natives to dedicated directories for each version, ensuring coexistence of multiple versions.
- **Asynchronous and Efficient**: Leverages Tokio + Reqwest for high-performance asynchronous downloads.
- **Detailed Error Handling**: Uses anyhow/thiserror to provide comprehensive error messages at every step.

### Build:
```bash
git clone https://github.com/HuanMeng-official/LauncherCore.git
cd LauncherCore
cargo build --release
```
After compilation, the executable can be found at ``target/release/mclc.exe`` or ``target/release/mclc``.

### Usage:
``mclc <COMMANDS> <OPTIONS>``
| Commands | Description |
| --- | --- |
| **list** | List available Minecraft versions |
| **install** | Install a Minecraft version |
| **launch** | Launch Minecraft |
| **help** | Print this message or the help of the given subcommand(s) |
| **login** | Login to Microsoft account |

| Options | Description |
| --- | --- |
| **-r, --runtime** | Set java path |
| **-h, --help** | Print help |

*For example:*  
Online:  
1. ``mclc login``  
2. ``mclc launcher <Version>``  
  
Offline:  
1. ``mclc launcher <Version> --username <PlayerName> --runtime "C:\Program Files\Java\bin\java.exe"``

### Operating principle:
 - Call the [Mojang official version manifest](https://launchermeta.mojang.com/mc/game/version_manifest.json) to retrieve the complete list of supported versions.
 - During installation, it automatically downloads the client JAR, dependency libraries (including natives), asset index, and asset files, and automatically extracts the natives.
 - At launch, it automatically assembles the classpath, native paths, and all required arguments, then invokes Java to start the Minecraft client.

### Directory Structure:
```
Projetc/
  ├── src/
  │    └── main.rs
  ├── Cargo.toml
  └── target/
        ├── debug/
        ├── .rust_info.json
        └── CACHEDIR.TAG
```

### FAQ:
 - Not found Java?  
Set the JAVA_HOME environment variable or explicitly specify the Java path with --runtime.
 - Launch fails or throws errors?  
Verify that dependencies and resources were downloaded completely, or check the detailed error output in the terminal.

### Contributing:
Issues and PRs are welcome! Please open an issue first to describe your idea before submitting a pull request.

### License:
MIT