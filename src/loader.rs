//! Mod loader installation entry point and shared dispatch.
//!
//! Fabric/Quilt use their metadata APIs; Forge and NeoForge resolve versions
//! from their own repositories and share the installer runner.

mod fabric;
mod fabric_like;
mod forge;
mod installer;
mod neoforge;
mod quilt;

use crate::install::Installer;

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum LoaderKind {
    Fabric,
    Quilt,
    Forge,
    Neoforge,
}

impl LoaderKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Fabric => "Fabric",
            Self::Quilt => "Quilt",
            Self::Forge => "Forge",
            Self::Neoforge => "NeoForge",
        }
    }
}

pub async fn install_loader(
    installer: &Installer,
    kind: LoaderKind,
    game_version: &str,
    loader_version: Option<&str>,
    java_path_override: Option<String>,
) -> anyhow::Result<String> {
    crate::outln!(
        "Installing {} for Minecraft {}...",
        kind.label(),
        game_version
    );
    crate::output::progress(
        "loader",
        0,
        0,
        format!("Installing {} for Minecraft {}", kind.label(), game_version),
    );
    installer.install_version(game_version).await?;

    let version_id = match kind {
        LoaderKind::Fabric => fabric::install(installer, game_version, loader_version).await?,
        LoaderKind::Quilt => quilt::install(installer, game_version, loader_version).await?,
        LoaderKind::Forge => {
            forge::install_forge(installer, game_version, loader_version, java_path_override)
                .await?
        }
        LoaderKind::Neoforge => {
            neoforge::install_neoforge(installer, game_version, loader_version, java_path_override)
                .await?
        }
    };

    crate::outln!(
        "Installed {} for Minecraft {} as version '{}'.",
        kind.label(),
        game_version,
        version_id
    );
    crate::outln!("Launch with: mclc launch {}", version_id);
    crate::output::progress("loader", 1, 1, format!("Installed {}", version_id));
    Ok(version_id)
}
