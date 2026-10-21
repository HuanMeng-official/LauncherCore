//! Shared plumbing for Fabric and Quilt, whose metadata and profiles have the
//! same shape. Loader-specific endpoints and profile options live in
//! `fabric.rs` and `quilt.rs`.

use crate::net::download;
use crate::game::install::Installer;
use serde::Deserialize;
use std::fs;

#[derive(Debug, Deserialize)]
pub(super) struct LoaderEntry {
    pub(super) loader: LoaderCoordinate,
    #[serde(default)]
    pub(super) intermediary: Option<LoaderCoordinate>,
    #[serde(default)]
    pub(super) hashed: Option<LoaderCoordinate>,
    #[serde(rename = "launcherMeta", default)]
    pub(super) launcher_meta: Option<LauncherMeta>,
}

#[derive(Debug, Deserialize)]
pub(super) struct LoaderCoordinate {
    pub(super) maven: String,
    pub(super) version: String,
    #[serde(default)]
    pub(super) stable: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct LauncherMeta {
    #[serde(default)]
    pub(super) libraries: LauncherLibraries,
    #[serde(rename = "mainClass")]
    pub(super) main_class: LauncherMainClass,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct LauncherLibraries {
    #[serde(default)]
    pub(super) client: Vec<serde_json::Value>,
    #[serde(default)]
    pub(super) common: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(super) enum LauncherMainClass {
    Detailed { client: String },
    Simple(String),
}

impl LauncherMainClass {
    fn client(&self) -> &str {
        match self {
            Self::Detailed { client } => client,
            Self::Simple(value) => value,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum Selection {
    FirstStable,
    FirstNonBeta,
}

pub(super) struct ProfileOptions<'a> {
    pub id_prefix: &'a str,
    pub loader_maven: &'a str,
    pub jvm_arguments: Option<&'a str>,
    pub hashed_maven: Option<&'a str>,
}

pub(super) async fn install(
    installer: &Installer,
    game_version: &str,
    requested_version: Option<&str>,
    versions_url: &str,
    label: &str,
    selection: Selection,
    options: ProfileOptions<'_>,
) -> anyhow::Result<String> {
    let client = reqwest::Client::new();
    let entries: Vec<LoaderEntry> =
        download::fetch_json(&client, versions_url, installer.source).await?;

    let entry = match requested_version {
        Some(version) => entries
            .iter()
            .find(|entry| entry.loader.version == version)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "{} loader version {} was not found for Minecraft {}",
                    label,
                    version,
                    game_version
                )
            })?,
        None => select(selection, &entries).ok_or_else(|| {
            anyhow::anyhow!(
                "No {} loader versions found for Minecraft {}",
                label,
                game_version
            )
        })?,
    };

    let profile = build_profile(game_version, entry, &options)?;
    let version_id = profile
        .get("id")
        .and_then(|value| value.as_str())
        .ok_or_else(|| anyhow::anyhow!("Loader profile is missing an id"))?
        .to_string();
    let profile_json = serde_json::to_string_pretty(&profile)?;

    let version_dir = installer.versions_dir.join(&version_id);
    fs::create_dir_all(&version_dir)?;
    fs::write(
        version_dir.join(format!("{}.json", version_id)),
        profile_json,
    )?;

    installer.install_loaded_version(&version_id).await?;
    Ok(version_id)
}

pub(super) fn select(selection: Selection, entries: &[LoaderEntry]) -> Option<&LoaderEntry> {
    let selected = match selection {
        Selection::FirstStable => entries.iter().find(|entry| entry.loader.stable),
        Selection::FirstNonBeta => entries
            .iter()
            .find(|entry| !entry.loader.version.contains("beta")),
    };
    selected.or_else(|| entries.first())
}

pub(super) fn build_profile(
    game_version: &str,
    entry: &LoaderEntry,
    options: &ProfileOptions<'_>,
) -> anyhow::Result<serde_json::Value> {
    let launcher_meta = entry
        .launcher_meta
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Loader metadata is missing launcherMeta"))?;

    let mut libraries = launcher_meta.libraries.client.clone();
    libraries.extend(launcher_meta.libraries.common.clone());

    let fabric_maven = "https://maven.fabricmc.net/";

    if let Some(intermediary) = &entry.intermediary {
        libraries.push(serde_json::json!({
            "name": intermediary.maven,
            "url": fabric_maven
        }));
    }

    libraries.push(serde_json::json!({
        "name": entry.loader.maven,
        "url": options.loader_maven
    }));

    if let (Some(hashed), Some(hashed_maven)) = (&entry.hashed, options.hashed_maven) {
        libraries.push(serde_json::json!({
            "name": hashed.maven,
            "url": hashed_maven
        }));
    }

    let mut arguments = serde_json::json!({ "game": [] });
    if let Some(jvm) = options.jvm_arguments {
        arguments["jvm"] = serde_json::json!([jvm]);
    }

    Ok(serde_json::json!({
        "id": format!("{}-loader-{}-{}", options.id_prefix, entry.loader.version, game_version),
        "inheritsFrom": game_version,
        "type": "release",
        "mainClass": launcher_meta.main_class.client(),
        "arguments": arguments,
        "libraries": libraries
    }))
}

#[cfg(test)]
pub(super) fn test_entry(version: &str, stable: bool) -> LoaderEntry {
    LoaderEntry {
        loader: LoaderCoordinate {
            maven: format!("net.fabricmc:fabric-loader:{}", version),
            version: version.to_string(),
            stable,
        },
        intermediary: Some(LoaderCoordinate {
            maven: "net.fabricmc:intermediary:1.21.1".to_string(),
            version: "1.21.1".to_string(),
            stable: true,
        }),
        hashed: None,
        launcher_meta: Some(LauncherMeta {
            libraries: LauncherLibraries {
                client: Vec::new(),
                common: vec![serde_json::json!({
                    "name": "org.ow2.asm:asm:9.10.1",
                    "url": "https://maven.fabricmc.net/"
                })],
            },
            main_class: LauncherMainClass::Detailed {
                client: "net.fabricmc.loader.impl.launch.knot.KnotClient".to_string(),
            },
        }),
    }
}
