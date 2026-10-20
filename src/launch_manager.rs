use crate::auth::{Authenticator, cache_needs_refresh};
use crate::download::DownloadSource;
use crate::install::Installer;
use crate::launch::Launcher;
use crate::loader::LoaderKind;
use crate::models::AuthCache;
use crate::yggdrasil::{
    AuthlibInjector, YggdrasilAccount, YggdrasilAuthenticator, YggdrasilProfile, select_profile,
};
use std::fs;
use std::path::{Path, PathBuf};

fn normalize_api_url(url: &str) -> String {
    let mut url = url.trim().trim_end_matches('/').to_string();
    if !url.starts_with("http://") && !url.starts_with("https://") {
        url = format!("https://{}", url);
    }
    url
}

fn write_private_file(path: &Path, contents: &str) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        file.write_all(contents.as_bytes())?;
    }

    #[cfg(not(unix))]
    {
        fs::write(path, contents)?;
    }

    Ok(())
}

fn set_private_dir_permissions(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }

    Ok(())
}

#[derive(Debug)]
pub struct LauncherManager {
    pub launcher: Launcher,
    pub installer: Installer,
    pub authenticator: Authenticator,
    pub config_dir: PathBuf,
}

impl LauncherManager {
    pub fn new(source: DownloadSource) -> anyhow::Result<Self> {
        let current_dir = std::env::current_dir()?;
        let minecraft_dir = current_dir.join(".minecraft");
        fs::create_dir_all(&minecraft_dir)?;

        let versions_dir = minecraft_dir.join("versions");
        let libraries_dir = minecraft_dir.join("libraries");
        let assets_dir = minecraft_dir.join("assets");
        let assets_objects_dir = assets_dir.join("objects");
        let assets_indexes_dir = assets_dir.join("indexes");

        let config_dir = dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?
            .join("mclc");
        fs::create_dir_all(&config_dir)?;
        set_private_dir_permissions(&config_dir)?;

        fs::create_dir_all(&versions_dir)?;
        fs::create_dir_all(&libraries_dir)?;
        fs::create_dir_all(&assets_dir)?;
        fs::create_dir_all(&assets_objects_dir)?;
        fs::create_dir_all(&assets_indexes_dir)?;

        Ok(Self {
            launcher: Launcher {
                minecraft_dir: minecraft_dir.clone(),
                versions_dir: versions_dir.clone(),
                libraries_dir: libraries_dir.clone(),
                assets_dir,
            },
            installer: Installer {
                game_dir: minecraft_dir.clone(),
                versions_dir,
                libraries_dir,
                assets_objects_dir,
                assets_indexes_dir,
                source,
            },
            authenticator: Authenticator::default(),
            config_dir,
        })
    }

    #[allow(dead_code)]
    pub fn set_client_id(&mut self, client_id: String) {
        self.authenticator = Authenticator::new(client_id);
    }

    pub fn get_auth_cache_path(&self) -> PathBuf {
        self.config_dir.join("auth_cache.json")
    }

    pub fn save_auth_cache(&self, cache: &AuthCache) -> anyhow::Result<()> {
        let cache_path = self.get_auth_cache_path();
        let json = serde_json::to_string_pretty(cache)?;
        fs::create_dir_all(&self.config_dir)?;
        set_private_dir_permissions(&self.config_dir)?;
        write_private_file(&cache_path, &json)?;
        println!("Authentication information saved to {:?}", cache_path);
        Ok(())
    }

    pub fn load_auth_cache(&self) -> anyhow::Result<Option<AuthCache>> {
        let cache_path = self.get_auth_cache_path();
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

    pub fn get_accounts_path(&self) -> PathBuf {
        self.config_dir.join("accounts.json")
    }

    pub fn get_authlib_injector(&self) -> AuthlibInjector {
        AuthlibInjector::new(self.config_dir.join("cache"))
    }

    pub fn save_accounts(&self, accounts: &[YggdrasilAccount]) -> anyhow::Result<()> {
        let accounts_path = self.get_accounts_path();
        let json = serde_json::to_string_pretty(accounts)?;
        fs::create_dir_all(&self.config_dir)?;
        set_private_dir_permissions(&self.config_dir)?;
        write_private_file(&accounts_path, &json)?;
        println!("Accounts saved to {:?}", accounts_path);
        Ok(())
    }

    pub fn load_accounts(&self) -> anyhow::Result<Vec<YggdrasilAccount>> {
        let accounts_path = self.get_accounts_path();
        if !accounts_path.exists() {
            return Ok(Vec::new());
        }
        let json = fs::read_to_string(&accounts_path)?;
        let accounts: Vec<YggdrasilAccount> = serde_json::from_str(&json)?;
        Ok(accounts)
    }

    pub fn find_account_by_identifier(&self, identifier: &str, api_url: &str) -> anyhow::Result<Option<YggdrasilAccount>> {
        let accounts = self.load_accounts()?;
        let normalized_url = normalize_api_url(api_url);
        for account in accounts {
            if account.identifier == identifier
                && normalize_api_url(&account.api_url) == normalized_url
            {
                return Ok(Some(account));
            }
        }
        Ok(None)
    }

    /// Resolve the user-supplied API URL the same way `external_login` does
    /// before looking up a stored account.
    pub async fn find_account_by_identifier_resolved(
        &self,
        identifier: &str,
        api_url: &str,
    ) -> anyhow::Result<Option<YggdrasilAccount>> {
        let resolved_url = YggdrasilAuthenticator::resolve_api_url(api_url)
            .await
            .unwrap_or_else(|_| normalize_api_url(api_url));
        self.find_account_by_identifier(identifier, &resolved_url)
    }

    pub async fn external_login(
        &self,
        identifier: &str,
        password: &str,
        api_url: &str,
    ) -> anyhow::Result<YggdrasilAccount> {
        self.external_login_with_selector(identifier, password, api_url, select_profile)
            .await
    }

    pub async fn external_login_with_selector<F>(
        &self,
        identifier: &str,
        password: &str,
        api_url: &str,
        select_profile: F,
    ) -> anyhow::Result<YggdrasilAccount>
    where
        F: Fn(&[YggdrasilProfile]) -> anyhow::Result<YggdrasilProfile>,
    {
        // Resolve API URL via ALI
        let resolved_url = YggdrasilAuthenticator::resolve_api_url(api_url).await?;
        println!("Resolved API URL: {}", resolved_url);

        let authenticator = YggdrasilAuthenticator::new(resolved_url.clone());

        // Get server metadata
        let metadata = authenticator.get_api_metadata().await?;
        let server_name = metadata.meta.as_ref()
            .and_then(|m| m.server_name.clone());

        println!("Connected to: {}", server_name.as_deref().unwrap_or(&resolved_url));

        // Authenticate
        let mut auth_response = authenticator.authenticate(identifier, password).await?;

        // Some Yggdrasil servers return no selected profile when the account has
        // multiple characters. The token is unbound until a refresh selects one.
        if auth_response.selected_profile.is_none() {
            if auth_response.available_profiles.is_empty() {
                anyhow::bail!("No profiles available for this account");
            }

            let profile = select_profile(&auth_response.available_profiles)?;
            println!("Binding profile: {}", profile.name);

            let mut refreshed = authenticator
                .refresh(
                    &auth_response.access_token,
                    Some(&auth_response.client_token),
                    Some(profile.clone()),
                )
                .await?;

            if refreshed.selected_profile.is_none() {
                refreshed.selected_profile = Some(profile);
            }
            auth_response = refreshed;
        }

        // Create account from response
        let account = YggdrasilAccount::from_auth_response(
            resolved_url,
            server_name,
            identifier.to_string(),
            auth_response,
        )?;

        println!("Logged in as: {}", account.get_display_name());

        // Save/update accounts
        self.save_account(&account)?;

        Ok(account)
    }

    pub fn save_account(&self, account: &YggdrasilAccount) -> anyhow::Result<()> {
        let mut accounts = self.load_accounts()?;
        let normalized_url = account.api_url.trim_end_matches('/');

        // Remove existing account with same identifier and API URL
        accounts.retain(|a| {
            !(a.identifier == account.identifier && a.api_url.trim_end_matches('/') == normalized_url)
        });

        accounts.push(account.clone());
        self.save_accounts(&accounts)?;
        Ok(())
    }

    pub async fn list_versions(&self) -> anyhow::Result<()> {
        self.installer.list_versions().await
    }

    pub async fn install_version(&self, version_id: &str) -> anyhow::Result<()> {
        self.installer.install_version(version_id).await
    }

    pub async fn install_loader(
        &self,
        kind: LoaderKind,
        game_version: &str,
        loader_version: Option<&str>,
        java_path_override: Option<String>,
    ) -> anyhow::Result<String> {
        crate::loader::install_loader(
            &self.installer,
            kind,
            game_version,
            loader_version,
            java_path_override,
        )
        .await
    }

    pub async fn login(&self) -> anyhow::Result<AuthCache> {
        self.authenticator.perform_full_authentication().await
    }

    pub async fn ensure_fresh_auth_cache(
        &self,
        cache: AuthCache,
    ) -> anyhow::Result<AuthCache> {
        if !cache_needs_refresh(&cache) {
            return Ok(cache);
        }

        let Some(refresh_token) = cache.refresh_token.clone() else {
            anyhow::bail!(
                "Microsoft authentication has expired. Please run 'mclc login' again."
            );
        };

        println!("Refreshing Microsoft authentication...");
        let refreshed = self
            .authenticator
            .refresh_authentication(&refresh_token)
            .await?;
        self.save_auth_cache(&refreshed)?;
        Ok(refreshed)
    }

    pub fn launch(
        &self,
        version_id: &str,
        username: String,
        access_token: String,
        uuid: String,
        user_type: String,
        jvm_args: Option<String>,
        java_path: Option<String>,
        authlib_injector_jar: Option<std::path::PathBuf>,
        prefetched_metadata: Option<String>,
        api_url: Option<String>,
    ) -> anyhow::Result<()> {
        self.launcher.launch_game(
            version_id,
            username,
            access_token,
            uuid,
            user_type,
            jvm_args,
            java_path,
            authlib_injector_jar,
            prefetched_metadata,
            api_url,
        )
    }
}

impl Default for LauncherManager {
    fn default() -> Self {
        Self::new(DownloadSource::Auto).expect("Failed to initialize LauncherManager")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_http;

    fn manager_with_config(config_dir: PathBuf) -> LauncherManager {
        let minecraft_dir = config_dir.join(".minecraft");
        LauncherManager {
            launcher: Launcher {
                minecraft_dir: minecraft_dir.clone(),
                versions_dir: minecraft_dir.join("versions"),
                libraries_dir: minecraft_dir.join("libraries"),
                assets_dir: minecraft_dir.join("assets"),
            },
            installer: Installer {
                game_dir: minecraft_dir.clone(),
                versions_dir: minecraft_dir.join("versions"),
                libraries_dir: minecraft_dir.join("libraries"),
                assets_objects_dir: minecraft_dir.join("assets/objects"),
                assets_indexes_dir: minecraft_dir.join("assets/indexes"),
                source: DownloadSource::Auto,
            },
            authenticator: Authenticator::default(),
            config_dir,
        }
    }

    fn account(api_url: &str) -> YggdrasilAccount {
        YggdrasilAccount {
            api_url: api_url.to_string(),
            server_name: None,
            identifier: "alice".to_string(),
            uuid: "00000000-0000-0000-0000-000000000000".to_string(),
            name: "Alice".to_string(),
            access_token: "access".to_string(),
            client_token: "client".to_string(),
            user_id: "00000000-0000-0000-0000-000000000000".to_string(),
            user_properties: "{}".to_string(),
        }
    }

    #[cfg(unix)]
    #[test]
    fn auth_files_are_private() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let manager = manager_with_config(temp.path().to_path_buf());
        fs::create_dir_all(&manager.config_dir).unwrap();

        manager
            .save_auth_cache(&AuthCache {
                access_token: "token".to_string(),
                uuid: "uuid".to_string(),
                username: "Alice".to_string(),
                refresh_token: Some("refresh".to_string()),
                expires_at: Some(u64::MAX),
            })
            .unwrap();
        manager
            .save_accounts(&[account("https://example.invalid")])
            .unwrap();

        for path in [manager.get_auth_cache_path(), manager.get_accounts_path()] {
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "{:?} must not be readable by other users", path);
        }
    }

    #[tokio::test]
    async fn fresh_msa_cache_is_returned_unchanged() {
        let temp = tempfile::tempdir().unwrap();
        let manager = manager_with_config(temp.path().to_path_buf());
        let cache = AuthCache {
            access_token: "token".to_string(),
            uuid: "uuid".to_string(),
            username: "Alice".to_string(),
            refresh_token: Some("refresh".to_string()),
            expires_at: Some(u64::MAX),
        };

        let result = manager.ensure_fresh_auth_cache(cache).await.unwrap();

        assert_eq!(result.access_token, "token");
    }

    #[tokio::test]
    async fn expired_msa_cache_without_refresh_token_requires_login() {
        let temp = tempfile::tempdir().unwrap();
        let manager = manager_with_config(temp.path().to_path_buf());
        let cache = AuthCache {
            access_token: "token".to_string(),
            uuid: "uuid".to_string(),
            username: "Alice".to_string(),
            refresh_token: None,
            expires_at: Some(1),
        };

        let error = manager
            .ensure_fresh_auth_cache(cache)
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("mclc login"));
    }

    #[tokio::test]
    async fn external_login_binds_the_selected_profile() {
        let metadata =
            r#"{"meta":{"serverName":"Test Server"},"skinDomains":[],"signaturePublickey":"key"}"#;
        let authenticate = r#"{
            "accessToken": "initial-token",
            "clientToken": "client-token",
            "availableProfiles": [
                {"id": "00112233445566778899aabbccddeeff", "name": "Alice"}
            ],
            "selectedProfile": null,
            "user": {"id": "00112233445566778899aabbccddeeff", "properties": []}
        }"#;
        let refresh = r#"{
            "accessToken": "refreshed-token",
            "clientToken": "client-token",
            "availableProfiles": [
                {"id": "00112233445566778899aabbccddeeff", "name": "Alice"}
            ],
            "selectedProfile": {"id": "00112233445566778899aabbccddeeff", "name": "Alice"},
            "user": {"id": "00112233445566778899aabbccddeeff", "properties": []}
        }"#;
        let server = test_http::serve_sequence(vec![
            test_http::response(
                "200 OK",
                &[("X-Authlib-Injector-API-Location", "/api/yggdrasil")],
                b"",
            ),
            test_http::response("200 OK", &[], metadata.as_bytes()),
            test_http::response("200 OK", &[], authenticate.as_bytes()),
            test_http::response("200 OK", &[], refresh.as_bytes()),
        ])
        .await;
        let temp = tempfile::tempdir().unwrap();
        let manager = manager_with_config(temp.path().to_path_buf());
        fs::create_dir_all(&manager.config_dir).unwrap();

        let account = manager
            .external_login_with_selector("alice", "password", &server.base_url, |profiles| {
                Ok(profiles[0].clone())
            })
            .await
            .unwrap();

        assert_eq!(account.access_token, "refreshed-token");

        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 4);
        assert_eq!(requests[0].method, "GET");
        assert!(requests[3].method == "POST");
        assert!(requests[3].path.ends_with("/authserver/refresh"));
        assert!(requests[3].body.contains("selectedProfile"));
        assert!(requests[3]
            .body
            .contains("00112233445566778899aabbccddeeff"));
    }

    #[tokio::test]
    async fn finds_account_using_ali_resolved_url() {
        let server = test_http::serve_once(test_http::response(
            "200 OK",
            &[("X-Authlib-Injector-API-Location", "/api/yggdrasil")],
            b"",
        ))
        .await;
        let temp = tempfile::tempdir().unwrap();
        let manager = manager_with_config(temp.path().to_path_buf());
        let resolved_url = format!("{}/api/yggdrasil", server.base_url);
        manager
            .save_accounts(&[account(&resolved_url)])
            .unwrap();

        let found = manager
            .find_account_by_identifier_resolved("alice", &server.base_url)
            .await
            .unwrap();

        let found = found.expect("account must be found after ALI resolution");
        assert_eq!(found.api_url, resolved_url);
    }
}
