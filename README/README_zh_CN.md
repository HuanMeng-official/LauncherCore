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
- **下载校验**：在信息可用时校验 HTTP 状态、文件大小和 SHA-1，先写临时文件再原子替换目标文件
- **可靠安装**：下载、解压或资源索引解析失败时会中止安装，不再误报成功
- **多线程下载**：支持并发下载，加速安装过程（最多 16 个并行下载）
- **进度可视化**：下载过程中实时显示进度条
- **跨平台支持**：支持 Windows、Linux 和 macOS
- **Java 运行时检测**：扫描 `JAVA_HOME`、`PATH` 和常见安装目录，自动选择目标 Minecraft 版本要求的大版本；可用 `--runtime` 覆盖
- **原生库管理**：应用版本规则，只下载并解压当前系统和架构对应的原生库
- **版本参数驱动**：读取版本 manifest 的 `arguments`（含系统/架构规则），并为旧版本回退到 `minecraftArguments`
- **认证支持**：支持离线、微软账户和外置 Yggdrasil（authlib-injector）认证
- **微软令牌自动刷新**：保存微软 refresh token 和 Minecraft 令牌过期时间，启动前按需自动续期
- **标准离线 UUID**：离线模式按 Java 的 `OfflinePlayer:<名称>` 规则生成 UUID
- **多账号支持**：可保存多个外置账号，并在令牌过期时自动刷新
- **凭据保护**：启动日志会隐藏访问令牌；Unix 下凭据文件使用 `0600` 权限
- **自定义 JVM 参数**：支持传入自定义 JVM 参数进行性能调优
- **异步高效**：利用 Tokio + Reqwest 实现高性能异步下载
- **ARM64 支持**：为 Linux ARM64 提供专门的 LWJGL 原生库处理
- **详细错误处理**：使用 anyhow/thiserror 提供每一步的完整错误信息

## 环境要求

- 构建需要 Rust 1.88 或更高版本（edition 2024；已在 1.99.1 上测试）
- 需要与目标 Minecraft 版本兼容的 Java 运行时。启动器会从 `JAVA_HOME`、`PATH` 和常见安装目录自动检测，也可用 `--runtime` 覆盖。不同 Minecraft 版本可能需要 Java 8、16、17 或 21。

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
| **java** | 列出检测到的 Java 运行时 |
| **install <版本>** | 安装指定的 Minecraft 版本 |
| **launch <版本>** | 启动指定的 Minecraft 版本 |
| **login** | 登录到微软账户 |
| **external-login <账号>** | 登录到外置 Yggdrasil 服务器（authlib-injector） |
| **help** | 显示帮助信息 |

### 全局选项

| 选项 | 描述 |
|------|------|
| `-r, --runtime <路径>` | 覆盖自动 Java 检测 |

### 启动选项

| 选项 | 描述 |
|------|------|
| `-u, --username <名称>` | 游戏用户名（离线模式）或账号标识（外置登录） |
| `--access-token <令牌>` | 已有的 Minecraft 访问令牌（MSA 认证） |
| `-j, --jvm-args <参数>` | 自定义 JVM 参数（如 `-Xmx4G -XX:+UseG1GC`） |
| `--auth <类型>` | 认证类型：`offline`（默认）、`msa` 或 `external` |
| `--api-url <地址>` | Yggdrasil API 地址（外置登录） |
| `--authlib-jar <路径>` | 使用本地的 authlib-injector.jar，跳过自动下载 |
| `-r, --runtime <路径>` | 覆盖自动 Java 检测 |

### 外置登录选项

| 选项 | 描述 |
|------|------|
| `-p, --password <密码>` | 密码。省略时会无回显提示输入；脚本中可使用 `MCLC_PASSWORD` 环境变量 |
| `-a, --api-url <地址>` | 认证服务器的 API 地址 |

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

使用已有的 Minecraft 访问令牌启动：

```bash
mclc launch 1.21.3 --auth msa --access-token <Minecraft访问令牌>
```

### 外置登录（authlib-injector）

```bash
mclc external-login player@example.com --api-url https://littleskin.cn
# 密码会以无回显方式提示输入；脚本中可设置 MCLC_PASSWORD
mclc install 1.21.3
mclc launch 1.21.3 --auth external --api-url https://littleskin.cn --username player@example.com
```

API 地址会通过服务器的 ALI 响应头解析，因此 `external-login` 和 `launch` 使用同一个 `--api-url` 即可。如果已有本地 authlib-injector.jar，可使用 `--authlib-jar` 跳过下载。

### 列出可用版本

```bash
mclc list
```

## 运行原理

1. **版本发现**：调用 Mojang 官方的[版本清单](https://launchermeta.mojang.com/mc/game/version_manifest.json)获取所有支持版本
2. **安装过程**：
   - 下载版本清单 JSON
   - 下载客户端 JAR 文件
   - 应用依赖库规则，只下载当前系统和架构可用的依赖库/原生库
   - 下载资源索引和所有资源文件
   - 在信息可用时校验 HTTP 状态、文件大小和 SHA-1，先写临时文件再原子替换
   - 将原生库提取到版本独立目录
   - 使用并发下载并显示进度
3. **启动过程**：
   - 按版本的 `javaVersion.majorVersion` 选择 Java（或使用 `--runtime`）
   - 组装完整的类路径
   - 配置原生库路径
   - 解析版本 manifest 的 JVM 和游戏参数，包括系统/架构规则与占位符
   - 为旧版本回退到 `minecraftArguments`
   - 缓存的微软凭据临近过期时自动刷新
   - 离线模式下按用户名生成标准离线 UUID
   - 在打印的命令中隐藏访问令牌等敏感信息
   - 使用 Java 启动 Minecraft 客户端

### 认证流程（微软账户）

1. 通过微软 OAuth2 发起设备码流程
2. 用户在网页浏览器或 Microsoft Authenticator 应用中完成认证
3. 轮询获取认证令牌
4. 将微软令牌兑换为 Xbox Live 令牌
5. 将 Xbox Live 令牌兑换为 XSTS 令牌
6. 使用 Minecraft 服务进行认证
7. 获取 Minecraft 资料信息（UUID、用户名）
8. 缓存 `access_token`、`uuid`、`username`、微软 `refresh_token` 和令牌过期时间
9. 后续启动时，如果缓存的令牌临近过期，会自动续期微软令牌和 Minecraft 凭据；续期失败时重新执行 `mclc login`

### 认证流程（外置 Yggdrasil）

1. 通过 `X-Authlib-Injector-API-Location` 响应头解析服务器 API 地址
2. 获取服务器元数据（服务器名称、签名公钥）
3. 使用账号密码认证并获取访问令牌
4. 如果账号有多个角色且未选中角色，提示用户选择，并通过 `/authserver/refresh` 绑定所选角色
5. 将账号保存到 `accounts.json` 供后续启动使用
6. 启动时校验令牌，必要时自动刷新，并以 Java agent 方式加载 authlib-injector

## 项目结构

```
LauncherCore/
├── src/
│   ├── main.rs           # 主入口
│   ├── cli.rs            # CLI 参数定义
│   ├── install.rs        # 安装逻辑和下载
│   ├── launch.rs         # 启动参数组装和执行
│   ├── launch_manager.rs # 启动配置和账号存储
│   ├── java.rs           # Java 运行时检测和选择
│   ├── auth.rs           # 微软账户认证
│   ├── yggdrasil.rs      # 外置 Yggdrasil 认证和 authlib-injector
│   ├── models.rs         # 数据模型和 JSON 结构
│   ├── error.rs          # 错误类型
│   └── test_http.rs      # HTTP 测试辅助模块（仅测试构建）
├── README/
│   └── README_zh_CN.md   # 简体中文文档（本文件）
├── Cargo.toml            # 项目配置和依赖
├── Cargo.lock            # 依赖版本锁定
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

## 游戏与配置目录

游戏文件存放在 `./.minecraft`，相对于执行 `mclc` 时的工作目录（不是用户主目录）。

| 数据 | 位置 |
|------|------|
| 游戏文件 | `./.minecraft`（当前工作目录） |
| 配置（Windows） | `%APPDATA%\mclc` |
| 配置（Linux） | `$XDG_CONFIG_HOME/mclc` 或 `~/.config/mclc` |
| 配置（macOS） | `~/Library/Application Support/mclc` |

配置目录中包含 `auth_cache.json`、`accounts.json` 和下载的 `cache/authlib-injector.jar`。在 Unix 下，目录权限为 `0700`，凭据文件权限为 `0600`。

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

先运行 `mclc java` 查看检测到的 Java 运行时。安装目标 Minecraft 版本所需的大版本，或显式指定路径：

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
4. 如果缓存的令牌无法继续刷新，请重新执行 `mclc login`

### 外置登录问题？

1. 确认 API 地址可访问，并且提供 `X-Authlib-Injector-API-Location` 响应头或有效的 Yggdrasil API
2. `external-login` 和 `launch` 使用相同的 `--api-url` 值
3. 如果令牌过期，重新执行 `external-login`，或让 `launch` 自动刷新
4. 可使用 `--authlib-jar` 跳过 authlib-injector 下载

### Linux ARM64 问题？

启动器会自动下载 Linux ARM64 的 LWJGL 原生库。如果遇到问题，请确保系统已安装必要的库：

```bash
sudo apt-get install libx11-dev libxcursor-dev libxrandr-dev libxinerama-dev
```

## 贡献

欢迎提交 Issue 和 Pull Request！请先提交 Issue 描述你的想法，然后再提交 Pull Request。

## 许可证

MIT
