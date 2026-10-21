//! Fabric loader installation from the Fabric metadata API.

use super::fabric_like::{self, ProfileOptions, Selection};
use crate::game::install::Installer;

pub(super) async fn install(
    installer: &Installer,
    game_version: &str,
    requested_version: Option<&str>,
) -> anyhow::Result<String> {
    let versions_url = format!(
        "https://meta.fabricmc.net/v2/versions/loader/{}",
        game_version
    );
    fabric_like::install(
        installer,
        game_version,
        requested_version,
        &versions_url,
        "Fabric",
        Selection::FirstStable,
        profile_options(),
    )
    .await
}

fn profile_options() -> ProfileOptions<'static> {
    ProfileOptions {
        id_prefix: "fabric",
        loader_maven: "https://maven.fabricmc.net/",
        jvm_arguments: Some("-DFabricMcEmu= net.minecraft.client.main.Main "),
        hashed_maven: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chooses_a_stable_fabric_loader_when_available() {
        let entries = vec![
            fabric_like::test_entry("0.20.1", false),
            fabric_like::test_entry("0.19.0", true),
        ];

        assert_eq!(
            fabric_like::select(Selection::FirstStable, &entries)
                .map(|entry| entry.loader.version.as_str()),
            Some("0.19.0")
        );
    }

    #[test]
    fn falls_back_to_the_first_fabric_loader() {
        let entries = vec![fabric_like::test_entry("0.20.1", false)];

        assert_eq!(
            fabric_like::select(Selection::FirstStable, &entries)
                .map(|entry| entry.loader.version.as_str()),
            Some("0.20.1")
        );
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

        let entries: Vec<fabric_like::LoaderEntry> = serde_json::from_str(json).unwrap();
        let profile =
            fabric_like::build_profile("1.14.4", &entries[0], &profile_options()).unwrap();

        assert_eq!(
            profile["mainClass"],
            "net.fabricmc.loader.launch.knot.KnotClient"
        );
    }

    #[test]
    fn builds_fabric_profile_for_new_minecraft_versions() {
        let entry = fabric_like::test_entry("0.19.5", true);
        let profile = fabric_like::build_profile("26.3", &entry, &profile_options()).unwrap();

        assert_eq!(profile["id"], "fabric-loader-0.19.5-26.3");
        assert_eq!(profile["inheritsFrom"], "26.3");
        assert_eq!(
            profile["mainClass"],
            "net.fabricmc.loader.impl.launch.knot.KnotClient"
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

        let entries: Vec<fabric_like::LoaderEntry> = serde_json::from_str(json).unwrap();
        let profile =
            fabric_like::build_profile("1.21.1", &entries[0], &profile_options()).unwrap();

        assert_eq!(profile["id"], "fabric-loader-0.20.1-1.21.1");
        assert_eq!(profile["libraries"].as_array().unwrap().len(), 3);
        assert_eq!(
            profile["arguments"]["jvm"][0],
            "-DFabricMcEmu= net.minecraft.client.main.Main "
        );

        let details: crate::common::models::VersionDetails =
            serde_json::from_value(profile.clone()).unwrap();
        assert_eq!(details.id, "fabric-loader-0.20.1-1.21.1");
        assert_eq!(details.inherits_from.as_deref(), Some("1.21.1"));
        assert_eq!(details.libraries.len(), 3);
    }
}
