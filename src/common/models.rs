use serde::{Deserialize, Serialize};

// Authentication models
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthCache {
    pub access_token: String,
    pub uuid: String,
    pub username: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_at: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
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
    #[serde(default)]
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
    #[serde(rename = "inheritsFrom", default)]
    pub inherits_from: Option<String>,
    #[serde(default)]
    pub jar: Option<String>,
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
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
}

impl Library {
    /// Returns the library artifact, supporting both Mojang's `downloads`
    /// layout and the Maven-coordinate layout used by Fabric/Quilt.
    pub fn artifact(&self) -> Option<Artifact> {
        if let Some(artifact) = self
            .downloads
            .as_ref()
            .and_then(|downloads| downloads.artifact.clone())
        {
            return Some(artifact);
        }

        let base_url = self.url.as_ref()?;
        let path = maven_path(&self.name)?;
        Some(Artifact {
            url: format!("{}/{}", base_url.trim_end_matches('/'), path),
            sha1: self.sha1.clone().unwrap_or_default(),
            path,
            size: self.size,
        })
    }
}

/// Selects the best native artifact for the given platform, supporting both
/// Mojang's nested `downloads.classifiers` format and the newer format where
/// each native is a separate `group:artifact:version:natives-...` library.
pub(crate) fn selected_native_artifacts(
    libraries: &[Library],
    os: &str,
    arch: &str,
) -> Vec<Artifact> {
    let mut selected: HashMap<String, (u8, Artifact)> = HashMap::new();

    for library in libraries {
        if !library.is_allowed_for(os, arch) {
            continue;
        }

        if let Some(artifact) = library.native_artifact_for(os, arch) {
            selected
                .entry(library.name.clone())
                .or_insert_with(|| (2, artifact.clone()));
        }

        let Some(score) = native_classifier_score(&library.name, os, arch) else {
            continue;
        };
        let Some(artifact) = library.artifact() else {
            continue;
        };

        let module = library
            .name
            .split(':')
            .take(3)
            .collect::<Vec<_>>()
            .join(":");
        match selected.get(&module) {
            Some((best_score, _)) if *best_score >= score => {}
            _ => {
                selected.insert(module, (score, artifact));
            }
        }
    }

    selected
        .into_values()
        .map(|(_, artifact)| artifact)
        .collect()
}

fn native_classifier_score(name: &str, os: &str, arch: &str) -> Option<u8> {
    let classifier = name.split(':').nth(3)?;
    let suffix = classifier.strip_prefix("natives-")?;

    let os_token = match os {
        "windows" => "windows",
        "linux" => "linux",
        "macos" if suffix == "osx" || suffix.starts_with("osx-") => "osx",
        "macos" => "macos",
        _ => return None,
    };

    let rest = suffix.strip_prefix(os_token)?;
    if rest.is_empty() {
        return Some(1);
    }

    let arch_token = rest.strip_prefix('-')?;
    if arch_classifier_names(arch)
        .iter()
        .any(|candidate| *candidate == arch_token)
    {
        Some(2)
    } else {
        None
    }
}

fn maven_path(name: &str) -> Option<String> {
    let parts: Vec<&str> = name.split(':').collect();
    if parts.len() < 3 {
        return None;
    }

    let group = parts[0].replace('.', "/");
    let artifact = parts[1];
    let version = parts[2];

    let (classifier, extension) = match parts.get(3) {
        Some(extra) => match extra.split_once('@') {
            Some((classifier, extension)) => (Some(classifier), extension),
            None => (Some(*extra), "jar"),
        },
        None => (None, "jar"),
    };

    let file_name = match classifier {
        Some(classifier) => format!("{}-{}-{}.{}", artifact, version, classifier, extension),
        None => format!("{}-{}.{}", artifact, version, extension),
    };

    Some(format!("{}/{}/{}/{}", group, artifact, version, file_name))
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
    #[serde(default)]
    pub arch: Option<String>,
}

impl Rule {
    fn matches(&self, os: &str, arch: &str) -> bool {
        let Some(os_rule) = &self.os else {
            return true;
        };

        let name_matches = match (os_rule.name.as_str(), os) {
            ("osx", "macos") | ("macos", "macos") => true,
            (rule_name, actual_os) => rule_name == actual_os,
        };

        let arch_matches = match &os_rule.arch {
            None => true,
            Some(rule_arch) => rule_arch == normalize_arch(arch),
        };

        name_matches && arch_matches
    }
}

pub(crate) fn normalize_arch(arch: &str) -> &str {
    match arch {
        "amd64" | "x64" => "x86_64",
        "i386" | "i586" | "i686" | "32" => "x86",
        "aarch64" => "arm64",
        other => other,
    }
}

fn arch_classifier_names(arch: &str) -> Vec<&str> {
    match normalize_arch(arch) {
        "x86_64" => vec!["x86_64", "x64", "64"],
        "x86" => vec!["x86", "i386", "32"],
        "arm64" => vec!["arm64", "aarch64"],
        other => vec![other],
    }
}

fn arch_bits(arch: &str) -> &str {
    match normalize_arch(arch) {
        "x86_64" => "64",
        "x86" => "32",
        other => other,
    }
}

impl Library {
    pub fn is_allowed_for(&self, os: &str, arch: &str) -> bool {
        let Some(rules) = &self.rules else {
            return true;
        };

        let mut allowed = false;
        for rule in rules {
            let matches = rule.matches(os, arch);
            if rule.action == "allow" && matches {
                allowed = true;
            } else if rule.action == "disallow" && matches {
                return false;
            }
        }
        allowed
    }

    pub fn is_allowed_for_current_platform(&self) -> bool {
        self.is_allowed_for(std::env::consts::OS, std::env::consts::ARCH)
    }

    pub fn native_artifact_for(&self, os: &str, arch: &str) -> Option<&Artifact> {
        let classifiers = self.downloads.as_ref()?.classifiers.as_ref()?;

        if let Some(natives) = &self.natives {
            let os_key = if os == "macos" { "osx" } else { os };
            if let Some(classifier_name) = natives.get(os_key) {
                let name = classifier_name.replace("${arch}", arch_bits(arch));
                if let Some(artifact) = classifiers.get(&name) {
                    return Some(artifact);
                }
            }
        }

        classifiers.native_for(os, arch)
    }
}

impl Classifiers {
    pub fn get(&self, name: &str) -> Option<&Artifact> {
        match name {
            "natives-linux" => self.natives_linux.as_ref(),
            "natives-windows" => self.natives_windows.as_ref(),
            "natives-macos" => self.natives_macos.as_ref(),
            other => self.other.get(other),
        }
    }

    pub fn native_for(&self, os: &str, arch: &str) -> Option<&Artifact> {
        let os_names: &[&str] = match os {
            "windows" => &["windows"],
            "linux" => &["linux"],
            "macos" => &["macos", "osx"],
            _ => return None,
        };

        for os_name in os_names {
            for arch_name in arch_classifier_names(arch) {
                if let Some(artifact) = self.get(&format!("natives-{}-{}", os_name, arch_name)) {
                    return Some(artifact);
                }
            }
        }

        for os_name in os_names {
            if let Some(artifact) = self.get(&format!("natives-{}", os_name)) {
                return Some(artifact);
            }
        }

        None
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LibraryDownloads {
    pub artifact: Option<Artifact>,
    #[serde(rename = "classifiers")]
    pub classifiers: Option<Classifiers>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
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
    #[serde(default)]
    pub game: Vec<serde_json::Value>,
    #[serde(default)]
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

// Yggdrasil API models
#[derive(Debug, Deserialize, Serialize)]
pub struct YggdrasilApiMetadata {
    #[serde(rename = "meta")]
    pub meta: Option<YggdrasilMeta>,
    #[serde(rename = "skinDomains")]
    pub skin_domains: Vec<String>,
    #[serde(rename = "signaturePublickey")]
    pub signature_public_key: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct YggdrasilMeta {
    #[serde(rename = "serverName")]
    pub server_name: Option<String>,
    #[serde(rename = "implementationName")]
    pub implementation_name: Option<String>,
    #[serde(rename = "implementationVersion")]
    pub implementation_version: Option<String>,
    pub links: Option<YggdrasilLinks>,
    #[serde(rename = "feature.non_email_login")]
    pub feature_non_email_login: Option<bool>,
    #[serde(rename = "feature.legacy_skin_api")]
    pub feature_legacy_skin_api: Option<bool>,
    #[serde(rename = "feature.no_mojang_namespace")]
    pub feature_no_mojang_namespace: Option<bool>,
    #[serde(rename = "feature.enable_mojang_anti_features")]
    pub feature_enable_mojang_anti_features: Option<bool>,
    #[serde(rename = "feature.enable_profile_key")]
    pub feature_enable_profile_key: Option<bool>,
    #[serde(rename = "feature.username_check")]
    pub feature_username_check: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct YggdrasilLinks {
    pub homepage: Option<String>,
    pub register: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct YggdrasilAuthenticateRequest {
    pub username: String,
    pub password: String,
    #[serde(rename = "clientToken", skip_serializing_if = "Option::is_none")]
    pub client_token: Option<String>,
    #[serde(rename = "requestUser")]
    pub request_user: bool,
    pub agent: YggdrasilAgent,
}

#[derive(Debug, Serialize)]
pub struct YggdrasilAgent {
    pub name: String,
    pub version: u32,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct YggdrasilAuthenticateResponse {
    #[serde(rename = "accessToken")]
    pub access_token: String,
    #[serde(rename = "clientToken")]
    pub client_token: String,
    #[serde(rename = "availableProfiles", default)]
    pub available_profiles: Vec<YggdrasilProfile>,
    #[serde(rename = "selectedProfile")]
    pub selected_profile: Option<YggdrasilProfile>,
    pub user: Option<YggdrasilUser>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct YggdrasilProfile {
    pub id: String,
    pub name: String,
    pub properties: Option<Vec<YggdrasilProperty>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct YggdrasilProperty {
    pub name: String,
    pub value: String,
    pub signature: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct YggdrasilUser {
    pub id: String,
    /// Some servers (including LittleSkin) omit this field or send null.
    #[serde(default)]
    pub properties: Option<Vec<YggdrasilProperty>>,
}

#[derive(Debug, Serialize)]
pub struct YggdrasilValidateRequest {
    #[serde(rename = "accessToken")]
    pub access_token: String,
    #[serde(rename = "clientToken", skip_serializing_if = "Option::is_none")]
    pub client_token: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct YggdrasilRefreshRequest {
    #[serde(rename = "accessToken")]
    pub access_token: String,
    #[serde(rename = "clientToken", skip_serializing_if = "Option::is_none")]
    pub client_token: Option<String>,
    #[serde(rename = "requestUser")]
    pub request_user: bool,
    #[serde(rename = "selectedProfile")]
    pub selected_profile: Option<YggdrasilProfile>,
}

#[derive(Debug, Deserialize)]
pub struct YggdrasilErrorResponse {
    pub error: String,
    #[serde(rename = "errorMessage")]
    pub error_message: String,
    #[allow(dead_code)]
    pub cause: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact(path: &str) -> Artifact {
        Artifact {
            url: format!("https://example.invalid/{}.jar", path),
            sha1: "0000000000000000000000000000000000000000".to_string(),
            path: format!("{}.jar", path),
            size: Some(1),
        }
    }

    fn library_with(classifiers: Classifiers, natives: Option<HashMap<String, String>>) -> Library {
        Library {
            name: "test:lib:1.0".to_string(),
            downloads: Some(LibraryDownloads {
                artifact: Some(artifact("artifact")),
                classifiers: Some(classifiers),
            }),
            rules: None,
            natives,
            url: None,
            sha1: None,
            size: None,
        }
    }

    fn all_classifiers() -> Classifiers {
        Classifiers {
            natives_linux: Some(artifact("linux")),
            natives_windows: Some(artifact("windows")),
            natives_macos: Some(artifact("macos")),
            other: HashMap::from([
                ("natives-linux-arm64".to_string(), artifact("linux-arm64")),
                (
                    "natives-windows-arm64".to_string(),
                    artifact("windows-arm64"),
                ),
                ("natives-macos-arm64".to_string(), artifact("macos-arm64")),
                ("natives-windows-x86".to_string(), artifact("windows-x86")),
            ]),
        }
    }

    fn library_with_rules(rules: Vec<Rule>) -> Library {
        Library {
            name: "test:lib:1.0".to_string(),
            downloads: None,
            rules: Some(rules),
            natives: None,
            url: None,
            sha1: None,
            size: None,
        }
    }

    #[test]
    fn decodes_yggdrasil_response_without_user_properties() {
        let response: YggdrasilAuthenticateResponse = serde_json::from_str(
            r#"{
                "accessToken": "access",
                "clientToken": "client",
                "availableProfiles": [
                    {"id": "00112233445566778899aabbccddeeff", "name": "Steve"}
                ],
                "user": {"id": "00112233445566778899aabbccddeeff"}
            }"#,
        )
        .unwrap();

        assert_eq!(response.available_profiles.len(), 1);
        assert!(response.selected_profile.is_none());
        assert!(response.user.unwrap().properties.is_none());
    }

    #[test]
    fn allows_yggdrasil_response_without_available_profiles() {
        let response: YggdrasilAuthenticateResponse = serde_json::from_str(
            r#"{
                "accessToken": "access",
                "clientToken": "client",
                "selectedProfile": {"id": "00112233445566778899aabbccddeeff", "name": "Steve"},
                "user": {"id": "00112233445566778899aabbccddeeff", "properties": null}
            }"#,
        )
        .unwrap();

        assert!(response.available_profiles.is_empty());
        assert_eq!(response.selected_profile.unwrap().name, "Steve");
        assert!(response.user.unwrap().properties.is_none());
    }

    #[test]
    fn selects_the_best_native_from_separate_library_entries() {
        let libraries: Vec<Library> = serde_json::from_str(
            r#"[
                {
                    "name": "org.lwjgl:lwjgl:3.3.3",
                    "downloads": {
                        "artifact": {
                            "url": "https://example.invalid/lwjgl.jar",
                            "sha1": "main",
                            "path": "lwjgl.jar"
                        }
                    }
                },
                {
                    "name": "org.lwjgl:lwjgl:3.3.3:natives-windows",
                    "downloads": {
                        "artifact": {
                            "url": "https://example.invalid/win.jar",
                            "sha1": "win",
                            "path": "win.jar"
                        }
                    }
                },
                {
                    "name": "org.lwjgl:lwjgl:3.3.3:natives-windows-arm64",
                    "downloads": {
                        "artifact": {
                            "url": "https://example.invalid/win-arm64.jar",
                            "sha1": "win-arm64",
                            "path": "win-arm64.jar"
                        }
                    }
                },
                {
                    "name": "org.lwjgl:lwjgl:3.3.3:natives-linux",
                    "downloads": {
                        "artifact": {
                            "url": "https://example.invalid/linux.jar",
                            "sha1": "linux",
                            "path": "linux.jar"
                        }
                    }
                }
            ]"#,
        )
        .unwrap();

        let windows = selected_native_artifacts(&libraries, "windows", "x86_64");
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].path, "win.jar");

        let windows_arm = selected_native_artifacts(&libraries, "windows", "aarch64");
        assert_eq!(windows_arm.len(), 1);
        assert_eq!(windows_arm[0].path, "win-arm64.jar");

        let linux = selected_native_artifacts(&libraries, "linux", "x86_64");
        assert_eq!(linux.len(), 1);
        assert_eq!(linux[0].path, "linux.jar");
    }

    #[test]
    fn resolves_maven_style_library_artifacts() {
        let library: Library = serde_json::from_str(
            r#"{
                "name": "org.ow2.asm:asm:9.10.1",
                "url": "https://maven.fabricmc.net/",
                "sha1": "ada2141c0cc52ee8f5c48cd5fa4ce0e794f22236",
                "size": 126151
            }"#,
        )
        .unwrap();

        let artifact = library.artifact().unwrap();

        assert_eq!(artifact.path, "org/ow2/asm/asm/9.10.1/asm-9.10.1.jar");
        assert_eq!(
            artifact.url,
            "https://maven.fabricmc.net/org/ow2/asm/asm/9.10.1/asm-9.10.1.jar"
        );
        assert_eq!(artifact.size, Some(126151));
    }

    #[test]
    fn resolves_maven_style_library_classifiers() {
        let library: Library = serde_json::from_str(
            r#"{
                "name": "com.example:demo:1.0:natives-linux",
                "url": "https://maven.example.invalid/"
            }"#,
        )
        .unwrap();

        let artifact = library.artifact().unwrap();

        assert_eq!(
            artifact.path,
            "com/example/demo/1.0/demo-1.0-natives-linux.jar"
        );
        assert_eq!(
            artifact.url,
            "https://maven.example.invalid/com/example/demo/1.0/demo-1.0-natives-linux.jar"
        );
    }

    #[test]
    fn library_without_rules_is_allowed_everywhere() {
        let library = library_with(all_classifiers(), None);

        assert!(library.is_allowed_for("windows", "x86_64"));
        assert!(library.is_allowed_for("linux", "aarch64"));
        assert!(library.is_allowed_for("macos", "x86_64"));
    }

    #[test]
    fn library_rule_only_allows_matching_os() {
        let library = library_with_rules(vec![Rule {
            action: "allow".to_string(),
            os: Some(OsRule {
                name: "windows".to_string(),
                arch: None,
            }),
        }]);

        assert!(library.is_allowed_for("windows", "x86_64"));
        assert!(!library.is_allowed_for("linux", "x86_64"));
        assert!(!library.is_allowed_for("macos", "x86_64"));
    }

    #[test]
    fn library_rule_respects_arch() {
        let library = library_with_rules(vec![Rule {
            action: "allow".to_string(),
            os: Some(OsRule {
                name: "windows".to_string(),
                arch: Some("x86".to_string()),
            }),
        }]);

        assert!(library.is_allowed_for("windows", "x86"));
        assert!(!library.is_allowed_for("windows", "x86_64"));
    }

    #[test]
    fn disallow_rule_beats_allow_rule() {
        let library = library_with_rules(vec![
            Rule {
                action: "allow".to_string(),
                os: None,
            },
            Rule {
                action: "disallow".to_string(),
                os: Some(OsRule {
                    name: "linux".to_string(),
                    arch: None,
                }),
            },
        ]);

        assert!(library.is_allowed_for("windows", "x86_64"));
        assert!(!library.is_allowed_for("linux", "x86_64"));
    }

    #[test]
    fn native_artifact_matches_os() {
        let library = library_with(all_classifiers(), None);

        assert_eq!(
            library
                .native_artifact_for("windows", "x86_64")
                .unwrap()
                .path,
            "windows.jar"
        );
        assert_eq!(
            library.native_artifact_for("linux", "x86_64").unwrap().path,
            "linux.jar"
        );
        assert_eq!(
            library.native_artifact_for("macos", "x86_64").unwrap().path,
            "macos.jar"
        );
    }

    #[test]
    fn native_artifact_selects_arm64_variant() {
        let library = library_with(all_classifiers(), None);

        assert_eq!(
            library
                .native_artifact_for("linux", "aarch64")
                .unwrap()
                .path,
            "linux-arm64.jar"
        );
        assert_eq!(
            library
                .native_artifact_for("windows", "aarch64")
                .unwrap()
                .path,
            "windows-arm64.jar"
        );
        assert_eq!(
            library
                .native_artifact_for("macos", "aarch64")
                .unwrap()
                .path,
            "macos-arm64.jar"
        );
    }

    #[test]
    fn native_artifact_selects_32_bit_variant() {
        let library = library_with(all_classifiers(), None);

        assert_eq!(
            library.native_artifact_for("windows", "x86").unwrap().path,
            "windows-x86.jar"
        );
    }

    #[test]
    fn native_artifact_never_returns_another_os() {
        let mut classifiers = all_classifiers();
        classifiers.natives_macos = None;
        classifiers.other.remove("natives-macos-arm64");
        let library = library_with(classifiers, None);

        assert!(library.native_artifact_for("macos", "x86_64").is_none());
        assert!(library.native_artifact_for("macos", "aarch64").is_none());
    }

    #[test]
    fn native_artifact_honors_legacy_natives_map() {
        let library = library_with(
            all_classifiers(),
            Some(HashMap::from([(
                "osx".to_string(),
                "natives-macos".to_string(),
            )])),
        );

        assert_eq!(
            library.native_artifact_for("macos", "x86_64").unwrap().path,
            "macos.jar"
        );
    }
}
