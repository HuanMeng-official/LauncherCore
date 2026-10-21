# LauncherCore JSON 接入指南

**协议版本：** 1  
**读者：** GUI 启动器、安装器前端、自动化工具以及其他集成 LauncherCore 的客户端  
**适用命令：** `mclc json` 以及 `mclc web` WebSocket 传输

本文说明 `mclc json` 提供的常驻 NDJSON 协议，以及如何围绕它编写客户端。文档与具体 GUI 框架无关：任何能够启动子进程并读写管道的运行时都可以接入。命令行用法见
[README_zh_CN.md](README_zh_CN.md)，实现位于 [../src/json_server.rs](../src/json_server.rs) 和
[../src/output.rs](../src/output.rs)。

English version: [../INTEGRATION.md](../INTEGRATION.md).

---

## 目录

1. [设计原则](#1-设计原则)
2. [进程模型](#2-进程模型)
3. [消息分帧](#3-消息分帧)
4. [请求、结果与错误](#4-请求结果与错误)
5. [命令参考](#5-命令参考)
6. [事件](#6-事件)
7. [编写客户端](#7-编写客户端)
8. [生命周期与故障处理](#8-生命周期与故障处理)
9. [打包与部署](#9-打包与部署)
10. [安全](#10-安全)
11. [已知限制](#11-已知限制)
12. [故障排查](#12-故障排查)
13. [附录：完整会话示例](#13-附录完整会话示例)

---

## 1. 设计原则

- `mclc json` 是一个**常驻子进程**，不是 HTTP 服务。它不监听端口，也不提供网络接口。
- **stdin 接收请求，stdout 输出协议消息**，两者都使用 UTF-8 NDJSON：每行一个完整的 JSON 对象。
- 启动器中原本的人类可读输出会转换成 `log` 事件，不会以普通文本混入 stdout。stderr 仅用于 panic 等诊断信息。
- 请求按照到达顺序**串行处理**，同一时刻只有一个正在执行的请求。
- 执行过程中的信息通过**事件**（`log`、`progress`、`device_code`、`profiles`）上报，请求最终以 `result` 或 `error` 结束。
- 通过 `ready` 事件中的 `protocol` 和 `version` 进行兼容性握手。

典型 GUI 分为三层：

```text
┌──────────────────────────────────────┐
│ UI 层（渲染进程、视图模型）          │
│   request(command, params)           │
│   onEvent(event)                     │
└──────────────────┬───────────────────┘
                   │ IPC 或直接函数调用
┌──────────────────▼───────────────────┐
│ 桥接层                               │
│   • 持有 mclc 子进程                 │
│   • 写入请求、读取 stdout            │
│   • 按 id 关联结果                   │
│   • 串行化请求                       │
│   • 把事件转发给 UI                  │
└──────────────────┬───────────────────┘
                   │ stdin/stdout，NDJSON
┌──────────────────▼───────────────────┐
│ mclc json                            │
│   • 下载并安装版本                   │
│   • 组装启动参数                     │
│   • 处理账号认证                     │
└──────────────────┬───────────────────┘
                   │
                   ▼
              <工作目录>/.minecraft
```

不要为每个操作单独启动一次 CLI 并解析人类可读输出。只启动一个 `mclc json` 并持续使用它。

---

## 2. 进程模型

### 启动服务

```bash
mclc json
mclc json --download-source bmclapi
mclc json --instance fabric
mclc json --game-dir "D:\Minecraft\modded"
mclc json --runtime "C:\Program Files\Java\jdk-21\bin\java.exe"
```

启动时提供的全局选项会成为本次会话的默认值：

| 选项 | 在 JSON 模式中的作用 |
|------|----------------------|
| `--download-source <auto\|official\|bmclapi>` | `versions.list` 的下载源，以及未指定 `download_source` 的 `install` 请求的默认下载源 |
| `-r, --runtime <路径>` | 未指定 `runtime` 的 `install` 和 `launch` 请求的默认 Java 可执行文件 |
| `--instance <名称>` | 使用已注册命名实例的游戏目录 |
| `--game-dir <路径>` | 使用显式指定的游戏目录（包含 `versions`、`libraries`、`assets`） |

### 工作目录

默认情况下，启动器把游戏文件存放在 **`<当前工作目录>/.minecraft`**。可用 `--instance <名称>` 或 `--game-dir <路径>` 选择其他游戏目录；`--game-dir` 指向 `.minecraft` 式目录本身，而不是它的上级目录。选定的目录在启动时固定，协议请求无法更改。要切换实例，请用目标选项重新启动服务。

账号数据与游戏目录分离，存放在操作系统用户配置目录下的 `mclc/` 中，例如 Windows 的 `%APPDATA%\mclc`、Linux 的 `~/.config/mclc`、macOS 的 `~/Library/Application Support/mclc`。同一个系统用户使用的所有游戏目录共享同一份微软账号缓存和外置账号列表。

### 启动握手

第一条协议消息始终是：

```json
{"type":"ready","protocol":1,"version":"0.1.1"}
```

- `protocol` 是协议版本。本文描述版本 `1`。如果该值不受支持，客户端应拒绝继续，而不是猜测行为。
- `version` 是启动器构建版本。请展示在诊断信息中，并与 GUI 版本保持一致。
- `ready` 的 `id` 为 `null`，因为此时没有活动请求。

如果启动超时仍未收到 `ready`，请检查子进程的退出状态和 stderr；它可能在协议启动前就失败了。

### 关闭

- 发送 `{"command":"shutdown"}`，服务会返回结果后退出。
- 关闭 stdin 也会在当前请求执行完后结束服务。
- 必须回收子进程；如果优雅关闭超时，应终止它。否则 GUI 重启后可能留下仍占用游戏目录的孤儿进程。

### 每个客户端一个服务

协议只有一条请求流和一条事件流。每个应用实例（以及每个游戏根目录）应运行一个服务。如果多个窗口都需要启动器操作，应统一经过 GUI 中的一个桥接进程，而不是对同一个游戏目录启动多个服务。

---

## 3. 消息分帧

- 编码为 **UTF-8**。
- 每行恰好一个 JSON 对象。JSON 字符串会转义内部换行，因此每行都以分帧换行符结束。
- 输出行以 `\n` 结尾。客户端按 `\n` 切分，并容忍行尾的 `\r`。
- 输入行会被去除首尾空白；空行会被忽略。
- stdout 只承载协议。不要把 stderr 当作协议解析，应将其作为诊断信息展示。
- 服务每写入一条消息都会 flush。
- 不要发送格式化后的多行 JSON。每个对象必须在一行内。
- JSON 对象的键顺序没有意义，不要依赖示例中的字段顺序。
- stdout 管道可能一次只给出半行，也可能一次给出多行。解析前必须先缓冲到换行符。

---

## 4. 请求、结果与错误

### 请求

```json
{"id":1,"command":"ping"}
{"id":2,"command":"install","params":{"version":"1.21.1","loader":"fabric"}}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `id` | 任意 JSON 值 | 否 | 客户端选择的关联 ID。对应的结果或错误会原样带回，请求执行期间产生的事件也会携带它。建议使用唯一的数字或字符串。省略时为 `null`。 |
| `command` | string | 是 | 命令名，见[命令参考](#5-命令参考)。 |
| `params` | object | 视命令而定 | 命令参数。无参数命令会忽略该字段；有参数命令要求传入对象。 |

服务会忽略未知字段。客户端仍应只发送文档中定义的字段。

如果某行不是合法 JSON，服务会返回 `id` 为 `null` 的错误，因为无法还原请求 ID：

```json
{"type":"error","id":null,"ok":false,"error":"Invalid request: expected ident at line 1 column 2"}
```

如果 JSON 合法但命令或参数有误，错误会携带请求 ID：

```json
{"type":"error","id":2,"ok":false,"error":"Invalid install parameters: missing field `version`"}
```

### 结果

一个成功请求以且仅以一个结果结束：

```json
{"type":"result","id":2,"ok":true,"data":{"version_id":"1.21.1"}}
```

- `data` 由命令决定，可能是对象、数组、字符串、数字、布尔值或 `null`。
- 结果中的 `ok` 始终为 `true`。

### 错误

一个失败请求以且仅以一个错误结束：

```json
{"type":"error","id":2,"ok":false,"error":"Version 1.99 not found"}
```

- `error` 是供人阅读的消息，可能包含原因链，例如 `Invalid install parameters: missing field \`version\``。
- 不要用 `error` 文本做控制流。它只用于展示和诊断。健壮的客户端应根据操作和上下文映射常见失败，而不是精确匹配字符串。
- 请求产生错误后不会再产生结果。

### 顺序

- 请求按顺序读取和处理；请求 N 的响应会在请求 N+1 开始之前发出。
- `log`、`progress`、`device_code`、`profiles` 事件可能出现在请求与最终结果之间，并且始终属于当前正在执行的请求。
- 在请求之外产生的事件（尤其是 `ready`）`id` 为 `null`。
- 并发下载产生的进度事件可能以略微不同的顺序到达。把 `current`/`total` 当作估算值，不要假设它严格单调递增。

---

## 5. 命令参考

总览：

| 命令 | 参数 | 结果数据 | 说明 |
|------|------|----------|------|
| `ping` | 无 | `{"pong":true}` | 健康检查 |
| `shutdown` | 无 | `{"shutdown":true}` | 返回结果后退出 |
| `java.list` | 无 | `{"runtimes":[...]}` | 扫描本机 Java |
| `versions.list` | 无 | `{"versions":[...]}` | 网络请求 |
| `installed.list` | 无 | `{"versions":[{"id":"..."}]}` | 读取本地 versions 目录 |
| `install` | 对象 | `{"version_id":"..."}` | 下载可能耗时较长 |
| `launch` | 对象 | `{"launched":true,"version":"..."}` | 创建游戏进程后返回 |
| `login` | 无 | `{"username":"...","uuid":"..."}` | 设备码流程期间阻塞 |
| `external_login` | 对象 | 账号信息 | 可能需要选择角色 |
| `accounts.list` | 无 | `{"accounts":[...],"msa":...}` | 不返回令牌 |

### 5.1 `ping`

请求：

```json
{"id":1,"command":"ping"}
```

结果：

```json
{"type":"result","id":1,"ok":true,"data":{"pong":true}}
```

用于确认子进程仍然存活、协议仍能响应。

### 5.2 `shutdown`

请求：

```json
{"id":2,"command":"shutdown"}
```

结果：

```json
{"type":"result","id":2,"ok":true,"data":{"shutdown":true}}
```

写出结果后服务退出。不要在该连接上继续发送请求。

### 5.3 `java.list`

每次调用都会扫描 `JAVA_HOME`、`PATH` 以及常见厂商安装目录。启动器不会自动下载 Java。

结果：

```json
{
  "type": "result",
  "id": 3,
  "ok": true,
  "data": {
    "runtimes": [
      {
        "path": "C:\\Program Files\\Zulu\\zulu-21\\bin\\java.exe",
        "major": 21,
        "version": "21.0.3",
        "arch": "x86_64"
      }
    ]
  }
}
```

字段：

| 字段 | 类型 | 说明 |
|------|------|------|
| `path` | string | Java 可执行文件路径 |
| `major` | number 或 `null` | 检测到的主版本号 |
| `version` | string 或 `null` | 完整版本字符串 |
| `arch` | string 或 `null` | 检测到的架构 |

`runtimes` 的顺序没有保证。界面可以自行排序或筛选。

### 5.4 `versions.list`

使用当前下载源获取 Mojang 版本清单。这是网络请求，可能较慢或失败。

结果：

```json
{
  "type": "result",
  "id": 4,
  "ok": true,
  "data": {
    "versions": [
      {
        "id": "1.21.1",
        "type": "release",
        "url": "https://piston-meta.mojang.com/v1/packages/.../1.21.1.json"
      }
    ]
  }
}
```

`type` 是清单中的版本类型，例如 `release`、`snapshot` 或 `old_beta`。

### 5.5 `install`

安装原版版本，或安装原版父版本加模组加载器。安装是幂等的：已下载的文件会被复用。

参数：

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `version` | string | 是 | Minecraft 版本，例如 `"1.21.1"` |
| `loader` | string | 否 | `fabric`、`quilt`、`forge` 或 `neoforge`；省略则安装原版 |
| `loader_version` | string | 否 | 固定加载器版本；默认选择稳定/推荐版本 |
| `download_source` | string | 否 | `auto`、`official` 或 `bmclapi`；仅覆盖本次请求的默认下载源 |
| `runtime` | string | 否 | 用于安装加载器的 Java 可执行文件，尤其是 Forge/NeoForge；默认使用启动参数 `--runtime` |

示例：

```json
{
  "id": 5,
  "command": "install",
  "params": {
    "version": "1.21.1",
    "loader": "fabric",
    "download_source": "bmclapi"
  }
}
```

结果：

```json
{"type":"result","id":5,"ok":true,"data":{"version_id":"fabric-loader-0.19.5-1.21.1"}}
```

启动时务必使用返回的 `version_id`。安装加载器后，它通常不等于请求中的原版 `version`。

行为与事件：

- 会先安装原版父版本。
- Fabric 和 Quilt 的版本 JSON 由各自的元数据 API 构建。
- Forge 和 NeoForge 使用官方安装器无界面运行。安装器的控制台输出会被捕获，不会流式发送给客户端；安装期间通常只会看到 `loader_install` 的开始和结束事件。
- 下载内部可能并行，但请求本身只有一个，并通过 `progress` 事件上报进度。
- 安装失败会返回错误。稍后重试时可能会复用已经下载的文件。
- `download_source` 只对本次请求生效，结束后恢复会话默认值。

### 5.6 `launch`

启动一个**已经安装**的版本。

参数：

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `version` | string | 是 | 已安装的版本 ID，例如 `"1.21.1"` 或 `install` 返回的 `version_id` |
| `auth` | string | 否 | `offline`（默认）、`msa` 或 `external` |
| `username` | string | 视认证方式 | 离线玩家名（默认 `Player`）或外置账号标识 |
| `access_token` | string | 否 | 已有的 Minecraft 访问令牌，配合 `auth: "msa"` |
| `jvm_args` | string | 否 | 附加 JVM 参数，按空白字符切分 |
| `api_url` | string | 仅外置登录 | Yggdrasil API 地址 |
| `authlib_jar` | string | 否 | 本地 authlib-injector.jar 路径，仅外置登录使用 |
| `runtime` | string | 否 | Java 可执行文件；默认使用启动参数 `--runtime` 和自动选择 |

认证组合：

| `auth` | `username` | `access_token` | `api_url` | 行为 |
|--------|------------|----------------|-----------|------|
| `offline` | 可选，默认 `Player` | 可选，作为会话令牌，默认 `0` | 忽略 | 按用户名生成标准离线 UUID |
| `msa` | 忽略 | 可选 | 忽略 | 不传令牌时使用缓存的微软账号，必要时自动刷新；传令牌时用该令牌获取 Minecraft 档案 |
| `external` | 必填 | 忽略 | 必填 | 使用 `external_login` 缓存的账号；未提供 `authlib_jar` 时会下载 authlib-injector |

`jvm_args` 按空白字符切分，不支持引号。需要包含空格的参数请避免使用这种方式，Java 路径应通过 `runtime` 传入。

结果：

```json
{"type":"result","id":6,"ok":true,"data":{"launched":true,"version":"1.21.1"}}
```

重要行为：

- 结果只表示游戏**进程已创建**，不代表游戏已经进入主菜单。
- 启动器不会等待游戏退出，也不会返回 PID。
- JSON 模式下，游戏的 stdin、stdout、stderr 都被重定向到 null，避免污染协议。游戏日志由游戏自身写入 `<游戏根目录>/.minecraft/logs/latest.log`，崩溃报告位于 `.minecraft/crash-reports/`。
- 游戏的工作目录被设置为 `.minecraft`，与原版启动器一致。
- 关闭 GUI 不一定会终止已经创建的游戏进程。
- 如果 Java 缺失或不符合版本要求，请求会在创建进程前失败。
- 缓存的微软凭据可用时会自动刷新；没有缓存时请求失败，界面应先执行 `login`。
- 外置启动要求事先用相同标识和 API 地址执行过 `external_login`。首次外置启动可能下载 authlib-injector，并产生 `authlib` 进度事件。
- 访问令牌等敏感参数会从启动器日志中脱敏。

### 5.7 `login`

启动微软设备码流程，无参数。

服务会先发送 `device_code` 事件，然后一直阻塞到用户完成授权、设备码过期或发生错误。

```json
{
  "type": "device_code",
  "id": 7,
  "user_code": "ABCD1234",
  "verification_uri": "https://microsoft.com/link",
  "expires_in": 900,
  "message": "To sign in, use a web browser to open ... and enter the code ..."
}
```

成功时：

```json
{"type":"result","id":7,"ok":true,"data":{"username":"Steve","uuid":"..."}}
```

- 向用户展示 `user_code` 和 `verification_uri`；`message` 可以直接展示。
- `expires_in` 的单位是秒。
- 凭据会被缓存，之后 `auth: "msa"` 的 `launch` 会使用并在需要时刷新。
- 只保留一份微软账号缓存。
- 该请求会一直占用服务直到结束。不要在 UI 中把其他操作悄悄排在它后面，应明确展示等待状态。

### 5.8 `external_login`

登录 Yggdrasil 兼容服务器（authlib-injector）。

参数：

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `identifier` | string | 是 | 邮箱或用户名 |
| `password` | string | JSON 模式下必填 | 不会从终端提示输入 |
| `api_url` | string | 是 | Yggdrasil API 地址；服务会解析其 ALI 位置 |
| `profile` | string | 否 | 要选择的角色 UUID 或精确角色名 |

结果：

```json
{
  "type": "result",
  "id": 8,
  "ok": true,
  "data": {
    "name": "Steve",
    "uuid": "00112233-4455-6677-8899-aabbccddeeff",
    "api_url": "https://example.invalid/api/yggdrasil",
    "server_name": "Example Server"
  }
}
```

`server_name` 可能为 `null`。账号会被缓存，供之后的 `launch` 使用。

角色选择：

- 如果服务器只返回一个角色，或已经返回选中角色，登录会直接完成，不会询问。
- 如果服务器没有选中角色且有多个可用角色，服务会发送：

  ```json
  {
    "type": "profiles",
    "id": 8,
    "profiles": [
      {"id": "00112233...","name":"Steve"},
      {"id": "ffeeddcc...","name":"Alex"}
    ]
  }
  ```

  随后请求失败：

  ```json
  {"type":"error","id":8,"ok":false,"error":"profile selection required"}
  ```

- 带上 `profile`（角色 ID 或名称）重新发送同一个 `external_login` 请求。密码必须再次提供；在选择成功之前不会缓存任何账号。
- 未知角色会返回 `Unknown profile: <值>`。
- 返回的 `api_url` 是解析后的 API 地址。之后 `launch` 使用它或原始地址都可以，双方都会再次解析。

### 5.9 `accounts.list`

列出缓存的账号，不会返回访问令牌。

结果：

```json
{
  "type": "result",
  "id": 9,
  "ok": true,
  "data": {
    "accounts": [
      {
        "identifier": "player@example.com",
        "name": "Steve",
        "uuid": "00112233-4455-6677-8899-aabbccddeeff",
        "api_url": "https://example.invalid/api/yggdrasil",
        "server_name": "Example Server"
      }
    ],
    "msa": {
      "username": "Steve",
      "uuid": "6f617b0c-1d60-4de9-84fe-19edf26fe3ce"
    }
  }
}
```

- `accounts` 是外置/Yggdrasil 账号。
- 没有微软账号缓存或缓存无法读取时，`msa` 为 `null`。
- 该响应永远不会包含访问令牌。

### 5.10 `installed.list`

返回游戏 `versions` 目录下存在可读 `<id>/<id>.json` 的版本 ID，包括原版安装和加载器配置。

结果：

```json
{
  "type": "result",
  "id": 10,
  "ok": true,
  "data": {
    "versions": [
      {"id": "1.21.1"},
      {"id": "fabric-loader-0.19.5-1.21.1"}
    ]
  }
}
```

- 列表按 ID 排序。
- 它读取的是服务启动时选定的游戏目录（`--instance`、`--game-dir` 或默认目录）。
- 已安装版本选择器应使用该命令；`versions.list` 只包含远程版本清单。

---

## 6. 事件

所有事件都是带有 `type` 字段的对象；除 `ready` 外，都带有当前活动请求的 `id`。客户端必须忽略未知事件类型，以便协议扩展时不会破坏兼容性。

### 6.1 `log`

```json
{"type":"log","id":5,"level":"info","message":"Downloading metadata..."}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `level` | string | `info` 或 `warn` |
| `message` | string | 人类可读消息 |

日志消息适合展示在日志面板中，不是用于控制流的稳定 API。

### 6.2 `progress`

```json
{"type":"progress","id":5,"phase":"downloads","current":42,"total":317,"message":"Downloading files"}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `phase` | string | 操作阶段 |
| `current` | number | 已完成单位数 |
| `total` | number | 总单位数；未知时为 `0` |
| `message` | string | 人类可读描述 |

已知阶段：

| 阶段 | 出现时机 | 单位 |
|------|----------|------|
| `metadata` | 下载资源索引之前 | 始终为 `0/0`，仅作提示 |
| `downloads` | 客户端、依赖库和资源下载 | 已完成文件数 / 总下载任务数 |
| `extract` | 原生库解压 | 已完成原生库数 / 总解压任务数 |
| `loader` | 加载器安装开始与结束 | 开始时 `0/0`，结束时 `1/1` |
| `loader_install` | Forge/NeoForge 安装器执行 | 开始时 `0/1`，结束时 `1/1` |
| `authlib` | 外置启动时下载 authlib-injector | 通常为已下载字节数 / 总字节数，最后会有一个 `1/1` |

进度语义是尽力而为的：

- 所有文件都命中缓存时，某些阶段可能被跳过。
- 阶段可能重复（例如 `loader` 之后又出现嵌套安装进度）。
- `total` 可能为 `0`，也可能与前一阶段的单位不同。
- 并发下载的更新可能略微乱序到达。
- 请求可能在没有任何进度事件的情况下完成。

`downloads` 和 `extract` 适合用于确定性进度条。其他阶段可以展示阶段名和消息，或使用不确定进度指示器。

### 6.3 `device_code`

在 `login` 期间发送：

```json
{
  "type": "device_code",
  "id": 7,
  "user_code": "ABCD1234",
  "verification_uri": "https://microsoft.com/link",
  "expires_in": 900,
  "message": "To sign in, use a web browser to open ..."
}
```

应显著展示设备码和 URI。不要记录用户令牌；该事件本身不包含令牌。

### 6.4 `profiles`

在 `external_login` 需要选择角色时发送。参见 [`external_login`](#58-external_login)。每个条目包含 `id`、`name` 和可选的 `properties`。

### 6.5 `ready`

启动事件，见[进程模型](#启动握手)。其 `id` 为 `null`。

---

## 7. 编写客户端

### 7.1 桥接层职责

把所有子进程处理集中到桥接层：

1. 解析开发环境和打包后的启动器可执行文件路径。
2. 启动 `mclc json`，使用管道连接 stdin/stdout，设置工作目录，并单独捕获 stderr。
3. 持续读取 stdout 并按行切分。**即使 UI 很忙也不要停止读取 stdout**：管道写满后启动器会阻塞。
4. 把每一行解析为 JSON。
5. 把 `result` 和 `error` 交给对应 `id` 的 Promise。
6. 把所有其他消息作为事件转发给 UI。
7. 串行化请求，保证同一时刻只有一个在途请求。
8. 子进程退出时，让所有挂起请求失败并通知 UI。
9. 应用退出时发送 `shutdown`，短暂等待后终止子进程。

### 7.2 与语言无关的伪代码

```text
state:
    nextId = 1
    pending = map<id, {resolve, reject}>
    queue = 已完成的 promise
    child = null

function start():
    child = spawn(launcherPath, ["json"], {
        cwd: gameRoot,
        stdin: pipe, stdout: pipe, stderr: pipe,
        windowsHide: true
    })
    startLineReader(child.stdout)
    startStderrCollector(child.stderr)
    await event "ready" with timeout

function request(command, params):
    return queue = queue.then(() => {
        id = nextId++
        return new Promise((resolve, reject) => {
            pending[id] = {resolve, reject}
            writeLine(child.stdin, {id, command, params})
        })
    })

function onStdoutLine(line):
    msg = json.parse(line)
    if msg.type == "result":
        pending.remove(msg.id).resolve(msg.data)
    else if msg.type == "error":
        pending.remove(msg.id).reject(new Error(msg.error))
    else:
        ui.emit("launcher-event", msg)

function writeLine(stdin, object):
    stdin.write(JSON.stringify(object) + "\n")
    stdin.flush()

function onChildExit():
    for each request in pending:
        request.reject(new Error("launcher process exited"))
    clear pending
    ui.emit("launcher-exit")
```

注意事项：

- 非法输入行会产生 `id: null` 的不可关联错误。记录它即可，因为没有挂起请求可以拒绝。
- 如果写入 stdin 失败，应移除挂起项并立即拒绝。
- 服务没有请求超时。只在 UI 确实需要时设置超时；不要为 `install`、`login` 或 `launch` 设置过短的超时。
- `login` 可能持续数分钟。在此期间，请求队列会挡住后续请求，直到它结束。

### 7.3 推荐的 UI 状态

一个小型状态机可以让界面行为更可预测：

| 状态 | 含义 | 允许的操作 |
|------|------|------------|
| `starting` | 子进程已启动，等待 `ready` | 取消/重试启动 |
| `ready` | 空闲 | 任意命令 |
| `busy` | 有请求在执行 | 展示进度和日志；可选地通过终止并重启来取消 |
| `awaiting_login` | 登录进行中，已展示设备码 | 展示设备码、打开 URI、等待 |
| `awaiting_profile` | 收到 `profiles` | 选择角色并重试 `external_login` |
| `exited` | 子进程结束 | 重启并展示最后的错误 |

处于 `busy` 时，应禁用会产生冲突操作的控制项。虽然服务会串行处理额外请求，但界面应把排队状态明确展示出来。

### 7.4 框架对照

| 技术栈 | 接入要点 |
|--------|----------|
| Electron | 使用 `child_process.spawn` 和管道 stdio，用 `readline` 读取 stdout；`ipcMain.handle` 处理请求，`webContents.send` 发送事件；通过 preload + `contextBridge` 暴露窄接口。保持 `contextIsolation` 开启、`nodeIntegration` 关闭。二进制放在 `extraResources` 中，不要放进 asar。 |
| Tauri 2 | 使用 `tauri-plugin-shell` 的 sidecar 和 `externalBin`。sidecar stdout 以字节块到达，必须缓冲到换行再解析。在 Rust 桥接层向 WebView 发送事件。 |
| .NET / WPF / WinForms | 使用 `Process`，设置 `RedirectStandardInput/Output/Error`、`CreateNoWindow = true`，异步按行读取，并把事件切回 UI 线程。 |
| Python / PySide / Tk | 使用文本模式的 `subprocess.Popen`，专用读取线程，以及线程安全队列把事件送入 UI 循环。 |
| Rust 原生（egui、iced） | 使用 `tokio::process::Command` 或 `std::process::Command`，独立读取任务，通过事件通道送入 UI。 |

无论使用哪种框架，都要把子进程管理放在 UI 线程之外，并且不要在请求执行期间阻塞 UI。

### 7.5 WebSocket 传输（`mclc web`）

如果前端是浏览器页面而不是桌面进程，`mclc web` 提供了本地传输适配层。它会启动一个 HTTP 服务（默认绑定回环地址），在内部启动 `mclc json`，并在以下地址暴露相同的消息：

- 默认 WebSocket 地址：`ws://127.0.0.1:7420/ws`
- `GET /health` 查询启动器状态

浏览器发送相同的请求对象，接收相同的结果、错误和事件。变化只在传输层，命令语义不变。

运行差异：

- 请求 ID 会在桥接层内部重映射；返回给浏览器时，会恢复成该连接自己的 ID。
- 多个 WebSocket 客户端可以共用一个启动器。请求全局串行，结果只发给发起请求的连接；无法关联的事件会以 `id: null` 广播。
- 每个新连接都会收到当前的 `ready` 事件。事件不会重放；重连后应重新查询 `accounts.list`、`java.list`、`installed.list` 和 `versions.list`。请求在执行中断开后仍会继续，但结果只会发送给仍然打开的连接。
- 启动器子进程退出时，客户端会收到 Web 专用的 `launcher_exit` 事件（`id: null`），并且 `/health` 会返回 `launcher_alive: false`。发送 `shutdown` 只会停止启动器子进程，Web 服务仍可用，并且不会自动重启子进程。
- `mclc web` 不提供前端静态文件。前端需要单独部署；localhost 上的 Vite 开发服务器无需额外参数即可连接。直接用 `file://` 打开的页面会发送 `Origin: null`，仅在可信的本地开发场景下用 `--allow-origin null` 显式允许。
- 启动器子进程使用 `--instance` 或 `--game-dir` 选定的游戏目录；都不传时使用 `./.minecraft`。`--runtime` 和 `--download-source` 会转发给子进程。
- 安全：服务默认绑定回环地址，允许回环 Origin；`--allow-origin` 追加其他来源；`--token` 要求客户端通过 `?token=` 或 `Authorization: Bearer` 提供令牌，绑定非回环地址时必须设置令牌。回环绑定时会校验 Host。不要绑定 `0.0.0.0`。
- 公网 HTTPS 前端连接回环服务时，可能被浏览器的私有网络访问或混合内容策略拦截；仅设置 `--allow-origin` 并不能保证一定可用。

---

## 8. 生命周期与故障处理

### 启动

1. 创建或校验游戏目录，确保可写。
2. 以该目录为工作目录启动启动器。
3. 从第一个字节开始读取 stdout；`ready` 会在任何请求之前到达。
4. 校验 `protocol == 1` 并记录 `version`。
5. 如果启动超时或进程退出，展示 stderr 和退出码。

### 进程退出

- 用明确的错误拒绝所有挂起请求。
- 不要在安装中途自动重启且不告知用户。请求已经丢失，不过已经下载的文件可以在重试时复用。
- 如果 UI 提供自动重启，只在空闲时重启，并重新检查 `ready`。

### 关闭

1. 发送 `shutdown` 并等待结果。
2. 关闭 stdin。
3. 等待一小段宽限时间，让进程退出。
4. 如果进程仍然存活，终止它。
5. 回收子进程，避免留下僵尸进程。

如果关闭 GUI 时游戏仍在运行，应明确决定游戏是否继续。启动器创建游戏后不会等待或管理它，因此终止启动器不一定影响已经创建的游戏进程。

### 健康检查

启动后以及崩溃重启后，使用 `ping` 检查桥接是否正常。不要把 `ping` 成功当作网络可用或 Java 可用的证明。

---

## 9. 打包与部署

- 为所有要发布的操作系统和架构构建 `mclc`。
- 不要把启动器二进制放进 Electron 的 asar 等无法直接执行的归档格式；安装时解压为真实文件。
- macOS 和 Linux 上要保留可执行权限。如果 GUI 做了签名，启动器二进制也应签名/公证。
- Windows 上应以隐藏窗口方式启动（`windowsHide` / `CREATE_NO_WINDOW`），避免控制台子系统程序闪出黑框。
- 相对于应用安装目录解析二进制路径，而不是用户当前目录。开发布局和打包布局通常不同，要显式处理两者。
- 选择可写且持久的游戏根目录。如果应用安装目录可能是只读的，不要使用它。
- 使用 `ready.version` 校验启动器版本。GUI 和启动器应一起发布；新 GUI 配旧 sidecar（或反过来）是常见故障来源。
- 协议版本与应用版本相互独立。拒绝未知的 `protocol`。
- 账号数据存放在操作系统配置目录中，同一用户的所有游戏目录共享。卸载 GUI 时不应在用户未要求的情况下删除账号。
- 在 GUI 中增加单实例锁，或对游戏目录加锁，避免两个实例同时安装到同一个 `.minecraft`。
- 如果支持多个游戏根目录，每个根目录运行一个服务，并在 UI 中明确当前根目录。

---

## 10. 安全

- 协议是本地协议，没有认证。不要通过 TCP 套接字暴露 stdin/stdout，也不要让不受信任的内容发送任意命令。
- 面向 UI 的桥接层应设置命令白名单并校验参数类型。不要把通用的“执行命令”API 暴露给 Web 内容。
- JSON 模式下，密码只作为 `external_login` 的 JSON `params` 接受。不要把它们放进 argv、环境变量、日志或遥测。
- 不要记录完整请求对象；其中可能包含密码或访问令牌。只记录命令名、ID 和脱敏后的错误消息。
- `accounts.list` 有意不返回访问令牌。`launch` 日志会对敏感参数脱敏，但桥接层仍不应在生产日志中倾倒原始协议流量。
- 把 `auth_cache.json` 和 `accounts.json` 当作凭据。启动器在 Unix 下会限制权限，但备份和崩溃报告仍可能泄漏它们。
- 用户提供的 `api_url` 会让启动器访问该服务器。只接受用户确实信任的地址，并在可用时展示解析后的服务器名称。
- 在信息可用时，下载会按元数据校验。不要关闭校验，也不要用未校验的文件替换已下载文件。
- Electron 中保持 `contextIsolation` 开启、`nodeIntegration` 关闭，并只暴露窄接口。Tauri 中使用 capabilities 限制插件访问。

---

## 11. 已知限制

- **请求串行：** 通过一个服务无法并发安装或启动。
- **没有取消：** 没有取消命令。终止服务是中止在途操作的唯一方式，且可能留下部分文件。
- **启动即返回：** 不返回 PID、退出码或退出事件，也没有实时游戏 stdout。游戏日志请读取 `.minecraft/logs/latest.log`。
- **JSON 模式下游戏 stdio 被抑制：** 游戏 stdout/stderr 被重定向到 null 以保护协议。
- **不会自动安装 Java：** Java 必须已存在并可检测，或通过 `runtime` 提供。
- **Forge/NeoForge 安装器输出被缓冲：** 客户端只能看到开始/结束进度，看不到安装器控制台行。
- **只有一份微软账号缓存：** 外置账号支持多账号，但微软账号只有一份缓存。
- **账号存储按操作系统用户全局共享：** 不按游戏目录隔离。
- **工作目录在启动时固定：** 更换实例或游戏根目录需要用不同的 `--instance`/`--game-dir`（或工作目录）重启服务。
- **没有事件重放：** 晚连接的客户端会错过之前的事件，事件只实时发送。
- **协议 1：** 任何不兼容变更都应视为新的协议版本并进行协商。

---

## 12. 故障排查

| 现象 | 可能原因 | 处理方式 |
|------|----------|----------|
| 收不到 `ready` | 二进制缺失、不可执行、架构不匹配或启动失败 | 检查子进程路径、权限、退出状态和 stderr |
| 有 `ready` 但请求一直无响应 | 缺少 `\n`、未 flush、没有持续读取 stdout，或已有请求在执行 | 每个请求写一行并 flush；持续读取 stdout；串行化请求 |
| 错误中的 `id` 为 `null` | 请求行不是合法 JSON | 记录原始行并修复客户端序列化 |
| `Invalid install parameters: missing field \`version\`` | `params` 缺失或不是对象 | 发送 `{"version":"..."}` |
| `Unknown command: ...` | 命令拼写错误或不受支持 | 使用文档中定义的命令 |
| `Invalid request: ...` | JSON 语法错误 | 确保每行恰好一个合法对象 |
| `profile selection required` | 外置账号有多个角色且未传 `profile` | 读取前面的 `profiles` 事件，选择角色后带 `profile` 重试 |
| `No cached credentials found ...` | 在 `external_login` 之前尝试外置 `launch` | 先登录，再用相同标识和 API 地址启动 |
| `Authentication required but not found` | `auth: "msa"` 启动但没有缓存账号 | 先执行 `login`，或传入 `access_token` |
| 游戏窗口一直不出现 | Java 缺失/不匹配、版本未安装，或游戏在显示窗口前崩溃 | 检查启动错误、Java 选择和 `.minecraft/logs/latest.log` |
| 进度条往回跳 | 并发下载的进度事件可能乱序到达 | 把数值当作估算值，或在界面上限制为不后退 |
| Windows 上闪出控制台窗口 | 以控制台子系统启动 sidecar 且未隐藏窗口 | 使用 `windowsHide` / `CREATE_NO_WINDOW` |
| GUI 退出后启动器仍在运行 | 应用退出时没有执行关闭/终止 | 发送 `shutdown`、关闭 stdin，宽限期后终止 |
| 新游戏目录中看不到账号 | 账号数据存放在操作系统配置目录，而不是 `.minecraft` | 使用同一个用户/配置目录；不要误删它 |
| 两个安装互相覆盖 | 多个启动器实例共享同一游戏目录 | 增加单实例锁，或每个目录只运行一个服务 |

---

## 13. 附录：完整会话示例

### 13.1 离线安装与启动

```text
S: {"type":"ready","protocol":1,"version":"0.1.1"}
C: {"id":1,"command":"ping"}
S: {"type":"result","id":1,"ok":true,"data":{"pong":true}}
C: {"id":2,"command":"install","params":{"version":"1.21.1"}}
S: {"type":"log","id":2,"level":"info","message":"Installing Minecraft 1.21.1..."}
S: {"type":"progress","id":2,"phase":"metadata","current":0,"total":0,"message":"Downloading metadata"}
S: {"type":"progress","id":2,"phase":"downloads","current":42,"total":317,"message":"Downloading files"}
S: {"type":"result","id":2,"ok":true,"data":{"version_id":"1.21.1"}}
C: {"id":3,"command":"launch","params":{"version":"1.21.1","username":"Steve"}}
S: {"type":"log","id":3,"level":"info","message":"Launching Minecraft version: 1.21.1 for user: Steve"}
S: {"type":"result","id":3,"ok":true,"data":{"launched":true,"version":"1.21.1"}}
C: {"id":4,"command":"shutdown"}
S: {"type":"result","id":4,"ok":true,"data":{"shutdown":true}}
```

`C:` 是客户端写入 stdin 的行，`S:` 是从 stdout 读取的服务端行。事件行仅作示意，实际阶段和计数会变化。

### 13.2 微软登录后启动

```text
C: {"id":1,"command":"login"}
S: {"type":"device_code","id":1,"user_code":"ABCD1234","verification_uri":"https://microsoft.com/link","expires_in":900,"message":"To sign in, ..."}
   ... 用户完成授权 ...
S: {"type":"result","id":1,"ok":true,"data":{"username":"Steve","uuid":"..."}}
C: {"id":2,"command":"launch","params":{"version":"1.21.1","auth":"msa"}}
S: {"type":"result","id":2,"ok":true,"data":{"launched":true,"version":"1.21.1"}}
```

### 13.3 外置登录并选择角色

```text
C: {"id":1,"command":"external_login","params":{"identifier":"player@example.com","password":"secret","api_url":"https://example.invalid"}}
S: {"type":"profiles","id":1,"profiles":[{"id":"00112233...","name":"Steve"},{"id":"ffeeddcc...","name":"Alex"}]}
S: {"type":"error","id":1,"ok":false,"error":"profile selection required"}
C: {"id":2,"command":"external_login","params":{"identifier":"player@example.com","password":"secret","api_url":"https://example.invalid","profile":"Alex"}}
S: {"type":"result","id":2,"ok":true,"data":{"name":"Alex","uuid":"ffeeddcc-...","api_url":"https://example.invalid/api/yggdrasil","server_name":"Example Server"}}
C: {"id":3,"command":"launch","params":{"version":"1.21.1","auth":"external","api_url":"https://example.invalid","username":"player@example.com"}}
S: {"type":"result","id":3,"ok":true,"data":{"launched":true,"version":"1.21.1"}}
```

### 13.4 错误示例

```text
C: {"id":1,"command":"launch","params":{"version":"not-installed"}}
S: {"type":"log","id":1,"level":"info","message":"Launching Minecraft version: not-installed for user: Player"}
S: {"type":"error","id":1,"ok":false,"error":"Version not-installed not found"}
```
