use crate::auth::Authenticator;
use crate::install::Installer;
use crate::launch::Launcher;
use crate::models::AuthCache;
use crate::yggdrasil::{AuthlibInjector, YggdrasilAccount, YggdrasilAuthenticator};
use std::fs;
use std::path::PathBuf;

#[derive(Debug)]
pub struct LauncherManager {
    pub launcher: Launcher,
    pub installer: Installer,
    pub authenticator: Authenticator,
    pub config_dir: PathBuf,
}

impl LauncherManager {
    pub fn new() -> anyhow::Result<Self> {
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
                versions_dir,
                libraries_dir,
                assets_objects_dir,
                assets_indexes_dir,
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
        fs::write(&cache_path, json)?;
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
        fs::write(&accounts_path, json)?;
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
        let normalized_url = api_url.trim_end_matches('/');
        for account in accounts {
            if account.identifier == identifier && account.api_url.trim_end_matches('/') == normalized_url {
                return Ok(Some(account));
            }
        }
        Ok(None)
    }

    pub async fn external_login(
        &self,
        identifier: &str,
        password: &str,
        api_url: &str,
    ) -> anyhow::Result<YggdrasilAccount> {
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
        let auth_response = authenticator.authenticate(identifier, password).await?;

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

    pub async fn login(&self) -> anyhow::Result<AuthCache> {
        self.authenticator.perform_full_authentication().await
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
        Self::new().expect("Failed to initialize LauncherManager")
    }
}
