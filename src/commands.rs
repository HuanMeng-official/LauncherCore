use crate::auth::Authenticator;
use crate::auth::yggdrasil::{
    YggdrasilAccount, YggdrasilAuthenticator, YggdrasilProfile, format_uuid, offline_uuid,
};
use crate::cli::{AuthType, InstanceAction};
use crate::game::instance::InstanceStore;
use crate::game::launch_manager::LauncherManager;
use crate::game::loader::LoaderKind;
use anyhow::Context;

pub fn instance(action: &InstanceAction) -> anyhow::Result<()> {
    let store = InstanceStore::new()?;
    match action {
        InstanceAction::List => {
            let instances = store.list()?;
            if instances.is_empty() {
                crate::outln!("No named instances registered.");
            } else {
                crate::outln!("Named instances ({}):", instances.len());
                for (name, entry) in instances {
                    crate::outln!("  {:<24} {}", name, entry.path.display());
                }
            }
            crate::outln!("Default game directory (no flags): ./.minecraft");
            crate::outln!("Instance data root: {}", store.instances_root().display());
        }
        InstanceAction::Create { name, path } => {
            let entry = store.create(name, path.as_deref())?;
            crate::outln!("Created instance '{}' at {}", name, entry.path.display());
            crate::outln!("Use it with: mclc --instance {} <command>", name);
        }
        InstanceAction::Delete { name, delete_files } => {
            let entry = store.delete(name, *delete_files)?;
            if *delete_files {
                crate::outln!(
                    "Deleted instance '{}' and its directory {}",
                    name,
                    entry.path.display()
                );
            } else {
                crate::outln!(
                    "Removed instance '{}' from the registry; files remain at {}",
                    name,
                    entry.path.display()
                );
            }
        }
    }
    Ok(())
}

pub async fn install(
    manager: &LauncherManager,
    version: &str,
    loader: Option<LoaderKind>,
    loader_version: Option<&str>,
    java_path: Option<String>,
) -> anyhow::Result<String> {
    match loader {
        Some(loader) => {
            manager
                .install_loader(loader, version, loader_version, java_path)
                .await
        }
        None => {
            manager.install_version(version).await?;
            Ok(version.to_string())
        }
    }
}

pub struct LaunchOptions {
    pub version: String,
    pub username: Option<String>,
    pub access_token: Option<String>,
    pub jvm_args: Option<String>,
    pub auth_type: AuthType,
    pub api_url: Option<String>,
    pub authlib_jar: Option<String>,
    pub java_path: Option<String>,
}

pub async fn launch(manager: &LauncherManager, options: LaunchOptions) -> anyhow::Result<()> {
    match options.auth_type {
        AuthType::Offline => {
            let username = options.username.unwrap_or_else(|| "Player".to_string());
            let access_token = options.access_token.unwrap_or_else(|| "0".to_string());
            let uuid = offline_uuid(&username);
            manager.launch(
                &options.version,
                username,
                access_token,
                uuid,
                "legacy".to_string(),
                options.jvm_args,
                options.java_path,
                None,
                None,
                None,
            )
        }
        AuthType::Msa => {
            if let Some(access_token) = options.access_token {
                let profile = Authenticator::default()
                    .get_minecraft_profile(&access_token)
                    .await?;
                manager.launch(
                    &options.version,
                    profile.name,
                    access_token,
                    format_uuid(&profile.id),
                    "msa".to_string(),
                    options.jvm_args,
                    options.java_path,
                    None,
                    None,
                    None,
                )
            } else {
                let Some(cache) = manager.load_auth_cache()? else {
                    anyhow::bail!(
                        "Authentication required but not found. Please run 'mclc login'."
                    );
                };
                let cache = manager.ensure_fresh_auth_cache(cache).await?;
                manager.launch(
                    &options.version,
                    cache.username,
                    cache.access_token,
                    cache.uuid,
                    "msa".to_string(),
                    options.jvm_args,
                    options.java_path,
                    None,
                    None,
                    None,
                )
            }
        }
        AuthType::External => {
            let api_url = options
                .api_url
                .as_deref()
                .context("--api-url is required for external auth")?;
            let username = options
                .username
                .as_deref()
                .context("--username is required for external auth")?;

            let Some(account) = manager
                .find_account_by_identifier_resolved(username, api_url)
                .await?
            else {
                anyhow::bail!(
                    "No cached credentials found for {} on {}. Please login first using external-login command.",
                    username,
                    api_url
                );
            };

            let authenticator = YggdrasilAuthenticator::new(account.api_url.clone());
            let account_to_use = if !authenticator
                .validate(&account.access_token, Some(&account.client_token))
                .await
            {
                crate::outln!("Token expired, refreshing...");
                let profile_for_refresh = YggdrasilProfile {
                    id: account.uuid.replace('-', ""),
                    name: account.name.clone(),
                    properties: None,
                };
                let response = authenticator
                    .refresh(
                        &account.access_token,
                        Some(&account.client_token),
                        Some(profile_for_refresh),
                    )
                    .await?;
                let updated_account = YggdrasilAccount {
                    access_token: response.access_token,
                    client_token: response.client_token,
                    ..account.clone()
                };
                manager.save_account(&updated_account)?;
                crate::outln!("Token refreshed for {}", updated_account.get_display_name());
                updated_account
            } else {
                crate::outln!(
                    "Using cached credentials for {}",
                    account.get_display_name()
                );
                account
            };

            let jar_path = match options.authlib_jar {
                Some(path) => std::path::PathBuf::from(path),
                None => manager.get_authlib_injector().get_or_download().await?,
            };
            let prefetched = authenticator.pre_fetch_metadata().await?;

            manager.launch(
                &options.version,
                account_to_use.name,
                account_to_use.access_token,
                account_to_use.uuid,
                "mojang".to_string(),
                options.jvm_args,
                options.java_path,
                Some(jar_path),
                Some(prefetched),
                Some(account_to_use.api_url),
            )
        }
    }
}
