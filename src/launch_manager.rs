use crate::auth::Authenticator;
use crate::install::Installer;
use crate::launch::Launcher;
use crate::models::AuthCache;
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
    ) -> anyhow::Result<()> {
        self.launcher.launch_game(
            version_id,
            username,
            access_token,
            uuid,
            user_type,
            jvm_args,
            java_path,
        )
    }
}

impl Default for LauncherManager {
    fn default() -> Self {
        Self::new().expect("Failed to initialize LauncherManager")
    }
}
