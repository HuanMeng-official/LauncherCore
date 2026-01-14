mod cli;
mod error;
mod install;
mod launch;
mod launch_manager;
mod models;
mod auth;

use anyhow::Result;
use clap::Parser;
use cli::{AuthType, Cli, Commands};
use error::LauncherError;
use launch_manager::LauncherManager;

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
                    println!("Login successful! You can now launch with '--auth-type msa'.");
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
                        )?;
                    }
                    None => {
                        eprintln!("{}", LauncherError::AuthNotFound);
                        std::process::exit(1);
                    }
                }
            }
        },
    }

    Ok(())
}
