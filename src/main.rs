mod auth;
mod cli;
mod commands;
mod common;
mod game;
mod net;
mod server;
#[cfg(test)]
mod test_http;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Commands};
use game::java;
use game::launch_manager::LauncherManager;

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
        Commands::Json => {
            server::json::run(manager, global_java_path).await?;
        }
        Commands::Web {
            port,
            host,
            token,
            allow_origins,
        } => {
            server::web::run(
                server::web::WebOptions {
                    host: host.clone(),
                    port: *port,
                    token: token.clone(),
                    allow_origins: allow_origins.clone(),
                },
                cli.download_source,
                global_java_path,
            )
            .await?;
        }
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
                        Some(major) if version == major.to_string() => {
                            println!("  Java {} ({}) - {}", major, arch, runtime.path.display())
                        }
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
        } => {
            commands::install(
                &manager,
                version,
                *loader,
                loader_version.as_deref(),
                global_java_path,
            )
            .await?;
        }
        Commands::Login => match manager.login().await {
            Ok(auth_cache) => {
                manager.save_auth_cache(&auth_cache)?;
                println!("Login successful! You can now launch with '--auth msa'.");
            }
            Err(e) => {
                eprintln!("Authentication failed: {}", e);
                std::process::exit(1);
            }
        },
        Commands::Launch {
            version,
            username,
            access_token,
            jvm_args,
            auth_type,
            api_url,
            authlib_jar,
        } => {
            commands::launch(
                &manager,
                commands::LaunchOptions {
                    version: version.clone(),
                    username: username.clone(),
                    access_token: access_token.clone(),
                    jvm_args: jvm_args.clone(),
                    auth_type: *auth_type,
                    api_url: api_url.clone(),
                    authlib_jar: authlib_jar.clone(),
                    java_path: global_java_path,
                },
            )
            .await?;
        }
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
                    println!(
                        "You can now launch with: mclc launch --version <version> --auth external --api-url {} --username {}",
                        api_url, identifier
                    );
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
