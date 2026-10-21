pub mod yggdrasil;

use crate::common::models::*;
use reqwest::Client;
use std::time::{SystemTime, UNIX_EPOCH};

const MSA_REFRESH_MARGIN_SECS: u64 = 300;

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub(crate) fn cache_needs_refresh(cache: &AuthCache) -> bool {
    cache_needs_refresh_at(cache, now_unix())
}

fn cache_needs_refresh_at(cache: &AuthCache, now: u64) -> bool {
    match cache.expires_at {
        Some(expires_at) => now.saturating_add(MSA_REFRESH_MARGIN_SECS) >= expires_at,
        None => false,
    }
}

const MS_DEVICE_CODE_URL: &str =
    "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";
const MS_TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const XBL_AUTH_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_AUTH_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MC_LOGIN_WITH_XBOX_URL: &str =
    "https://api.minecraftservices.com/authentication/login_with_xbox";
const MC_PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";

#[derive(Debug)]
pub struct Authenticator {
    pub client_id: String,
}

impl Authenticator {
    pub fn new(client_id: String) -> Self {
        Self { client_id }
    }

    pub async fn authenticate_with_msa(&self) -> anyhow::Result<MicrosoftTokenResponse> {
        let client = Client::new();

        crate::outln!("Starting Microsoft Account login...");
        let params = [
            ("client_id", self.client_id.as_str()),
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
        crate::common::output::device_code(&device_code_res);

        let poll_params = [
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("device_code", &device_code_res.device_code),
            ("client_id", self.client_id.as_str()),
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
                crate::outln!("Microsoft Account login successful!");
                return Ok(token_res);
            } else {
                let status = res.status();
                let error_body = res.text().await.unwrap_or_default();
                if status == reqwest::StatusCode::BAD_REQUEST {
                    if let Ok(error_json) = serde_json::from_str::<serde_json::Value>(&error_body) {
                        if let Some(error_code) = error_json.get("error").and_then(|e| e.as_str()) {
                            match error_code {
                                "authorization_pending" => {
                                    crate::outln!("Waiting for user to authorize...");
                                    continue;
                                }
                                "slow_down" => {
                                    crate::outln!("Server requested to slow down polling.");
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

    pub async fn get_xbl_token(&self, ms_token: &str) -> anyhow::Result<(String, String)> {
        let client = Client::new();
        let request_body = serde_json::json!({
            "Properties": {
                "AuthMethod": "RPS",
                "SiteName": "user.auth.xboxlive.com",
                "RpsTicket": format!("d={}", ms_token)
            },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT"
        });

        let res = client.post(XBL_AUTH_URL).json(&request_body).send().await?;

        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Failed to get XBL token: {} - {}",
                status,
                body
            ));
        }

        let xbl_res: XblResponse = res.json().await?;
        let token = xbl_res.token;
        let user_hash = xbl_res
            .display_claims
            .xui
            .first()
            .ok_or_else(|| anyhow::anyhow!("XBL response missing user hash"))?
            .uhs
            .clone();

        crate::outln!("XBL token acquired.");
        Ok((token, user_hash))
    }

    pub async fn get_xsts_token(&self, xbl_token: &str) -> anyhow::Result<(String, String)> {
        let client = Client::new();
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
                    return Err(anyhow::anyhow!(
                        "The account doesn't have an Xbox account (2148916233)."
                    ));
                } else if body.contains("2148916238") {
                    return Err(anyhow::anyhow!(
                        "The account is a child (2148916238) and cannot proceed unless the account is added to a Family by an adult."
                    ));
                } else if body.contains("2148916235") {
                    return Err(anyhow::anyhow!(
                        "The account is from a country where Xbox Live is not available/banned (2148916235)."
                    ));
                }
            }
            return Err(anyhow::anyhow!(
                "Failed to get XSTS token: {} - {}",
                status,
                body
            ));
        }

        let xsts_res: XstsResponse = res.json().await?;
        let token = xsts_res.token;
        let user_hash = xsts_res
            .display_claims
            .xui
            .first()
            .ok_or_else(|| anyhow::anyhow!("XSTS response missing user hash"))?
            .uhs
            .clone();

        crate::outln!("XSTS token acquired.");
        Ok((token, user_hash))
    }

    pub async fn login_to_minecraft(
        &self,
        xsts_token: &str,
        user_hash: &str,
    ) -> anyhow::Result<MinecraftLoginResponse> {
        let client = Client::new();
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
            return Err(anyhow::anyhow!(
                "Failed to login to Minecraft: {} - {}",
                status,
                body
            ));
        }

        let mc_login_res: MinecraftLoginResponse = res.json().await?;
        crate::outln!(
            "Logged into Minecraft services as '{}'.",
            mc_login_res.username
        );
        Ok(mc_login_res)
    }

    pub async fn get_minecraft_profile(
        &self,
        mc_access_token: &str,
    ) -> anyhow::Result<MinecraftProfile> {
        let client = Client::new();

        let res = client
            .get(MC_PROFILE_URL)
            .header("Authorization", format!("Bearer {}", mc_access_token))
            .send()
            .await?;

        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Failed to get Minecraft profile: {} - {}",
                status,
                body
            ));
        }

        let profile: MinecraftProfile = res.json().await?;
        crate::outln!(
            "Retrieved Minecraft profile: {} ({})",
            profile.name,
            profile.id
        );
        Ok(profile)
    }

    async fn refresh_ms_token_at(
        &self,
        refresh_token: &str,
        token_url: &str,
    ) -> anyhow::Result<MicrosoftTokenResponse> {
        let client = Client::new();
        let params = [
            ("grant_type", "refresh_token"),
            ("client_id", self.client_id.as_str()),
            ("refresh_token", refresh_token),
            ("scope", "XboxLive.signin offline_access"),
        ];
        let res = client.post(token_url).form(&params).send().await?;

        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            if body.contains("invalid_grant") {
                return Err(anyhow::anyhow!(
                    "Microsoft refresh token is invalid or expired. Please run 'mclc login' again. ({} - {})",
                    status,
                    body
                ));
            }
            return Err(anyhow::anyhow!(
                "Failed to refresh Microsoft token: {} - {}",
                status,
                body
            ));
        }

        Ok(res.json::<MicrosoftTokenResponse>().await?)
    }

    pub async fn refresh_authentication(&self, refresh_token: &str) -> anyhow::Result<AuthCache> {
        let ms_token_res = self
            .refresh_ms_token_at(refresh_token, MS_TOKEN_URL)
            .await?;
        self.complete_authentication(ms_token_res, Some(refresh_token.to_string()))
            .await
    }

    async fn complete_authentication(
        &self,
        ms_token_res: MicrosoftTokenResponse,
        previous_refresh_token: Option<String>,
    ) -> anyhow::Result<AuthCache> {
        let ms_access_token = ms_token_res.access_token;

        let (xbl_token, user_hash) = self.get_xbl_token(&ms_access_token).await?;
        let (xsts_token, _) = self.get_xsts_token(&xbl_token).await?;
        let mc_login_res = self.login_to_minecraft(&xsts_token, &user_hash).await?;
        let profile = self
            .get_minecraft_profile(&mc_login_res.access_token)
            .await?;

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
            refresh_token: ms_token_res.refresh_token.or(previous_refresh_token),
            expires_at: Some(now_unix().saturating_add(mc_login_res.expires_in)),
        })
    }

    pub async fn perform_full_authentication(&self) -> anyhow::Result<AuthCache> {
        let ms_token_res = self.authenticate_with_msa().await?;
        self.complete_authentication(ms_token_res, None).await
    }
}

impl Default for Authenticator {
    fn default() -> Self {
        Self::new("YOUR_AZURE_CLIENT_ID".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_http;

    fn cache(expires_at: Option<u64>) -> AuthCache {
        AuthCache {
            access_token: "mc-token".to_string(),
            uuid: "uuid".to_string(),
            username: "Alice".to_string(),
            refresh_token: Some("refresh-token".to_string()),
            expires_at,
        }
    }

    #[test]
    fn cache_refreshes_only_when_expired_or_close() {
        assert!(!cache_needs_refresh_at(&cache(Some(10_000)), 1_000));
        assert!(cache_needs_refresh_at(&cache(Some(1_200)), 1_000));
        assert!(cache_needs_refresh_at(&cache(Some(1_000)), 1_000));
        assert!(!cache_needs_refresh_at(&cache(None), 1_000));
    }

    #[test]
    fn old_cache_without_refresh_fields_still_deserializes() {
        let cache: AuthCache =
            serde_json::from_str(r#"{"access_token":"token","uuid":"uuid","username":"Alice"}"#)
                .unwrap();

        assert!(cache.refresh_token.is_none());
        assert!(cache.expires_at.is_none());
    }

    #[tokio::test]
    async fn refreshes_microsoft_token_with_refresh_grant() {
        let body = r#"{
            "token_type": "Bearer",
            "expires_in": 3600,
            "scope": "XboxLive.signin offline_access",
            "access_token": "new-ms-token",
            "refresh_token": "rotated-refresh-token"
        }"#;
        let server =
            test_http::serve_once(test_http::response("200 OK", &[], body.as_bytes())).await;
        let authenticator = Authenticator::new("client-id".to_string());

        let response = authenticator
            .refresh_ms_token_at("old-refresh-token", &format!("{}/token", server.base_url))
            .await
            .unwrap();

        assert_eq!(response.access_token, "new-ms-token");
        assert_eq!(
            response.refresh_token.as_deref(),
            Some("rotated-refresh-token")
        );

        let requests = server.requests.lock().unwrap();
        assert_eq!(requests[0].method, "POST");
        assert!(requests[0].body.contains("grant_type=refresh_token"));
        assert!(requests[0].body.contains("refresh_token=old-refresh-token"));
        assert!(requests[0].body.contains("client_id=client-id"));
    }
}
