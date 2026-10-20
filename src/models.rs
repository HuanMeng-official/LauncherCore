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
    #[serde(rename = "availableProfiles")]
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
    pub properties: Vec<YggdrasilProperty>,
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

    fn library_with(
        classifiers: Classifiers,
        natives: Option<HashMap<String, String>>,
    ) -> Library {
        Library {
            name: "test:lib:1.0".to_string(),
            downloads: Some(LibraryDownloads {
                artifact: Some(artifact("artifact")),
                classifiers: Some(classifiers),
            }),
            rules: None,
            natives,
        }
    }

    fn all_classifiers() -> Classifiers {
        Classifiers {
            natives_linux: Some(artifact("linux")),
            natives_windows: Some(artifact("windows")),
            natives_macos: Some(artifact("macos")),
            other: HashMap::from([
                ("natives-linux-arm64".to_string(), artifact("linux-arm64")),
                ("natives-windows-arm64".to_string(), artifact("windows-arm64")),
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
        }
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
            library.native_artifact_for("windows", "x86_64").unwrap().path,
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
            library.native_artifact_for("linux", "aarch64").unwrap().path,
            "linux-arm64.jar"
        );
        assert_eq!(
            library.native_artifact_for("windows", "aarch64").unwrap().path,
            "windows-arm64.jar"
        );
        assert_eq!(
            library.native_artifact_for("macos", "aarch64").unwrap().path,
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
