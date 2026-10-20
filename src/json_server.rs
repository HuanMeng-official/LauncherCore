//! Persistent NDJSON communication server for GUI integration.
//!
//! Requests are read from stdin as one JSON object per line. Responses, logs,
//! progress, and interactive events are written to stdout as one JSON object
//! per line. Human-readable output from the rest of the launcher is routed
//! through `crate::output`, so it never corrupts the protocol stream.

use crate::cli::AuthType;
use crate::commands::{self, LaunchOptions};
use crate::download::DownloadSource;
use crate::launch_manager::LauncherManager;
use crate::loader::LoaderKind;
use crate::output;
use anyhow::Context;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, BufReader};

#[derive(Debug, Deserialize)]
struct JsonRequest {
    #[serde(default)]
    id: Option<Value>,
    command: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Deserialize)]
struct InstallParams {
    version: String,
    #[serde(default)]
    loader: Option<String>,
    #[serde(default)]
    loader_version: Option<String>,
    #[serde(default)]
    download_source: Option<String>,
    #[serde(default)]
    runtime: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LaunchParams {
    version: String,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    jvm_args: Option<String>,
    #[serde(default)]
    auth: Option<String>,
    #[serde(default)]
    api_url: Option<String>,
    #[serde(default)]
    authlib_jar: Option<String>,
    #[serde(default)]
    runtime: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ExternalLoginParams {
    identifier: String,
    #[serde(default)]
    password: Option<String>,
    api_url: String,
    #[serde(default)]
    profile: Option<String>,
}

pub async fn run(
    mut manager: LauncherManager,
    default_java_path: Option<String>,
) -> anyhow::Result<()> {
    output::init_json();
    output::write_event(json!({
        "type": "ready",
        "protocol": 1,
        "version": env!("CARGO_PKG_VERSION")
    }));

    let mut lines = BufReader::new(tokio::io::stdin()).lines();

    while let Some(line) = lines.next_line().await? {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let request: JsonRequest = match serde_json::from_str(line) {
            Ok(request) => request,
            Err(error) => {
                output::write_event(json!({
                    "type": "error",
                    "id": null,
                    "ok": false,
                    "error": format!("Invalid request: {}", error)
                }));
                continue;
            }
        };

        let id = request.id.unwrap_or(Value::Null);
        output::set_request(Some(id.clone()));

        let shutdown = request.command == "shutdown";
        let result = if shutdown {
            Ok(json!({ "shutdown": true }))
        } else {
            dispatch(
                &mut manager,
                &request.command,
                request.params,
                default_java_path.clone(),
            )
            .await
        };

        match result {
            Ok(data) => output::write_event(json!({
                "type": "result",
                "id": id,
                "ok": true,
                "data": data
            })),
            Err(error) => output::write_event(json!({
                "type": "error",
                "id": id,
                "ok": false,
                "error": format!("{:#}", error)
            })),
        }

        output::clear_request();

        if shutdown {
            break;
        }
    }

    Ok(())
}

async fn dispatch(
    manager: &mut LauncherManager,
    command: &str,
    params: Value,
    default_java_path: Option<String>,
) -> anyhow::Result<Value> {
    match command {
        "ping" => Ok(json!({ "pong": true })),

        "java.list" => {
            let runtimes = crate::java::detect_java_runtimes();
            let runtimes: Vec<Value> = runtimes
                .iter()
                .map(|runtime| {
                    json!({
                        "path": runtime.path.to_string_lossy(),
                        "major": runtime.version,
                        "version": runtime.version_string,
                        "arch": runtime.arch
                    })
                })
                .collect();
            Ok(json!({ "runtimes": runtimes }))
        }

        "versions.list" => {
            let versions = manager.installer.fetch_versions().await?;
            Ok(json!({ "versions": versions }))
        }

        "install" => {
            let params: InstallParams =
                serde_json::from_value(params).context("Invalid install parameters")?;
            let source = parse_download_source(params.download_source.as_deref())?;
            let loader = parse_loader(params.loader.as_deref())?;
            let previous_source = manager.installer.source;
            manager.installer.source = source;

            let install_result = commands::install(
                manager,
                &params.version,
                loader,
                params.loader_version.as_deref(),
                params.runtime.or(default_java_path),
            )
            .await;
            manager.installer.source = previous_source;

            let version_id = install_result?;
            Ok(json!({ "version_id": version_id }))
        }

        "launch" => {
            let params: LaunchParams =
                serde_json::from_value(params).context("Invalid launch parameters")?;
            let auth_type = parse_auth(params.auth.as_deref())?;

            commands::launch(
                manager,
                LaunchOptions {
                    version: params.version.clone(),
                    username: params.username,
                    access_token: params.access_token,
                    jvm_args: params.jvm_args,
                    auth_type,
                    api_url: params.api_url,
                    authlib_jar: params.authlib_jar,
                    java_path: params.runtime.or(default_java_path),
                },
            )
            .await?;

            Ok(json!({ "launched": true, "version": params.version }))
        }

        "login" => {
            let cache = manager.login().await?;
            manager.save_auth_cache(&cache)?;
            Ok(json!({
                "username": cache.username,
                "uuid": cache.uuid
            }))
        }

        "external_login" => {
            let params: ExternalLoginParams =
                serde_json::from_value(params).context("Invalid external_login parameters")?;
            let password = params
                .password
                .as_deref()
                .context("password is required in JSON mode")?;
            let selected_profile = params.profile.clone();

            let account = manager
                .external_login_with_selector(
                    &params.identifier,
                    password,
                    &params.api_url,
                    |profiles| {
                        if let Some(selected) = &selected_profile {
                            return profiles
                                .iter()
                                .find(|profile| {
                                    profile.id == *selected || profile.name == *selected
                                })
                                .cloned()
                                .ok_or_else(|| anyhow::anyhow!("Unknown profile: {}", selected));
                        }

                        if profiles.len() == 1 {
                            return Ok(profiles[0].clone());
                        }

                        output::profiles(profiles);
                        anyhow::bail!("profile selection required")
                    },
                )
                .await?;

            Ok(json!({
                "name": account.name,
                "uuid": account.uuid,
                "api_url": account.api_url,
                "server_name": account.server_name
            }))
        }

        "accounts.list" => {
            let accounts = manager.load_accounts()?;
            let accounts: Vec<Value> = accounts
                .iter()
                .map(|account| {
                    json!({
                        "identifier": account.identifier,
                        "name": account.name,
                        "uuid": account.uuid,
                        "api_url": account.api_url,
                        "server_name": account.server_name
                    })
                })
                .collect();
            let msa = match manager.load_auth_cache() {
                Ok(Some(cache)) => json!({
                    "username": cache.username,
                    "uuid": cache.uuid
                }),
                _ => Value::Null,
            };
            Ok(json!({ "accounts": accounts, "msa": msa }))
        }

        other => anyhow::bail!("Unknown command: {}", other),
    }
}

fn parse_download_source(value: Option<&str>) -> anyhow::Result<DownloadSource> {
    match value {
        None | Some("auto") => Ok(DownloadSource::Auto),
        Some("official") => Ok(DownloadSource::Official),
        Some("bmclapi") => Ok(DownloadSource::Bmclapi),
        Some(other) => anyhow::bail!("Unknown download source: {}", other),
    }
}

fn parse_loader(value: Option<&str>) -> anyhow::Result<Option<LoaderKind>> {
    match value {
        None => Ok(None),
        Some("fabric") => Ok(Some(LoaderKind::Fabric)),
        Some("quilt") => Ok(Some(LoaderKind::Quilt)),
        Some("forge") => Ok(Some(LoaderKind::Forge)),
        Some("neoforge") => Ok(Some(LoaderKind::Neoforge)),
        Some(other) => anyhow::bail!("Unknown loader: {}", other),
    }
}

fn parse_auth(value: Option<&str>) -> anyhow::Result<AuthType> {
    match value {
        None | Some("offline") => Ok(AuthType::Offline),
        Some("msa") => Ok(AuthType::Msa),
        Some("external") => Ok(AuthType::External),
        Some(other) => anyhow::bail!("Unknown authentication type: {}", other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_download_sources() {
        assert_eq!(parse_download_source(None).unwrap(), DownloadSource::Auto);
        assert_eq!(
            parse_download_source(Some("official")).unwrap(),
            DownloadSource::Official
        );
        assert_eq!(
            parse_download_source(Some("bmclapi")).unwrap(),
            DownloadSource::Bmclapi
        );
        assert!(parse_download_source(Some("mirror")).is_err());
    }

    #[test]
    fn parses_loaders_and_auth_types() {
        assert_eq!(parse_loader(None).unwrap(), None);
        assert_eq!(
            parse_loader(Some("fabric")).unwrap(),
            Some(LoaderKind::Fabric)
        );
        assert_eq!(
            parse_loader(Some("neoforge")).unwrap(),
            Some(LoaderKind::Neoforge)
        );
        assert!(parse_loader(Some("unknown")).is_err());

        assert_eq!(parse_auth(None).unwrap(), AuthType::Offline);
        assert_eq!(parse_auth(Some("msa")).unwrap(), AuthType::Msa);
        assert_eq!(parse_auth(Some("external")).unwrap(), AuthType::External);
        assert!(parse_auth(Some("unknown")).is_err());
    }

    #[test]
    fn rejects_malformed_requests() {
        assert!(serde_json::from_str::<JsonRequest>("{}").is_err());
        assert!(serde_json::from_str::<JsonRequest>(r#"{"command":"ping"}"#).is_ok());
    }
}
