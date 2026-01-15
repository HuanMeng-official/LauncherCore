mod cli;
mod error;
mod install;
mod launch;
mod launch_manager;
mod models;
mod auth;
mod yggdrasil;

use anyhow::Result;
use clap::Parser;
use cli::{AuthType, Cli, Commands};
use error::LauncherError;
use launch_manager::LauncherManager;
use yggdrasil::YggdrasilAuthenticator;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let manager = LauncherManager::new()?;
    let global_java_path = cli.java_runtime_path;

    match &cli.command {
        Commands::List => {
            manager.list_versions().await?;
        }
        Commands::Install { version } => {
            manager.install_version(&version).await?;
        }
        Commands::Login => {
            match manager.login().await {
                Ok(auth_cache) => {
                    manager.save_auth_cache(&auth_cache)?;
                    println!("Login successful! You can now launch with '--auth msa'.");
                }
                Err(e) => {
                    eprintln!("Authentication failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::Launch {
            version,
            username,
            access_token,
            jvm_args,
            auth_type,
            api_url,
        } => match auth_type {
            AuthType::Offline => {
                let launch_username = username.clone().unwrap_or_else(|| "Player".to_string());
                let launch_access_token = access_token.clone().unwrap_or_else(|| "0".to_string());
                let launch_uuid = "00000000-0000-0000-0000-000000000000".to_string();

                manager.launch(
                    &version,
                    launch_username,
                    launch_access_token,
                    launch_uuid,
                    "legacy".to_string(),
                    jvm_args.clone(),
                    global_java_path,
                    None,
                    None,
                )?;
            }
            AuthType::Msa => {
                match manager.load_auth_cache()? {
                    Some(auth_cache) => {
                        manager.launch(
                            &version,
                            auth_cache.username,
                            auth_cache.access_token,
                            auth_cache.uuid,
                            "msa".to_string(),
                            jvm_args.clone(),
                            global_java_path,
                            None,
                            None,
                        )?;
                    }
                    None => {
                        eprintln!("{}", LauncherError::AuthNotFound);
                        std::process::exit(1);
                    }
                }
            }
            AuthType::External => {
                let api_url = api_url.as_ref().expect("--api-url is required for external auth");
                let Some(username) = username else {
                    eprintln!("--username is required for external auth");
                    std::process::exit(1);
                };

                // Try to find existing account
                if let Some(account) = manager.find_account_by_identifier(&username, api_url)? {
                    // Validate the token
                    let authenticator = YggdrasilAuthenticator::new(account.api_url.clone());
                    if authenticator.validate(&account.access_token, Some(&account.client_token)).await {
                        println!("Using cached credentials for {}", account.get_display_name());

                        // Download authlib-injector if needed
                        let authlib_injector = manager.get_authlib_injector();
                        let jar_path = authlib_injector.get_or_download().await?;

                        // Pre-fetch metadata
                        let prefetched = authenticator.pre_fetch_metadata().await?;

                        manager.launch(
                            &version,
                            account.name.clone(),
                            account.access_token.clone(),
                            account.uuid.clone(),
                            "mojang".to_string(),
                            jvm_args.clone(),
                            global_java_path,
                            Some(jar_path),
                            Some(prefetched),
                        )?;
                    } else {
                        eprintln!("Cached credentials expired. Please login again using external-login command.");
                        std::process::exit(1);
                    }
                } else {
                    eprintln!("No cached credentials found for {} on {}. Please login first using external-login command.",
                        username, api_url);
                    std::process::exit(1);
                }
            }
        },
        Commands::ExternalLogin {
            identifier,
            password,
            api_url,
        } => {
            match manager.external_login(&identifier, &password, &api_url).await {
                Ok(_) => {
                    println!("External login successful!");
                    println!("You can now launch with: mclc launch --version <version> --auth external --api-url {} --username {}",
                        api_url, identifier);
                }
                Err(e) => {
                    eprintln!("External authentication failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }

    Ok(())
}
