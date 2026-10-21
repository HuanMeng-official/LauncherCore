//! Shared download, verification, and execution for loader installers.

use crate::net::download;
use crate::game::install::Installer;
use anyhow::Context;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::process::Command;

pub(super) async fn run_installer(
    installer: &Installer,
    client: &reqwest::Client,
    installer_url: &str,
    game_version: &str,
    java_path_override: Option<String>,
) -> anyhow::Result<String> {
    let sha1_text =
        download::fetch_text(client, &format!("{}.sha1", installer_url), installer.source)
            .await
            .context("Failed to fetch the loader installer checksum")?;
    let expected_sha1 = sha1_text
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if expected_sha1.len() != 40
        || !expected_sha1
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        anyhow::bail!("Invalid installer SHA-1 for {}", installer_url);
    }

    let temp_dir = tempfile::tempdir().context("Failed to create a temporary directory")?;
    let installer_path = temp_dir.path().join("installer.jar");
    let urls = download::url_candidates(installer_url, installer.source);
    download::download_file(client, &urls, &installer_path, Some(&expected_sha1), None).await?;

    let version_id = read_embedded_version_id(&installer_path)?;
    let parent = crate::game::version::load_version_details(&installer.versions_dir, game_version)?;
    let java = crate::game::java::select_java(
        parent.java_version.as_ref().map(|java| java.major_version),
        java_path_override,
    )?;

    ensure_launcher_profiles(&installer.game_dir, game_version)?;

    crate::outln!("Running the {} installer...", version_id);
    crate::common::output::progress("loader_install", 0, 1, "Running loader installer");

    if crate::common::output::is_json() {
        let output = installer_command(&java, &installer_path, &installer.game_dir)
            .output()
            .with_context(|| format!("Failed to run the installer with Java {}", java.display()))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let lines: Vec<&str> = stderr.lines().collect();
            for line in lines.iter().rev().take(20).rev() {
                crate::outwarn!("{}", line);
            }
            anyhow::bail!("Loader installer exited with {}", output.status);
        }
    } else {
        let status = installer_command(&java, &installer_path, &installer.game_dir)
            .status()
            .with_context(|| format!("Failed to run the installer with Java {}", java.display()))?;
        if !status.success() {
            anyhow::bail!("Loader installer exited with {}", status);
        }
    }
    crate::common::output::progress("loader_install", 1, 1, "Loader installer finished");

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

fn ensure_launcher_profiles(game_dir: &Path, game_version: &str) -> anyhow::Result<()> {
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
    use std::io::Write;

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
