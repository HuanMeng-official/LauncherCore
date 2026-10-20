# LauncherCore JSON Integration Guide

**Protocol version:** 1  
**Audience:** GUI launchers, installer frontends, automation, and other clients that embed LauncherCore  
**Applies to:** the `mclc json` command and the `mclc web` WebSocket transport

This guide describes the persistent NDJSON protocol exposed by `mclc json` and how to build a
client around it. It is intentionally framework-agnostic: any runtime that can spawn a child
process and read/write pipes can use it. For command-line usage, see [README.md](README.md).
The implementation lives in [src/json_server.rs](src/json_server.rs) and
[src/output.rs](src/output.rs).

Chinese version: [README/INTEGRATION_zh_CN.md](README/INTEGRATION_zh_CN.md).

---

## Contents

1. [Design principles](#1-design-principles)
2. [Process model](#2-process-model)
3. [Message framing](#3-message-framing)
4. [Requests, results, and errors](#4-requests-results-and-errors)
5. [Command reference](#5-command-reference)
6. [Events](#6-events)
7. [Building a client](#7-building-a-client)
8. [Lifecycle and failure handling](#8-lifecycle-and-failure-handling)
9. [Packaging and deployment](#9-packaging-and-deployment)
10. [Security](#10-security)
11. [Known limitations](#11-known-limitations)
12. [Troubleshooting](#12-troubleshooting)
13. [Appendix: example sessions](#13-appendix-example-sessions)

---

## 1. Design principles

- `mclc json` is a **long-running subprocess**, not an HTTP server. It has no ports and no
  network listener.
- **stdin carries requests; stdout carries protocol messages.** Both are UTF-8 NDJSON: one
  complete JSON object per line.
- Human-readable output from the launcher is converted into `log` events. It never appears as
  unstructured text on stdout. stderr is reserved for diagnostics such as panics.
- Requests are processed **sequentially**, in arrival order. There is exactly one in-flight
  request at a time.
- Work in progress is reported through **events** (`log`, `progress`, `device_code`,
  `profiles`) while the matching `result` or `error` terminates the request.
- Protocol compatibility is negotiated with the `ready` event (`protocol` and `version`).

A typical GUI has three layers:

```text
┌──────────────────────────────────────┐
│ UI layer (renderer, view models)     │
│   request(command, params)           │
│   onEvent(event)                     │
└──────────────────┬───────────────────┘
                   │ IPC or direct calls
┌──────────────────▼───────────────────┐
│ Bridge layer                         │
│   • owns the mclc child process      │
│   • writes requests, reads stdout    │
│   • correlates results by id         │
│   • serializes requests              │
│   • forwards events to the UI        │
└──────────────────┬───────────────────┘
                   │ stdin/stdout, NDJSON
┌──────────────────▼───────────────────┐
│ mclc json                            │
│   • downloads and installs versions  │
│   • assembles launch arguments       │
│   • authenticates accounts           │
└──────────────────┬───────────────────┘
                   │
                   ▼
              <cwd>/.minecraft
```

Do not shell out per operation and parse human-readable CLI output. Start `mclc json` once and
keep it running.

---

## 2. Process model

### Starting the server

```bash
mclc json
mclc json --download-source bmclapi
mclc json --runtime "C:\Program Files\Java\jdk-21\bin\java.exe"
```

Global options provided at startup become session defaults:

| Option | Effect in JSON mode |
|--------|---------------------|
| `--download-source <auto\|official\|bmclapi>` | Default source for `versions.list` and for `install` requests that omit `download_source` |
| `-r, --runtime <PATH>` | Default Java executable for `install` and `launch` requests that omit `runtime` |

### Working directory

The launcher stores game files in **`<current working directory>/.minecraft`**. The directory is
created at startup and cannot be changed by a protocol request. To use a different game
directory, restart the server with a different working directory.

Account data is separate from the game directory. It is stored under the OS user config
directory, in `mclc/` (for example `%APPDATA%\mclc` on Windows, `~/.config/mclc` on Linux, and
`~/Library/Application Support/mclc` on macOS). All game directories used by the same OS user
share the same cached Microsoft account and external accounts.

### Startup handshake

The first protocol message is always:

```json
{"type":"ready","protocol":1,"version":"0.1.1"}
```

- `protocol` is the protocol version. This document describes version `1`. If the value is not
  supported, refuse to continue rather than guessing.
- `version` is the launcher build version. Display it for diagnostics and keep it aligned with
  the GUI build.
- `id` is `null` on `ready` because no request is active.

If `ready` does not arrive within a reasonable startup timeout, check the child's exit status and
stderr; it may have failed before the protocol started.

### Shutdown

- Send `{"command":"shutdown"}`; the server returns a result and exits.
- Closing stdin also ends the server after the current request completes.
- Always reap the child process and terminate it if graceful shutdown times out. Otherwise a
  GUI restart can leave an orphaned launcher holding the game directory.

### One server per client

The protocol has a single request stream and a single event stream. Run one server per
application instance (and per game root). If multiple windows need launcher operations,
multiplex them through one bridge process in the GUI rather than opening multiple servers against
the same game directory.

---

## 3. Message framing

- Encoding is **UTF-8**.
- Every line is exactly one JSON object. JSON strings escape embedded newlines, so a line always
  ends at a framing newline.
- Output lines end with `\n`. Clients should split on `\n` and tolerate a trailing `\r`.
- Input lines are trimmed; blank lines are ignored.
- stdout is protocol-only. Do not parse stderr as protocol; expose it as diagnostics.
- stdout writes are flushed after every message by the server.
- Do not pretty-print requests. Send each object on a single line.
- JSON object key order is not significant. Do not depend on the field order shown in examples.
- The stdout pipe can deliver partial lines or several lines in one chunk. Buffer until a
  newline before parsing.

---

## 4. Requests, results, and errors

### Requests

```json
{"id":1,"command":"ping"}
{"id":2,"command":"install","params":{"version":"1.21.1","loader":"fabric"}}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `id` | any JSON value | No | Correlation id chosen by the client. It is echoed on the matching result or error and on events emitted during the request. Use a unique number or string. If omitted, the server uses `null`. |
| `command` | string | Yes | Command name. See [Command reference](#5-command-reference). |
| `params` | object | Depends | Command parameters. Commands with no parameters ignore this field. Commands with parameters require an object. |

Unknown fields are ignored by the server. Clients should still send only documented fields.

If a line is not valid JSON, the server replies with an error whose `id` is `null`, because no
request id could be recovered:

```json
{"type":"error","id":null,"ok":false,"error":"Invalid request: expected ident at line 1 column 2"}
```

If the JSON is valid but the command or parameters are invalid, the error carries the request id:

```json
{"type":"error","id":2,"ok":false,"error":"Invalid install parameters: missing field `version`"}
```

### Results

A successful request ends with exactly one result:

```json
{"type":"result","id":2,"ok":true,"data":{"version_id":"1.21.1"}}
```

- `data` is command-specific. It may be an object, array, string, number, boolean, or `null`.
- `ok` is always `true` on a result.

### Errors

A failed request ends with exactly one error:

```json
{"type":"error","id":2,"ok":false,"error":"Version 1.99 not found"}
```

- `error` is a human-readable message. It can include a cause chain such as
  `Invalid install parameters: missing field \`version\``.
- Do not parse `error` for control flow. Use it for display and diagnostics. For robust clients,
  map common failures to UI states by operation and context, not by exact string matching.
- A request that produces an error does not produce a result.

### Ordering

- Requests are read and processed in order; a response for request N is sent before request N+1
  is started.
- `log`, `progress`, `device_code`, and `profiles` events may appear between a request and its
  result. They always refer to the request that is currently executing.
- Events emitted outside a request (notably `ready`) have `id: null`.
- Download progress from concurrent workers may arrive slightly out of order. Treat
  `current`/`total` as an estimate and avoid making monotonicity assumptions.

---

## 5. Command reference

Summary:

| Command | Parameters | Result data | Notes |
|---------|------------|-------------|-------|
| `ping` | none | `{"pong":true}` | Health check |
| `shutdown` | none | `{"shutdown":true}` | Server exits after replying |
| `java.list` | none | `{"runtimes":[...]}` | Scans the machine |
| `versions.list` | none | `{"versions":[...]}` | Network request |
| `installed.list` | none | `{"versions":[{"id":"..."}]}` | Reads the local versions directory |
| `install` | object | `{"version_id":"..."}` | Downloads may take a long time |
| `launch` | object | `{"launched":true,"version":"..."}` | Returns after spawning the game |
| `login` | none | `{"username":"...","uuid":"..."}` | Blocks during the device-code flow |
| `external_login` | object | account details | May require profile selection |
| `accounts.list` | none | `{"accounts":[...],"msa":...}` | Does not expose tokens |

### 5.1 `ping`

Request:

```json
{"id":1,"command":"ping"}
```

Result:

```json
{"type":"result","id":1,"ok":true,"data":{"pong":true}}
```

Use this to verify that the child process is alive and the protocol is responsive.

### 5.2 `shutdown`

Request:

```json
{"id":2,"command":"shutdown"}
```

Result:

```json
{"type":"result","id":2,"ok":true,"data":{"shutdown":true}}
```

After writing the result, the server exits. Do not send further requests on this connection.

### 5.3 `java.list`

Scans `JAVA_HOME`, `PATH`, and common vendor installation directories on every call. No Java
runtime is downloaded automatically.

Result:

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

Fields:

| Field | Type | Description |
|-------|------|-------------|
| `path` | string | Java executable path |
| `major` | number or `null` | Detected Java major version |
| `version` | string or `null` | Full version string |
| `arch` | string or `null` | Detected architecture |

The order of `runtimes` is not guaranteed. Sort or filter in the UI as needed.

### 5.4 `versions.list`

Fetches the Mojang version manifest using the current download source. This is a network request
and can be slow or fail.

Result:

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

`type` is the manifest's version type, such as `release`, `snapshot`, or `old_beta`.

### 5.5 `install`

Installs a vanilla version, or a vanilla parent plus a mod loader. Installation is idempotent:
already-downloaded files are reused.

Parameters:

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `version` | string | Yes | Minecraft version, for example `"1.21.1"` |
| `loader` | string | No | `fabric`, `quilt`, `forge`, or `neoforge`; omit for vanilla |
| `loader_version` | string | No | Pin a loader version; defaults to the stable/recommended release |
| `download_source` | string | No | `auto`, `official`, or `bmclapi`; overrides the startup default for this request |
| `runtime` | string | No | Java executable for loader installation, especially Forge/NeoForge; defaults to the startup `--runtime` |

Example:

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

Result:

```json
{"type":"result","id":5,"ok":true,"data":{"version_id":"fabric-loader-0.19.5-1.21.1"}}
```

Always launch the returned `version_id`. For a loader installation it is not the same as the
vanilla `version` you requested.

Behavior and events:

- The vanilla parent is installed first.
- Fabric and Quilt profiles are built from their metadata APIs.
- Forge and NeoForge use their official installers, run headlessly. Their console output is
  captured and is not streamed to the client. Expect only `loader_install` start/finish events
  while the installer runs.
- Downloads may be parallel internally, but the request itself is single and reports progress
  through `progress` events.
- A failed installation returns an error. Already-downloaded files may be reused by a later retry.
- `download_source` applies only to this request. The session default is restored afterwards.

### 5.6 `launch`

Launches an **already installed** version.

Parameters:

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `version` | string | Yes | Installed version ID, such as `"1.21.1"` or the `version_id` returned by `install` |
| `auth` | string | No | `offline` (default), `msa`, or `external` |
| `username` | string | Depends | Offline player name (defaults to `Player`) or the external account identifier |
| `access_token` | string | No | Existing Minecraft access token, used with `auth: "msa"` |
| `jvm_args` | string | No | Extra JVM arguments, split on whitespace |
| `api_url` | string | External only | Yggdrasil API URL |
| `authlib_jar` | string | No | Path to a local authlib-injector.jar; external auth only |
| `runtime` | string | No | Java executable; defaults to the startup `--runtime` and automatic selection |

Authentication matrix:

| `auth` | `username` | `access_token` | `api_url` | Behavior |
|--------|------------|----------------|-----------|----------|
| `offline` | Optional; default `Player` | Optional session token; default `0` | Ignored | Uses the standard offline UUID for the username |
| `msa` | Ignored | Optional | Ignored | Without a token, uses the cached Microsoft account and refreshes it when needed. With a token, fetches the Minecraft profile for that token. |
| `external` | Required | Ignored | Required | Uses a cached account created by `external_login`. Downloads authlib-injector unless `authlib_jar` is supplied. |

`jvm_args` is split on whitespace. There is no quote handling; avoid arguments that require
embedded spaces or supply a Java path via `runtime` instead.

Result:

```json
{"type":"result","id":6,"ok":true,"data":{"launched":true,"version":"1.21.1"}}
```

Important behavior:

- The result means the game **process was created**, not that the game reached the main menu.
- The launcher does not wait for the game to exit and does not return a PID.
- In JSON mode, the game's stdin, stdout, and stderr are redirected to null so they cannot
  corrupt the protocol. The game writes its own log to `<game root>/.minecraft/logs/latest.log`
  and crash reports under `.minecraft/crash-reports/`.
- The game runs with its working directory set to `.minecraft`, matching the official launcher.
- Closing the GUI does not necessarily stop a game that has already been spawned.
- If Java is missing or unsuitable for the version, the request fails before spawning.
- MSA cached credentials are refreshed automatically when possible. If no cache exists, the
  request fails and the UI should run `login` first.
- External launch requires a prior `external_login` for the same identifier and API URL. The
  first external launch may download authlib-injector and emit `authlib` progress events.
- Sensitive arguments such as access tokens are redacted from launcher logs.

### 5.7 `login`

Starts the Microsoft device-code flow. Takes no parameters.

The server first emits a `device_code` event, then blocks until the user completes authorization,
the code expires, or an error occurs.

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

On success:

```json
{"type":"result","id":7,"ok":true,"data":{"username":"Steve","uuid":"..."}}
```

- Show `user_code` and `verification_uri` to the user. `message` can be displayed verbatim.
- `expires_in` is in seconds.
- Credentials are cached, and later `launch` requests with `auth: "msa"` use and refresh them.
- Only one Microsoft account cache is kept.
- The request occupies the server until it completes. Do not queue unrelated operations behind it
  without making the wait visible in the UI.

### 5.8 `external_login`

Logs in to a Yggdrasil-compatible server (authlib-injector).

Parameters:

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `identifier` | string | Yes | Email or username |
| `password` | string | Yes in JSON mode | Never prompted from a terminal |
| `api_url` | string | Yes | Yggdrasil API URL; the server resolves its ALI location |
| `profile` | string | No | Profile UUID or exact profile name to select |

Result:

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

`server_name` may be `null`. The account is cached for later `launch` requests.

Profile selection:

- If the server returns exactly one profile, or a selected profile, login completes without
  asking.
- If the server returns no selected profile and multiple available profiles, the server emits:

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

  The request then fails with:

  ```json
  {"type":"error","id":8,"ok":false,"error":"profile selection required"}
  ```

- Retry the same `external_login` request with `profile` set to the chosen id or name. The
  password must be supplied again; nothing is cached until selection succeeds.
- An unknown profile returns `Unknown profile: <value>`.
- The returned `api_url` is the resolved API URL. You may keep using either it or the original
  URL for `launch`; both are resolved again.

### 5.9 `accounts.list`

Lists cached accounts without exposing access tokens.

Result:

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

- `accounts` contains external/Yggdrasil accounts.
- `msa` is `null` when no Microsoft account cache exists or the cache cannot be read.
- Access tokens are never included in this response.

### 5.10 `installed.list`

Returns version ids that have a readable `<id>/<id>.json` under the game's `versions` directory.
This includes vanilla installations and loader profiles.

Result:

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

- The list is sorted by id.
- It reads the working directory fixed when the server started.
- Use this for installed-version pickers; `versions.list` only contains remote manifest entries.

---

## 6. Events

All events are objects with a `type` field and, except for `ready`, the id of the active request.
Clients must ignore unknown event types so that protocol extensions do not break them.

### 6.1 `log`

```json
{"type":"log","id":5,"level":"info","message":"Downloading metadata..."}
```

| Field | Type | Description |
|-------|------|-------------|
| `level` | string | `info` or `warn` |
| `message` | string | Human-readable message |

Log messages are intended for a log pane. They are not a stable API for control flow.

### 6.2 `progress`

```json
{"type":"progress","id":5,"phase":"downloads","current":42,"total":317,"message":"Downloading files"}
```

| Field | Type | Description |
|-------|------|-------------|
| `phase` | string | Operation phase |
| `current` | number | Completed units |
| `total` | number | Total units, or `0` when unknown |
| `message` | string | Human-readable description |

Known phases:

| Phase | When | Units |
|-------|------|-------|
| `metadata` | Before downloading an asset index | Always `0/0`; informational |
| `downloads` | Client, library, and asset downloads | Completed files / total download tasks |
| `extract` | Native library extraction | Completed natives / total extraction tasks |
| `loader` | Start and finish of loader installation | `0/0` at start, then `1/1` |
| `loader_install` | Forge/NeoForge installer execution | `0/1` at start, then `1/1` |
| `authlib` | authlib-injector download during external launch | Usually bytes downloaded / total bytes, then a final `1/1` |

Progress semantics are best-effort:

- Phases may be skipped when everything is cached.
- Phases can repeat (for example, `loader` followed by nested install progress).
- `total` may be `0` or may not be in the same unit as a previous phase.
- Concurrent downloads can deliver updates slightly out of order.
- A request can complete without any progress event.

Use `downloads` and `extract` for determinate progress bars. For other phases, show the phase and
message, or use an indeterminate indicator.

### 6.3 `device_code`

Emitted during `login`:

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

Display the code and URI prominently. Do not log the user's tokens; this event contains no
tokens.

### 6.4 `profiles`

Emitted during `external_login` when a profile must be chosen. See
[external_login](#58-external_login). Each entry has `id`, `name`, and optional `properties`.

### 6.5 `ready`

Startup event described in [Process model](#startup-handshake). Its `id` is `null`.

---

## 7. Building a client

### 7.1 Bridge responsibilities

Keep all child-process handling in a bridge layer:

1. Resolve the launcher executable path for development and packaged builds.
2. Spawn `mclc json` with piped stdin/stdout, a chosen working directory, and stderr captured
   separately.
3. Continuously read stdout and split it into lines. **Never stop reading stdout**, even when the
   UI is busy: if the pipe fills, the launcher blocks.
4. Parse each line as JSON.
5. Route `result` and `error` messages to the promise registered for that `id`.
6. Forward all other messages to the UI as events.
7. Serialize requests and enforce one in-flight request at a time.
8. On child exit, reject all pending requests and notify the UI.
9. On application exit, send `shutdown`, wait briefly, then terminate the child.

### 7.2 Language-neutral pseudocode

```text
state:
    nextId = 1
    pending = map<id, {resolve, reject}>
    queue = resolved promise
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

Notes:

- A malformed input line produces an uncorrelated error with `id: null`. Log it; there is no
  pending request to reject.
- If writing to stdin fails, remove the pending entry and reject immediately.
- The server has no request timeout. Apply timeouts only if the UI needs them; do not use a
  short timeout for `install`, `login`, or `launch`.
- `login` can remain active for several minutes. The queue will hold later requests until it
  finishes.

### 7.3 UI state suggestions

A small state machine keeps the UI predictable:

| State | Meaning | Allowed actions |
|-------|---------|-----------------|
| `starting` | Child spawned; waiting for `ready` | Cancel/retry startup |
| `ready` | Idle | Any command |
| `busy` | One request in flight | Show progress/logs; optionally cancel by killing and restarting |
| `awaiting_login` | Login in progress; device code shown | Show code, open URI, wait |
| `awaiting_profile` | `profiles` received | Choose a profile and retry `external_login` |
| `exited` | Child ended | Restart; show last error |

While `busy`, disable controls that would enqueue conflicting work. Since the server is serial,
extra requests are queued, but the UI should make that visible.

### 7.4 Framework mapping

| Stack | Integration notes |
|-------|-------------------|
| Electron | `child_process.spawn` with piped stdio, `readline` for stdout, `ipcMain.handle` for requests, `webContents.send` for events, preload with `contextBridge`. Keep `contextIsolation` on and `nodeIntegration` off. Copy the binary via `extraResources`, not inside asar. |
| Tauri 2 | Use `tauri-plugin-shell` sidecar support and `externalBin`. Sidecar stdout arrives as byte chunks; buffer until newline before parsing. Emit events to the webview from the Rust bridge. |
| .NET / WPF / WinForms | `Process` with `RedirectStandardInput/Output/Error`, `CreateNoWindow = true`, asynchronous line reads. Marshal events onto the UI dispatcher. |
| Python / PySide / Tk | `subprocess.Popen` with text mode, a dedicated reader thread, and a thread-safe queue into the UI loop. |
| Rust native (egui, iced) | `tokio::process::Command` or `std::process::Command`, a reader task, and an event channel into the UI. |

In every case, keep the child process management outside the UI thread and never block the UI
while a request is running.

### 7.5 WebSocket transport (`mclc web`)

If the frontend is a browser page rather than a desktop process, `mclc web` provides a local
transport adapter. It starts an HTTP server (loopback by default), spawns `mclc json` internally,
and exposes the same messages at:

- `ws://127.0.0.1:7420/ws` by default
- `GET /health` for launcher status

The browser sends the same request objects and receives the same results, errors, and events.
Only the transport changes; command semantics are unchanged.

Operational differences:

- Request ids are remapped inside the bridge. The browser's own id is restored on its results,
  errors, and events.
- Multiple WebSocket clients can share one launcher. Requests are serialized globally; each
  outcome goes to the requesting connection. Uncorrelated events are broadcast with `id: null`.
- Every new connection receives the current `ready` event. Events are not replayed; after
  reconnecting, query `accounts.list`, `java.list`, `installed.list`, and `versions.list` again.
  An in-flight request continues after a disconnect, but its result is delivered only to an open
  connection.
- If the launcher subprocess exits, clients receive a web-specific `launcher_exit` event with
  `id: null`, and `/health` reports `launcher_alive: false`. Sending `shutdown` stops the launcher
  subprocess only; the web server remains available and does not restart it automatically.
- `mclc web` serves no frontend assets. Host the frontend separately; a Vite dev server on
  localhost works without extra flags. A page opened directly from `file://` sends
  `Origin: null`; allow it explicitly with `--allow-origin null` only for trusted local
  development.
- The launcher child inherits the working directory of `mclc web`, so start it from the game
  root. `--runtime` and `--download-source` are forwarded to the child.
- Security: the server binds to loopback by default, allows loopback Origins, accepts additional
  origins through `--allow-origin`, and accepts a token through `?token=` or an
  `Authorization: Bearer` header with `--token`. A token is required for non-loopback binds. Host
  is validated for loopback binds. Do not bind `0.0.0.0`.
- A public HTTPS frontend that connects to a loopback server may be blocked by browser
  private-network or mixed-content policy; `--allow-origin` alone does not guarantee it works.

---

## 8. Lifecycle and failure handling

### Startup

1. Create or validate the game directory; it must be writable.
2. Spawn the launcher with that directory as the working directory.
3. Read stdout from the first byte; `ready` arrives before any request.
4. Verify `protocol == 1` and record `version`.
5. If startup times out or the process exits, show stderr and the exit code.

### Process exit

- Reject every pending request with a clear error.
- Do not automatically restart in the middle of an install without telling the user. The
  request is lost, although already-downloaded files can be reused by retrying.
- If the UI offers automatic restart, restart only when idle and re-check `ready`.

### Shutdown

1. Send `shutdown` and wait for its result.
2. Close stdin.
3. Wait a short grace period for the process to exit.
4. Terminate the process if it is still alive.
5. Reap the child so it does not become a zombie.

If the GUI is closed while a game is running, decide explicitly whether the game should keep
running. The launcher does not wait on or manage the game after spawning it, so killing the
launcher does not necessarily affect an already-spawned game.

### Health checks

Use `ping` after startup and after restarting a crashed bridge. Do not interpret a successful
`ping` as proof that network access or Java is available.

---

## 9. Packaging and deployment

- Build `mclc` for every OS and architecture you ship.
- Keep the launcher binary outside archives such as Electron's asar or any format that cannot be
  executed. Extract it to a real file at install time.
- On macOS and Linux, preserve the executable bit. Sign/notarize the binary if the GUI is signed.
- On Windows, spawn with a hidden window (`windowsHide`/`CREATE_NO_WINDOW`) to avoid a console
  flash from the console-subsystem executable.
- Resolve the binary path relative to the application installation, not the user's current
  directory. Development and packaged layouts usually differ; handle both explicitly.
- Choose a writable, persistent game root. Do not use the application installation directory if
  it may be read-only.
- Verify the launcher version from `ready.version`. Ship the GUI and launcher together; a newer
  GUI with an older sidecar (or the reverse) is a common source of confusing failures.
- Protocol version is independent of app version. Reject unknown `protocol` values.
- Account data lives in the OS config directory and is shared by all game directories for that
  user. Uninstalling the GUI should not silently delete accounts unless the user asks.
- Add a single-instance lock in the GUI, or another lock around the game directory, so two
  instances cannot install into the same `.minecraft` concurrently.
- If you support multiple game roots, run one server per root and make the active root explicit in
  the UI.

---

## 10. Security

- The protocol is local and has no authentication. Never expose stdin/stdout over a network
  socket, and do not let untrusted content send arbitrary commands.
- The UI-facing bridge should whitelist commands and validate parameter types. Do not expose a
  generic "run command" API to web content.
- Passwords are accepted only as JSON `params` to `external_login` in JSON mode. Do not put them
  in argv, environment variables, log messages, or telemetry.
- Do not log complete request objects; they can contain passwords or access tokens. Log command
  names, ids, and sanitized error messages instead.
- `accounts.list` intentionally omits access tokens. `launch` logs redact sensitive arguments,
  but the bridge should still avoid dumping raw protocol traffic in production logs.
- Treat `auth_cache.json` and `accounts.json` as credentials. The launcher restricts their
  permissions on Unix, but backups and crash reports can still leak them.
- A user-supplied `api_url` causes the launcher to contact that server. Only accept URLs the user
  intends to trust, and display the resolved server name when available.
- Downloads are verified against metadata where available. Do not disable verification or
  replace downloaded files with unverified ones.
- In Electron, keep `contextIsolation` enabled, `nodeIntegration` disabled, and expose a narrow
  preload API. In Tauri, use capabilities to restrict plugin access.

---

## 11. Known limitations

- **Serial requests:** no concurrent installs or launches through one server.
- **No cancellation:** there is no cancel command. Killing the server is the only way to abort an
  in-flight operation, and it may leave partial files.
- **Launch is fire-and-forget:** no PID, no exit code, no exit event, and no live game stdout.
  Read `.minecraft/logs/latest.log` for game logs.
- **Game stdio is suppressed in JSON mode:** the game's stdout/stderr are redirected to null to
  protect the protocol.
- **No automatic Java installation:** Java must be present and detectable, or supplied via
  `runtime`.
- **Forge/NeoForge installer output is buffered:** the client sees only start/finish progress,
  not the installer's console lines.
- **One Microsoft account cache:** external accounts are multi-account, but MSA is a single cache.
- **Account storage is global per OS user:** not per game directory.
- **Working directory is fixed at startup:** changing game roots requires restarting the server.
- **No event replay:** a client that attaches late misses earlier events. Events are live only.
- **Protocol 1:** treat any incompatible change as a new protocol version and negotiate it.

---

## 12. Troubleshooting

| Symptom | Likely cause | What to do |
|---------|--------------|------------|
| No `ready` event | Binary missing, not executable, wrong architecture, or startup failure | Check the child path, permissions, exit status, and stderr |
| `ready` arrives but requests hang | Missing `\n`, output not flushed, stdout not being drained, or another request is running | Write one line and flush; keep reading stdout; serialize requests |
| Error with `id: null` | Malformed JSON request line | Log the raw line and fix the client serializer |
| `Invalid install parameters: missing field \`version\`` | `params` is missing or not an object | Send `{"version":"..."}` |
| `Unknown command: ...` | Command typo or unsupported command | Use a documented command |
| `Invalid request: ...` | JSON syntax error | Ensure one valid object per line |
| `profile selection required` | External account has multiple profiles and no `profile` was sent | Read the preceding `profiles` event, choose one, and retry with `profile` |
| `No cached credentials found ...` | External `launch` attempted before `external_login` | Log in first, then launch with the same identifier and API URL |
| `Authentication required but not found` | `launch` with `auth: "msa"` but no cached account | Run `login` first, or pass `access_token` |
| Game never appears | Java missing/mismatched, version not installed, or game crashed before showing a window | Check the launch error, Java selection, and `.minecraft/logs/latest.log` |
| Progress bar jumps backwards | Concurrent download progress events can arrive out of order | Treat values as estimates or clamp the displayed value |
| Console window flashes on Windows | Console-subsystem sidecar started without a hidden window | Use `windowsHide` / `CREATE_NO_WINDOW` |
| Orphaned launcher after GUI exit | No shutdown/kill on application exit | Send `shutdown`, close stdin, then terminate after a grace period |
| Accounts missing in a new game directory | Account data is stored in the OS config directory, not in `.minecraft` | Use the same OS user/config directory; do not delete it |
| Two installs overwrite each other | Multiple launcher instances share a game directory | Add a single-instance lock or one server per directory |

---

## 13. Appendix: example sessions

### 13.1 Offline install and launch

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

`C:` is a client line written to stdin. `S:` is a server line read from stdout. Event lines are
illustrative; actual phases and counts vary.

### 13.2 Microsoft login then launch

```text
C: {"id":1,"command":"login"}
S: {"type":"device_code","id":1,"user_code":"ABCD1234","verification_uri":"https://microsoft.com/link","expires_in":900,"message":"To sign in, ..."}
   ... user completes authorization ...
S: {"type":"result","id":1,"ok":true,"data":{"username":"Steve","uuid":"..."}}
C: {"id":2,"command":"launch","params":{"version":"1.21.1","auth":"msa"}}
S: {"type":"result","id":2,"ok":true,"data":{"launched":true,"version":"1.21.1"}}
```

### 13.3 External login with profile selection

```text
C: {"id":1,"command":"external_login","params":{"identifier":"player@example.com","password":"secret","api_url":"https://example.invalid"}}
S: {"type":"profiles","id":1,"profiles":[{"id":"00112233...","name":"Steve"},{"id":"ffeeddcc...","name":"Alex"}]}
S: {"type":"error","id":1,"ok":false,"error":"profile selection required"}
C: {"id":2,"command":"external_login","params":{"identifier":"player@example.com","password":"secret","api_url":"https://example.invalid","profile":"Alex"}}
S: {"type":"result","id":2,"ok":true,"data":{"name":"Alex","uuid":"ffeeddcc-...","api_url":"https://example.invalid/api/yggdrasil","server_name":"Example Server"}}
C: {"id":3,"command":"launch","params":{"version":"1.21.1","auth":"external","api_url":"https://example.invalid","username":"player@example.com"}}
S: {"type":"result","id":3,"ok":true,"data":{"launched":true,"version":"1.21.1"}}
```

### 13.4 Error example

```text
C: {"id":1,"command":"launch","params":{"version":"not-installed"}}
S: {"type":"log","id":1,"level":"info","message":"Launching Minecraft version: not-installed for user: Player"}
S: {"type":"error","id":1,"ok":false,"error":"Version not-installed not found"}
```
