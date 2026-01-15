# LauncherCore

> LauncherCore 是一个基于 Rust 的命令行 Minecraft 启动器，处理了 Minecraft 启动的核心流程，包括版本管理、依赖处理、资源下载、参数组装和进程启动。适合作为 CLI 启动器的后端模块或学习 Minecraft 启动流程的参考代码。

[![EN](https://img.shields.io/badge/English-Language-blue)](../README.md)
[![CN](https://img.shields.io/badge/简体中文-Current-green)](README_zh_CN.md)
![MIT](https://img.shields.io/badge/License-MIT-green)
![Rust](https://img.shields.io/badge/Rust-100%25-orange)

## 功能特性

- **命令行界面**：基于 Clap 构建，简单易用
- **支持所有官方版本**：自动从 Mojang 官方服务器获取完整的版本列表
- **自动资源下载**：自动下载客户端 JAR、依赖库（包括原生库）和资源文件
- **多线程下载**：支持并发下载，加速安装过程（最多 16 个并行下载）
- **进度可视化**：下载过程中实时显示进度条
- **跨平台支持**：支持 Windows、Linux 和 macOS
- **灵活配置**：可通过命令行参数或 `JAVA_HOME` 环境变量指定 Java 运行时
- **原生库管理**：自动将原生库提取到版本独立目录
- **认证支持**：支持离线和微软账户两种认证方式
- **自定义 JVM 参数**：支持传入自定义 JVM 参数进行性能调优
- **异步高效**：利用 Tokio + Reqwest 实现高性能异步下载
- **ARM64 支持**：为 Linux ARM64 提供专门的 LWJGL 原生库处理
- **详细错误处理**：使用 anyhow/thiserror 提供每一步的完整错误信息

## 构建

```bash
git clone https://github.com/HuanMeng-official/LauncherCore.git
cd LauncherCore
cargo build --release
```

构建完成后，可执行文件将位于 `target/release/mclc`（Windows 上为 `target/release/mclc.exe`）。

## 使用方法

```
mclc <命令> [选项]
```

### 命令

| 命令 | 描述 |
|------|------|
| **list** | 列出所有可用的 Minecraft 版本 |
| **install <版本>** | 安装指定的 Minecraft 版本 |
| **launch <版本>** | 启动指定的 Minecraft 版本 |
| **login** | 登录到微软账户 |
| **help** | 显示帮助信息 |

### 全局选项

| 选项 | 描述 |
|------|------|
| `-r, --runtime <路径>` | 指定 Java 运行时路径 |

### 启动选项

| 选项 | 描述 |
|------|------|
| `-u, --username <名称>` | 游戏用户名（离线模式必需） |
| `--access-token <令牌>` | 微软访问令牌（用于 MSA 认证） |
| `-j, --jvm-args <参数>` | 自定义 JVM 参数（如 `-Xmx4G -XX:+UseG1GC`） |
| `--auth <类型>` | 认证类型：`offline`（默认）或 `msa` |
| `-r, --runtime <路径>` | 指定 Java 运行时路径 |

## 使用示例

### 离线模式

```bash
mclc install 1.21.3
mclc launch 1.21.3 --username 玩家名
```

使用自定义 Java 路径和 JVM 参数：

```bash
mclc launch 1.21.3 --username 玩家名 --runtime "C:\Program Files\Java\jdk-21\bin\java.exe" --jvm-args "-Xmx4G -XX:+UseG1GC"
```

### 在线模式（微软账户）

```bash
mclc login
# 按照设备码认证流程完成登录
mclc install 1.21.3
mclc launch 1.21.3 --auth msa
```

或使用缓存的访问令牌：

```bash
mclc launch 1.21.3 --auth msa --access-token <你的令牌>
```

### 列出可用版本

```bash
mclc list
```

## 运行原理

1. **版本发现**：调用 Mojang 官方的[版本清单](https://launchermeta.mojang.com/mc/game/version_manifest.json)获取所有支持版本
2. **安装过程**：
   - 下载版本清单 JSON
   - 下载客户端 JAR 文件
   - 下载所有必需的依赖库（包括原生库）
   - 下载资源索引和所有资源文件
   - 将原生库提取到版本独立目录
   - 使用多线程下载并显示进度
3. **启动过程**：
   - 组装完整的类路径
   - 配置原生库路径
   - 构建所有必需的 JVM 和游戏参数
   - 使用 Java 启动 Minecraft 客户端

### 认证流程（微软账户）

1. 通过微软 OAuth2 发起设备码流程
2. 用户在网页浏览器或 Microsoft Authenticator 应用中完成认证
3. 轮询获取认证令牌
4. 将微软令牌兑换为 Xbox Live 令牌
5. 将 Xbox Live 令牌兑换为 XSTS 令牌
6. 使用 Minecraft 服务进行认证
7. 获取 Minecraft 资料信息（UUID、用户名）
8. 缓存 `access_token`、`uuid` 和 `username` 供后续启动使用

## 项目结构

```
LauncherCore/
├── src/
│   ├── main.rs           # 主入口
│   ├── cli.rs            # CLI 参数定义
│   ├── install.rs        # 安装逻辑和下载
│   ├── launch.rs         # 启动参数组装和执行
│   ├── launch_manager.rs # 启动配置管理
│   ├── auth.rs           # 微软账户认证
│   ├── models.rs         # 数据模型和 JSON 结构
│   └── error.rs          # 错误类型
├── README/
│   ├── README_en_US.md   # 英文文档
│   ├── README_zh_CN.md   # 简体中文文档（本文件）
│   └── README_fr_FR.md   # 法文文档
├── Cargo.toml            # 项目配置和依赖
└── README.md             # 主 README
```

## 依赖项

- `clap` - CLI 参数解析
- `tokio` - 异步运行时
- `reqwest` - HTTP 客户端（支持流式传输）
- `serde` / `serde_json` - JSON 序列化/反序列化
- `anyhow` - 便捷的错误处理
- `thiserror` - 错误派生宏
- `zip` - ZIP 归档提取（用于原生库）
- `dirs` - 跨平台目录路径
- `url` - URL 解析和操作
- `futures-util` - 异步工具（用于并行下载）
- `indicatif` - 进度条可视化

## 游戏目录结构

启动器将游戏文件存储在以下位置：

| 平台 | 目录 |
|------|------|
| Windows | `%USERPROFILE%\.minecraft` |
| Linux | `~/.minecraft` |
| macOS | `~/Library/Application Support/minecraft` |

游戏目录内部结构：
```
.minecraft/
├── versions/
│   └── <版本ID>/
│       ├── <版本ID>.json    # 版本清单
│       ├── <版本ID>.jar     # 客户端 JAR
│       └── natives/         # 已提取的原生库
├── libraries/               # 共享库文件
└── assets/
    ├── indexes/             # 资源索引 JSON 文件
    └── objects/             # 已下载的资源文件
```

## 常见问题

### 找不到 Java？

设置 `JAVA_HOME` 环境变量或使用 `--runtime` 显式指定 Java 路径：

```bash
mclc launch 1.21.3 --username 玩家名 --runtime "C:\Program Files\Java\jdk-21\bin\java.exe"
```

### 启动失败或报错？

1. 验证所有资源和依赖是否完整下载
2. 检查终端中的详细错误输出
3. 确保您的 Java 版本与目标 Minecraft 版本兼容
4. 尝试重新安装版本：`mclc install <版本>`

### 微软账户认证问题？

1. 确保您的微软账户已关联 Xbox 账户
2. 儿童账户可能需要由成年人添加到家庭中
3. 您的账户所在地区必须支持 Xbox Live

### Linux ARM64 问题？

启动器会自动下载 Linux ARM64 的 LWJGL 原生库。如果遇到问题，请确保系统已安装必要的库：

```bash
sudo apt-get install libx11-dev libxcursor-dev libxrandr-dev libxinerama-dev
```

## 贡献

欢迎提交 Issue 和 Pull Request！请先提交 Issue 描述你的想法，然后再提交 Pull Request。

## 许可证

MIT
