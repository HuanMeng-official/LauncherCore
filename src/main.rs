use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
enum AuthType {
    Offline,
    Msa,
}

#[derive(Parser)]
#[command(name = "mclc")]
#[command(override_usage = "mclc <COMMAND> <OPTIONS>")]
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
    #[command(about = "Minecraft Launcher Cli", long_about = "A Simple Minecraft Launcher")]
    Launch {
        version: String,
        #[arg(short, long)]
        username: Option<String>,
        #[arg(long)]
        access_token: Option<String>,
        #[arg(short, long)]
        jvm_args: Option<String>,
        #[arg(long, value_enum, default_value_t = AuthType::Offline)]
        auth_type: AuthType,
    },
    Login,
}

const MS_DEVICE_CODE_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";
const MS_TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";

const XBL_AUTH_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_AUTH_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MC_LOGIN_WITH_XBOX_URL: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";
const MC_PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";

#[derive(Debug, Serialize, Deserialize)]
struct AuthCache {
    access_token: String,
    uuid: String,
    username: String,
}

#[derive(Debug, Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: u64,
    message: String,
}

#[derive(Debug, Deserialize)]
struct MicrosoftTokenResponse {
    token_type: String,
    expires_in: u64,
    scope: String,
    access_token: String,
    refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct XblResponse {
    Token: String,
    DisplayClaims: XblDisplayClaims,
}

#[derive(Debug, Deserialize)]
struct XblDisplayClaims {
    xui: Vec<XblXui>,
}

#[derive(Debug, Deserialize)]
struct XblXui {
    uhs: String,
}

#[derive(Debug, Deserialize)]
struct XstsResponse {
    Token: String,
    DisplayClaims: XstsDisplayClaims,
}

#[derive(Debug, Deserialize)]
struct XstsDisplayClaims {
    xui: Vec<XstsXui>,
}

#[derive(Debug, Deserialize)]
struct XstsXui {
    uhs: String,
    xid: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MinecraftLoginResponse {
    username: String,
    roles: Vec<String>,
    access_token: String,
    token_type: String,
    expires_in: u64,
}

#[derive(Debug, Deserialize)]
struct MinecraftProfile {
    id: String,
    name: String,
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
    #[error("Authentication required but not found. Please run 'mclc login'.")]
    AuthNotFound,
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

    // 保存登录信息到文件（实际生产环境中避免明文，LauncherCore项目只是为了演示）
    fn get_auth_cache_path() -> Result<PathBuf> {
        let proj_dirs = dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?
            .join("mclc");
        if !proj_dirs.exists() {
            fs::create_dir_all(&proj_dirs)?;
        }
        Ok(proj_dirs.join("auth_cache.json"))
    }

    fn save_auth_cache(&self, cache: &AuthCache) -> Result<()> {
        let cache_path = Self::get_auth_cache_path()?;
        let json = serde_json::to_string_pretty(cache)?;
        fs::write(&cache_path, json)?;
        println!("Authentication information saved to {:?}", cache_path);
        Ok(())
    }

    fn load_auth_cache(&self) -> Result<Option<AuthCache>> {
        let cache_path = Self::get_auth_cache_path()?;
        if !cache_path.exists() {
            return Ok(None);
        }
        let json = fs::read_to_string(&cache_path)?;
        let cache: AuthCache = serde_json::from_str(&json)?;
        if cache.access_token.is_empty() || cache.uuid.is_empty() || cache.username.is_empty() {
            return Ok(None);
        }
        Ok(Some(cache))
    }

async fn authenticate_with_msa(&self) -> Result<MicrosoftTokenResponse> {
    // 请将下面这行的字符串替换为你自己的 Azure AD 应用的客户端 ID
    let client_id = "YOUR_AZURE_CLIENT_ID".to_string();
    let client = reqwest::Client::new();

    println!("Starting Microsoft Account login...");
    let params = [
        ("client_id", client_id.as_str()),
        ("scope", "XboxLive.signin offline_access"),
    ];
    let res = client.post(MS_DEVICE_CODE_URL).form(&params).send().await?;

    if !res.status().is_success() {
        let status = res.status();
        let body = res
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(anyhow::anyhow!(
            "Failed to get device code: {} - {}",
            status,
            body
        ));
    }

    let device_code_res: DeviceCodeResponse = res.json().await?;
    println!("{}", device_code_res.message);

    let poll_params = [
        ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ("device_code", &device_code_res.device_code),
        ("client_id", client_id.as_str()),
    ];

    let start_time = std::time::Instant::now();
    let timeout_duration = std::time::Duration::from_secs(device_code_res.expires_in);
    let poll_interval = std::time::Duration::from_secs(device_code_res.interval.max(1));

    loop {
        if start_time.elapsed() > timeout_duration {
            return Err(anyhow::anyhow!("Authentication timed out."));
        }

        tokio::time::sleep(poll_interval).await;

        let res = client.post(MS_TOKEN_URL).form(&poll_params).send().await?;

        if res.status().is_success() {
            let token_res: MicrosoftTokenResponse = res.json().await?;
            println!("Microsoft Account login successful!");
            return Ok(token_res);
        } else {
            let status = res.status();
            let error_body = res.text().await.unwrap_or_default();
            if status == reqwest::StatusCode::BAD_REQUEST {
                if let Ok(error_json) = serde_json::from_str::<serde_json::Value>(&error_body) {
                    if let Some(error_code) = error_json.get("error").and_then(|e| e.as_str()) {
                        match error_code {
                            "authorization_pending" => {
                                println!("Waiting for user to authorize...");
                                continue;
                            }
                            "slow_down" => {
                                println!("Server requested to slow down polling.");
                                continue;
                            }
                            "expired_token" => {
                                return Err(anyhow::anyhow!(
                                    "Authentication expired. Please try again."
                                ));
                            }
                            _ => {
                                return Err(anyhow::anyhow!(
                                    "Authentication failed with error '{}': {}",
                                    error_code,
                                    error_body
                                ));
                            }
                        }
                    }
                }
            }
            return Err(anyhow::anyhow!(
                "Failed to poll for token: {} - {}",
                status,
                error_body
            ));
        }
    }
}

    async fn get_xbl_token(&self, ms_token: &str) -> Result<(String, String)> {
        let client = reqwest::Client::new();
        let request_body = serde_json::json!({
            "Properties": {
                "AuthMethod": "RPS",
                "SiteName": "user.auth.xboxlive.com",
                "RpsTicket": format!("d={}", ms_token)
            },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT"
        });

        let res = client
            .post(XBL_AUTH_URL)
            .json(&request_body)
            .send()
            .await?;

        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Failed to get XBL token: {} - {}", status, body));
        }

        let xbl_res: XblResponse = res.json().await?;
        let token = xbl_res.Token;
        let user_hash = xbl_res.DisplayClaims.xui.first()
            .ok_or_else(|| anyhow::anyhow!("XBL response missing user hash"))?
            .uhs.clone();

        println!("XBL token acquired.");
        Ok((token, user_hash))
    }

    async fn get_xsts_token(&self, xbl_token: &str) -> Result<(String, String)> {
        let client = reqwest::Client::new();
        let request_body = serde_json::json!({
            "Properties": {
                "SandboxId": "RETAIL",
                "UserTokens": [xbl_token]
            },
            "RelyingParty": "rp://api.minecraftservices.com/",
            "TokenType": "JWT"
        });

        let res = client
            .post(XSTS_AUTH_URL)
            .json(&request_body)
            .send()
            .await?;

        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            if status == reqwest::StatusCode::FORBIDDEN {
                 if body.contains("2148916233") {
                     return Err(anyhow::anyhow!("The account doesn't have an Xbox account (2148916233)."));
                 } else if body.contains("2148916238") {
                     return Err(anyhow::anyhow!("The account is a child (2148916238) and cannot proceed unless the account is added to a Family by an adult."));
                 } else if body.contains("2148916235") {
                     return Err(anyhow::anyhow!("The account is from a country where Xbox Live is not available/banned (2148916235)."));
                 }
            }
            return Err(anyhow::anyhow!("Failed to get XSTS token: {} - {}", status, body));
        }

        let xsts_res: XstsResponse = res.json().await?;
        let token = xsts_res.Token;
        let user_hash = xsts_res.DisplayClaims.xui.first()
            .ok_or_else(|| anyhow::anyhow!("XSTS response missing user hash"))?
            .uhs.clone();

        println!("XSTS token acquired.");
        Ok((token, user_hash))
    }

    async fn login_to_minecraft(&self, xsts_token: &str, user_hash: &str) -> Result<MinecraftLoginResponse> {
         let client = reqwest::Client::new();
         let request_body = serde_json::json!({
             "identityToken": format!("XBL3.0 x={};{}", user_hash, xsts_token)
         });

         let res = client
             .post(MC_LOGIN_WITH_XBOX_URL)
             .json(&request_body)
             .send()
             .await?;

         if !res.status().is_success() {
             let status = res.status();
             let body = res.text().await.unwrap_or_default();
             return Err(anyhow::anyhow!("Failed to login to Minecraft: {} - {}", status, body));
         }

         let mc_login_res: MinecraftLoginResponse = res.json().await?;
         println!("Logged into Minecraft services as '{}'.", mc_login_res.username);
         Ok(mc_login_res)
    }

    async fn get_minecraft_profile(&self, mc_access_token: &str) -> Result<MinecraftProfile> {
         let client = reqwest::Client::new();

         let res = client
             .get(MC_PROFILE_URL)
             .header("Authorization", format!("Bearer {}", mc_access_token))
             .send()
             .await?;

         if !res.status().is_success() {
             let status = res.status();
             let body = res.text().await.unwrap_or_default();
             return Err(anyhow::anyhow!("Failed to get Minecraft profile: {} - {}", status, body));
         }

         let profile: MinecraftProfile = res.json().await?;
         println!("Retrieved Minecraft profile: {} ({})", profile.name, profile.id);
         Ok(profile)
    }

    async fn perform_full_authentication(&self) -> Result<AuthCache> {
        let ms_token_res = self.authenticate_with_msa().await?;
        let ms_access_token = ms_token_res.access_token;

        let (xbl_token, user_hash) = self.get_xbl_token(&ms_access_token).await?;
        let (xsts_token, _) = self.get_xsts_token(&xbl_token).await?;
        let mc_login_res = self.login_to_minecraft(&xsts_token, &user_hash).await?;
        let profile = self.get_minecraft_profile(&mc_login_res.access_token).await?;

        let formatted_uuid = format!(
            "{}-{}-{}-{}-{}",
            &profile.id[0..8],
            &profile.id[8..12],
            &profile.id[12..16],
            &profile.id[16..20],
            &profile.id[20..32]
        );

        Ok(AuthCache {
            access_token: mc_login_res.access_token,
            uuid: formatted_uuid,
            username: profile.name,
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

    async fn install_lwjgl_arm64_natives(&self, version_id: &str, version_details: &VersionDetails) -> Result<()> {
        if std::env::consts::OS != "linux" || std::env::consts::ARCH != "aarch64" {
            return Ok(());
        }
        println!("LWJGL ARM64: Checking for LWJGL libraries to replace with ARM64 versions...");
        let mut lwjgl_versions: HashMap<String, String> = HashMap::new();
        let mut lwjgl_modules_found: HashSet<String> = HashSet::new();
        let target_lwjgl_modules: HashSet<&str> = [
            "lwjgl", "lwjgl-glfw", "lwjgl-jemalloc", "lwjgl-openal",
            "lwjgl-opengl", "lwjgl-stb", "lwjgl-tinyfd", "lwjgl-freetype"
        ].iter().cloned().collect();
        for lib in &version_details.libraries {
            if lib.name.starts_with("org.lwjgl:") {
                let parts: Vec<&str> = lib.name.split(':').collect();
                if parts.len() == 3 {
                    let group_id = parts[0];
                    let artifact_id = parts[1];
                    let version = parts[2];
                    if target_lwjgl_modules.contains(artifact_id) {
                         lwjgl_versions.insert(artifact_id.to_string(), version.to_string());
                         lwjgl_modules_found.insert(artifact_id.to_string());
                    }
                }
            }
        }
        if lwjgl_versions.is_empty() {
             println!("LWJGL ARM64: No LWJGL libraries found in version manifest for {}, skipping ARM64 native install.", version_id);
             return Ok(());
        }
        println!("LWJGL ARM64: Found LWJGL modules for version {}: {:?}", version_id, lwjgl_versions);
        let os_name = "linux";
        let arch_name = "arm64";
        let classifier = format!("natives-{}-{}", os_name, arch_name);
        let client = reqwest::Client::new();
        let temp_dir = tempfile::tempdir()?;
        for (module_artifact_id, module_version) in &lwjgl_versions {
            let group_path = "org/lwjgl";
            let url = format!(
                "https://repo1.maven.org/maven2/{group_path}/{module}/{version}/{module}-{version}-{classifier}.jar",
                group_path = group_path,
                module = module_artifact_id,
                version = module_version,
                classifier = classifier
            );
            println!("LWJGL ARM64: Preparing to download {}", url);
            let temp_file_path = temp_dir.path().join(format!("{}-{}-{}.jar", module_artifact_id, module_version, classifier));
            println!("LWJGL ARM64: Downloading {}:{} ARM64 natives...", module_artifact_id, module_version);
            let download_result = self.download_file(&client, &url, &temp_file_path).await;
            if let Err(e) = download_result {
                let is_not_found = if let Some(reqwest_err) = e.downcast_ref::<reqwest::Error>() {
                    if let Some(status) = reqwest_err.status() {
                         status == reqwest::StatusCode::NOT_FOUND
                    } else {
                        false
                    }
                } else {
                    false
                };
                if is_not_found {
                    println!("LWJGL ARM64: Warning: ARM64 native library not found at {}. It seems this LWJGL version ({}) does not provide official ARM64 binaries. Skipping this module.", url, module_version);
                    continue;
                } else {
                    println!("LWJGL ARM64: Warning: Failed to download {} ({}). Error: {}. Skipping this module.", module_artifact_id, url, e);
                    continue;
                }
            }
            let version_natives_dir = self.versions_dir.join(version_id).join("natives");
            println!("LWJGL ARM64: Extracting {}:{} natives to {:?}", module_artifact_id, module_version, version_natives_dir);
            self.extract_lwjgl3_native_library(&temp_file_path, &version_natives_dir)?;
        }
        println!("LWJGL ARM64: Installation of ARM64 native libraries completed (where available) for version {}.", version_id);
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
        self.install_lwjgl_arm64_natives(version_id, &version_details).await?;
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
            let is_native_for_os_and_arch = classifier_name == &format!("natives-{}-{}", os_name, arch_name) ||
                                            (arch_name == "x64" && classifier_name == &format!("natives-{}", os_name));
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
            "windows" => {
                classifiers
                    .natives_windows
                    .as_ref()
                    .or_else(|| classifiers.other.get("natives-windows"))
            }
            "linux" => {
                classifiers
                    .natives_linux
                    .as_ref()
                    .or_else(|| classifiers.other.get("natives-linux"))
            }
            "macos" => {
                classifiers
                    .natives_macos
                    .as_ref()
                    .or_else(|| classifiers.other.get("natives-macos"))
            }
            _ => None,
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
        uuid: String,
        user_type: String,
        jvm_args: Option<String>,
        global_java_path_override: Option<String>,
    ) -> Result<()> {
        println!("Launching Minecraft version: {} for user: {}", version_id, username);
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
            uuid,
            "--userType".to_string(),
            user_type,
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
        Commands::Login => {
             match launcher.perform_full_authentication().await {
                 Ok(auth_cache) => {
                     launcher.save_auth_cache(&auth_cache)?;
                     println!("Login successful! You can now launch with '--auth-type msa'.");
                 }
                 Err(e) => {
                     eprintln!("Authentication failed: {}", e);
                     std::process::exit(1);
                 }
             }
        }
        Commands::Launch {
            version,
            username,
            access_token,
            jvm_args,
            auth_type,
        } => {
            match auth_type {
                AuthType::Offline => {
                    let launch_username = username.clone().unwrap_or_else(|| "Player".to_string());
                    let launch_access_token = access_token.clone().unwrap_or_else(|| "0".to_string());
                    let launch_uuid = "00000000-0000-0000-0000-000000000000".to_string();

                    launcher.launch_game(
                        version,
                        launch_username,
                        launch_access_token,
                        launch_uuid,
                        "legacy".to_string(),
                        jvm_args.clone(),
                        global_java_path.clone(),
                    )?;
                }
                AuthType::Msa => {
                    match launcher.load_auth_cache()? {
                        Some(auth_cache) => {
                            launcher.launch_game(
                                version,
                                auth_cache.username,
                                auth_cache.access_token,
                                auth_cache.uuid,
                                "msa".to_string(),
                                jvm_args.clone(),
                                global_java_path.clone(),
                            )?;
                        }
                        None => {
                            eprintln!("{}", LauncherError::AuthNotFound);
                            std::process::exit(1);
                        }
                    }
                }
            }
        }
    }
    Ok(())
}