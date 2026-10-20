mod cli;
mod error;
mod install;
mod launch;
mod launch_manager;
mod models;
mod auth;
mod yggdrasil;
mod java;
mod download;
mod version;
mod loader;
#[cfg(test)]
mod test_http;

use anyhow::Result;
use auth::Authenticator;
use clap::Parser;
use cli::{AuthType, Cli, Commands};
use error::LauncherError;
use launch_manager::LauncherManager;
use yggdrasil::{
    YggdrasilAccount, YggdrasilAuthenticator, YggdrasilProfile, format_uuid, offline_uuid,
};

fn resolve_password(cli_password: Option<&str>) -> Result<String> {
    if let Some(password) = cli_password {
        eprintln!(
            "Warning: passing --password on the command line can expose it in shell history and process listings."
        );
        return Ok(password.to_string());
    }

    if let Ok(password) = std::env::var("MCLC_PASSWORD") {
        if !password.is_empty() {
            return Ok(password);
        }
    }

    let password = rpassword::prompt_password("Password: ")?;
    if password.is_empty() {
        anyhow::bail!("Password cannot be empty");
    }
    Ok(password)
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let manager = LauncherManager::new(cli.download_source)?;
    let global_java_path = cli.java_runtime_path;

    match &cli.command {
        Commands::List => {
            manager.list_versions().await?;
        }
        Commands::Java => {
            let mut runtimes = java::detect_java_runtimes();
            if runtimes.is_empty() {
                println!("No Java runtime found.");
            } else {
                runtimes.sort_by(|left, right| {
                    right
                        .version
                        .cmp(&left.version)
                        .then_with(|| left.path.cmp(&right.path))
                });
                println!("Detected {} Java runtime(s):", runtimes.len());
                for runtime in &runtimes {
                    let version = runtime.version_string.as_deref().unwrap_or("unknown");
                    let arch = runtime.arch.as_deref().unwrap_or("unknown");
                    match runtime.version {
                        Some(major) if version == major.to_string() => println!(
                            "  Java {} ({}) - {}",
                            major,
                            arch,
                            runtime.path.display()
                        ),
                        Some(major) => println!(
                            "  Java {} ({}, {}) - {}",
                            major,
                            version,
                            arch,
                            runtime.path.display()
                        ),
                        None => println!(
                            "  Java ? ({}, {}) - {}",
                            version,
                            arch,
                            runtime.path.display()
                        ),
                    }
                }
            }
        }
        Commands::Install {
            version,
            loader,
            loader_version,
        } => match loader {
            Some(loader) => {
                manager
                    .install_loader(
                        *loader,
                        version,
                        loader_version.as_deref(),
                        global_java_path,
                    )
                    .await?;
            }
            None => {
                manager.install_version(version).await?;
            }
        },
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
                let launch_uuid = offline_uuid(&launch_username);

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
                if let Some(access_token) = access_token {
                    let profile = Authenticator::default()
                        .get_minecraft_profile(access_token)
                        .await?;

                    manager.launch(
                        &version,
                        profile.name,
                        access_token.clone(),
                        format_uuid(&profile.id),
                        "msa".to_string(),
                        jvm_args.clone(),
                        global_java_path,
                        None,
                        None,
                        None,
                    )?;
                } else {
                    match manager.load_auth_cache()? {
                        Some(auth_cache) => {
                            let auth_cache =
                                manager.ensure_fresh_auth_cache(auth_cache).await?;
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
            }
            AuthType::External => {
                let api_url = api_url.as_ref().expect("--api-url is required for external auth");
                let Some(username) = username else {
                    eprintln!("--username is required for external auth");
                    std::process::exit(1);
                };

                // Try to find existing account
                if let Some(account) = manager
                    .find_account_by_identifier_resolved(&username, api_url)
                    .await?
                {
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
            let password = match resolve_password(password.as_deref()) {
                Ok(password) => password,
                Err(e) => {
                    eprintln!("Failed to read password: {}", e);
                    std::process::exit(1);
                }
            };

            match manager.external_login(identifier, &password, api_url).await {
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
