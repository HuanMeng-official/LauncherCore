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
use yggdrasil::{YggdrasilAccount, YggdrasilAuthenticator, YggdrasilProfile};

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
            authlib_jar,
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
                    let authenticator = YggdrasilAuthenticator::new(account.api_url.clone());

                    // Validate the token, if expired try to refresh
                    let account_to_use = if !authenticator.validate(&account.access_token, Some(&account.client_token)).await {
                        println!("Token expired, refreshing...");

                        // Need to create a profile for refresh (remove dashes from UUID for the API)
                        let profile_for_refresh = YggdrasilProfile {
                            id: account.uuid.replace('-', ""),
                            name: account.name.clone(),
                            properties: None,
                        };

                        match authenticator.refresh(&account.access_token, Some(&account.client_token), Some(profile_for_refresh)).await {
                            Ok(response) => {
                                let updated_account = YggdrasilAccount {
                                    access_token: response.access_token.clone(),
                                    client_token: response.client_token,
                                    ..account.clone()
                                };
                                manager.save_account(&updated_account)?;
                                println!("Token refreshed for {}", updated_account.get_display_name());
                                updated_account
                            }
                            Err(e) => {
                                eprintln!("Failed to refresh token: {}", e);
                                eprintln!("Please login again using external-login command.");
                                std::process::exit(1);
                            }
                        }
                    } else {
                        println!("Using cached credentials for {}", account.get_display_name());
                        account.clone()
                    };

                    // Get authlib-injector jar path - either from provided path or auto-download
                    let jar_path = if let Some(custom_jar_path) = authlib_jar {
                        println!("Using custom authlib-injector: {}", custom_jar_path);
                        std::path::PathBuf::from(custom_jar_path)
                    } else {
                        let authlib_injector = manager.get_authlib_injector();
                        authlib_injector.get_or_download().await?
                    };

                    // Pre-fetch metadata
                    let prefetched = authenticator.pre_fetch_metadata().await?;

                    manager.launch(
                        &version,
                        account_to_use.name.clone(),
                        account_to_use.access_token.clone(),
                        account_to_use.uuid.clone(),
                        "mojang".to_string(),
                        jvm_args.clone(),
                        global_java_path,
                        Some(jar_path),
                        Some(prefetched),
                        Some(account_to_use.api_url.clone()),
                    )?;
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
