//! NeoForge version resolution and installer invocation.

use super::installer::run_installer;
use crate::download;
use crate::install::Installer;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct NeoForgeVersions {
    versions: Vec<String>,
}

pub(super) async fn install_neoforge(
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
                    anyhow::anyhow!("No NeoForge version found for Minecraft {}", game_version)
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
                    anyhow::anyhow!("No NeoForge version found for Minecraft {}", game_version)
                })?
            }
        };
        let url = format!(
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/{version}/neoforge-{version}-installer.jar",
            version = version
        );
        (version, url)
    };

    crate::outln!("Using NeoForge {}", neoforge_version);
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
    if parts.len() < 2 || parts.iter().any(|part| part.is_empty()) {
        return None;
    }

    if parts[0] == "1" {
        // Legacy Minecraft versioning: 1.21.1 -> NeoForge 21.1.<build>.
        let minor = parts[1];
        let patch = parts.get(2).copied().unwrap_or("0");
        Some(format!("{}.{}.", minor, patch))
    } else {
        // New Minecraft versioning (26.x and later): 26.3 -> 26.3.0.<build>,
        // 26.1.2 -> 26.1.2.<build>.
        let mut normalized = parts;
        while normalized.len() < 3 {
            normalized.push("0");
        }
        Some(format!("{}.", normalized.join(".")))
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
        .filter(|version| {
            !version.contains("alpha") && !version.contains("beta") && !version.contains("snapshot")
        })
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

fn select_neoforge_legacy_version(versions: &[String], game_version: &str) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(neoforge_prefix("26.3"), Some("26.3.0.".to_string()));
        assert_eq!(neoforge_prefix("26.1.2"), Some("26.1.2.".to_string()));
    }

    #[test]
    fn chooses_neoforge_for_new_minecraft_versions() {
        let versions = vec![
            "26.2.0.93".to_string(),
            "26.3.0.94".to_string(),
            "26.3.0.95".to_string(),
            "26.3.0.96-beta".to_string(),
        ];

        assert_eq!(
            select_neoforge_version(&versions, "26.3"),
            Some("26.3.0.95".to_string())
        );
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
}
