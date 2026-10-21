use crate::common::models::{Arguments, Library, VersionDetails};
use anyhow::Context;
use std::collections::HashSet;
use std::fs;
use std::path::Path;

/// Loads a version JSON and resolves its `inheritsFrom` chain.
pub fn load_version_details(
    versions_dir: &Path,
    version_id: &str,
) -> anyhow::Result<VersionDetails> {
    let mut chain = Vec::new();
    let mut current = version_id.to_string();
    let mut seen = HashSet::new();

    loop {
        if !seen.insert(current.clone()) {
            anyhow::bail!("Circular version inheritance detected at '{}'", current);
        }

        let path = versions_dir
            .join(&current)
            .join(format!("{}.json", current));
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read version JSON: {:?}", path))?;
        let details: VersionDetails = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse version JSON: {:?}", path))?;

        let parent = details.inherits_from.clone();
        chain.push(details);

        match parent {
            Some(parent) => current = parent,
            None => break,
        }
    }

    let mut merged = chain.pop().expect("version chain is non-empty");
    while let Some(child) = chain.pop() {
        merged = merge_versions(merged, child);
    }

    if merged.jar.is_none() {
        merged.jar = Some(merged.id.clone());
    }

    Ok(merged)
}

fn merge_versions(parent: VersionDetails, child: VersionDetails) -> VersionDetails {
    let parent_id = parent.id.clone();

    VersionDetails {
        id: child.id,
        version_type: child.version_type,
        downloads: child.downloads.or(parent.downloads),
        libraries: merge_libraries(parent.libraries, child.libraries),
        main_class: child.main_class,
        minecraft_arguments: child.minecraft_arguments.or(parent.minecraft_arguments),
        arguments: merge_arguments(parent.arguments, child.arguments),
        asset_index: child.asset_index.or(parent.asset_index),
        java_version: child.java_version.or(parent.java_version),
        inherits_from: None,
        jar: child.jar.or(parent.jar).or(Some(parent_id)),
    }
}

fn merge_libraries(parent: Vec<Library>, child: Vec<Library>) -> Vec<Library> {
    let mut merged = parent;

    for library in child {
        if let Some(existing) = merged
            .iter_mut()
            .find(|existing| existing.name == library.name)
        {
            *existing = library;
        } else {
            merged.push(library);
        }
    }

    merged
}

fn merge_arguments(parent: Option<Arguments>, child: Option<Arguments>) -> Option<Arguments> {
    match (parent, child) {
        (None, None) => None,
        (Some(arguments), None) | (None, Some(arguments)) => Some(arguments),
        (Some(mut parent), Some(child)) => {
            // Do not deduplicate: JVM flags such as --add-opens and
            // --add-exports legitimately repeat, and removing duplicates
            // breaks their flag/value pairing.
            parent.game.extend(child.game);
            parent.jvm.extend(child.jvm);
            Some(parent)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    fn write_version(versions_dir: &Path, id: &str, json: &str) {
        let version_dir = versions_dir.join(id);
        fs::create_dir_all(&version_dir).unwrap();
        fs::write(version_dir.join(format!("{}.json", id)), json).unwrap();
    }

    #[test]
    fn merges_parent_and_child_versions() {
        let temp = tempdir().unwrap();
        write_version(
            temp.path(),
            "1.21.1",
            r#"{
                "id": "1.21.1",
                "type": "release",
                "mainClass": "net.minecraft.client.main.Main",
                "assetIndex": {
                    "id": "17",
                    "url": "https://example.invalid/17.json",
                    "sha1": "abc",
                    "size": 1
                },
                "javaVersion": { "majorVersion": 21 },
                "downloads": {
                    "client": {
                        "url": "https://example.invalid/client.jar",
                        "sha1": "def",
                        "size": 2
                    }
                },
                "libraries": [
                    {
                        "name": "base:lib:1.0",
                        "downloads": {
                            "artifact": {
                                "url": "https://example.invalid/base.jar",
                                "sha1": "base",
                                "path": "base/lib.jar"
                            }
                        }
                    }
                ],
                "arguments": {
                    "game": ["--username", "${auth_player_name}"],
                    "jvm": ["-Dparent=1"]
                }
            }"#,
        );
        write_version(
            temp.path(),
            "fabric-loader-1.0",
            r#"{
                "id": "fabric-loader-1.0",
                "type": "release",
                "inheritsFrom": "1.21.1",
                "mainClass": "net.fabricmc.loader.impl.launch.knot.KnotClient",
                "libraries": [
                    {
                        "name": "base:lib:1.0",
                        "url": "https://maven.example.invalid/",
                        "sha1": "new",
                        "size": 3
                    },
                    {
                        "name": "net.fabricmc:fabric-loader:1.0",
                        "url": "https://maven.fabricmc.net/",
                        "sha1": "loader",
                        "size": 4
                    }
                ],
                "arguments": {
                    "game": [],
                    "jvm": ["-DFabricMcEmu=net.minecraft.client.main.Main"]
                }
            }"#,
        );

        let merged = load_version_details(temp.path(), "fabric-loader-1.0").unwrap();

        assert_eq!(merged.id, "fabric-loader-1.0");
        assert_eq!(
            merged.main_class,
            "net.fabricmc.loader.impl.launch.knot.KnotClient"
        );
        assert_eq!(merged.asset_index.as_ref().unwrap().id, "17");
        assert_eq!(merged.java_version.as_ref().unwrap().major_version, 21);
        assert_eq!(merged.downloads.as_ref().unwrap().client.sha1, "def");
        assert_eq!(merged.jar.as_deref(), Some("1.21.1"));
        assert_eq!(merged.libraries.len(), 2);

        let base = merged
            .libraries
            .iter()
            .find(|library| library.name == "base:lib:1.0")
            .unwrap();
        assert_eq!(base.artifact().unwrap().sha1, "new");
        assert_eq!(merged.arguments.as_ref().unwrap().jvm.len(), 2);
        assert_eq!(merged.arguments.as_ref().unwrap().game.len(), 2);
    }

    #[test]
    fn merges_a_grandparent_chain() {
        let temp = tempdir().unwrap();
        write_version(
            temp.path(),
            "root",
            r#"{
                "id": "root",
                "type": "release",
                "mainClass": "root.Main",
                "javaVersion": { "majorVersion": 17 },
                "libraries": [{ "name": "a:a:1", "url": "https://example.invalid/" }],
                "arguments": { "game": [], "jvm": [] }
            }"#,
        );
        write_version(
            temp.path(),
            "middle",
            r#"{
                "id": "middle",
                "type": "release",
                "inheritsFrom": "root",
                "mainClass": "middle.Main",
                "libraries": [{ "name": "b:b:1", "url": "https://example.invalid/" }],
                "arguments": { "game": [], "jvm": [] }
            }"#,
        );
        write_version(
            temp.path(),
            "child",
            r#"{
                "id": "child",
                "type": "release",
                "inheritsFrom": "middle",
                "mainClass": "child.Main",
                "jar": "custom-jar",
                "libraries": [{ "name": "c:c:1", "url": "https://example.invalid/" }],
                "arguments": { "game": [], "jvm": [] }
            }"#,
        );

        let merged = load_version_details(temp.path(), "child").unwrap();

        assert_eq!(merged.main_class, "child.Main");
        assert_eq!(merged.java_version.as_ref().unwrap().major_version, 17);
        assert_eq!(merged.jar.as_deref(), Some("custom-jar"));
        assert_eq!(
            merged
                .libraries
                .iter()
                .map(|library| library.name.as_str())
                .collect::<Vec<_>>(),
            vec!["a:a:1", "b:b:1", "c:c:1"]
        );
    }

    #[test]
    fn keeps_repeated_jvm_flags_when_merging() {
        let temp = tempdir().unwrap();
        write_version(
            temp.path(),
            "parent",
            r#"{
                "id": "parent",
                "type": "release",
                "mainClass": "Parent",
                "libraries": [],
                "arguments": { "game": [], "jvm": [] }
            }"#,
        );
        write_version(
            temp.path(),
            "child",
            r#"{
                "id": "child",
                "type": "release",
                "inheritsFrom": "parent",
                "mainClass": "Child",
                "libraries": [],
                "arguments": {
                    "game": [],
                    "jvm": [
                        "--add-opens", "a",
                        "--add-opens", "b",
                        "--add-exports", "c",
                        "--add-exports", "d"
                    ]
                }
            }"#,
        );

        let merged = load_version_details(temp.path(), "child").unwrap();
        let jvm = &merged.arguments.unwrap().jvm;

        assert_eq!(jvm.len(), 8);
        assert_eq!(
            jvm.iter()
                .filter(|argument| argument.as_str() == Some("--add-opens"))
                .count(),
            2
        );
        assert_eq!(
            jvm.iter()
                .filter(|argument| argument.as_str() == Some("--add-exports"))
                .count(),
            2
        );
    }

    #[test]
    fn detects_inheritance_cycles() {
        let temp = tempdir().unwrap();
        write_version(
            temp.path(),
            "a",
            r#"{"id":"a","type":"release","inheritsFrom":"b","mainClass":"A"}"#,
        );
        write_version(
            temp.path(),
            "b",
            r#"{"id":"b","type":"release","inheritsFrom":"a","mainClass":"B"}"#,
        );

        let error = load_version_details(temp.path(), "a")
            .unwrap_err()
            .to_string();

        assert!(error.contains("Circular"));
    }

    #[test]
    fn reports_a_missing_parent() {
        let temp = tempdir().unwrap();
        write_version(
            temp.path(),
            "child",
            r#"{"id":"child","type":"release","inheritsFrom":"missing","mainClass":"Child"}"#,
        );

        assert!(load_version_details(temp.path(), "child").is_err());
    }
}
