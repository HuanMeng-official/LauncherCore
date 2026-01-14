use serde::{Deserialize, Serialize};

// Authentication models
#[derive(Debug, Serialize, Deserialize)]
pub struct AuthCache {
    pub access_token: String,
    pub uuid: String,
    pub username: String,
}

#[derive(Debug, Deserialize)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    #[allow(dead_code)]
    pub user_code: String,
    #[allow(dead_code)]
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct MicrosoftTokenResponse {
    #[allow(dead_code)]
    pub token_type: String,
    #[allow(dead_code)]
    pub expires_in: u64,
    #[allow(dead_code)]
    pub scope: String,
    pub access_token: String,
    #[allow(dead_code)]
    pub refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct XblResponse {
    #[serde(rename = "Token")]
    pub token: String,
    #[serde(rename = "DisplayClaims")]
    pub display_claims: XblDisplayClaims,
}

#[derive(Debug, Deserialize)]
pub struct XblDisplayClaims {
    pub xui: Vec<XblXui>,
}

#[derive(Debug, Deserialize)]
pub struct XblXui {
    pub uhs: String,
}

#[derive(Debug, Deserialize)]
pub struct XstsResponse {
    #[serde(rename = "Token")]
    pub token: String,
    #[serde(rename = "DisplayClaims")]
    pub display_claims: XstsDisplayClaims,
}

#[derive(Debug, Deserialize)]
pub struct XstsDisplayClaims {
    pub xui: Vec<XstsXui>,
}

#[derive(Debug, Deserialize)]
pub struct XstsXui {
    pub uhs: String,
    #[allow(dead_code)]
    pub xid: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct MinecraftLoginResponse {
    pub username: String,
    #[allow(dead_code)]
    pub roles: Vec<String>,
    pub access_token: String,
    #[allow(dead_code)]
    pub token_type: String,
    #[allow(dead_code)]
    pub expires_in: u64,
}

#[derive(Debug, Deserialize)]
pub struct MinecraftProfile {
    pub id: String,
    pub name: String,
}

// Version models
#[derive(Debug, Deserialize, Serialize)]
pub struct VersionManifest {
    pub versions: Vec<VersionInfo>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VersionInfo {
    pub id: String,
    #[serde(rename = "type")]
    pub version_type: String,
    pub url: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct VersionDetails {
    pub id: String,
    #[serde(rename = "type")]
    pub version_type: String,
    pub downloads: Option<Downloads>,
    pub libraries: Vec<Library>,
    #[serde(rename = "mainClass")]
    pub main_class: String,
    #[serde(rename = "minecraftArguments")]
    pub minecraft_arguments: Option<String>,
    pub arguments: Option<Arguments>,
    #[serde(rename = "assetIndex")]
    pub asset_index: Option<AssetIndex>,
    #[serde(rename = "javaVersion")]
    pub java_version: Option<JavaVersionSpec>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct JavaVersionSpec {
    #[serde(rename = "majorVersion")]
    pub major_version: u32,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AssetIndex {
    pub id: String,
    pub url: String,
    pub sha1: String,
    pub size: u64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Downloads {
    pub client: DownloadInfo,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DownloadInfo {
    pub url: String,
    pub sha1: String,
    pub size: u64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Library {
    pub name: String,
    pub downloads: Option<LibraryDownloads>,
    pub rules: Option<Vec<Rule>>,
    pub natives: Option<HashMap<String, String>>,
}

use std::collections::HashMap;

#[derive(Debug, Deserialize, Serialize)]
pub struct Rule {
    pub action: String,
    pub os: Option<OsRule>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct OsRule {
    pub name: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LibraryDownloads {
    pub artifact: Option<Artifact>,
    #[serde(rename = "classifiers")]
    pub classifiers: Option<Classifiers>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Artifact {
    pub url: String,
    pub sha1: String,
    pub path: String,
    pub size: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Classifiers {
    #[serde(rename = "natives-linux")]
    pub natives_linux: Option<Artifact>,
    #[serde(rename = "natives-windows")]
    pub natives_windows: Option<Artifact>,
    #[serde(rename = "natives-macos")]
    pub natives_macos: Option<Artifact>,
    #[serde(flatten)]
    pub other: HashMap<String, Artifact>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Arguments {
    pub game: Vec<serde_json::Value>,
    pub jvm: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AssetsIndex {
    pub objects: HashMap<String, AssetObject>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AssetObject {
    pub hash: String,
    pub size: u64,
}
