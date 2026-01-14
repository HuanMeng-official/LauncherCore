use crate::models::*;
use anyhow::Context;
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, atomic::{AtomicU64, Ordering}};

const VERSION_MANIFEST_URL: &str = "https://launchermeta.mojang.com/mc/game/version_manifest.json";
const ASSET_BASE_URL: &str = "https://resources.download.minecraft.net";
const MAVEN_BASE_URL: &str = "https://repo1.maven.org/maven2";

#[derive(Debug)]
pub struct Installer {
    pub versions_dir: std::path::PathBuf,
    pub libraries_dir: std::path::PathBuf,
    pub assets_objects_dir: std::path::PathBuf,
    pub assets_indexes_dir: std::path::PathBuf,
}

impl Installer {
    pub async fn list_versions(&self) -> anyhow::Result<()> {
        println!("Fetching available Minecraft versions...");
        let client = Client::new();
        let manifest: VersionManifest = client
            .get(VERSION_MANIFEST_URL)
            .send()
            .await?
            .json()
            .await?;
        println!("Available versions:");
        for version in &manifest.versions {
            println!("  {} ({})", version.id, version.version_type);
        }
        Ok(())
    }

    pub async fn install_version(&self, version_id: &str) -> anyhow::Result<()> {
        println!("Installing Minecraft version: {}", version_id);

        let client = Client::new();
        let (_version_info, version_details) = self.fetch_version_details(&client, version_id).await?;

        let version_dir = self.versions_dir.join(version_id);
        fs::create_dir_all(&version_dir)?;

        // Use a simple spinner for download progress
        let pb = ProgressBar::new_spinner();
        pb.set_style(ProgressStyle::default_spinner()
            .template("{spinner:.green} Downloading files... {msg}")
            .unwrap());
        pb.enable_steady_tick(std::time::Duration::from_millis(100));

        let counter = Arc::new(AtomicU64::new(0));
        let pb = Arc::new(pb);

        // Download client JAR
        if let Some(downloads) = &version_details.downloads {
            let client_jar_path = version_dir.join(format!("{}.jar", version_id));
            self.download_file_with_simple_progress(&client, &downloads.client.url, &client_jar_path, &counter, &pb).await?;
        }

        // Save version JSON
        let version_json_path = version_dir.join(format!("{}.json", version_id));
        let version_json_content = serde_json::to_string_pretty(&version_details)?;
        fs::write(&version_json_path, version_json_content)?;

        // Download libraries
        let version_natives_dir = version_dir.join("natives");
        fs::create_dir_all(&version_natives_dir)?;
        self.download_libraries_with_simple_progress(&client, &version_details, &version_natives_dir, &counter, &pb).await?;

        // Install ARM64 natives for Linux
        self.install_lwjgl_arm64_natives_with_simple_progress(&client, version_id, &version_details, &counter, &pb).await?;

        // Download assets
        if let Some(asset_index) = &version_details.asset_index {
            let asset_index_path = self.assets_indexes_dir.join(format!("{}.json", asset_index.id));
            self.download_file_with_simple_progress(&client, &asset_index.url, &asset_index_path, &counter, &pb).await?;
            self.download_assets_with_simple_progress(&client, &asset_index_path, &counter, &pb).await?;
        }

        let downloaded = counter.load(Ordering::SeqCst);
        pb.finish_with_message(format!("Downloaded {} files for version {}", downloaded, version_id));
        println!("Version {} installed successfully!", version_id);
        Ok(())
    }

    async fn fetch_version_details(
        &self,
        client: &Client,
        version_id: &str,
    ) -> anyhow::Result<(VersionInfo, VersionDetails)> {
        let manifest: VersionManifest = client.get(VERSION_MANIFEST_URL).send().await?.json().await?;
        let version_info = manifest
            .versions
            .iter()
            .find(|v| v.id == version_id)
            .ok_or_else(|| anyhow::anyhow!("Version {} not found", version_id))?;

        let version_details: VersionDetails = client.get(&version_info.url).send().await?.json().await?;
        Ok((version_info.clone(), version_details))
    }

    async fn download_libraries_with_simple_progress(
        &self,
        client: &Client,
        version_details: &VersionDetails,
        version_natives_dir: &Path,
        counter: &Arc<AtomicU64>,
        pb: &Arc<ProgressBar>,
    ) -> anyhow::Result<()> {
        for library in &version_details.libraries {
            let Some(downloads) = &library.downloads else {
                continue;
            };

            self.process_library_artifact_with_simple_progress(client, downloads, counter, pb).await?;

            let Some(classifiers) = &downloads.classifiers else {
                continue;
            };

            self.process_native_artifact_with_simple_progress(client, classifiers, version_natives_dir, counter, pb)
                .await?;
            self.process_other_natives_with_simple_progress(client, classifiers, version_natives_dir, counter, pb)
                .await?;
        }
        Ok(())
    }

    async fn process_library_artifact_with_simple_progress(
        &self,
        client: &Client,
        downloads: &LibraryDownloads,
        counter: &Arc<AtomicU64>,
        pb: &Arc<ProgressBar>,
    ) -> anyhow::Result<()> {
        let Some(artifact) = &downloads.artifact else {
            return Ok(());
        };
        let library_path = self.libraries_dir.join(&artifact.path);
        if !library_path.exists() {
            if let Some(parent) = library_path.parent() {
                fs::create_dir_all(parent)?;
            }
            self.download_file_with_simple_progress(client, &artifact.url, &library_path, counter, pb)
                .await?;
        }
        Ok(())
    }

    async fn process_native_artifact_with_simple_progress(
        &self,
        client: &Client,
        classifiers: &Classifiers,
        version_natives_dir: &Path,
        counter: &Arc<AtomicU64>,
        pb: &Arc<ProgressBar>,
    ) -> anyhow::Result<()> {
        let Some(artifact) = self.get_native_artifact(classifiers) else {
            return Ok(());
        };
        let native_path = self.libraries_dir.join(&artifact.path);
        if !native_path.exists() {
            if let Some(parent) = native_path.parent() {
                fs::create_dir_all(parent)?;
            }
            self.download_file_with_simple_progress(client, &artifact.url, &native_path, counter, pb)
                .await?;
        }
        self.extract_lwjgl3_native_library(&native_path, version_natives_dir)?;
        Ok(())
    }

    async fn process_other_natives_with_simple_progress(
        &self,
        client: &Client,
        classifiers: &Classifiers,
        version_natives_dir: &Path,
        counter: &Arc<AtomicU64>,
        pb: &Arc<ProgressBar>,
    ) -> anyhow::Result<()> {
        let os_name = match std::env::consts::OS {
            "windows" => "windows",
            "linux" => "linux",
            "macos" => "macos",
            _ => return Ok(()),
        };
        let arch_name = match std::env::consts::ARCH {
            "aarch64" => "aarch64",
            "x86_64" => "x64",
            _ => std::env::consts::ARCH,
        };

        for (classifier_name, artifact) in &classifiers.other {
            let is_native_for_os_and_arch = classifier_name
                == &format!("natives-{}-{}", os_name, arch_name)
                || (arch_name == "x64" && classifier_name == &format!("natives-{}", os_name));

            if is_native_for_os_and_arch || classifier_name == &format!("natives-{}", os_name) {
                let native_path = self.libraries_dir.join(&artifact.path);
                if !native_path.exists() {
                    if let Some(parent) = native_path.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    self.download_file_with_simple_progress(client, &artifact.url, &native_path, counter, pb)
                        .await?;
                }
                self.extract_lwjgl3_native_library(&native_path, version_natives_dir)?;
            }
        }
        Ok(())
    }

    async fn install_lwjgl_arm64_natives_with_simple_progress(
        &self,
        client: &Client,
        version_id: &str,
        version_details: &VersionDetails,
        counter: &Arc<AtomicU64>,
        pb: &Arc<ProgressBar>,
    ) -> anyhow::Result<()> {
        if std::env::consts::OS != "linux" || std::env::consts::ARCH != "aarch64" {
            return Ok(());
        }

        let mut lwjgl_versions: HashMap<String, String> = HashMap::new();
        let target_lwjgl_modules: HashSet<&str> = [
            "lwjgl",
            "lwjgl-glfw",
            "lwjgl-jemalloc",
            "lwjgl-openal",
            "lwjgl-opengl",
            "lwjgl-stb",
            "lwjgl-tinyfd",
            "lwjgl-freetype",
        ]
        .iter()
        .cloned()
        .collect();

        for lib in &version_details.libraries {
            if lib.name.starts_with("org.lwjgl:") {
                let parts: Vec<&str> = lib.name.split(':').collect();
                if parts.len() == 3 {
                    let artifact_id = parts[1];
                    let version = parts[2];
                    if target_lwjgl_modules.contains(artifact_id) {
                        lwjgl_versions.insert(artifact_id.to_string(), version.to_string());
                    }
                }
            }
        }

        if lwjgl_versions.is_empty() {
            return Ok(());
        }

        let os_name = "linux";
        let arch_name = "arm64";
        let classifier = format!("natives-{}-{}", os_name, arch_name);
        let temp_dir = tempfile::tempdir()?;

        for (module_artifact_id, module_version) in &lwjgl_versions {
            let group_path = "org/lwjgl";
            let url = format!(
                "{}/{}/{}/{}-{}-{}.jar",
                MAVEN_BASE_URL,
                group_path,
                module_artifact_id,
                module_version,
                module_artifact_id,
                classifier
            );

            let temp_file_path = temp_dir.path().join(format!(
                "{}-{}-{}.jar",
                module_artifact_id, module_version, classifier
            ));

            let download_result = self
                .download_file_with_simple_progress(&client, &url, &temp_file_path, counter, pb)
                .await;

            if download_result.is_err() {
                continue;
            }

            let version_natives_dir = self.versions_dir.join(version_id).join("natives");
            self.extract_lwjgl3_native_library(&temp_file_path, &version_natives_dir)?;
        }

        Ok(())
    }

    async fn download_assets_with_simple_progress(
        &self,
        client: &Client,
        asset_index_path: &Path,
        counter: &Arc<AtomicU64>,
        pb: &Arc<ProgressBar>,
    ) -> anyhow::Result<()> {
        let asset_index_content = fs::read_to_string(asset_index_path)?;
        let assets_index: AssetsIndex = serde_json::from_str(&asset_index_content)?;

        for (_name, asset_object) in &assets_index.objects {
            let hash = &asset_object.hash;
            let first_two = &hash[..2];
            let asset_path = self.assets_objects_dir.join(first_two).join(hash);

            if !asset_path.exists() {
                if let Some(parent) = asset_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                let asset_url = format!("{}/{}/{}", ASSET_BASE_URL, first_two, hash);
                self.download_file_with_simple_progress(client, &asset_url, &asset_path, counter, pb).await?;
            }
        }

        Ok(())
    }

    fn get_native_artifact<'a>(&self, classifiers: &'a Classifiers) -> Option<&'a Artifact> {
        let os_name = match std::env::consts::OS {
            "windows" => "windows",
            "linux" => "linux",
            "macos" => "macos",
            _ => return None,
        };
        let arch_name = match std::env::consts::ARCH {
            "aarch64" => "aarch64",
            "x86_64" => "x64",
            _ => std::env::consts::ARCH,
        };

        let specific_key = format!("natives-{}-{}", os_name, arch_name);
        if let Some(artifact) = classifiers.other.get(&specific_key) {
            return Some(artifact);
        }

        match std::env::consts::OS {
            "windows" => classifiers
                .natives_windows
                .as_ref()
                .or_else(|| classifiers.other.get("natives-windows")),
            "linux" => classifiers
                .natives_linux
                .as_ref()
                .or_else(|| classifiers.other.get("natives-linux")),
            "macos" => classifiers
                .natives_macos
                .as_ref()
                .or_else(|| classifiers.other.get("natives-macos")),
            _ => None,
        }
    }

    async fn download_file_with_simple_progress(
        &self,
        client: &Client,
        url: &str,
        path: &Path,
        counter: &Arc<AtomicU64>,
        pb: &Arc<ProgressBar>,
    ) -> anyhow::Result<()> {
        if path.exists() {
            return Ok(());
        }

        let response = client.get(url).send().await?;

        let mut file = File::create(path).context("Failed to create file")?;
        let mut stream = response.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("Failed to read chunk")?;
            file.write_all(&chunk).context("Failed to write chunk")?;
        }

        // Update progress
        let downloaded = counter.fetch_add(1, Ordering::SeqCst) + 1;
        pb.set_position(downloaded);

        Ok(())
    }

    fn extract_lwjgl3_native_library(&self, jar_path: &Path, extract_dir: &Path) -> anyhow::Result<()> {
        let file = std::fs::File::open(jar_path)
            .with_context(|| format!("Failed to open native JAR file: {:?}", jar_path))?;
        let mut archive = zip::ZipArchive::new(file)
            .with_context(|| format!("Failed to read ZIP archive: {:?}", jar_path))?;

        for i in 0..archive.len() {
            let mut file = archive
                .by_index(i)
                .with_context(|| format!("Failed to get file entry {} from archive: {:?}", i, jar_path))?;
            let outpath = extract_dir.join(file.mangled_name());
            let file_name = file.name().to_lowercase();

            if file_name.ends_with(".dll") || file_name.ends_with(".so") || file_name.ends_with(".dylib") {
                if let Some(p) = outpath.parent() {
                    if !p.exists() {
                        fs::create_dir_all(p).with_context(|| {
                            format!("Failed to create directory for native file: {:?}", p)
                        })?;
                    }
                }
                let mut outfile = std::fs::File::create(&outpath)
                    .with_context(|| format!("Failed to create output file for native: {:?}", outpath))?;
                std::io::copy(&mut file, &mut outfile)
                    .with_context(|| format!("Failed to write native file: {:?}", outpath))?;
            }
        }
        Ok(())
    }
}
