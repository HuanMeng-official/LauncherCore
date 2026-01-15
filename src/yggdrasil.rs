use crate::models::*;
use anyhow::Context;
use base64::Engine as _;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{stdout, Write};
use std::path::PathBuf;
use uuid::Uuid;

// Re-export YggdrasilProfile for use in main.rs
pub use crate::models::YggdrasilProfile;

const AUTHLIB_INJECTOR_API_BASE: &str = "https://authlib-injector.yushi.moe";
const AUTHLIB_INJECTOR_API_MIRROR: &str = "https://bmclapi2.bangbang93.com/mirrors/authlib-injector";

/// Returns the formatted UUID with dashes
fn format_uuid(uuid: &str) -> String {
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
            return Err(anyhow::anyhow!("Authentication failed: {} - {}", status, body));
        }

        let response: YggdrasilAuthenticateResponse = res.json().await?;
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
            return Err(anyhow::anyhow!("Token refresh failed: {} - {}", status, body));
        }

        let response: YggdrasilAuthenticateResponse = res.json().await?;
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

        let user_id = response.user.as_ref().context("User info not available")?.id.clone();
        let user_properties_json = serde_json::to_string(
            &response.user.as_ref().context("User info not available")?.properties,
        )?;

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
        Self::from_auth_response_with_profile_selection(
            api_url,
            server_name,
            identifier,
            response,
        )
    }

    pub fn get_display_name(&self) -> String {
        if let Some(name) = &self.server_name {
            format!("{} ({})", self.name, name)
        } else {
            format!("{} ({})", self.name, self.api_url)
        }
    }
}

pub struct AuthlibInjector {
    cache_dir: PathBuf,
}

impl AuthlibInjector {
    pub fn new(cache_dir: PathBuf) -> Self {
        Self { cache_dir }
    }

    pub async fn get_or_download(&self) -> anyhow::Result<PathBuf> {
        let jar_path = self.cache_dir.join("authlib-injector.jar");

        if jar_path.exists() {
            return Ok(jar_path);
        }

        println!("Fetching authlib-injector download URL...");

        // Try to get latest version info from API
        let client = Client::new();
        let api_url = format!("{}/artifact/latest.json", AUTHLIB_INJECTOR_API_BASE);

        let download_url = match client.get(&api_url).send().await {
            Ok(response) if response.status().is_success() => {
                let json: serde_json::Value = response.json().await?;
                match json["download_url"].as_str() {
                    Some(url) => url.to_string(),
                    None => {
                        // Fallback to mirror
                        println!("Using BMCLAPI mirror...");
                        format!("{}/artifact/latest/authlib-injector.jar", AUTHLIB_INJECTOR_API_MIRROR)
                    }
                }
            }
            _ => {
                println!("Using BMCLAPI mirror...");
                format!("{}/artifact/latest/authlib-injector.jar", AUTHLIB_INJECTOR_API_MIRROR)
            }
        };

        println!("Downloading authlib-injector from: {}", download_url);

        // Create cache directory if it doesn't exist
        if let Some(parent) = jar_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {:?}", parent))?;
        }

        let response = client
            .get(&download_url)
            .send()
            .await
            .context("Failed to connect to authlib-injector download server")?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Failed to download authlib-injector: HTTP {}",
                response.status()
            ));
        }

        // Read the response body first to get total size
        let total_bytes = response.content_length().unwrap_or(0);

        // Create the file
        {
            let mut file = std::fs::File::create(&jar_path)
                .with_context(|| format!("Failed to create file: {:?}", jar_path))?;

            let mut downloaded = 0u64;

            // Use bytes() to get a stream of bytes
            let mut stream = response.bytes_stream();

            use futures_util::StreamExt;

            while let Some(chunk_result) = stream.next().await {
                let chunk = chunk_result
                    .with_context(|| "Failed to read response chunk")?;
                let n = chunk.len();
                if n == 0 {
                    break;
                }
                downloaded += n as u64;
                file.write_all(&chunk)
                    .with_context(|| format!("Failed to write to file: {:?}", jar_path))?;

                if total_bytes > 0 {
                    let progress = (downloaded as f64 / total_bytes as f64 * 100.0) as u32;
                    if downloaded % (1024 * 1024) == 0 || downloaded == total_bytes {
                        println!(
                            "Downloaded: {}/{} bytes ({:.0}%)",
                            downloaded,
                            total_bytes,
                            progress
                        );
                    }
                }
            }
        }

        println!("authlib-injector.jar downloaded successfully");
        Ok(jar_path)
    }
}
