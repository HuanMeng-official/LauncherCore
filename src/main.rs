use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Parser)]
#[command(name = "mclauncher-core.exe")]
#[command(override_usage = "mclauncher-core.exe [OPTIONS] <COMMAND>")]
#[command(disable_version_flag = true)]
#[command(about = "A simple Minecraft launcher core.")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
    #[arg(long = "runtime", short = 'r', value_name = "PATH", global = true)]
    java_runtime_path: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    List,
    Install {
        version: String,
    },
    #[command(about = "Minecraft Launcher", long_about = "A Simple Minecraft Launcher")]
    Launch {
        version: String,
        #[arg(short, long, default_value = "Player")]
        username: String,
        #[arg(long, default_value = "0")]
        access_token: String,
        #[arg(short, long)]
        jvm_args: Option<String>,
    },
}

#[derive(Debug, Deserialize, Serialize)]
struct VersionManifest {
    versions: Vec<VersionInfo>,
}

#[derive(Debug, Deserialize, Serialize)]
struct VersionInfo {
    id: String,
    #[serde(rename = "type")]
    version_type: String,
    url: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct VersionDetails {
    id: String,
    #[serde(rename = "type")]
    version_type: String,
    downloads: Option<Downloads>,
    libraries: Vec<Library>,
    #[serde(rename = "mainClass")]
    main_class: String,
    #[serde(rename = "minecraftArguments")]
    minecraft_arguments: Option<String>,
    arguments: Option<Arguments>,
    #[serde(rename = "assetIndex")]
    asset_index: Option<AssetIndex>,
    javaVersion: Option<JavaVersionSpec>,
}

#[derive(Debug, Deserialize, Serialize)]
struct JavaVersionSpec {
    majorVersion: u32,
}

#[derive(Debug, Deserialize, Serialize)]
struct AssetIndex {
    id: String,
    url: String,
    sha1: String,
    size: u64,
}

#[derive(Debug, Deserialize, Serialize)]
struct Downloads {
    client: DownloadInfo,
}

#[derive(Debug, Deserialize, Serialize)]
struct DownloadInfo {
    url: String,
    sha1: String,
    size: u64,
}

#[derive(Debug, Deserialize, Serialize)]
struct Library {
    name: String,
    downloads: Option<LibraryDownloads>,
    rules: Option<Vec<Rule>>,
    natives: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Rule {
    action: String,
    os: Option<OsRule>,
}

#[derive(Debug, Deserialize, Serialize)]
struct OsRule {
    name: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct LibraryDownloads {
    artifact: Option<Artifact>,
    #[serde(rename = "classifiers")]
    classifiers: Option<Classifiers>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Artifact {
    url: String,
    sha1: String,
    path: String,
    size: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Classifiers {
    #[serde(rename = "natives-linux")]
    natives_linux: Option<Artifact>,
    #[serde(rename = "natives-windows")]
    natives_windows: Option<Artifact>,
    #[serde(rename = "natives-macos")]
    natives_macos: Option<Artifact>,
    #[serde(flatten)]
    other: HashMap<String, Artifact>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Arguments {
    game: Vec<serde_json::Value>,
    jvm: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize, Serialize)]
struct AssetsIndex {
    objects: HashMap<String, AssetObject>,
}

#[derive(Debug, Deserialize, Serialize)]
struct AssetObject {
    hash: String,
    size: u64,
}

#[derive(thiserror::Error, Debug)]
pub enum LauncherError {
    #[error("Version {0} not found")]
    VersionNotFound(String),
    #[error("Java not found. Please set JAVA_HOME or use --runtime")]
    JavaNotFound,
}

struct MinecraftLauncher {
    minecraft_dir: PathBuf,
    versions_dir: PathBuf,
    libraries_dir: PathBuf,
    assets_dir: PathBuf,
    assets_objects_dir: PathBuf,
    assets_indexes_dir: PathBuf,
}

impl MinecraftLauncher {
    fn new() -> Result<Self> {
        let current_dir = std::env::current_dir()?;
        let minecraft_dir = current_dir.join(".minecraft");
        if !minecraft_dir.exists() {
            fs::create_dir_all(&minecraft_dir)?;
        }
        let versions_dir = minecraft_dir.join("versions");
        let libraries_dir = minecraft_dir.join("libraries");
        let assets_dir = minecraft_dir.join("assets");
        let assets_objects_dir = assets_dir.join("objects");
        let assets_indexes_dir = assets_dir.join("indexes");
        if !versions_dir.exists() {
            fs::create_dir_all(&versions_dir)?;
        }
        if !libraries_dir.exists() {
            fs::create_dir_all(&libraries_dir)?;
        }
        if !assets_dir.exists() {
            fs::create_dir_all(&assets_dir)?;
        }
        if !assets_objects_dir.exists() {
            fs::create_dir_all(&assets_objects_dir)?;
        }
        if !assets_indexes_dir.exists() {
            fs::create_dir_all(&assets_indexes_dir)?;
        }
        Ok(MinecraftLauncher {
            minecraft_dir,
            versions_dir,
            libraries_dir,
            assets_dir,
            assets_objects_dir,
            assets_indexes_dir,
        })
    }

    async fn list_versions(&self) -> Result<()> {
        println!("Fetching available Minecraft versions...");
        let client = reqwest::Client::new();
        let manifest_url = "https://launchermeta.mojang.com/mc/game/version_manifest.json";
        let manifest: VersionManifest = client
            .get(manifest_url)
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

    async fn install_version(&self, version_id: &str) -> Result<()> {
        println!("Installing Minecraft version: {}", version_id);
        let client = reqwest::Client::new();
        let manifest_url = "https://launchermeta.mojang.com/mc/game/version_manifest.json";
        let manifest: VersionManifest = client
            .get(manifest_url)
            .send()
            .await?
            .json()
            .await?;
        let version_info = manifest
            .versions
            .iter()
            .find(|v| v.id == version_id)
            .ok_or_else(|| LauncherError::VersionNotFound(version_id.to_string()))?;
        println!("Fetching version details...");
        let version_details: VersionDetails = client
            .get(&version_info.url)
            .send()
            .await?
            .json()
            .await?;
        let version_dir = self.versions_dir.join(version_id);
        if !version_dir.exists() {
            fs::create_dir_all(&version_dir)?;
        }
        if let Some(downloads) = &version_details.downloads {
            println!("Downloading client JAR...");
            self.download_file(
                &client,
                &downloads.client.url,
                &version_dir.join(format!("{}.jar", version_id)),
            )
            .await?;
        }
        let version_json_path = version_dir.join(format!("{}.json", version_id));
        let version_json_content = serde_json::to_string_pretty(&version_details)?;
        fs::write(&version_json_path, version_json_content)?;
        println!("Downloading libraries...");
        let version_natives_dir = version_dir.join("natives");
        if !version_natives_dir.exists() {
            fs::create_dir_all(&version_natives_dir)?;
        }

        for library in &version_details.libraries {
            let Some(downloads) = &library.downloads else {
                continue;
            };

            self.process_library_artifact(&client, downloads).await?;

            let Some(classifiers) = &downloads.classifiers else {
                continue;
            };

            self.process_native_artifact(&client, classifiers, &version_natives_dir)
                .await?;

            self.process_other_natives(&client, classifiers, &version_natives_dir)
                .await?;
        }

        if let Some(asset_index) = &version_details.asset_index {
            println!("Downloading asset index...");
            let asset_index_path = self.assets_indexes_dir.join(format!("{}.json", asset_index.id));
            self.download_file(&client, &asset_index.url, &asset_index_path)
                .await?;
            println!("Downloading assets...");
            self.download_assets(&client, &asset_index_path).await?;
        }
        println!("Version {} installed successfully!", version_id);
        Ok(())
    }

    async fn process_library_artifact(
        &self,
        client: &reqwest::Client,
        downloads: &LibraryDownloads,
    ) -> Result<()> {
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
        client: &reqwest::Client,
        classifiers: &Classifiers,
        version_natives_dir: &Path,
    ) -> Result<()> {
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
        client: &reqwest::Client,
        classifiers: &Classifiers,
        version_natives_dir: &Path,
    ) -> Result<()> {
        for (classifier_name, artifact) in &classifiers.other {
            let is_native_for_os = if cfg!(target_os = "windows") {
                classifier_name.contains("natives-windows")
            } else if cfg!(target_os = "linux") {
                classifier_name.contains("natives-linux")
            } else if cfg!(target_os = "macos") {
                classifier_name.contains("natives-macos")
            } else {
                false
            };

            if is_native_for_os {
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

    async fn download_assets(&self, client: &reqwest::Client, asset_index_path: &Path) -> Result<()> {
        let asset_index_content = fs::read_to_string(asset_index_path)?;
        let assets_index: AssetsIndex = serde_json::from_str(&asset_index_content)?;
        let total_assets = assets_index.objects.len();
        let mut downloaded = 0;
        for (name, asset_object) in &assets_index.objects {
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
                let asset_url = format!(
                    "https://resources.download.minecraft.net/{}/{}",
                    first_two, hash
                );
                self.download_file(client, &asset_url, &asset_path).await?;
            }
        }
        println!("Assets downloaded successfully!");
        Ok(())
    }

    fn get_native_artifact<'a>(&self, classifiers: &'a Classifiers) -> Option<&'a Artifact> {
        if cfg!(target_os = "windows") {
            classifiers
                .natives_windows
                .as_ref()
                .or_else(|| classifiers.other.get("natives-windows"))
        } else if cfg!(target_os = "linux") {
            classifiers
                .natives_linux
                .as_ref()
                .or_else(|| classifiers.other.get("natives-linux"))
        } else if cfg!(target_os = "macos") {
            classifiers
                .natives_macos
                .as_ref()
                .or_else(|| classifiers.other.get("natives-macos"))
        } else {
            None
        }
    }

    async fn download_file(&self, client: &reqwest::Client, url: &str, path: &Path) -> Result<()> {
        if path.exists() {
            return Ok(());
        }
        let response = client.get(url).send().await?;
        let bytes = response.bytes().await?;
        fs::write(path, bytes)?;
        Ok(())
    }

    fn extract_lwjgl3_native_library(&self, jar_path: &Path, extract_dir: &Path) -> Result<()> {
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
                        fs::create_dir_all(p)
                            .with_context(|| format!("Failed to create directory for native file: {:?}", p))?;
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

    fn launch_game(
        &self,
        version_id: &str,
        username: String,
        access_token: String,
        jvm_args: Option<String>,
        global_java_path_override: Option<String>,
    ) -> Result<()> {
        println!("Launching Minecraft version: {}", version_id);
        let version_dir = self.versions_dir.join(version_id);
        if !version_dir.exists() {
            return Err(LauncherError::VersionNotFound(version_id.to_string()).into());
        }
        let version_json_path = version_dir.join(format!("{}.json", version_id));
        let version_json = fs::read_to_string(&version_json_path)?;
        let version_details: VersionDetails = serde_json::from_str(&version_json)?;

        let is_version_1_16_5 = version_id == "1.16.5";
        if is_version_1_16_5 {
            println!("Detected version 1.16.5. Filtering out LWJGL 3.2.1 libraries.");
        }

        let java_path = if let Some(override_path) = global_java_path_override {
            println!("Using explicitly provided global Java path: {}", override_path);
            PathBuf::from(override_path)
        } else {
            self.find_java_from_env()?
        };
        println!("Using Java: {:?}", java_path);

        let mut classpath = Vec::new();
        classpath.push(version_dir.join(format!("{}.jar", version_id)));

        for library in &version_details.libraries {
            if is_version_1_16_5 {
                if library.name.starts_with("org.lwjgl:") {
                    let parts: Vec<&str> = library.name.split(':').collect();
                    if parts.len() == 3 {
                        let library_version = parts[2];
                        if library_version == "3.2.1" {
                            println!("  Skipping LWJGL 3.2.1 library: {}", library.name);
                            continue;
                        }
                    }
                }
            }

            if let Some(downloads) = &library.downloads {
                if let Some(artifact) = &downloads.artifact {
                    let library_path = self.libraries_dir.join(&artifact.path);
                    if library_path.exists() {
                        classpath.push(library_path);
                    }
                }
            }
        }

        let classpath_str = classpath
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join(if cfg!(windows) { ";" } else { ":" });

        let version_natives_dir = version_dir.join("natives");
        let mut jvm_arguments = vec![
            "-Xmx2G".to_string(),
            "-Xms1G".to_string(),
            "-XX:+UnlockExperimentalVMOptions".to_string(),
            "-XX:+UnlockDiagnosticVMOptions".to_string(),
            "-XX:+UseG1GC".to_string(),
            "-XX:G1MixedGCCountTarget=5".to_string(),
            "-XX:G1NewSizePercent=20".to_string(),
            "-XX:G1ReservePercent=20".to_string(),
            "-XX:MaxGCPauseMillis=50".to_string(),
            "-XX:G1HeapRegionSize=32m".to_string(),
            "-XX:-OmitStackTraceInFastThrow".to_string(),
            "-XX:-DontCompileHugeMethods".to_string(),
            "-XX:MaxNodeLimit=240000".to_string(),
            "-XX:NodeLimitFudgeFactor=8000".to_string(),
            "-XX:TieredCompileTaskTimeout=10000".to_string(),
            "-XX:ReservedCodeCacheSize=400M".to_string(),
            "-XX:NmethodSweepActivity=1".to_string(),
            "-Djava.library.path=".to_string() + &version_natives_dir.to_string_lossy(),
        ];
        if let Some(custom_jvm_args) = jvm_args {
            jvm_arguments.extend(custom_jvm_args.split_whitespace().map(|s| s.to_string()));
        }
        jvm_arguments.push("-cp".to_string());
        jvm_arguments.push(classpath_str);
        jvm_arguments.push(version_details.main_class);

        let client_id = "0";
        let asset_index_id = if let Some(asset_index) = &version_details.asset_index {
            asset_index.id.clone()
        } else {
            version_id.to_string()
        };
        let game_args = vec![
            "--username".to_string(),
            username,
            "--version".to_string(),
            version_id.to_string(),
            "--gameDir".to_string(),
            self.minecraft_dir.to_string_lossy().to_string(),
            "--assetsDir".to_string(),
            self.assets_dir.to_string_lossy().to_string(),
            "--assetIndex".to_string(),
            asset_index_id,
            "--accessToken".to_string(),
            access_token,
            "--clientId".to_string(),
            client_id.to_string(),
            "--uuid".to_string(),
            "00000000-0000-0000-0000-000000000000".to_string(),
            "--userType".to_string(),
            "legacy".to_string(),
            "--userProperties".to_string(),
            "{}".to_string(),
        ];

        let mut command_args = jvm_arguments;
        command_args.extend(game_args);
        println!("Launching with command: {} {:?}", java_path.display(), command_args);

        let status = Command::new(&java_path)
            .args(&command_args)
            .status()
            .context("Failed to start Minecraft")?;
        if status.success() {
            println!("Minecraft exited successfully");
        } else {
            eprintln!("Minecraft exited with error code: {:?}", status.code());
        }
        Ok(())
    }

    fn find_java_from_env(&self) -> Result<PathBuf, LauncherError> {
        if let Ok(java_home) = std::env::var("JAVA_HOME") {
            let java_home_path = PathBuf::from(java_home);
            let java_bin = java_home_path.join("bin").join(if cfg!(target_os = "windows") {
                "java.exe"
            } else {
                "java"
            });
            if java_bin.exists() {
                println!("Found Java in JAVA_HOME: {:?}", java_home_path);
                return Ok(java_bin);
            } else {
                eprintln!(
                    "JAVA_HOME is set to '{:?}' but Java executable not found at '{:?}'",
                    java_home_path, java_bin
                );
            }
        } else {
            eprintln!("JAVA_HOME environment variable is not set.");
        }
        Err(LauncherError::JavaNotFound)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let launcher = MinecraftLauncher::new()?;
    let global_java_path = cli.java_runtime_path;
    match &cli.command {
        Commands::List => {
            launcher.list_versions().await?;
        }
        Commands::Install { version } => {
            launcher.install_version(version).await?;
        }
        Commands::Launch {
            version,
            username,
            access_token,
            jvm_args,
        } => {
            launcher.launch_game(
                version,
                username.clone(),
                access_token.clone(),
                jvm_args.clone(),
                global_java_path.clone(),
            )?;
        }
    }
    Ok(())
}