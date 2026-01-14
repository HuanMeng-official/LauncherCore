use crate::models::*;
use anyhow::Context;
use reqwest::Client;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

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

        // Download client JAR
        if let Some(downloads) = &version_details.downloads {
            println!("Downloading client JAR...");
            self.download_file(
                &client,
                &downloads.client.url,
                &version_dir.join(format!("{}.jar", version_id)),
            )
            .await?;
        }

        // Save version JSON
        let version_json_path = version_dir.join(format!("{}.json", version_id));
        let version_json_content = serde_json::to_string_pretty(&version_details)?;
        fs::write(&version_json_path, version_json_content)?;

        // Download libraries
        println!("Downloading libraries...");
        let version_natives_dir = version_dir.join("natives");
        fs::create_dir_all(&version_natives_dir)?;
        self.download_libraries(&client, &version_details, &version_natives_dir)
            .await?;

        // Install ARM64 natives for Linux
        self.install_lwjgl_arm64_natives(&client, version_id, &version_details)
            .await?;

        // Download assets
        if let Some(asset_index) = &version_details.asset_index {
            println!("Downloading asset index...");
            let asset_index_path = self
                .assets_indexes_dir
                .join(format!("{}.json", asset_index.id));
            self.download_file(&client, &asset_index.url, &asset_index_path)
                .await?;
            println!("Downloading assets...");
            self.download_assets(&client, &asset_index_path).await?;
        }

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

        println!("Fetching version details...");
        let version_details: VersionDetails = client.get(&version_info.url).send().await?.json().await?;
        Ok((version_info.clone(), version_details))
    }

    async fn download_libraries(
        &self,
        client: &Client,
        version_details: &VersionDetails,
        version_natives_dir: &Path,
    ) -> anyhow::Result<()> {
        for library in &version_details.libraries {
            let Some(downloads) = &library.downloads else {
                continue;
            };

            self.process_library_artifact(client, downloads).await?;

            let Some(classifiers) = &downloads.classifiers else {
                continue;
            };

            self.process_native_artifact(client, classifiers, version_natives_dir)
                .await?;
            self.process_other_natives(client, classifiers, version_natives_dir)
                .await?;
        }
        Ok(())
    }

    async fn process_library_artifact(
        &self,
        client: &Client,
        downloads: &LibraryDownloads,
    ) -> anyhow::Result<()> {
        let Some(artifact) = &downloads.artifact else {
            return Ok(());
        };
        let library_path = self.libraries_dir.join(&artifact.path);
        if !library_path.exists() {
            if let Some(parent) = library_path.parent() {
                fs::create_dir_all(parent)?;
            }
            self.download_file(client, &artifact.url, &library_path)
                .await?;
        }
        Ok(())
    }

    async fn process_native_artifact(
        &self,
        client: &Client,
        classifiers: &Classifiers,
        version_natives_dir: &Path,
    ) -> anyhow::Result<()> {
        let Some(artifact) = self.get_native_artifact(classifiers) else {
            return Ok(());
        };
        let native_path = self.libraries_dir.join(&artifact.path);
        if !native_path.exists() {
            if let Some(parent) = native_path.parent() {
                fs::create_dir_all(parent)?;
            }
            self.download_file(client, &artifact.url, &native_path)
                .await?;
        }
        self.extract_lwjgl3_native_library(&native_path, version_natives_dir)?;
        Ok(())
    }

    async fn process_other_natives(
        &self,
        client: &Client,
        classifiers: &Classifiers,
        version_natives_dir: &Path,
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
                    self.download_file(client, &artifact.url, &native_path)
                        .await?;
                }
                self.extract_lwjgl3_native_library(&native_path, version_natives_dir)?;
            }
        }
        Ok(())
    }

    async fn install_lwjgl_arm64_natives(
        &self,
        client: &Client,
        version_id: &str,
        version_details: &VersionDetails,
    ) -> anyhow::Result<()> {
        if std::env::consts::OS != "linux" || std::env::consts::ARCH != "aarch64" {
            return Ok(());
        }

        println!("LWJGL ARM64: Checking for LWJGL libraries to replace with ARM64 versions...");

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
            println!(
                "LWJGL ARM64: No LWJGL libraries found in version manifest for {}, skipping ARM64 native install.",
                version_id
            );
            return Ok(());
        }

        println!(
            "LWJGL ARM64: Found LWJGL modules for version {}: {:?}",
            version_id, lwjgl_versions
        );

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

            println!("LWJGL ARM64: Preparing to download {}", url);
            let temp_file_path = temp_dir.path().join(format!(
                "{}-{}-{}.jar",
                module_artifact_id, module_version, classifier
            ));

            println!(
                "LWJGL ARM64: Downloading {}:{} ARM64 natives...",
                module_artifact_id, module_version
            );

            let download_result = self
                .download_file(&client, &url, &temp_file_path)
                .await;

            if let Err(e) = download_result {
                let is_not_found = e
                    .downcast_ref::<reqwest::Error>()
                    .and_then(|reqwest_err| reqwest_err.status())
                    .map(|status| status == reqwest::StatusCode::NOT_FOUND)
                    .unwrap_or(false);

                let error_msg = if is_not_found {
                    format!(
                        "LWJGL ARM64: Warning: ARM64 native library not found at {}. It seems this LWJGL version ({}) does not provide official ARM64 binaries. Skipping this module.",
                        url, module_version
                    )
                } else {
                    format!(
                        "LWJGL ARM64: Warning: Failed to download {} ({}). Error: {}. Skipping this module.",
                        module_artifact_id, url, e
                    )
                };
                println!("{}", error_msg);
                continue;
            }

            let version_natives_dir = self.versions_dir.join(version_id).join("natives");
            println!(
                "LWJGL ARM64: Extracting {}:{} natives to {:?}",
                module_artifact_id, module_version, version_natives_dir
            );
            self.extract_lwjgl3_native_library(&temp_file_path, &version_natives_dir)?;
        }

        println!(
            "LWJGL ARM64: Installation of ARM64 native libraries completed (where available) for version {}.",
            version_id
        );
        Ok(())
    }

    async fn download_assets(&self, client: &Client, asset_index_path: &Path) -> anyhow::Result<()> {
        let asset_index_content = fs::read_to_string(asset_index_path)?;
        let assets_index: AssetsIndex = serde_json::from_str(&asset_index_content)?;
        let total_assets = assets_index.objects.len();
        let mut downloaded = 0;

        for (_name, asset_object) in &assets_index.objects {
            downloaded += 1;
            if downloaded % 50 == 0 || downloaded == total_assets {
                println!("Downloading assets: {}/{}", downloaded, total_assets);
            }

            let hash = &asset_object.hash;
            let first_two = &hash[..2];
            let asset_path = self.assets_objects_dir.join(first_two).join(hash);

            if !asset_path.exists() {
                if let Some(parent) = asset_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                let asset_url = format!("{}/{}/{}", ASSET_BASE_URL, first_two, hash);
                self.download_file(client, &asset_url, &asset_path).await?;
            }
        }

        println!("Assets downloaded successfully!");
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

    async fn download_file(&self, client: &Client, url: &str, path: &Path) -> anyhow::Result<()> {
        if path.exists() {
            return Ok(());
        }
        let response = client.get(url).send().await?;
        let bytes = response.bytes().await?;
        fs::write(path, bytes)?;
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
