use crate::common::models::*;
use anyhow::Context;
use base64::Engine as _;
use futures_util::StreamExt;
use md5::Md5;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write, stdout};
use std::path::{Path, PathBuf};
use uuid::Uuid;

// Re-export YggdrasilProfile for use in main.rs
pub use crate::common::models::YggdrasilProfile;

const AUTHLIB_INJECTOR_API_BASE: &str = "https://authlib-injector.yushi.moe";
const AUTHLIB_INJECTOR_API_MIRROR: &str =
    "https://bmclapi2.bangbang93.com/mirrors/authlib-injector";

/// Returns the standard Java offline-mode UUID for a username.
pub(crate) fn offline_uuid(username: &str) -> String {
    let mut hasher = Md5::new();
    hasher.update(format!("OfflinePlayer:{}", username).as_bytes());
    let digest = hasher.finalize();

    let mut bytes = [0u8; 16];
    for (target, source) in bytes.iter_mut().zip(digest.iter()) {
        *target = *source;
    }

    // Match java.util.UUID.nameUUIDFromBytes: version 3, RFC 4122 variant.
    bytes[6] = (bytes[6] & 0x0f) | 0x30;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;

    let hex: String = bytes.iter().map(|byte| format!("{:02x}", byte)).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// Returns the formatted UUID with dashes
pub(crate) fn format_uuid(uuid: &str) -> String {
    if uuid.len() == 32 {
        format!(
            "{}-{}-{}-{}-{}",
            &uuid[0..8],
            &uuid[8..12],
            &uuid[12..16],
            &uuid[16..20],
            &uuid[20..32]
        )
    } else {
        uuid.to_string()
    }
}

/// Interactive profile selector using arrow keys
pub fn select_profile(profiles: &[YggdrasilProfile]) -> anyhow::Result<YggdrasilProfile> {
    use std::io::{self, Write};

    loop {
        println!("Please select a profile:");
        for (i, profile) in profiles.iter().enumerate() {
            println!("  {}. {}", i + 1, profile.name);
        }
        print!("Enter the number (1-{}): ", profiles.len());
        stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        match input.trim().parse::<usize>() {
            Ok(num) if num >= 1 && num <= profiles.len() => {
                return Ok(profiles[num - 1].clone());
            }
            _ => {
                println!("Invalid input. Please try again.\n");
            }
        }
    }
}

#[derive(Debug)]
pub struct YggdrasilAuthenticator {
    pub api_url: String,
    pub client: Client,
}

impl YggdrasilAuthenticator {
    pub fn new(api_url: String) -> Self {
        Self {
            api_url: api_url.trim_end_matches('/').to_string(),
            client: Client::new(),
        }
    }

    /// Get API URL via ALI (API Location Indication)
    pub async fn resolve_api_url(initial_url: &str) -> anyhow::Result<String> {
        let mut url = initial_url.trim().trim_end_matches('/').to_string();

        // Add https:// if no protocol specified
        if !url.starts_with("http://") && !url.starts_with("https://") {
            url = format!("https://{}", url);
        }

        let client = Client::new();
        let res = client.get(&url).send().await?;

        if res.status().is_success() {
            if let Some(ali_header) = res.headers().get("X-Authlib-Injector-API-Location") {
                let ali_value = ali_header.to_str().unwrap_or("");
                if !ali_value.is_empty() {
                    // Make it absolute URL
                    let resolved_url = url::Url::parse(&url)?;
                    let absolute_url = resolved_url.join(ali_value)?;
                    let absolute_str = absolute_url.to_string();
                    if absolute_str != url {
                        return Ok(absolute_str.trim_end_matches('/').to_string());
                    }
                }
            }
        }

        Ok(url.trim_end_matches('/').to_string())
    }

    pub async fn get_api_metadata(&self) -> anyhow::Result<YggdrasilApiMetadata> {
        let url = format!("{}/", self.api_url);
        let res = self.client.get(&url).send().await?;

        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Failed to get API metadata: {} - {}",
                status,
                body
            ));
        }

        let metadata: YggdrasilApiMetadata = res.json().await?;
        Ok(metadata)
    }

    pub async fn pre_fetch_metadata(&self) -> anyhow::Result<String> {
        let metadata = self.get_api_metadata().await?;
        let json = serde_json::to_string(&metadata)?;
        Ok(base64::engine::general_purpose::STANDARD.encode(json))
    }

    pub async fn authenticate(
        &self,
        username: &str,
        password: &str,
    ) -> anyhow::Result<YggdrasilAuthenticateResponse> {
        let client_token = Some(Uuid::new_v4().to_string().replace('-', ""));
        let request = YggdrasilAuthenticateRequest {
            username: username.to_string(),
            password: password.to_string(),
            client_token,
            request_user: true,
            agent: YggdrasilAgent {
                name: "Minecraft".to_string(),
                version: 1,
            },
        };

        let url = format!("{}/authserver/authenticate", self.api_url);
        let res = self.client.post(&url).json(&request).send().await?;

        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            if let Ok(error_resp) = serde_json::from_str::<YggdrasilErrorResponse>(&body) {
                return Err(anyhow::anyhow!(
                    "Authentication failed: {} - {}",
                    error_resp.error,
                    error_resp.error_message
                ));
            }
            return Err(anyhow::anyhow!(
                "Authentication failed: {} - {}",
                status,
                body
            ));
        }

        let response: YggdrasilAuthenticateResponse = res
            .json()
            .await
            .with_context(|| format!("Failed to decode authentication response from {}", url))?;
        Ok(response)
    }

    pub async fn validate(&self, access_token: &str, client_token: Option<&str>) -> bool {
        let request = YggdrasilValidateRequest {
            access_token: access_token.to_string(),
            client_token: client_token.map(|s| s.to_string()),
        };

        let url = format!("{}/authserver/validate", self.api_url);
        let res = self.client.post(&url).json(&request).send().await;

        match res {
            Ok(response) => response.status().is_success(),
            Err(_) => false,
        }
    }

    pub async fn refresh(
        &self,
        access_token: &str,
        client_token: Option<&str>,
        selected_profile: Option<YggdrasilProfile>,
    ) -> anyhow::Result<YggdrasilAuthenticateResponse> {
        let request = YggdrasilRefreshRequest {
            access_token: access_token.to_string(),
            client_token: client_token.map(|s| s.to_string()),
            request_user: true,
            selected_profile,
        };

        let url = format!("{}/authserver/refresh", self.api_url);
        let res = self.client.post(&url).json(&request).send().await?;

        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            if let Ok(error_resp) = serde_json::from_str::<YggdrasilErrorResponse>(&body) {
                return Err(anyhow::anyhow!(
                    "Token refresh failed: {} - {}",
                    error_resp.error,
                    error_resp.error_message
                ));
            }
            return Err(anyhow::anyhow!(
                "Token refresh failed: {} - {}",
                status,
                body
            ));
        }

        let response: YggdrasilAuthenticateResponse = res
            .json()
            .await
            .with_context(|| format!("Failed to decode token refresh response from {}", url))?;
        Ok(response)
    }

    #[allow(dead_code)]
    pub async fn invalidate(&self, access_token: &str, client_token: Option<&str>) -> bool {
        let request = YggdrasilValidateRequest {
            access_token: access_token.to_string(),
            client_token: client_token.map(|s| s.to_string()),
        };

        let url = format!("{}/authserver/invalidate", self.api_url);
        let res = self.client.post(&url).json(&request).send().await;

        match res {
            Ok(response) => response.status().is_success(),
            Err(_) => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YggdrasilAccount {
    /// The API URL of the auth server
    pub api_url: String,
    /// Server name (from metadata)
    pub server_name: Option<String>,
    /// Account identifier (email or username)
    pub identifier: String,
    /// Profile UUID
    pub uuid: String,
    /// Profile name
    pub name: String,
    /// Access token
    pub access_token: String,
    /// Client token
    pub client_token: String,
    /// User ID
    pub user_id: String,
    /// User properties (JSON string)
    pub user_properties: String,
}

impl YggdrasilAccount {
    pub fn from_auth_response_with_profile_selection(
        api_url: String,
        server_name: Option<String>,
        identifier: String,
        response: YggdrasilAuthenticateResponse,
    ) -> anyhow::Result<Self> {
        let selected_profile = if let Some(profile) = response.selected_profile {
            profile
        } else {
            // User has multiple profiles, need to select one
            if response.available_profiles.is_empty() {
                return Err(anyhow::anyhow!("No profiles available for this account"));
            }

            // Use interactive selection
            let profile = select_profile(&response.available_profiles)?;
            profile
        };

        let user = response.user.as_ref().context("User info not available")?;
        let user_id = user.id.clone();
        let user_properties = user.properties.clone().unwrap_or_default();
        let user_properties_json = serde_json::to_string(&user_properties)?;

        Ok(Self {
            api_url,
            server_name,
            identifier,
            uuid: format_uuid(&selected_profile.id),
            name: selected_profile.name.clone(),
            access_token: response.access_token,
            client_token: response.client_token,
            user_id: format_uuid(&user_id),
            user_properties: user_properties_json,
        })
    }

    pub fn from_auth_response(
        api_url: String,
        server_name: Option<String>,
        identifier: String,
        response: YggdrasilAuthenticateResponse,
    ) -> anyhow::Result<Self> {
        Self::from_auth_response_with_profile_selection(api_url, server_name, identifier, response)
    }

    pub fn get_display_name(&self) -> String {
        if let Some(name) = &self.server_name {
            format!("{} ({})", self.name, name)
        } else {
            format!("{} ({})", self.name, self.api_url)
        }
    }
}

#[derive(Debug, Deserialize)]
struct AuthlibLatest {
    download_url: String,
    #[serde(default)]
    checksums: Option<AuthlibChecksums>,
}

#[derive(Debug, Deserialize)]
struct AuthlibChecksums {
    sha256: Option<String>,
}

#[cfg(test)]
fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|byte| format!("{:02x}", byte))
        .collect()
}

fn file_sha256(path: &Path) -> anyhow::Result<String> {
    let mut file =
        fs::File::open(path).with_context(|| format!("Failed to open file: {:?}", path))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];

    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{:02x}", byte))
        .collect())
}

pub struct AuthlibInjector {
    cache_dir: PathBuf,
}

impl AuthlibInjector {
    pub fn new(cache_dir: PathBuf) -> Self {
        Self { cache_dir }
    }

    pub async fn get_or_download(&self) -> anyhow::Result<PathBuf> {
        self.get_or_download_from(AUTHLIB_INJECTOR_API_BASE, AUTHLIB_INJECTOR_API_MIRROR)
            .await
    }

    async fn fetch_latest(&self, client: &Client, api_base: &str) -> anyhow::Result<AuthlibLatest> {
        let api_url = format!("{}/artifact/latest.json", api_base.trim_end_matches('/'));
        let response = client.get(&api_url).send().await?.error_for_status()?;
        Ok(response.json::<AuthlibLatest>().await?)
    }

    async fn get_or_download_from(
        &self,
        api_base: &str,
        mirror_base: &str,
    ) -> anyhow::Result<PathBuf> {
        let jar_path = self.cache_dir.join("authlib-injector.jar");
        let client = Client::new();

        let latest = match self.fetch_latest(&client, api_base).await {
            Ok(latest) => latest,
            Err(error) => {
                crate::outwarn!(
                    "Failed to query authlib-injector API ({}), trying BMCLAPI mirror...",
                    error
                );
                self.fetch_latest(&client, mirror_base)
                    .await
                    .context("Failed to query authlib-injector mirror")?
            }
        };

        let expected_sha256 = latest
            .checksums
            .as_ref()
            .and_then(|checksums| checksums.sha256.as_deref());

        if jar_path.exists() {
            match expected_sha256 {
                Some(expected) if file_sha256(&jar_path)?.eq_ignore_ascii_case(expected) => {
                    return Ok(jar_path);
                }
                Some(_) => {
                    crate::outwarn!(
                        "Cached authlib-injector.jar failed checksum verification, re-downloading..."
                    );
                }
                None => return Ok(jar_path),
            }
        }

        crate::outln!("Downloading authlib-injector from: {}", latest.download_url);
        crate::common::output::progress("authlib", 0, 0, "Downloading authlib-injector");
        fs::create_dir_all(&self.cache_dir)
            .with_context(|| format!("Failed to create directory: {:?}", self.cache_dir))?;

        let response = client
            .get(&latest.download_url)
            .send()
            .await
            .context("Failed to connect to authlib-injector download server")?
            .error_for_status()
            .context("Failed to download authlib-injector")?;

        let total_bytes = response.content_length().unwrap_or(0);
        let mut temp = tempfile::NamedTempFile::new_in(&self.cache_dir)
            .with_context(|| format!("Failed to create temporary file in {:?}", self.cache_dir))?;
        let mut hasher = Sha256::new();
        let mut downloaded = 0u64;
        let mut stream = response.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("Failed to read response chunk")?;
            temp.write_all(&chunk)?;
            hasher.update(&chunk);
            downloaded += chunk.len() as u64;

            if total_bytes > 0 && (downloaded % (1024 * 1024) == 0 || downloaded == total_bytes) {
                crate::outln!(
                    "Downloaded: {}/{} bytes ({:.0}%)",
                    downloaded,
                    total_bytes,
                    downloaded as f64 / total_bytes as f64 * 100.0
                );
                crate::common::output::progress(
                    "authlib",
                    downloaded,
                    total_bytes,
                    "Downloading authlib-injector",
                );
            }
        }
        temp.flush().context("Failed to flush downloaded data")?;

        if let Some(expected) = expected_sha256 {
            let actual: String = hasher
                .finalize()
                .iter()
                .map(|byte| format!("{:02x}", byte))
                .collect();
            if !actual.eq_ignore_ascii_case(expected) {
                anyhow::bail!(
                    "SHA-256 mismatch for authlib-injector: expected {}, got {}",
                    expected,
                    actual
                );
            }
        }

        temp.persist(&jar_path)
            .map_err(|error| anyhow::anyhow!("Failed to save authlib-injector.jar: {}", error))?;

        crate::outln!("authlib-injector.jar downloaded successfully");
        crate::common::output::progress("authlib", 1, 1, "authlib-injector downloaded");
        Ok(jar_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_http;

    async fn latest_json_server(download_url: &str, sha256: &str) -> test_http::TestServer {
        let body = format!(
            r#"{{"download_url":"{}","checksums":{{"sha256":"{}"}}}}"#,
            download_url, sha256
        );
        test_http::serve_once(test_http::response("200 OK", &[], body.as_bytes())).await
    }

    #[tokio::test]
    async fn decodes_a_littleskin_authenticate_response() {
        let body = r#"{
            "accessToken": "access",
            "clientToken": "client",
            "availableProfiles": [
                {"id": "00112233445566778899aabbccddeeff", "name": "Steve"},
                {"id": "ffeeddccbbaa99887766554433221100", "name": "Alex"}
            ],
            "user": {"id": "00112233445566778899aabbccddeeff"}
        }"#;
        let server =
            test_http::serve_once(test_http::response("200 OK", &[], body.as_bytes())).await;
        let authenticator = YggdrasilAuthenticator::new(server.base_url);

        let response = authenticator
            .authenticate("player@example.com", "password")
            .await
            .unwrap();

        assert_eq!(response.available_profiles.len(), 2);
        assert!(response.selected_profile.is_none());
        assert!(response.user.unwrap().properties.is_none());
    }

    #[test]
    fn offline_uuid_matches_java_offline_player_uuid() {
        assert_eq!(
            offline_uuid("Notch"),
            "b50ad385-829d-3141-a216-7e7d7539ba7f"
        );
        assert_eq!(
            offline_uuid("Player"),
            "a01e3843-e521-3998-958a-f459800e4d11"
        );
    }

    #[tokio::test]
    async fn downloads_authlib_injector_with_checksum() {
        let jar = b"authlib-injector-bytes";
        let jar_server = test_http::serve_once(test_http::response("200 OK", &[], jar)).await;
        let api_server = latest_json_server(
            &format!("{}/artifact.jar", jar_server.base_url),
            &sha256_hex(jar),
        )
        .await;
        let temp = tempfile::tempdir().unwrap();
        let injector = AuthlibInjector::new(temp.path().to_path_buf());

        let path = injector
            .get_or_download_from(&api_server.base_url, &api_server.base_url)
            .await
            .unwrap();

        assert_eq!(std::fs::read(path).unwrap(), jar);
    }

    #[tokio::test]
    async fn rejects_authlib_injector_with_wrong_checksum() {
        let jar = b"tampered";
        let jar_server = test_http::serve_once(test_http::response("200 OK", &[], jar)).await;
        let api_server = latest_json_server(
            &format!("{}/artifact.jar", jar_server.base_url),
            &"0".repeat(64),
        )
        .await;
        let temp = tempfile::tempdir().unwrap();
        let injector = AuthlibInjector::new(temp.path().to_path_buf());

        let result = injector
            .get_or_download_from(&api_server.base_url, &api_server.base_url)
            .await;

        assert!(result.is_err());
        assert!(!temp.path().join("authlib-injector.jar").exists());
    }

    #[tokio::test]
    async fn replaces_a_corrupt_cached_jar() {
        let jar = b"correct-jar";
        let jar_server = test_http::serve_once(test_http::response("200 OK", &[], jar)).await;
        let api_server = latest_json_server(
            &format!("{}/artifact.jar", jar_server.base_url),
            &sha256_hex(jar),
        )
        .await;
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("authlib-injector.jar"), b"truncated").unwrap();
        let injector = AuthlibInjector::new(temp.path().to_path_buf());

        let path = injector
            .get_or_download_from(&api_server.base_url, &api_server.base_url)
            .await
            .unwrap();

        assert_eq!(std::fs::read(path).unwrap(), jar);
    }

    #[tokio::test]
    async fn falls_back_to_mirror_when_official_api_fails() {
        let jar = b"mirror-jar";
        let jar_server = test_http::serve_once(test_http::response("200 OK", &[], jar)).await;
        let mirror_server = latest_json_server(
            &format!("{}/artifact.jar", jar_server.base_url),
            &sha256_hex(jar),
        )
        .await;
        let official_server = test_http::serve_once(test_http::response(
            "500 Internal Server Error",
            &[],
            b"unavailable",
        ))
        .await;
        let temp = tempfile::tempdir().unwrap();
        let injector = AuthlibInjector::new(temp.path().to_path_buf());

        let path = injector
            .get_or_download_from(&official_server.base_url, &mirror_server.base_url)
            .await
            .unwrap();

        assert_eq!(std::fs::read(path).unwrap(), jar);
    }
}
