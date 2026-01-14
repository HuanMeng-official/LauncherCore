use crate::models::VersionDetails;
use anyhow::Context;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug)]
pub struct Launcher {
    pub minecraft_dir: PathBuf,
    pub versions_dir: PathBuf,
    pub libraries_dir: PathBuf,
    pub assets_dir: PathBuf,
}

impl Launcher {
    pub fn launch_game(
        &self,
        version_id: &str,
        username: String,
        access_token: String,
        uuid: String,
        user_type: String,
        jvm_args: Option<String>,
        java_path_override: Option<String>,
    ) -> anyhow::Result<()> {
        println!(
            "Launching Minecraft version: {} for user: {}",
            version_id, username
        );

        let version_dir = self.versions_dir.join(version_id);
        if !version_dir.exists() {
            return Err(anyhow::anyhow!("Version {} not found", version_id));
        }

        let version_json_path = version_dir.join(format!("{}.json", version_id));
        let version_json = fs::read_to_string(&version_json_path)?;
        let version_details: VersionDetails = serde_json::from_str(&version_json)?;

        let java_path = if let Some(override_path) = java_path_override {
            println!("Using explicitly provided Java path: {}", override_path);
            PathBuf::from(override_path)
        } else {
            self.find_java_from_env()?
        };

        println!("Using Java: {:?}", java_path);
        let classpath = self.build_classpath(&version_dir, &version_details)?;
        let version_natives_dir = version_dir.join("natives");

        let mut command_args = self.build_jvm_arguments(jvm_args, &version_natives_dir);
        command_args.push("-cp".to_string());
        command_args.push(classpath);
        command_args.push(version_details.main_class.clone());

        let game_args = self.build_game_args(
            &username,
            version_id,
            &access_token,
            &uuid,
            &user_type,
            &version_details,
        );
        command_args.extend(game_args);

        println!(
            "Launching with command: {} {:?}",
            java_path.display(),
            command_args
        );

        let status = Command::new(&java_path)
            .args(&command_args)
            .status()
            .context("Failed to start Minecraft")?;

        if status.success() {
            println!("Minecraft exited successfully");
        } else {
            eprintln!("Minecraft exited with error code: {:?}", status.code());
        }

        Ok(())
    }

    fn build_classpath(
        &self,
        version_dir: &PathBuf,
        version_details: &VersionDetails,
    ) -> anyhow::Result<String> {
        let mut classpath = Vec::new();
        classpath.push(version_dir.join(format!("{}.jar", version_details.id)));

        let is_version_1_16_5 = version_details.id == "1.16.5";
        if is_version_1_16_5 {
            println!("Detected version 1.16.5. Filtering out LWJGL 3.2.1 libraries.");
        }

        for library in &version_details.libraries {
            if is_version_1_16_5 && library.name.starts_with("org.lwjgl:") {
                let parts: Vec<&str> = library.name.split(':').collect();
                if parts.len() == 3 && parts[2] == "3.2.1" {
                    println!("  Skipping LWJGL 3.2.1 library: {}", library.name);
                    continue;
                }
            }

            if let Some(downloads) = &library.downloads {
                if let Some(artifact) = &downloads.artifact {
                    let library_path = self.libraries_dir.join(&artifact.path);
                    if library_path.exists() {
                        classpath.push(library_path);
                    }
                }
            }
        }

        Ok(classpath
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join(if cfg!(windows) { ";" } else { ":" }))
    }

    fn build_jvm_arguments(
        &self,
        custom_args: Option<String>,
        natives_dir: &PathBuf,
    ) -> Vec<String> {
        let mut args = vec![
            "-Xmx2G".to_string(),
            "-Xms1G".to_string(),
            "-XX:+UnlockExperimentalVMOptions".to_string(),
            "-XX:+UnlockDiagnosticVMOptions".to_string(),
            "-XX:+UseG1GC".to_string(),
            "-XX:G1MixedGCCountTarget=5".to_string(),
            "-XX:G1NewSizePercent=20".to_string(),
            "-XX:G1ReservePercent=20".to_string(),
            "-XX:MaxGCPauseMillis=50".to_string(),
            "-XX:G1HeapRegionSize=32m".to_string(),
            "-XX:-OmitStackTraceInFastThrow".to_string(),
            "-XX:-DontCompileHugeMethods".to_string(),
            "-XX:MaxNodeLimit=240000".to_string(),
            "-XX:NodeLimitFudgeFactor=8000".to_string(),
            "-XX:TieredCompileTaskTimeout=10000".to_string(),
            "-XX:ReservedCodeCacheSize=400M".to_string(),
            "-XX:NmethodSweepActivity=1".to_string(),
            format!("-Djava.library.path={}", natives_dir.to_string_lossy()),
        ];

        if let Some(custom) = custom_args {
            args.extend(custom.split_whitespace().map(String::from));
        }

        args
    }

    fn build_game_args(
        &self,
        username: &str,
        version_id: &str,
        access_token: &str,
        uuid: &str,
        user_type: &str,
        version_details: &VersionDetails,
    ) -> Vec<String> {
        let client_id = "0";
        let asset_index_id = version_details
            .asset_index
            .as_ref()
            .map(|ai| ai.id.clone())
            .unwrap_or_else(|| version_id.to_string());

        vec![
            "--username".to_string(),
            username.to_string(),
            "--version".to_string(),
            version_id.to_string(),
            "--gameDir".to_string(),
            self.minecraft_dir.to_string_lossy().to_string(),
            "--assetsDir".to_string(),
            self.assets_dir.to_string_lossy().to_string(),
            "--assetIndex".to_string(),
            asset_index_id,
            "--accessToken".to_string(),
            access_token.to_string(),
            "--clientId".to_string(),
            client_id.to_string(),
            "--uuid".to_string(),
            uuid.to_string(),
            "--userType".to_string(),
            user_type.to_string(),
            "--userProperties".to_string(),
            "{}".to_string(),
        ]
    }

    fn find_java_from_env(&self) -> anyhow::Result<PathBuf> {
        if let Ok(java_home) = std::env::var("JAVA_HOME") {
            let java_home_path = PathBuf::from(java_home);
            let java_bin = java_home_path.join("bin").join(if cfg!(target_os = "windows") {
                "java.exe"
            } else {
                "java"
            });

            if java_bin.exists() {
                println!("Found Java in JAVA_HOME: {:?}", java_home_path);
                return Ok(java_bin);
            } else {
                eprintln!(
                    "JAVA_HOME is set to '{:?}' but Java executable not found at '{:?}'",
                    java_home_path, java_bin
                );
            }
        } else {
            eprintln!("JAVA_HOME environment variable is not set.");
        }

        Err(anyhow::anyhow!("Java not found. Please set JAVA_HOME or use --runtime"))
    }
}
