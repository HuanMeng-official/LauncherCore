use crate::models::*;
use anyhow::Context;
use futures_util::stream::{self, StreamExt};
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
const MAX_CONCURRENT_DOWNLOADS: usize = 16;

#[derive(Debug, Clone)]
struct DownloadTask {
    url: String,
    path: std::path::PathBuf,
    task_type: String,
}

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

        // Collect all download tasks
        let mut tasks = Vec::new();
        let mut extraction_tasks = Vec::new();
        let mut asset_index_url = None;
        let mut asset_index_path = None;

        // Client JAR
        if let Some(downloads) = &version_details.downloads {
            let client_jar_path = version_dir.join(format!("{}.jar", version_id));
            if !client_jar_path.exists() {
                tasks.push(DownloadTask {
                    url: downloads.client.url.clone(),
                    path: client_jar_path,
                    task_type: "client".to_string(),
                });
            }
        }

        // Asset index URL (always need to check this for assets)
        if let Some(asset_index) = &version_details.asset_index {
            let index_path = self.assets_indexes_dir.join(format!("{}.json", asset_index.id));
            asset_index_url = Some(asset_index.url.clone());
            asset_index_path = Some(index_path.clone());
        }

        // Download client JAR and asset index first if needed
        let mut first_phase_tasks = Vec::new();
        if let Some(ref url) = asset_index_url {
            if let Some(ref path) = asset_index_path {
                if !path.exists() {
                    first_phase_tasks.push(DownloadTask {
                        url: url.clone(),
                        path: path.clone(),
                        task_type: "index".to_string(),
                    });
                }
            }
        }

        if !first_phase_tasks.is_empty() {
            println!("Downloading metadata...");
            let client = Arc::new(client);
            let results = stream::iter(first_phase_tasks)
                .map(|task| {
                    let client = Arc::clone(&client);
                    let url = task.url.clone();
                    let path = task.path.clone();
                    let task_type = task.task_type.clone();
                    tokio::spawn(async move {
                        (task_type, Self::download_file(&client, &url, &path).await)
                    })
                })
                .buffer_unordered(MAX_CONCURRENT_DOWNLOADS)
                .collect::<Vec<_>>()
                .await;

            for result in results {
                if let Ok((task_type, download_result)) = result {
                    if let Err(e) = download_result {
                        anyhow::bail!("Failed to download {}: {}", task_type, e);
                    }
                }
            }
        }

        // Now collect library and asset tasks
        let client = Client::new();
        let version_natives_dir = version_dir.join("natives");
        fs::create_dir_all(&version_natives_dir)?;
        self.collect_library_download_tasks(&version_details, version_id, &version_natives_dir, &mut tasks, &mut extraction_tasks);

        // ARM64 natives for Linux
        if std::env::consts::OS == "linux" && std::env::consts::ARCH == "aarch64" {
            self.collect_lwjgl_arm64_download_tasks(&version_details, version_id, &version_natives_dir, &mut tasks, &mut extraction_tasks);
        }

        // Assets (now index should exist)
        if let Some(ref index_path) = asset_index_path {
            if index_path.exists() {
                self.collect_asset_download_tasks(index_path, &mut tasks);
            }
        }

        if tasks.is_empty() && extraction_tasks.is_empty() {
            println!("All files already downloaded for version {}!", version_id);
            // Still need to extract natives if they exist
            for (jar_path, extract_dir) in &extraction_tasks {
                if jar_path.exists() {
                    self.extract_lwjgl3_native_library(jar_path, extract_dir)?;
                }
            }
            return Ok(());
        }

        // Create progress bar
        let pb = Arc::new(ProgressBar::new(tasks.len() as u64));
        pb.set_style(ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} files ({eta})")
            .unwrap()
            .progress_chars("#>-"));

        // Download all files concurrently
        let client = Arc::new(client);
        let counter = Arc::new(AtomicU64::new(0));
        let pb_clone = Arc::clone(&pb);

        let download_results = stream::iter(tasks)
            .map(|task| {
                let client = Arc::clone(&client);
                let counter = Arc::clone(&counter);
                let pb = Arc::clone(&pb_clone);
                tokio::spawn(async move {
                    let result = Self::download_file(&client, &task.url, &task.path).await;
                    if result.is_ok() {
                        let downloaded = counter.fetch_add(1, Ordering::SeqCst) + 1;
                        pb.set_position(downloaded);
                    }
                    (task, result)
                })
            })
            .buffer_unordered(MAX_CONCURRENT_DOWNLOADS)
            .collect::<Vec<_>>()
            .await;

        // Check for errors
        for result in download_results {
            if let Ok((task, download_result)) = result {
                if let Err(e) = download_result {
                    pb.println(format!("Failed to download {} ({}): {}", task.task_type, task.url, e));
                }
            }
        }

        pb.finish_with_message(format!("Downloaded {} files", counter.load(Ordering::SeqCst)));

        // Extract native libraries
        for (jar_path, extract_dir) in extraction_tasks {
            if jar_path.exists() {
                self.extract_lwjgl3_native_library(&jar_path, &extract_dir)?;
            }
        }

        // Save version JSON
        let version_json_path = version_dir.join(format!("{}.json", version_id));
        let version_json_content = serde_json::to_string_pretty(&version_details)?;
        fs::write(&version_json_path, version_json_content)?;

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

    fn collect_library_download_tasks(
        &self,
        version_details: &VersionDetails,
        _version_id: &str,
        version_natives_dir: &Path,
        tasks: &mut Vec<DownloadTask>,
        extraction_tasks: &mut Vec<(std::path::PathBuf, std::path::PathBuf)>,
    ) {
        for library in &version_details.libraries {
            let Some(downloads) = &library.downloads else {
                continue;
            };

            if let Some(artifact) = &downloads.artifact {
                let library_path = self.libraries_dir.join(&artifact.path);
                if !library_path.exists() {
                    if let Some(parent) = library_path.parent() {
                        let _ = fs::create_dir_all(parent);
                    }
                    tasks.push(DownloadTask {
                        url: artifact.url.clone(),
                        path: library_path,
                        task_type: "library".to_string(),
                    });
                }
            }

            if let Some(classifiers) = &downloads.classifiers {
                if let Some(artifact) = self.get_native_artifact(classifiers) {
                    let native_path = self.libraries_dir.join(&artifact.path);
                    if !native_path.exists() {
                        if let Some(parent) = native_path.parent() {
                            let _ = fs::create_dir_all(parent);
                        }
                        tasks.push(DownloadTask {
                            url: artifact.url.clone(),
                            path: native_path.clone(),
                            task_type: "native".to_string(),
                        });
                    }
                    extraction_tasks.push((native_path, version_natives_dir.to_path_buf()));
                }

                // Other natives
                for (classifier_name, artifact) in &classifiers.other {
                    if classifier_name.contains("natives-") {
                        let native_path = self.libraries_dir.join(&artifact.path);
                        if !native_path.exists() {
                            if let Some(parent) = native_path.parent() {
                                let _ = fs::create_dir_all(parent);
                            }
                            tasks.push(DownloadTask {
                                url: artifact.url.clone(),
                                path: native_path.clone(),
                                task_type: "native".to_string(),
                            });
                        }
                        extraction_tasks.push((native_path, version_natives_dir.to_path_buf()));
                    }
                }
            }
        }
    }

    fn collect_lwjgl_arm64_download_tasks(
        &self,
        version_details: &VersionDetails,
        _version_id: &str,
        version_natives_dir: &Path,
        tasks: &mut Vec<DownloadTask>,
        extraction_tasks: &mut Vec<(std::path::PathBuf, std::path::PathBuf)>,
    ) {
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
            return;
        }

        for (module_artifact_id, module_version) in &lwjgl_versions {
            let group_path = "org/lwjgl";
            let classifier = "natives-linux-arm64";
            let url = format!(
                "{}/{}/{}/{}-{}-{}.jar",
                MAVEN_BASE_URL,
                group_path,
                module_artifact_id,
                module_version,
                module_artifact_id,
                classifier
            );

            let temp_file_path = std::env::temp_dir().join(format!(
                "mclc-{}-{}-{}.jar",
                module_artifact_id, module_version, classifier
            ));

            tasks.push(DownloadTask {
                url,
                path: temp_file_path.clone(),
                task_type: "arm64-native".to_string(),
            });
            extraction_tasks.push((temp_file_path, version_natives_dir.to_path_buf()));
        }
    }

    fn collect_asset_download_tasks(&self, asset_index_path: &Path, tasks: &mut Vec<DownloadTask>) {
        if let Ok(asset_index_content) = fs::read_to_string(asset_index_path) {
            if let Ok(assets_index) = serde_json::from_str::<AssetsIndex>(&asset_index_content) {
                for (_name, asset_object) in &assets_index.objects {
                    let hash = &asset_object.hash;
                    let first_two = &hash[..2];
                    let asset_path = self.assets_objects_dir.join(first_two).join(hash);

                    if !asset_path.exists() {
                        if let Some(parent) = asset_path.parent() {
                            let _ = fs::create_dir_all(parent);
                        }
                        let asset_url = format!("{}/{}/{}", ASSET_BASE_URL, first_two, hash);
                        tasks.push(DownloadTask {
                            url: asset_url,
                            path: asset_path,
                            task_type: "asset".to_string(),
                        });
                    }
                }
            }
        }
    }

    async fn download_file(client: &Client, url: &str, path: &Path) -> anyhow::Result<()> {
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
