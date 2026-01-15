use crate::models::{Classifiers, Library, VersionDetails};
use anyhow::Context;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

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
        authlib_injector_jar: Option<std::path::PathBuf>,
        prefetched_metadata: Option<String>,
        api_url: Option<String>,
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

        // Verify and extract native libraries if needed
        let version_natives_dir = version_dir.join("natives");
        self.verify_and_extract_natives(
            &version_details, &version_natives_dir)?;

        let classpath = self.build_classpath(
            &version_dir, &version_details)?;

        let mut command_args = self.build_jvm_arguments(
            jvm_args,
            &version_natives_dir,
            authlib_injector_jar.as_ref(),
            prefetched_metadata.as_ref(),
            api_url.as_deref()
        );
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

        // Spawn the game process and let launcher exit
        let mut cmd = Command::new(&java_path);
        cmd.args(&command_args);

        #[cfg(target_os = "windows")]
        {
            cmd.creation_flags(0x00000008); // DETACHED_PROCESS
        }

        let _ = cmd.spawn().context("Failed to start Minecraft")?;

        println!("Minecraft launched successfully");
        println!("Please wait patiently for the game window to appear");
        Ok(())
    }

    fn verify_and_extract_natives(
        &self,
        version_details: &VersionDetails,
        natives_dir: &Path,
    ) -> anyhow::Result<()> {
        if !natives_dir.exists() {
            fs::create_dir_all(natives_dir)?;
        }

        let mut needs_extraction = false;

        for library in &version_details.libraries {
            if !self.should_include_library(library) {
                continue;
            }

            if let Some(downloads) = &library.downloads {
                if let Some(classifiers) = &downloads.classifiers {
                    if let Some(artifact) = self.get_native_artifact(classifiers) {
                        let native_path = self.libraries_dir.join(&artifact.path);

                        // Check if native library JAR exists and has been extracted
                        if native_path.exists() {
                            // Check if at least one native file exists
                            let has_natives = self.check_natives_exist(natives_dir);
                            if !has_natives {
                                needs_extraction = true;
                                break;
                            }
                        } else {
                            println!("Warning: Native library not found: {:?}. Please run install first.", native_path);
                        }
                    }

                    // Check other natives (natives-windows, natives-linux, etc.)
                    for (classifier_name, artifact) in &classifiers.other {
                        if classifier_name.contains("natives-") {
                            let native_path = self.libraries_dir.join(&artifact.path);
                            if native_path.exists() {
                                let has_natives = self.check_natives_exist(natives_dir);
                                if !has_natives {
                                    needs_extraction = true;
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }

        if needs_extraction {
            println!("Extracting native libraries...");
            for library in &version_details.libraries {
                if !self.should_include_library(library) {
                    continue;
                }

                if let Some(downloads) = &library.downloads {
                    if let Some(classifiers) = &downloads.classifiers {
                        if let Some(artifact) = self.get_native_artifact(classifiers) {
                            let native_path = self.libraries_dir.join(&artifact.path);
                            if native_path.exists() {
                                self.extract_lwjgl3_native_library(
                                    &native_path, natives_dir)?;
                            }
                        }

                        for (classifier_name, artifact) in &classifiers.other {
                            if classifier_name.contains("natives-") {
                                let native_path = self.libraries_dir.join(&artifact.path);
                                if native_path.exists() {
                                    self.extract_lwjgl3_native_library(
                                        &native_path, natives_dir)?;
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn check_natives_exist(
        &self, natives_dir: &Path
    ) -> bool {
        if !natives_dir.exists() {
            return false;
        }

        // Check for at least one native library file
        let entries = match fs::read_dir(natives_dir) {
            Ok(e) => e,
            Err(_) => return false,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let ext = path.extension();
            if let Some(e) = ext {
                match e.to_str() {
                    Some("dll") | Some("so") | Some("dylib") => return true,
                    _ => continue,
                }
            }
        }

        false
    }

    fn should_include_library(
        &self, library: &Library
    ) -> bool {
        if let Some(rules) = &library.rules {
            let mut allowed = false;
            for rule in rules {
                let matches = match &rule.os {
                    Some(os_rule) => {
                        match std::env::consts::OS {
                            "windows" => os_rule.name == "windows",
                            "linux" => os_rule.name == "linux",
                            "macos" => os_rule.name == "osx",
                            _ => false,
                        }
                    }
                    None => true,
                };

                if rule.action == "allow" {
                    if matches {
                        allowed = true;
                    }
                } else if rule.action == "disallow" {
                    if matches {
                        return false;
                    }
                }
            }

            // If there are rules but none allowed, check if default should be disallow
            // Minecraft's rule system: if action is "allow", it applies when matches
            // If action is "disallow", it applies when matches
            // If no rules match, the default behavior depends on the last rule
            return allowed;
        }
        true
    }

    fn get_native_artifact<'a>(
        &self, classifiers: &'a Classifiers
    ) -> Option<&'a crate::models::Artifact> {
        let os_name = match std::env::consts::OS {
            "windows" => "windows",
            "linux" => "linux",
            "macos" => "macos",
            _ => return None,
        };
        let arch_name = match std::env::consts::ARCH {
            "aarch64" => "aarch64",
            "x86_64" => "x64",
            _ => std::env::consts::ARCH,
        };

        let specific_key = format!("natives-{}-{}", os_name, arch_name);
        if let Some(artifact) = classifiers.other.get(&specific_key) {
            return Some(artifact);
        }

        match std::env::consts::OS {
            "windows" => classifiers
                .natives_windows
                .as_ref()
                .or_else(|| classifiers.other.get("natives-windows")),
            "linux" => classifiers
                .natives_linux
                .as_ref()
                .or_else(|| classifiers.other.get("natives-linux")),
            "macos" => classifiers
                .natives_macos
                .as_ref()
                .or_else(|| classifiers.other.get("natives-macos")),
            _ => None,
        }
    }

    fn extract_lwjgl3_native_library(
        &self, jar_path: &Path, extract_dir: &Path
    ) -> anyhow::Result<()> {
        let file = std::fs::File::open(jar_path)
            .with_context(|| format!("Failed to open native JAR file: {:?}", jar_path))?;
        let mut archive = zip::ZipArchive::new(file)
            .with_context(|| format!("Failed to read ZIP archive: {:?}", jar_path))?;

        for i in 0..archive.len() {
            let mut file = archive
                .by_index(i)
                .with_context(|| format!("Failed to get file entry {} from archive: {:?}", i, jar_path))?;
            let outpath = extract_dir.join(file.mangled_name());
            let file_name = file.name().to_lowercase();

            if file_name.ends_with(".dll") || file_name.ends_with(".so") || file_name.ends_with(".dylib") {
                if let Some(p) = outpath.parent() {
                    if !p.exists() {
                        fs::create_dir_all(p).with_context(|| {
                            format!("Failed to create directory for native file: {:?}", p)
                        })?;
                    }
                }
                let mut outfile = std::fs::File::create(&outpath)
                    .with_context(|| format!("Failed to create output file for native: {:?}", outpath))?;
                std::io::copy(&mut file, &mut outfile)
                    .with_context(|| format!("Failed to write native file: {:?}", outpath))?;
            }
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

        for library in &version_details.libraries {
            // Check if library should be included based on rules
            if !self.should_include_library(library) {
                continue;
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
        authlib_injector_jar: Option<&std::path::PathBuf>,
        prefetched_metadata: Option<&String>,
        api_url: Option<&str>,
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

        // Add authlib-injector arguments if provided
        if let (Some(jar_path), Some(prefetched)) = (authlib_injector_jar, prefetched_metadata) {
            // -javaagent:{jar_path}={api_url}
            // -Dauthlibinjector.yggdrasil.prefetched={base64_metadata}
            let api = api_url.unwrap_or("");
            args.insert(6, format!("-javaagent:{}={}", jar_path.display(), api));
            args.insert(7, format!("-Dauthlibinjector.yggdrasil.prefetched={}", prefetched));
            println!("Using authlib-injector: {} with API: {}", jar_path.display(), api);
        }

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

    fn find_java_from_env(
        &self
    ) -> anyhow::Result<PathBuf> {
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
