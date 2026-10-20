use crate::models::*;
use anyhow::Context;
use futures_util::stream::{self, StreamExt};
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use sha1::{Digest, Sha1};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, atomic::{AtomicU64, Ordering}};

const VERSION_MANIFEST_URL: &str = "https://launchermeta.mojang.com/mc/game/version_manifest.json";
const ASSET_BASE_URL: &str = "https://resources.download.minecraft.net";
const MAVEN_BASE_URL: &str = "https://repo1.maven.org/maven2";
const MAX_CONCURRENT_DOWNLOADS: usize = 16;

fn manifest_has_linux_arm64_native(library: &crate::models::Library) -> bool {
    library
        .downloads
        .as_ref()
        .and_then(|downloads| downloads.classifiers.as_ref())
        .map(|classifiers| {
            classifiers.get("natives-linux-arm64").is_some()
                || classifiers.get("natives-linux-aarch64").is_some()
        })
        .unwrap_or(false)
}

fn lwjgl_native_url(module: &str, version: &str, classifier: &str) -> String {
    format!(
        "{}/org/lwjgl/{}/{}/{}-{}-{}.jar",
        MAVEN_BASE_URL, module, version, module, version, classifier
    )
}

fn ensure_downloads_succeeded(failures: Vec<String>) -> anyhow::Result<()> {
    if failures.is_empty() {
        return Ok(());
    }

    anyhow::bail!(
        "{} file(s) failed to download:\n{}",
        failures.len(),
        failures.join("\n")
    )
}

#[derive(Debug, Clone)]
struct DownloadTask {
    url: String,
    path: std::path::PathBuf,
    task_type: String,
    sha1: Option<String>,
    size: Option<u64>,
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
        let mut asset_index_sha1 = None;
        let mut asset_index_size = None;

        // Client JAR
        if let Some(downloads) = &version_details.downloads {
            let client_jar_path = version_dir.join(format!("{}.jar", version_id));
            if !client_jar_path.exists() {
                tasks.push(DownloadTask {
                    url: downloads.client.url.clone(),
                    path: client_jar_path,
                    task_type: "client".to_string(),
                    sha1: Some(downloads.client.sha1.clone()),
                    size: Some(downloads.client.size),
                });
            }
        }

        // Asset index URL (always need to check this for assets)
        if let Some(asset_index) = &version_details.asset_index {
            let index_path = self.assets_indexes_dir.join(format!("{}.json", asset_index.id));
            asset_index_url = Some(asset_index.url.clone());
            asset_index_path = Some(index_path.clone());
            asset_index_sha1 = Some(asset_index.sha1.clone());
            asset_index_size = Some(asset_index.size);
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
                        sha1: asset_index_sha1.clone(),
                        size: asset_index_size,
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
                    let sha1 = task.sha1.clone();
                    let size = task.size;
                    tokio::spawn(async move {
                        (
                            task_type,
                            Self::download_file(&client, &url, &path, sha1.as_deref(), size)
                                .await,
                        )
                    })
                })
                .buffer_unordered(MAX_CONCURRENT_DOWNLOADS)
                .collect::<Vec<_>>()
                .await;

            let mut failures = Vec::new();
            for result in results {
                match result {
                    Ok((_, Ok(()))) => {}
                    Ok((task_type, Err(e))) => failures.push(format!("{}: {}", task_type, e)),
                    Err(join_error) => {
                        failures.push(format!("download task failed: {}", join_error))
                    }
                }
            }
            ensure_downloads_succeeded(failures)?;
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
                self.collect_asset_download_tasks(index_path, &mut tasks)?;
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
                    let result = Self::download_file(
                        &client,
                        &task.url,
                        &task.path,
                        task.sha1.as_deref(),
                        task.size,
                    )
                    .await;
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
        let mut failures = Vec::new();
        for result in download_results {
            match result {
                Ok((_, Ok(()))) => {}
                Ok((task, Err(e))) => {
                    let message = format!("{} ({}): {}", task.task_type, task.url, e);
                    pb.println(format!("Failed to download {}", message));
                    failures.push(message);
                }
                Err(join_error) => {
                    let message = format!("download task failed: {}", join_error);
                    pb.println(format!("Failed to download {}", message));
                    failures.push(message);
                }
            }
        }
        ensure_downloads_succeeded(failures)?;

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
            if !library.is_allowed_for_current_platform() {
                continue;
            }

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
                        sha1: Some(artifact.sha1.clone()),
                        size: artifact.size,
                    });
                }
            }

            if let Some(artifact) =
                library.native_artifact_for(std::env::consts::OS, std::env::consts::ARCH)
            {
                let native_path = self.libraries_dir.join(&artifact.path);
                if !native_path.exists() {
                    if let Some(parent) = native_path.parent() {
                        let _ = fs::create_dir_all(parent);
                    }
                    tasks.push(DownloadTask {
                        url: artifact.url.clone(),
                        path: native_path.clone(),
                        task_type: "native".to_string(),
                        sha1: Some(artifact.sha1.clone()),
                        size: artifact.size,
                    });
                }
                extraction_tasks.push((native_path, version_natives_dir.to_path_buf()));
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
            if !lib.is_allowed_for_current_platform() {
                continue;
            }

            // The version manifest already provides an arm64 native for this library.
            if manifest_has_linux_arm64_native(lib) {
                continue;
            }

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
            let classifier = "natives-linux-arm64";
            let url = lwjgl_native_url(module_artifact_id, module_version, classifier);

            let temp_file_path = std::env::temp_dir().join(format!(
                "mclc-{}-{}-{}.jar",
                module_artifact_id, module_version, classifier
            ));

            tasks.push(DownloadTask {
                url,
                path: temp_file_path.clone(),
                task_type: "arm64-native".to_string(),
                sha1: None,
                size: None,
            });
            extraction_tasks.push((temp_file_path, version_natives_dir.to_path_buf()));
        }
    }

    fn collect_asset_download_tasks(
        &self,
        asset_index_path: &Path,
        tasks: &mut Vec<DownloadTask>,
    ) -> anyhow::Result<()> {
        let asset_index_content = fs::read_to_string(asset_index_path)
            .with_context(|| format!("Failed to read asset index: {:?}", asset_index_path))?;
        let assets_index: AssetsIndex = serde_json::from_str(&asset_index_content)
            .with_context(|| format!("Failed to parse asset index: {:?}", asset_index_path))?;

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
                    sha1: Some(hash.clone()),
                    size: Some(asset_object.size),
                });
            }
        }

        Ok(())
    }

    async fn download_file(
        client: &Client,
        url: &str,
        path: &Path,
        expected_sha1: Option<&str>,
        expected_size: Option<u64>,
    ) -> anyhow::Result<()> {
        if path.exists() {
            let size_matches = expected_size
                .map(|expected| {
                    fs::metadata(path)
                        .map(|metadata| metadata.len() == expected)
                        .unwrap_or(false)
                })
                .unwrap_or(true);
            if size_matches {
                return Ok(());
            }
        }

        let response = client
            .get(url)
            .send()
            .await?
            .error_for_status()
            .with_context(|| format!("Failed to download {}", url))?;

        if let (Some(expected), Some(actual)) = (expected_size, response.content_length()) {
            if expected != actual {
                anyhow::bail!(
                    "Unexpected content length for {}: expected {} bytes, got {}",
                    url,
                    expected,
                    actual
                );
            }
        }

        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Invalid download path: {:?}", path))?;
        fs::create_dir_all(parent)?;

        let mut temp = tempfile::NamedTempFile::new_in(parent)
            .with_context(|| format!("Failed to create temporary file in {:?}", parent))?;

        let mut hasher = Sha1::new();
        let mut downloaded = 0u64;
        let mut stream = response.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("Failed to read chunk")?;
            temp.write_all(&chunk).context("Failed to write chunk")?;
            hasher.update(&chunk);
            downloaded += chunk.len() as u64;
        }
        temp.flush().context("Failed to flush downloaded data")?;

        if let Some(expected) = expected_size {
            if downloaded != expected {
                anyhow::bail!(
                    "Size mismatch for {}: expected {} bytes, got {}",
                    url,
                    expected,
                    downloaded
                );
            }
        }

        if let Some(expected) = expected_sha1 {
            let actual: String = hasher
                .finalize()
                .iter()
                .map(|byte| format!("{:02x}", byte))
                .collect();
            if !actual.eq_ignore_ascii_case(expected) {
                anyhow::bail!(
                    "SHA-1 mismatch for {}: expected {}, got {}",
                    url,
                    expected,
                    actual
                );
            }
        }

        temp.persist(path)
            .map_err(|e| anyhow::anyhow!("Failed to save downloaded file {:?}: {}", path, e))?;

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
            let outpath = extract_dir.join(
                file.mangled_name()
                    .with_context(|| format!("Failed to get entry name {} from archive: {:?}", i, jar_path))?,
            );
            let file_name = file
                .name()
                .with_context(|| format!("Failed to get entry name {} from archive: {:?}", i, jar_path))?
                .to_lowercase();

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::VersionDetails;
    use crate::test_http;
    use sha1::{Digest, Sha1};

    fn sha1_hex(data: &[u8]) -> String {
        Sha1::digest(data)
            .iter()
            .map(|byte| format!("{:02x}", byte))
            .collect()
    }

    fn test_installer(dir: &Path) -> Installer {
        Installer {
            versions_dir: dir.join("versions"),
            libraries_dir: dir.join("libraries"),
            assets_objects_dir: dir.join("assets/objects"),
            assets_indexes_dir: dir.join("assets/indexes"),
        }
    }

    #[tokio::test]
    async fn download_rejects_http_error_and_leaves_no_file() {
        let server = test_http::serve_once(test_http::response(
            "404 Not Found",
            &[],
            b"not found",
        ))
        .await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("client.jar");
        let client = Client::new();

        let result = Installer::download_file(
            &client,
            &format!("{}/client.jar", server.base_url),
            &path,
            None,
            None,
        )
        .await;

        assert!(result.is_err(), "HTTP 404 must not be treated as success");
        assert!(!path.exists(), "error body must not be written to the final path");
    }

    #[tokio::test]
    async fn download_rejects_checksum_mismatch_and_leaves_no_file() {
        let body = b"hello";
        let server =
            test_http::serve_once(test_http::response("200 OK", &[], body)).await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.jar");
        let client = Client::new();

        let result = Installer::download_file(
            &client,
            &format!("{}/library.jar", server.base_url),
            &path,
            Some("0000000000000000000000000000000000000000"),
            Some(body.len() as u64),
        )
        .await;

        assert!(result.is_err(), "checksum mismatch must fail the download");
        assert!(!path.exists(), "corrupt download must not be persisted");
    }

    #[tokio::test]
    async fn download_writes_verified_file() {
        let body = b"hello";
        let expected_sha1 = sha1_hex(body);
        let server =
            test_http::serve_once(test_http::response("200 OK", &[], body)).await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("asset.bin");
        let client = Client::new();

        Installer::download_file(
            &client,
            &format!("{}/asset.bin", server.base_url),
            &path,
            Some(&expected_sha1),
            Some(body.len() as u64),
        )
        .await
        .unwrap();

        assert_eq!(fs::read(&path).unwrap(), body);
    }

    #[tokio::test]
    async fn download_replaces_truncated_existing_file() {
        let body = b"hello";
        let expected_sha1 = sha1_hex(body);
        let server =
            test_http::serve_once(test_http::response("200 OK", &[], body)).await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("asset.bin");
        fs::write(&path, b"hel").unwrap();

        Installer::download_file(
            &Client::new(),
            &format!("{}/asset.bin", server.base_url),
            &path,
            Some(&expected_sha1),
            Some(body.len() as u64),
        )
        .await
        .unwrap();

        assert_eq!(
            fs::read(&path).unwrap(),
            body,
            "a file with the wrong size must be downloaded again"
        );
    }

    #[test]
    fn invalid_asset_index_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let installer = test_installer(dir.path());
        let index_path = dir.path().join("broken.json");
        fs::write(&index_path, "{ not valid json").unwrap();
        let mut tasks = Vec::new();

        let result = installer.collect_asset_download_tasks(&index_path, &mut tasks);

        assert!(result.is_err());
    }

    #[test]
    fn arm64_workaround_only_skips_when_manifest_has_arm64_native() {
        let with_arm64: crate::models::Library = serde_json::from_str(
            r#"{
                "name": "org.lwjgl:lwjgl:3.3.3",
                "downloads": {
                    "classifiers": {
                        "natives-linux": {
                            "url": "https://example.invalid/linux.jar",
                            "sha1": "0000000000000000000000000000000000000000",
                            "path": "natives/linux.jar"
                        },
                        "natives-linux-arm64": {
                            "url": "https://example.invalid/linux-arm64.jar",
                            "sha1": "0000000000000000000000000000000000000000",
                            "path": "natives/linux-arm64.jar"
                        }
                    }
                }
            }"#,
        )
        .unwrap();
        let without_arm64: crate::models::Library = serde_json::from_str(
            r#"{
                "name": "org.lwjgl:lwjgl:3.3.3",
                "downloads": {
                    "classifiers": {
                        "natives-linux": {
                            "url": "https://example.invalid/linux.jar",
                            "sha1": "0000000000000000000000000000000000000000",
                            "path": "natives/linux.jar"
                        }
                    }
                }
            }"#,
        )
        .unwrap();

        assert!(manifest_has_linux_arm64_native(&with_arm64));
        assert!(!manifest_has_linux_arm64_native(&without_arm64));
    }

    #[test]
    fn lwjgl_arm64_url_uses_maven_layout() {
        assert_eq!(
            lwjgl_native_url("lwjgl", "3.3.3", "natives-linux-arm64"),
            "https://repo1.maven.org/maven2/org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3-natives-linux-arm64.jar"
        );
    }

    #[test]
    fn failed_downloads_are_reported_as_errors() {
        let error = ensure_downloads_succeeded(vec![
            "library https://example.invalid/lib.jar: 404".to_string(),
        ])
        .unwrap_err();

        assert!(error.to_string().contains("https://example.invalid/lib.jar"));
        assert!(ensure_downloads_succeeded(Vec::new()).is_ok());
    }

    #[test]
    fn install_filters_disallowed_libraries_and_foreign_natives() {
        let json = r#"{
            "id": "test",
            "type": "release",
            "mainClass": "net.minecraft.client.main.Main",
            "libraries": [
                {
                    "name": "test:allowed:1.0",
                    "downloads": {
                        "artifact": {
                            "url": "https://example.invalid/allowed.jar",
                            "sha1": "0000000000000000000000000000000000000000",
                            "path": "allowed.jar",
                            "size": 1
                        },
                        "classifiers": {
                            "natives-windows": {
                                "url": "https://example.invalid/natives-windows.jar",
                                "sha1": "0000000000000000000000000000000000000000",
                                "path": "natives/windows.jar",
                                "size": 1
                            },
                            "natives-linux": {
                                "url": "https://example.invalid/natives-linux.jar",
                                "sha1": "0000000000000000000000000000000000000000",
                                "path": "natives/linux.jar",
                                "size": 1
                            },
                            "natives-macos": {
                                "url": "https://example.invalid/natives-macos.jar",
                                "sha1": "0000000000000000000000000000000000000000",
                                "path": "natives/macos.jar",
                                "size": 1
                            },
                            "natives-windows-arm64": {
                                "url": "https://example.invalid/natives-windows-arm64.jar",
                                "sha1": "0000000000000000000000000000000000000000",
                                "path": "natives/windows-arm64.jar",
                                "size": 1
                            },
                            "natives-linux-arm64": {
                                "url": "https://example.invalid/natives-linux-arm64.jar",
                                "sha1": "0000000000000000000000000000000000000000",
                                "path": "natives/linux-arm64.jar",
                                "size": 1
                            },
                            "natives-macos-arm64": {
                                "url": "https://example.invalid/natives-macos-arm64.jar",
                                "sha1": "0000000000000000000000000000000000000000",
                                "path": "natives/macos-arm64.jar",
                                "size": 1
                            }
                        }
                    }
                },
                {
                    "name": "test:foreign:1.0",
                    "rules": [
                        { "action": "allow", "os": { "name": "linux" } }
                    ],
                    "downloads": {
                        "artifact": {
                            "url": "https://example.invalid/foreign.jar",
                            "sha1": "0000000000000000000000000000000000000000",
                            "path": "foreign.jar",
                            "size": 1
                        }
                    }
                }
            ]
        }"#;
        let details: VersionDetails = serde_json::from_str(json).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let installer = test_installer(dir.path());
        let mut tasks = Vec::new();
        let mut extraction_tasks = Vec::new();

        installer.collect_library_download_tasks(
            &details,
            "test",
            dir.path(),
            &mut tasks,
            &mut extraction_tasks,
        );

        let expected_native = details.libraries[0]
            .native_artifact_for(std::env::consts::OS, std::env::consts::ARCH)
            .expect("current platform should have a native artifact");

        assert_eq!(
            extraction_tasks.len(),
            1,
            "only the current platform's native may be extracted"
        );
        assert_eq!(
            extraction_tasks[0].0,
            installer.libraries_dir.join(&expected_native.path)
        );
        assert_eq!(
            tasks.iter().filter(|task| task.task_type == "native").count(),
            1,
            "only the current platform's native may be downloaded"
        );

        let expected_library_tasks = 1 + usize::from(cfg!(target_os = "linux"));
        assert_eq!(
            tasks.iter().filter(|task| task.task_type == "library").count(),
            expected_library_tasks,
            "libraries disallowed by rules must not be downloaded"
        );
    }
}
