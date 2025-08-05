## LauncherCore
> 这个用Rust编写的启动器处理了Minecraft启动核心的大部分关键流程，包括版本管理、依赖处理、资源下载、启动参数组装和进程启动。适合作为CLI启动器的后端模块或学习Minecraft启动流程的参考代码。

[![EN](https://img.shields.io/badge/English-Click-blue)](../README.md)
[![CN](https://img.shields.io/badge/简体中文-Click-blue)](./README_zh_CN.md)
[![FR](https://img.shields.io/badge/Français-Click-yellow)](./README_fr_FR.md)
![MIT](https://img.shields.io/badge/License-MIT-green)
![Rust](https://img.shields.io/badge/Rust-100%25-orange)

### 功能特性：
- **命令行界面**：基于Clap构建，简单易用
- **支持所有官方版本**：自动获取完整的Minecraft版本列表，支持安装和启动任意官方版本
- **自动资源下载**：自动下载客户端JAR、依赖库（包括本地库）和资源文件（assets），无需人工干预
- **平台兼容性**：支持三大平台：Windows、Linux和macOS
- **灵活的Java路径配置**：可通过命令行参数或`JAVA_HOME`环境变量指定Java运行时
- **本地库管理**：自动为每个版本提取本地库到独立目录，确保多版本共存
- **异步高效**：利用Tokio+Reqwest实现高性能异步下载
- **详细错误处理**：使用anyhow/thiserror提供每一步的完整错误信息

### 构建：
```bash
git clone https://github.com/HuanMeng-official/LauncherCore.git
cd LauncherCore
cargo build --release
```
构建完成后，可执行文件将位于`target/release/mclc.exe`或`target/release/mclc`。

### 使用：
``mclc <COMMANDS> <OPTIONS>``
| Commands | Description |
| --- | --- |
| **list** | 列出可用的Minecraft版本 |
| **install** | 安装一个Minecraft版本 |
| **launch** | 启动Minecraft |
| **help** | 显示帮助信息或指定的子命令的帮助信息 |
| **login** | 登录到微软账户 |

| Options | Description |
| --- | --- |
| **-r, --runtime** | 设置Java路径 |
| **-h, --help** | 显示帮助 |

*示例：*  
在线登录：  
1. ``mclc login``  
2. ``mclc launcher <Version>``  
  
离线登录：  
1. ``mclc launcher <Version> --username <PlayerName> --runtime "C:\Program Files\Java\bin\java.exe"``

### 运行原理：
- 调用[Mojang官方版本清单](https://launchermeta.mojang.com/mc/game/version_manifest.json)获取支持的所有版本。
- 安装时，自动下载客户端JAR、依赖库（包括natives）、资源索引、资源文件，并自动解压natives。
- 启动时，自动组装类路径、natives路径和所有必需的参数，然后调用Java启动Minecraft客户端。

### 目录结构：
```
项目/
  ├── src/
  │    └── main.rs
  ├── Cargo.toml
  └── target/
        ├── debug/
        ├── .rust_info.json
        └── CACHEDIR.TAG
```

### 常见问题：
- 找不到Java？  
设置JAVA_HOME环境变量或使用--runtime显式指定Java路径
- 启动失败或报错？  
验证依赖和资源是否完整下载，或检查终端中的详细错误输出

### 贡献：
欢迎提交Issue和PR！请先提交Issue描述你的想法再提交Pull Request。

### 许可：
MIT