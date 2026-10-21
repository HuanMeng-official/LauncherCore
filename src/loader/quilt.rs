//! Quilt loader installation from the Quilt metadata API.

use super::fabric_like::{self, ProfileOptions, Selection};
use crate::install::Installer;

pub(super) async fn install(
    installer: &Installer,
    game_version: &str,
    requested_version: Option<&str>,
) -> anyhow::Result<String> {
    let versions_url = format!(
        "https://meta.quiltmc.org/v3/versions/loader/{}",
        game_version
    );
    fabric_like::install(
        installer,
        game_version,
        requested_version,
        &versions_url,
        "Quilt",
        Selection::FirstNonBeta,
        profile_options(),
    )
    .await
}

fn profile_options() -> ProfileOptions<'static> {
    ProfileOptions {
        id_prefix: "quilt",
        loader_maven: "https://maven.quiltmc.org/repository/release/",
        jvm_arguments: None,
        hashed_maven: Some("https://maven.quiltmc.org/repository/release/"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_a_stable_quilt_loader() {
        let entries = vec![
            fabric_like::test_entry("0.21.0-beta.1", false),
            fabric_like::test_entry("0.20.0", false),
        ];

        assert_eq!(
            fabric_like::select(Selection::FirstNonBeta, &entries)
                .map(|entry| entry.loader.version.as_str()),
            Some("0.20.0")
        );
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

        let entries: Vec<fabric_like::LoaderEntry> = serde_json::from_str(json).unwrap();
        let profile =
            fabric_like::build_profile("1.21.1", &entries[0], &profile_options()).unwrap();

        assert_eq!(profile["id"], "quilt-loader-0.20.0-beta.9-1.21.1");
        assert_eq!(
            profile["mainClass"],
            "org.quiltmc.loader.impl.launch.knot.KnotClient"
        );
        assert_eq!(profile["libraries"].as_array().unwrap().len(), 4);
        assert!(profile["arguments"].get("jvm").is_none());

        let details: crate::models::VersionDetails = serde_json::from_value(profile).unwrap();
        assert_eq!(details.id, "quilt-loader-0.20.0-beta.9-1.21.1");
        assert_eq!(details.inherits_from.as_deref(), Some("1.21.1"));
    }
}
