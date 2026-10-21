//! Forge version resolution and installer invocation.

use super::installer::run_installer;
use crate::download;
use crate::install::Installer;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
struct ForgePromotions {
    promos: HashMap<String, String>,
}

pub(super) async fn install_forge(
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

fn select_forge_version(promos: &HashMap<String, String>, game_version: &str) -> Option<String> {
    let recommended = format!("{}-recommended", game_version);
    let latest = format!("{}-latest", game_version);
    promos
        .get(&recommended)
        .or_else(|| promos.get(&latest))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn selects_forge_for_new_minecraft_versions() {
        let promos = HashMap::from([("26.3-latest".to_string(), "66.0.13".to_string())]);

        assert_eq!(
            select_forge_version(&promos, "26.3"),
            Some("66.0.13".to_string())
        );
        assert_eq!(normalize_forge_version("26.3", "26.3-66.0.13"), "66.0.13");
    }
}
