use crate::download;
use crate::install::Installer;
use anyhow::Context;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum LoaderKind {
    Fabric,
    Quilt,
    Forge,
    Neoforge,
}

impl LoaderKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Fabric => "Fabric",
            Self::Quilt => "Quilt",
            Self::Forge => "Forge",
            Self::Neoforge => "NeoForge",
        }
    }
}

#[derive(Debug, Deserialize)]
struct LoaderEntry {
    loader: LoaderCoordinate,
    #[serde(default)]
    intermediary: Option<LoaderCoordinate>,
    #[serde(default)]
    hashed: Option<LoaderCoordinate>,
    #[serde(rename = "launcherMeta", default)]
    launcher_meta: Option<LauncherMeta>,
}

#[derive(Debug, Deserialize)]
struct LoaderCoordinate {
    maven: String,
    version: String,
    #[serde(default)]
    stable: bool,
}

#[derive(Debug, Deserialize)]
struct LauncherMeta {
    #[serde(default)]
    libraries: LauncherLibraries,
    #[serde(rename = "mainClass")]
    main_class: LauncherMainClass,
}

#[derive(Debug, Default, Deserialize)]
struct LauncherLibraries {
    #[serde(default)]
    client: Vec<serde_json::Value>,
    #[serde(default)]
    common: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum LauncherMainClass {
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

#[derive(Debug, Deserialize)]
struct ForgePromotions {
    promos: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct NeoForgeVersions {
    versions: Vec<String>,
}

pub async fn install_loader(
    installer: &Installer,
    kind: LoaderKind,
    game_version: &str,
    loader_version: Option<&str>,
    java_path_override: Option<String>,
) -> anyhow::Result<String> {
    println!(
        "Installing {} for Minecraft {}...",
        kind.label(),
        game_version
    );
    installer.install_version(game_version).await?;

    let version_id = match kind {
        LoaderKind::Fabric => {
            install_fabric_like(installer, game_version, loader_version, true).await?
        }
        LoaderKind::Quilt => {
            install_fabric_like(installer, game_version, loader_version, false).await?
        }
        LoaderKind::Forge => {
            install_forge(installer, game_version, loader_version, java_path_override).await?
        }
        LoaderKind::Neoforge => {
            install_neoforge(installer, game_version, loader_version, java_path_override).await?
        }
    };

    println!(
        "Installed {} for Minecraft {} as version '{}'.",
        kind.label(),
        game_version,
        version_id
    );
    println!("Launch with: mclc launch {}", version_id);
    Ok(version_id)
}

async fn install_fabric_like(
    installer: &Installer,
    game_version: &str,
    requested_version: Option<&str>,
    fabric: bool,
) -> anyhow::Result<String> {
    let client = reqwest::Client::new();
    let versions_url = if fabric {
        format!(
            "https://meta.fabricmc.net/v2/versions/loader/{}",
            game_version
        )
    } else {
        format!(
            "https://meta.quiltmc.org/v3/versions/loader/{}",
            game_version
        )
    };

    let entries: Vec<LoaderEntry> =
        download::fetch_json(&client, &versions_url, installer.source).await?;
    let entry = match requested_version {
        Some(version) => entries
            .iter()
            .find(|entry| entry.loader.version == version)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "{} loader version {} was not found for Minecraft {}",
                    if fabric { "Fabric" } else { "Quilt" },
                    version,
                    game_version
                )
            })?,
        None if fabric => select_fabric_loader(&entries).ok_or_else(|| {
            anyhow::anyhow!(
                "No Fabric loader versions found for Minecraft {}",
                game_version
            )
        })?,
        None => select_quilt_loader(&entries).ok_or_else(|| {
            anyhow::anyhow!(
                "No Quilt loader versions found for Minecraft {}",
                game_version
            )
        })?,
    };

    let profile = build_loader_profile(game_version, fabric, entry)?;
    let version_id = profile
        .get("id")
        .and_then(|value| value.as_str())
        .ok_or_else(|| anyhow::anyhow!("Loader profile is missing an id"))?
        .to_string();
    let profile_json = serde_json::to_string_pretty(&profile)?;

    let version_dir = installer.versions_dir.join(&version_id);
    fs::create_dir_all(&version_dir)?;
    fs::write(version_dir.join(format!("{}.json", version_id)), profile_json)?;

    installer.install_loaded_version(&version_id).await?;
    Ok(version_id)
}

fn select_fabric_loader(entries: &[LoaderEntry]) -> Option<&LoaderEntry> {
    entries
        .iter()
        .find(|entry| entry.loader.stable)
        .or_else(|| entries.first())
}

fn select_quilt_loader(entries: &[LoaderEntry]) -> Option<&LoaderEntry> {
    entries
        .iter()
        .find(|entry| !entry.loader.version.contains("beta"))
        .or_else(|| entries.first())
}

fn build_loader_profile(
    game_version: &str,
    fabric: bool,
    entry: &LoaderEntry,
) -> anyhow::Result<serde_json::Value> {
    let launcher_meta = entry
        .launcher_meta
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Loader metadata is missing launcherMeta"))?;

    let mut libraries = launcher_meta.libraries.client.clone();
    libraries.extend(launcher_meta.libraries.common.clone());

    let fabric_maven = "https://maven.fabricmc.net/";
    let quilt_maven = "https://maven.quiltmc.org/repository/release/";

    if let Some(intermediary) = &entry.intermediary {
        libraries.push(serde_json::json!({
            "name": intermediary.maven,
            "url": fabric_maven
        }));
    }

    libraries.push(serde_json::json!({
        "name": entry.loader.maven,
        "url": if fabric { fabric_maven } else { quilt_maven }
    }));

    if let Some(hashed) = &entry.hashed {
        libraries.push(serde_json::json!({
            "name": hashed.maven,
            "url": quilt_maven
        }));
    }

    let (prefix, arguments) = if fabric {
        (
            "fabric",
            serde_json::json!({
                "game": [],
                "jvm": ["-DFabricMcEmu= net.minecraft.client.main.Main "]
            }),
        )
    } else {
        ("quilt", serde_json::json!({ "game": [] }))
    };

    Ok(serde_json::json!({
        "id": format!("{}-loader-{}-{}", prefix, entry.loader.version, game_version),
        "inheritsFrom": game_version,
        "type": "release",
        "mainClass": launcher_meta.main_class.client(),
        "arguments": arguments,
        "libraries": libraries
    }))
}

async fn install_forge(
    installer: &Installer,
    game_version: &str,
    requested_version: Option<&str>,
    java_path_override: Option<String>,
) -> anyhow::Result<String> {
    let client = reqwest::Client::new();
    let forge_version = match requested_version {
        Some(version) => normalize_forge_version(game_version, version),
        None => {
            let promotions: ForgePromotions = download::fetch_json(
                &client,
                "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json",
                installer.source,
            )
            .await?;
            select_forge_version(&promotions.promos, game_version).ok_or_else(|| {
                anyhow::anyhow!("No Forge version found for Minecraft {}", game_version)
            })?
        }
    };

    let installer_url = format!(
        "https://maven.minecraftforge.net/net/minecraftforge/forge/{game}-{forge}/forge-{game}-{forge}-installer.jar",
        game = game_version,
        forge = forge_version
    );
    run_installer(
        installer,
        &client,
        &installer_url,
        game_version,
        java_path_override,
    )
    .await
}

fn normalize_forge_version(game_version: &str, requested: &str) -> String {
    if let Some(version) = requested.strip_prefix(&format!("{}-forge-", game_version)) {
        return version.to_string();
    }
    if let Some(version) = requested.strip_prefix(&format!("{}-", game_version)) {
        return version.to_string();
    }
    requested.to_string()
}

fn select_forge_version(
    promos: &HashMap<String, String>,
    game_version: &str,
) -> Option<String> {
    let recommended = format!("{}-recommended", game_version);
    let latest = format!("{}-latest", game_version);
    promos
        .get(&recommended)
        .or_else(|| promos.get(&latest))
        .cloned()
}

async fn install_neoforge(
    installer: &Installer,
    game_version: &str,
    requested_version: Option<&str>,
    java_path_override: Option<String>,
) -> anyhow::Result<String> {
    let client = reqwest::Client::new();

    // NeoForge 1.20.1 is published as `net.neoforged:forge` with versions
    // like `1.20.1-47.1.106`; later versions use `net.neoforged:neoforge`.
    let (neoforge_version, installer_url) = if game_version == "1.20.1" {
        let version = match requested_version {
            Some(version) => normalize_neoforge_legacy_version(game_version, version),
            None => {
                let data: NeoForgeVersions = download::fetch_json(
                    &client,
                    "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/forge",
                    installer.source,
                )
                .await?;
                select_neoforge_legacy_version(&data.versions, game_version).ok_or_else(|| {
                    anyhow::anyhow!(
                        "No NeoForge version found for Minecraft {}",
                        game_version
                    )
                })?
            }
        };
        let url = format!(
            "https://maven.neoforged.net/releases/net/neoforged/forge/{version}/forge-{version}-installer.jar",
            version = version
        );
        (version, url)
    } else {
        let version = match requested_version {
            Some(version) => normalize_neoforge_version(game_version, version),
            None => {
                let data: NeoForgeVersions = download::fetch_json(
                    &client,
                    "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge",
                    installer.source,
                )
                .await?;
                select_neoforge_version(&data.versions, game_version).ok_or_else(|| {
                    anyhow::anyhow!(
                        "No NeoForge version found for Minecraft {}",
                        game_version
                    )
                })?
            }
        };
        let url = format!(
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/{version}/neoforge-{version}-installer.jar",
            version = version
        );
        (version, url)
    };

    println!("Using NeoForge {}", neoforge_version);
    run_installer(
        installer,
        &client,
        &installer_url,
        game_version,
        java_path_override,
    )
    .await
}

fn neoforge_prefix(game_version: &str) -> Option<String> {
    let parts: Vec<&str> = game_version.split('.').collect();
    if parts.len() >= 2 && parts[0] == "1" {
        let minor = parts[1];
        let patch = parts.get(2).copied().unwrap_or("0");
        Some(format!("{}.{}.", minor, patch))
    } else {
        None
    }
}

fn select_neoforge_version(versions: &[String], game_version: &str) -> Option<String> {
    let prefix = neoforge_prefix(game_version)?;
    let matching: Vec<&String> = versions
        .iter()
        .filter(|version| version.starts_with(&prefix))
        .collect();
    let stable: Vec<&String> = matching
        .iter()
        .filter(|version| !version.contains("beta"))
        .copied()
        .collect();
    let candidates = if stable.is_empty() { matching } else { stable };

    candidates
        .into_iter()
        .max_by_key(|version| version_key(version.as_str()))
        .cloned()
}

fn normalize_neoforge_version(game_version: &str, requested: &str) -> String {
    requested
        .strip_prefix("neoforge-")
        .or_else(|| requested.strip_prefix(&format!("{}-", game_version)))
        .unwrap_or(requested)
        .to_string()
}

fn normalize_neoforge_legacy_version(game_version: &str, requested: &str) -> String {
    if let Some(version) = requested.strip_prefix(&format!("{}-forge-", game_version)) {
        return format!("{}-{}", game_version, version);
    }
    if requested.starts_with(&format!("{}-", game_version)) {
        return requested.to_string();
    }
    format!("{}-{}", game_version, requested)
}

fn select_neoforge_legacy_version(
    versions: &[String],
    game_version: &str,
) -> Option<String> {
    let prefix = format!("{}-47.", game_version);
    versions
        .iter()
        .filter(|version| version.starts_with(&prefix))
        .max_by_key(|version| {
            let build = version.strip_prefix(&prefix).unwrap_or(version);
            version_key(build)
        })
        .cloned()
}

fn version_key(version: &str) -> Vec<u32> {
    version
        .split('-')
        .next()
        .unwrap_or(version)
        .split('.')
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

async fn run_installer(
    installer: &Installer,
    client: &reqwest::Client,
    installer_url: &str,
    game_version: &str,
    java_path_override: Option<String>,
) -> anyhow::Result<String> {
    let sha1_text = download::fetch_text(
        client,
        &format!("{}.sha1", installer_url),
        installer.source,
    )
    .await
    .context("Failed to fetch the loader installer checksum")?;
    let expected_sha1 = sha1_text
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if expected_sha1.len() != 40
        || !expected_sha1.chars().all(|character| character.is_ascii_hexdigit())
    {
        anyhow::bail!("Invalid installer SHA-1 for {}", installer_url);
    }

    let temp_dir = tempfile::tempdir().context("Failed to create a temporary directory")?;
    let installer_path = temp_dir.path().join("installer.jar");
    let urls = download::url_candidates(installer_url, installer.source);
    download::download_file(
        client,
        &urls,
        &installer_path,
        Some(&expected_sha1),
        None,
    )
    .await?;

    let version_id = read_embedded_version_id(&installer_path)?;
    let parent = crate::version::load_version_details(&installer.versions_dir, game_version)?;
    let java = crate::java::select_java(
        parent
            .java_version
            .as_ref()
            .map(|java| java.major_version),
        java_path_override,
    )?;

    ensure_launcher_profiles(&installer.game_dir, game_version)?;

    println!("Running the {} installer...", version_id);
    let status = installer_command(&java, &installer_path, &installer.game_dir)
        .status()
        .with_context(|| format!("Failed to run the installer with Java {}", java.display()))?;

    if !status.success() {
        anyhow::bail!("Loader installer exited with {}", status);
    }

    let version_json = installer
        .versions_dir
        .join(&version_id)
        .join(format!("{}.json", version_id));
    if !version_json.exists() {
        anyhow::bail!("Loader installer did not create {:?}", version_json);
    }

    installer.install_loaded_version(&version_id).await?;
    Ok(version_id)
}

fn installer_command(java: &Path, installer_path: &Path, game_dir: &Path) -> Command {
    let mut command = Command::new(java);
    command
        .arg("-Djava.awt.headless=true")
        .arg("-jar")
        .arg(installer_path)
        .arg("--installClient")
        .arg(game_dir)
        .current_dir(game_dir);
    command
}

fn read_embedded_version_id(jar_path: &Path) -> anyhow::Result<String> {
    let file = fs::File::open(jar_path)
        .with_context(|| format!("Failed to open installer: {:?}", jar_path))?;
    let mut archive = zip::ZipArchive::new(file)
        .with_context(|| format!("Failed to read installer archive: {:?}", jar_path))?;

    for name in ["version.json", "install_profile.json"] {
        let mut entry = match archive.by_name(name) {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let mut content = String::new();
        entry.read_to_string(&mut content)?;
        let value: serde_json::Value = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse {} from installer", name))?;

        if let Some(id) = value.get("id").and_then(|value| value.as_str()) {
            return Ok(id.to_string());
        }
        if let Some(version) = value.get("version").and_then(|value| value.as_str()) {
            return Ok(version.to_string());
        }
        if let Some(id) = value
            .get("versionInfo")
            .and_then(|value| value.get("id"))
            .and_then(|value| value.as_str())
        {
            return Ok(id.to_string());
        }
        if let Some(version) = value
            .get("install")
            .and_then(|value| value.get("version"))
            .and_then(|value| value.as_str())
        {
            return Ok(version.to_string());
        }
    }

    anyhow::bail!("Could not find a version id in {:?}", jar_path)
}

fn ensure_launcher_profiles(
    game_dir: &Path,
    game_version: &str,
) -> anyhow::Result<()> {
    let path = game_dir.join("launcher_profiles.json");
    if path.exists() {
        return Ok(());
    }

    let profiles = serde_json::json!({
        "profiles": {
            "mclc": {
                "name": "mclc",
                "type": "custom",
                "gameDir": ".",
                "lastVersionId": game_version
            }
        },
        "selectedProfile": "mclc",
        "settings": {},
        "version": 3
    });

    fs::write(&path, serde_json::to_string_pretty(&profiles)?)
        .with_context(|| format!("Failed to write {:?}", path))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::fs;
    use std::io::Write;

    fn loader_entry(version: &str, stable: bool) -> LoaderEntry {
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

    #[test]
    fn parses_legacy_string_main_classes() {
        let json = r#"[
            {
                "loader": {
                    "maven": "net.fabricmc:fabric-loader:0.1.0",
                    "version": "0.1.0"
                },
                "launcherMeta": {
                    "libraries": { "client": [], "common": [] },
                    "mainClass": "net.fabricmc.loader.launch.knot.KnotClient"
                }
            }
        ]"#;

        let entries: Vec<LoaderEntry> = serde_json::from_str(json).unwrap();
        let profile = build_loader_profile("1.14.4", true, &entries[0]).unwrap();

        assert_eq!(
            profile["mainClass"],
            "net.fabricmc.loader.launch.knot.KnotClient"
        );
    }

    #[test]
    fn chooses_a_stable_fabric_loader_when_available() {
        let entries = vec![loader_entry("0.20.1", false), loader_entry("0.19.0", true)];

        assert_eq!(
            select_fabric_loader(&entries).map(|entry| entry.loader.version.as_str()),
            Some("0.19.0")
        );
    }

    #[test]
    fn falls_back_to_the_first_fabric_loader() {
        let entries = vec![loader_entry("0.20.1", false)];

        assert_eq!(
            select_fabric_loader(&entries).map(|entry| entry.loader.version.as_str()),
            Some("0.20.1")
        );
    }

    #[test]
    fn prefers_a_stable_quilt_loader() {
        let entries = vec![
            loader_entry("0.21.0-beta.1", false),
            loader_entry("0.20.0", false),
        ];

        assert_eq!(
            select_quilt_loader(&entries).map(|entry| entry.loader.version.as_str()),
            Some("0.20.0")
        );
    }

    #[test]
    fn parses_real_fabric_metadata() {
        let json = r#"[
            {
                "loader": {
                    "separator": ".",
                    "build": 1,
                    "maven": "net.fabricmc:fabric-loader:0.20.1",
                    "version": "0.20.1",
                    "stable": false
                },
                "intermediary": {
                    "maven": "net.fabricmc:intermediary:1.21.1",
                    "version": "1.21.1",
                    "stable": true
                },
                "launcherMeta": {
                    "version": 2,
                    "min_java_version": 8,
                    "libraries": {
                        "client": [],
                        "common": [
                            {
                                "name": "org.ow2.asm:asm:9.10.1",
                                "url": "https://maven.fabricmc.net/",
                                "sha1": "ada2141c0cc52ee8f5c48cd5fa4ce0e794f22236",
                                "size": 126151
                            }
                        ],
                        "server": [],
                        "development": []
                    },
                    "mainClass": {
                        "client": "net.fabricmc.loader.impl.launch.knot.KnotClient",
                        "server": "net.fabricmc.loader.impl.launch.knot.KnotServer"
                    }
                }
            }
        ]"#;

        let entries: Vec<LoaderEntry> = serde_json::from_str(json).unwrap();
        let profile = build_loader_profile("1.21.1", true, &entries[0]).unwrap();

        assert_eq!(profile["id"], "fabric-loader-0.20.1-1.21.1");
        assert_eq!(profile["libraries"].as_array().unwrap().len(), 3);

        let details: crate::models::VersionDetails =
            serde_json::from_value(profile.clone()).unwrap();
        assert_eq!(details.id, "fabric-loader-0.20.1-1.21.1");
        assert_eq!(details.inherits_from.as_deref(), Some("1.21.1"));
        assert_eq!(details.libraries.len(), 3);
    }

    #[test]
    fn parses_real_quilt_metadata() {
        let json = r#"[
            {
                "loader": {
                    "maven": "org.quiltmc:quilt-loader:0.20.0-beta.9",
                    "version": "0.20.0-beta.9",
                    "build": 9,
                    "separator": ".",
                    "file_size": 1550096,
                    "hashes": { "sha1": "bed0a01be87c4378d9002611d9642e8f9e9c2cff" }
                },
                "hashed": {
                    "maven": "org.quiltmc:hashed:1.21.1",
                    "version": "1.21.1",
                    "file_size": 913310,
                    "hashes": { "sha1": "8470b4db40fde9b9a07b1c23e81156cb762a618a" }
                },
                "intermediary": {
                    "maven": "net.fabricmc:intermediary:1.21.1",
                    "version": "1.21.1"
                },
                "launcherMeta": {
                    "version": 1,
                    "libraries": {
                        "client": [],
                        "common": [
                            {
                                "name": "net.fabricmc:tiny-mappings-parser:0.3.0+build.17",
                                "url": "https://maven.fabricmc.net/"
                            }
                        ],
                        "server": []
                    },
                    "mainClass": {
                        "client": "org.quiltmc.loader.impl.launch.knot.KnotClient",
                        "server": "org.quiltmc.loader.impl.launch.knot.KnotServer",
                        "serverLauncher": "org.quiltmc.loader.impl.launch.server.QuiltServerLauncher"
                    }
                }
            }
        ]"#;

        let entries: Vec<LoaderEntry> = serde_json::from_str(json).unwrap();
        let profile = build_loader_profile("1.21.1", false, &entries[0]).unwrap();

        assert_eq!(profile["id"], "quilt-loader-0.20.0-beta.9-1.21.1");
        assert_eq!(
            profile["mainClass"],
            "org.quiltmc.loader.impl.launch.knot.KnotClient"
        );
        assert_eq!(profile["libraries"].as_array().unwrap().len(), 4);

        let details: crate::models::VersionDetails =
            serde_json::from_value(profile).unwrap();
        assert_eq!(details.id, "quilt-loader-0.20.0-beta.9-1.21.1");
        assert_eq!(details.inherits_from.as_deref(), Some("1.21.1"));
    }

    #[test]
    fn builds_a_fabric_profile_from_metadata() {
        let entry = loader_entry("0.20.1", true);

        let profile = build_loader_profile("1.21.1", true, &entry).unwrap();

        assert_eq!(profile["id"], "fabric-loader-0.20.1-1.21.1");
        assert_eq!(profile["inheritsFrom"], "1.21.1");
        assert_eq!(
            profile["mainClass"],
            "net.fabricmc.loader.impl.launch.knot.KnotClient"
        );
        assert_eq!(profile["libraries"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn prefers_the_recommended_forge_version() {
        let promos = HashMap::from([
            ("1.21.1-latest".to_string(), "52.1.21".to_string()),
            ("1.21.1-recommended".to_string(), "52.1.20".to_string()),
        ]);

        assert_eq!(
            select_forge_version(&promos, "1.21.1"),
            Some("52.1.20".to_string())
        );
    }

    #[test]
    fn falls_back_to_the_latest_forge_version() {
        let promos = HashMap::from([("1.21.1-latest".to_string(), "52.1.21".to_string())]);

        assert_eq!(
            select_forge_version(&promos, "1.21.1"),
            Some("52.1.21".to_string())
        );
    }

    #[test]
    fn normalizes_forge_loader_versions() {
        assert_eq!(normalize_forge_version("1.21.1", "52.1.20"), "52.1.20");
        assert_eq!(
            normalize_forge_version("1.21.1", "1.21.1-52.1.20"),
            "52.1.20"
        );
        assert_eq!(
            normalize_forge_version("1.21.1", "1.21.1-forge-52.1.20"),
            "52.1.20"
        );
    }

    #[test]
    fn normalizes_neoforge_loader_versions() {
        assert_eq!(normalize_neoforge_version("1.21.1", "21.1.264"), "21.1.264");
        assert_eq!(
            normalize_neoforge_version("1.21.1", "neoforge-21.1.264"),
            "21.1.264"
        );
        assert_eq!(
            normalize_neoforge_legacy_version("1.20.1", "47.1.106"),
            "1.20.1-47.1.106"
        );
        assert_eq!(
            normalize_neoforge_legacy_version("1.20.1", "1.20.1-forge-47.1.106"),
            "1.20.1-47.1.106"
        );
    }

    #[test]
    fn derives_neoforge_version_prefixes() {
        assert_eq!(neoforge_prefix("1.21.1"), Some("21.1.".to_string()));
        assert_eq!(neoforge_prefix("1.21"), Some("21.0.".to_string()));
        assert_eq!(neoforge_prefix("1.20.6"), Some("20.6.".to_string()));
    }

    #[test]
    fn chooses_the_newest_neoforge_legacy_version() {
        let versions = vec![
            "1.20.1-47.1.99".to_string(),
            "1.20.1-47.1.106".to_string(),
            "1.20.1-47.1.100".to_string(),
            "1.21.1-52.0.1".to_string(),
        ];

        assert_eq!(
            select_neoforge_legacy_version(&versions, "1.20.1"),
            Some("1.20.1-47.1.106".to_string())
        );
    }

    #[test]
    fn chooses_the_newest_stable_neoforge_version() {
        let versions = vec![
            "20.4.1".to_string(),
            "21.1.100".to_string(),
            "21.1.264".to_string(),
            "21.1.300-beta".to_string(),
            "21.2.1".to_string(),
        ];

        assert_eq!(
            select_neoforge_version(&versions, "1.21.1"),
            Some("21.1.264".to_string())
        );
    }

    #[test]
    fn falls_back_to_a_neoforge_beta_when_no_stable_exists() {
        let versions = vec!["20.2.3-beta".to_string(), "20.2.4-beta".to_string()];

        assert_eq!(
            select_neoforge_version(&versions, "1.20.2"),
            Some("20.2.4-beta".to_string())
        );
    }

    #[test]
    fn builds_a_headless_installer_command() {
        let command = installer_command(
            Path::new("java"),
            Path::new("installer.jar"),
            Path::new("/game"),
        );
        let args: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();

        assert_eq!(
            args,
            vec![
                "-Djava.awt.headless=true",
                "-jar",
                "installer.jar",
                "--installClient",
                "/game"
            ]
        );
    }

    #[test]
    fn reads_the_version_id_from_an_installer_jar() {
        let temp = tempfile::tempdir().unwrap();
        let jar_path = temp.path().join("installer.jar");
        {
            let file = fs::File::create(&jar_path).unwrap();
            let mut writer = zip::ZipWriter::new(file);
            writer
                .start_file("version.json", zip::write::SimpleFileOptions::default())
                .unwrap();
            writer
                .write_all(br#"{"id":"1.21.1-forge-52.1.20"}"#)
                .unwrap();
            writer.finish().unwrap();
        }

        assert_eq!(
            read_embedded_version_id(&jar_path).unwrap(),
            "1.21.1-forge-52.1.20"
        );
    }

    #[test]
    fn reads_a_legacy_install_profile_version() {
        let temp = tempfile::tempdir().unwrap();
        let jar_path = temp.path().join("installer.jar");
        {
            let file = fs::File::create(&jar_path).unwrap();
            let mut writer = zip::ZipWriter::new(file);
            writer
                .start_file(
                    "install_profile.json",
                    zip::write::SimpleFileOptions::default(),
                )
                .unwrap();
            writer
                .write_all(br#"{"install":{"version":"1.7.10-Forge10.13.4.1614"}}"#)
                .unwrap();
            writer.finish().unwrap();
        }

        assert_eq!(
            read_embedded_version_id(&jar_path).unwrap(),
            "1.7.10-Forge10.13.4.1614"
        );
    }

    #[test]
    fn reads_the_version_id_from_install_profile() {
        let temp = tempfile::tempdir().unwrap();
        let jar_path = temp.path().join("installer.jar");
        {
            let file = fs::File::create(&jar_path).unwrap();
            let mut writer = zip::ZipWriter::new(file);
            writer
                .start_file(
                    "install_profile.json",
                    zip::write::SimpleFileOptions::default(),
                )
                .unwrap();
            writer
                .write_all(br#"{"version":"1.12.2-forge-14.23.5.2859"}"#)
                .unwrap();
            writer.finish().unwrap();
        }

        assert_eq!(
            read_embedded_version_id(&jar_path).unwrap(),
            "1.12.2-forge-14.23.5.2859"
        );
    }
}
