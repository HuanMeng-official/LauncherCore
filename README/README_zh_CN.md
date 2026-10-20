# LauncherCore

> LauncherCore 是一个基于 Rust 的命令行 Minecraft 启动器，处理了 Minecraft 启动的核心流程，包括版本管理、依赖处理、资源下载、参数组装和进程启动。它适合作为 GUI 启动器的后端，也可作为学习 Minecraft 启动流程的参考代码。

[![EN](https://img.shields.io/badge/English-Language-blue)](../README.md)
[![CN](https://img.shields.io/badge/简体中文-Current-green)](README_zh_CN.md)
![MIT](https://img.shields.io/badge/License-MIT-green)
![Rust](https://img.shields.io/badge/Rust-100%25-orange)

## 功能特性

- **命令行界面**：基于 Clap 构建，简单易用
- **支持所有官方版本**：从 Mojang 版本清单获取完整的版本列表
- **下载校验**：在信息可用时校验 HTTP 状态、文件大小和 SHA-1，先写临时文件再原子替换目标文件
- **可靠安装**：下载、解压或资源索引解析失败时会中止安装，不再误报成功
- **下载源与重试**：支持 Mojang 官方源、BMCLAPI 镜像或自动回退；失败请求按指数退避重试
- **并发下载**：最多 16 个并行下载，并显示实时进度条
- **Java 运行时检测**：扫描 `JAVA_HOME`、`PATH` 和常见安装目录，自动选择目标版本要求的 Java 大版本；可用 `--runtime` 覆盖
- **版本继承**：解析 `inheritsFrom` 继承链，合并依赖库、参数、资源、下载信息和 Java 要求
- **版本参数驱动**：解析 `arguments` 中的系统和架构规则与占位符，保留重复的 `--add-opens` / `--add-exports` 等 JVM 标志，并兼容旧版 `minecraftArguments`
- **模组加载器支持**：可安装 Fabric、Quilt、Forge、NeoForge
- **认证支持**：支持离线、微软账户和外置 Yggdrasil（authlib-injector）认证
- **微软令牌自动刷新**：保存微软 refresh token 和 Minecraft 令牌过期时间，启动前按需自动续期
- **标准离线 UUID**：离线模式按 Java 的 `OfflinePlayer:<名称>` 规则生成 UUID
- **多账号支持**：可保存多个外置账号，并在令牌过期时自动刷新
- **凭据保护**：启动日志会隐藏访问令牌；Unix 下凭据文件使用 `0600` 权限
- **自定义 JVM 参数**：支持传入自定义 JVM 参数进行性能调优
- **JSON 通信模式**：常驻 NDJSON 服务向 GUI 前端提供安装、启动、登录和账号操作
- **跨平台支持**：支持 Windows、Linux 和 macOS，包括 Linux ARM64 原生库处理
- **详细错误处理**：使用 anyhow/thiserror 提供每一步的完整错误信息

## 环境要求

- 构建需要 Rust 1.88 或更高版本（edition 2024；已在 1.99.1 上测试）
- 需要与目标 Minecraft 版本兼容的 Java 运行时。启动器会按版本 manifest 中声明的 Java 大版本精确选择（常见为 8、16、17、21 或更新版本），并扫描 `JAVA_HOME`、`PATH` 和常见厂商安装目录；可用 `--runtime` 覆盖
- 首次安装需要网络。Forge/NeoForge 还需要 Java 来运行官方安装器

## 构建

```bash
git clone https://github.com/HuanMeng-official/LauncherCore.git
cd LauncherCore
cargo build --release
```

构建完成后，可执行文件位于 `target/release/mclc`（Windows 上为 `target/release/mclc.exe`）。

## 使用方法

```
mclc <命令> [选项]
```

### 命令

| 命令 | 描述 |
|------|------|
| **list** | 列出所有可用的 Minecraft 版本 |
| **java** | 列出检测到的 Java 运行时 |
| **install <版本>** | 安装 Minecraft 版本，可选安装模组加载器 |
| **launch <版本>** | 启动已安装的 Minecraft 版本 |
| **login** | 登录到微软账户 |
| **external-login <账号>** | 登录到外置 Yggdrasil 服务器（authlib-injector） |
| **json** | 启动面向 GUI 集成的常驻 NDJSON 服务 |
| **help** | 显示帮助信息 |

### 全局选项

| 选项 | 描述 |
|------|------|
| `-r, --runtime <路径>` | Java 可执行文件路径（覆盖自动检测） |
| `--download-source <来源>` | 下载源：`auto`（默认）、`official` 或 `bmclapi` |

### 安装选项

| 选项 | 描述 |
|------|------|
| `--loader <加载器>` | 安装模组加载器：`fabric`、`quilt`、`forge` 或 `neoforge` |
| `--loader-version <版本>` | 固定加载器版本；默认选择稳定/推荐版本 |

### 启动选项

| 选项 | 描述 |
|------|------|
| `-u, --username <名称>` | 游戏用户名（离线模式）或账号标识（外置登录） |
| `--access-token <令牌>` | 已有的 Minecraft 访问令牌（MSA 认证） |
| `-j, --jvm-args <参数>` | 自定义 JVM 参数（如 `-Xmx4G -XX:+UseG1GC`） |
| `--auth <类型>` | 认证类型：`offline`（默认）、`msa` 或 `external` |
| `--api-url <地址>` | Yggdrasil API 地址（外置登录） |
| `--authlib-jar <路径>` | 使用本地的 authlib-injector.jar，跳过自动下载 |
| `-r, --runtime <路径>` | Java 可执行文件路径（覆盖自动检测） |

### 外置登录选项

| 选项 | 描述 |
|------|------|
| `-p, --password <密码>` | 密码。省略时会无回显提示输入；脚本中可使用 `MCLC_PASSWORD` 环境变量 |
| `-a, --api-url <地址>` | 认证服务器的 API 地址 |

## 使用示例

### 查看已检测的 Java

```bash
mclc java
```

### 离线模式

```bash
mclc install 1.21.1
mclc launch 1.21.1 --username 玩家名
```

使用 BMCLAPI 镜像（适合国内网络）：

```bash
mclc install 1.21.1 --download-source bmclapi
```

使用自定义 Java 路径和 JVM 参数：

```bash
mclc launch 1.21.1 --username 玩家名 --runtime "C:\Program Files\Java\jdk-21\bin\java.exe" --jvm-args "-Xmx4G -XX:+UseG1GC"
```

### 模组加载器

```bash
mclc install 1.21.1 --loader fabric
mclc install 1.21.1 --loader quilt
mclc install 1.21.1 --loader forge
mclc install 1.21.1 --loader neoforge

# 固定加载器版本
mclc install 1.21.1 --loader fabric --loader-version <加载器版本>
```

安装命令会打印生成的加载器版本 ID，之后直接启动它：

```bash
mclc launch <加载器版本ID>
```

加载器版本选择规则：

- **Fabric**：Fabric 元数据中最新的稳定版加载器
- **Quilt**：最新的稳定版（非 beta），没有稳定版时回退到最新版
- **Forge**：优先使用 `recommended` 版本，没有时使用 `latest`
- **NeoForge**：选择与目标 Minecraft 版本兼容的最新稳定版（1.20.1 使用旧的 `net.neoforged:forge` 构件）

安装命令会先确保原版父版本已安装，再安装加载器。Fabric 和 Quilt 的版本 JSON 由加载器元数据构建；Forge 和 NeoForge 会下载官方安装器、校验 SHA-1，并使用检测到的 Java 以 `--installClient` 无界面运行。如果自动选择的 Java 不可用，可用 `--runtime` 指定。

### 在线模式（微软账户）

```bash
mclc login
# 按照设备码认证流程完成登录
mclc install 1.21.1
mclc launch 1.21.1 --auth msa
```

缓存的凭据会在启动前自动刷新。也可以直接使用已有的 Minecraft 访问令牌启动：

```bash
mclc launch 1.21.1 --auth msa --access-token <Minecraft访问令牌>
```

### 外置登录（authlib-injector）

```bash
mclc external-login player@example.com --api-url https://littleskin.cn
# 密码会以无回显方式提示输入；脚本中可设置 MCLC_PASSWORD
mclc install 1.21.1
mclc launch 1.21.1 --auth external --api-url https://littleskin.cn --username player@example.com
```

API 地址会通过服务器的 ALI 响应头解析，因此 `external-login` 和 `launch` 使用同一个 `--api-url` 即可。如果已有本地 authlib-injector.jar，可使用 `--authlib-jar` 跳过下载。

### 列出可用版本

```bash
mclc list
```

### JSON 通信模式

`mclc json` 会启动一个面向 GUI 集成的常驻 NDJSON 服务：从 stdin 每行读取一个 JSON 请求，向 stdout 每行输出一个 JSON 事件。原本的人类可读输出会转换为 `log` 事件，因此协议流中不会混入非结构化文本。

完整协议参考和与框架无关的客户端接入指南见 [INTEGRATION_zh_CN.md](INTEGRATION_zh_CN.md)。

```bash
mclc json
```

启动时首先输出 `{"type":"ready","protocol":1,"version":"..."}`。每个请求带有一个由客户端指定的 `id`，对应的结果或错误会原样返回该 `id`；请求执行期间产生的事件也会携带该 `id`。协议在 stdout 上每行只输出一个完整的 JSON 对象，客户端可以忽略 stderr（仅用于诊断）：

```json
{"id":1,"command":"install","params":{"version":"1.21.1","loader":"fabric"}}
{"type":"progress","id":1,"phase":"downloads","current":42,"total":317,"message":"..."}
{"type":"result","id":1,"ok":true,"data":{"version_id":"fabric-loader-0.19.5-1.21.1"}}
```

错误使用相同的信封格式，`ok` 为 `false`：

```json
{"type":"error","id":2,"ok":false,"error":"Unknown command: foo"}
```

支持的命令：

| 命令 | 参数 | 结果数据 |
|------|------|----------|
| `ping` | 无 | `{"pong":true}` |
| `shutdown` | 无 | `{"shutdown":true}`，随后服务退出 |
| `java.list` | 无 | `{"runtimes":[...]}` |
| `versions.list` | 无 | `{"versions":[...]}` |
| `install` | `version`，可选 `loader`、`loader_version`、`download_source`、`runtime` | `{"version_id":"..."}` |
| `launch` | `version`，可选 `username`、`access_token`、`jvm_args`、`auth`、`api_url`、`authlib_jar`、`runtime` | `{"launched":true,"version":"..."}` |
| `login` | 无 | `{"username":"...","uuid":"..."}` |
| `external_login` | `identifier`、`password`、`api_url`，可选 `profile` | 账号信息 |
| `accounts.list` | 无 | `{"accounts":[...],"msa":{...}}`（未缓存微软账号时 `msa` 为 `null`） |

除结果和错误外的事件：

- `log` — `level`（`info`/`warn`）和 `message`
- `progress` — `phase`、`current`、`total` 和 `message`
- `device_code` — 微软登录期间的 `user_code`、`verification_uri`、`expires_in` 和 `message`
- `profiles` — 外置登录需要选择角色时返回的可用角色列表

注意事项：

- 请求会依次处理。`launch` 在游戏进程创建后即返回，不会等待游戏退出。
- 如果 `external_login` 返回多个角色且未提供 `profile`，服务会输出 `profiles` 事件并返回 `profile selection required`；客户端应带上所选角色的 ID 或名称重试。
- JSON 模式只接受请求参数中的密码，不会从终端读取。
- 全局 `--runtime` 和 `--download-source` 选项提供默认值，请求参数可以覆盖它们。

## 运行原理

### 版本发现与安装

1. 获取 Mojang 的[版本清单](https://launchermeta.mojang.com/mc/game/version_manifest.json)
2. 下载客户端 JAR 和版本 JSON
3. 应用依赖库规则，下载所需的库、原生库、资源索引和资源对象
4. 在信息可用时校验 HTTP 状态、文件大小和 SHA-1，先写临时文件再原子替换
5. 失败请求按指数退避重试；`auto` 源会回退到 BMCLAPI
6. 解压当前平台的原生库，并在 classpath 上保留 LWJGL3 需要的 native JAR
7. 使用并发下载并显示进度

### 版本继承与参数组装

- 启动器会沿 `inheritsFrom` 链加载版本 JSON，并检测循环继承
- `mainClass`、`assetIndex`、`downloads`、`javaVersion` 等字段由子版本覆盖或从父版本继承
- 依赖库按父版本 + 子版本合并，同名依赖由子版本覆盖
- 参数按“父版本在前、子版本在后”追加，**不做去重**，保证重复的 `--add-opens`、`--add-exports` 等 JVM 标志和值保持配对
- 客户端 JAR 由子版本的 `jar` 字段、父版本的 `jar` 字段或父版本 ID 决定
- 替换 `${natives_directory}`、`${library_directory}`、`${classpath_separator}`、`${auth_player_name}`、`${auth_access_token}` 等占位符
- JVM 参数中的 `${version_name}` 解析为游戏 JAR 名（例如 `1.18.2`）；游戏参数中的 `${version_name}` 解析为启动的版本 ID
- 评估系统和架构规则；旧版本回退到 `minecraftArguments`

### 启动流程

- 按版本的 `javaVersion.majorVersion` 选择 Java；提供 `--runtime` 时直接使用指定路径
- 由解析后的游戏 JAR 和合并后的依赖库组装 classpath
- 配置原生库路径和 LWJGL 解压路径
- 组装 JVM 和游戏参数，包括系统/架构规则和占位符
- 将游戏进程的工作目录设置为 `.minecraft`，日志、崩溃报告和 options 文件都会留在游戏目录内
- 缓存的微软凭据临近过期时自动刷新
- 离线模式下按用户名生成标准离线 UUID
- 在打印的启动命令中隐藏访问令牌等敏感信息

### 模组加载器安装

- Fabric 和 Quilt 的版本 JSON 由加载器元数据（`launcherMeta`）构建并保存到本地
- Forge 和 NeoForge 的安装器从官方 Maven 下载，校验 SHA-1 后以 `--installClient` 无界面运行
- 版本 ID 从安装器内嵌的 `version.json` 或 `install_profile.json` 读取
- 安装器完成后，再校验并下载缺失的加载器依赖和父版本文件

### 认证流程（微软账户）

1. 通过微软 OAuth2 发起设备码流程
2. 用户在网页浏览器或 Microsoft Authenticator 应用中完成认证
3. 轮询获取认证令牌
4. 将微软令牌兑换为 Xbox Live 令牌
5. 将 Xbox Live 令牌兑换为 XSTS 令牌
6. 使用 Minecraft 服务进行认证
7. 获取 Minecraft 资料信息（UUID、用户名）
8. 缓存 `access_token`、`uuid`、`username`、微软 `refresh_token` 和令牌过期时间
9. 令牌临近过期时，在下次启动前自动续期；续期失败时重新执行 `mclc login`

### 认证流程（外置 Yggdrasil）

1. 通过 `X-Authlib-Injector-API-Location` 响应头解析服务器 API 地址
2. 获取服务器元数据（服务器名称、签名公钥）
3. 使用账号密码认证并获取访问令牌
4. 如果账号有多个角色且未选中角色，提示用户选择，并通过 `/authserver/refresh` 绑定所选角色
5. 将账号保存到 `accounts.json`
6. 启动时校验令牌，必要时自动刷新，并以 Java agent 方式加载 authlib-injector

## 已测试配置

以下配置在开发过程中做过端到端测试：

| 配置 | 结果 |
|------|------|
| Minecraft 1.21.1（原版） | 已安装（启动路径由加载器测试覆盖） |
| Fabric 1.21.1 | 已安装并启动（`fabric-loader-0.19.5-1.21.1`） |
| Forge 1.12.2 | 已安装并启动（`1.12.2-forge-14.23.5.2859`） |
| Forge 1.21.1 | 已安装并启动（`1.21.1-forge-52.1.0`） |
| NeoForge 1.21.1 | 已安装并启动（`neoforge-21.1.264`） |
| Forge 1.16.5 / 1.18.2 | 已按官方安装器 JSON 核对安装参数结构 |

该列表不是兼容性保证；Minecraft、加载器和 Java 版本会持续变化。

## 项目结构

```
LauncherCore/
├── src/
│   ├── main.rs           # 主入口
│   ├── cli.rs            # CLI 参数定义
│   ├── commands.rs       # 共享的安装与启动命令逻辑
│   ├── output.rs         # 人类可读/JSON 输出抽象
│   ├── json_server.rs    # 常驻 NDJSON 通信服务
│   ├── install.rs        # 版本安装与文件下载
│   ├── download.rs       # 下载源、URL 映射与重试
│   ├── version.rs        # 版本 JSON 继承与合并
│   ├── loader.rs         # Fabric/Quilt/Forge/NeoForge 安装
│   ├── launch.rs         # 启动参数组装与进程执行
│   ├── launch_manager.rs # 启动配置和账号存储
│   ├── java.rs           # Java 运行时检测和选择
│   ├── auth.rs           # 微软账户认证
│   ├── yggdrasil.rs      # 外置 Yggdrasil 认证和 authlib-injector
│   ├── models.rs         # 数据模型和 JSON 结构
│   ├── error.rs          # 错误类型
│   └── test_http.rs      # HTTP 测试辅助模块（仅测试构建）
├── README/
│   ├── README_zh_CN.md        # 简体中文文档（本文件）
│   └── INTEGRATION_zh_CN.md   # 简体中文接入指南
├── INTEGRATION.md        # JSON 接入指南（英文）
├── Cargo.toml            # 项目配置和依赖
├── Cargo.lock            # 依赖版本锁定
└── README.md             # 主 README
```

## 依赖项

- `clap` - CLI 参数解析
- `tokio` - 异步运行时
- `reqwest` - HTTP 客户端（支持流式传输和表单）
- `serde` / `serde_json` - JSON 序列化/反序列化
- `anyhow` - 便捷的错误处理
- `thiserror` - 错误派生宏
- `zip` - ZIP 归档解压和安装器检查
- `dirs` - 跨平台配置目录路径
- `url` - URL 解析和操作
- `futures-util` - 异步工具（用于并行下载）
- `indicatif` - 进度条可视化
- `base64` - 编码 authlib-injector 预取元数据
- `uuid` - 生成客户端令牌和 UUID
- `tempfile` - 校验下载时的临时文件
- `sha1` - 下载完整性校验
- `sha2` - authlib-injector 完整性校验
- `rpassword` - 无回显密码输入
- `md-5` - 生成 Java 兼容的离线 UUID

## 游戏与配置目录

游戏文件存放在 `./.minecraft`，相对于执行 `mclc` 时的工作目录（不是用户主目录）。

| 数据 | 位置 |
|------|------|
| 游戏文件 | `./.minecraft`（当前工作目录） |
| 日志 | `./.minecraft/logs` |
| 崩溃报告 | `./.minecraft/crash-reports` |
| 配置（Windows） | `%APPDATA%\mclc` |
| 配置（Linux） | `$XDG_CONFIG_HOME/mclc` 或 `~/.config/mclc` |
| 配置（macOS） | `~/Library/Application Support/mclc` |

配置目录中包含 `auth_cache.json`、`accounts.json` 和下载的 `cache/authlib-injector.jar`。在 Unix 下，目录权限为 `0700`，凭据文件权限为 `0600`。

游戏目录内部结构：

```
.minecraft/
├── versions/
│   └── <版本ID>/
│       ├── <版本ID>.json    # 版本 JSON（可能包含 inheritsFrom）
│       ├── <版本ID>.jar     # 原版版本的客户端 JAR
│       └── natives/         # 已提取的原生库
├── libraries/               # 共享库文件
├── assets/
│   ├── indexes/             # 资源索引 JSON 文件
│   └── objects/             # 已下载的资源文件
├── logs/                    # 游戏日志
└── crash-reports/           # 崩溃报告
```

加载器版本通常没有自己的客户端 JAR，而是通过 `inheritsFrom` 使用父版本的 JAR。Forge/NeoForge 安装器还可能在游戏目录中创建 `launcher_profiles.json`。

## 常见问题

### 找不到 Java？

先运行 `mclc java` 查看检测到的 Java 运行时。安装目标 Minecraft 版本所需的大版本，或显式指定路径：

```bash
mclc launch 1.21.1 --username 玩家名 --runtime "C:\Program Files\Java\jdk-21\bin\java.exe"
```

### 下载失败或卡住？

下载会自动重试。如果 Mojang 服务器较慢或无法访问，可显式使用 BMCLAPI 镜像：

```bash
mclc install 1.21.1 --download-source bmclapi
```

Forge/NeoForge 安装器在安装过程中仍可能从官方源下载部分文件。

### 模组加载器安装问题？

1. 用 `mclc java` 确认已安装匹配的 Java；Forge/NeoForge 安装器需要 Java
2. 如果自动选择的 Java 无法运行安装器，使用 `--runtime <路径>` 指定
3. 使用 `--loader-version <版本>` 固定加载器版本
4. Forge/NeoForge 安装需要能访问其 Maven 仓库
5. 安装命令会打印确切的加载器版本 ID，启动时使用该 ID

### 启动失败或报错？

1. 验证所有资源和依赖是否完整下载
2. 查看 `.minecraft/logs/latest.log` 和 `.minecraft/crash-reports/`
3. 确保 Java 版本与目标 Minecraft 版本兼容
4. 尝试重新安装版本：`mclc install <版本>`
5. 如果模组加载器报 `ResolutionException` 或缺少模块，请使用最新构建；启动器会保留重复的 JVM 标志并正确解析 Forge/NeoForge 的 `${version_name}`

### 微软账户认证问题？

1. 确保您的微软账户已关联 Xbox 账户
2. 儿童账户可能需要由成年人添加到家庭中
3. 您的账户所在地区必须支持 Xbox Live
4. 如果缓存的令牌无法继续刷新，请重新执行 `mclc login`

### 外置登录问题？

1. 确认 API 地址可访问，并且提供 `X-Authlib-Injector-API-Location` 响应头或有效的 Yggdrasil API
2. `external-login` 和 `launch` 使用相同的 `--api-url` 值
3. 如果令牌过期，重新执行 `external-login`，或让 `launch` 自动刷新
4. 可使用 `--authlib-jar` 跳过 authlib-injector 下载

### Linux ARM64 问题？

启动器会为 Linux ARM64 选择合适的 LWJGL 原生库；如果版本 manifest 中没有对应项，会回退到从 Maven 下载。如果遇到问题，请确保系统已安装必要的库：

```bash
sudo apt-get install libx11-dev libxcursor-dev libxrandr-dev libxinerama-dev
```

### 同时运行多个实例

从同一个 `.minecraft` 目录运行多个游戏实例会共享 `logs/latest.log`。如果需要隔离日志或存档，请使用不同的游戏目录。

## 贡献

欢迎提交 Issue 和 Pull Request！请先提交 Issue 描述你的想法，然后再提交 Pull Request。

## 许可证

MIT
