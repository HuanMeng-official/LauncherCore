use crate::java::select_java;
use crate::models::VersionDetails;
use anyhow::Context;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[derive(Clone, Copy)]
struct ArgumentContext<'a> {
    os: &'a str,
    arch: &'a str,
    game_dir: &'a str,
    assets_dir: &'a str,
    natives_dir: &'a str,
    library_dir: &'a str,
    classpath: &'a str,
    classpath_separator: &'a str,
    version_name: &'a str,
    version_type: &'a str,
    asset_index_name: &'a str,
    username: &'a str,
    access_token: &'a str,
    uuid: &'a str,
    user_type: &'a str,
    launcher_name: &'a str,
    launcher_version: &'a str,
}

fn substitute_placeholders(value: &str, ctx: &ArgumentContext) -> String {
    let replacements = [
        ("auth_player_name", ctx.username),
        ("version_name", ctx.version_name),
        ("game_directory", ctx.game_dir),
        ("assets_root", ctx.assets_dir),
        ("game_assets", ctx.assets_dir),
        ("assets_index_name", ctx.asset_index_name),
        ("auth_uuid", ctx.uuid),
        ("auth_access_token", ctx.access_token),
        ("user_type", ctx.user_type),
        ("version_type", ctx.version_type),
        ("natives_directory", ctx.natives_dir),
        ("launcher_name", ctx.launcher_name),
        ("launcher_version", ctx.launcher_version),
        ("classpath", ctx.classpath),
        ("classpath_separator", ctx.classpath_separator),
        ("library_directory", ctx.library_dir),
        ("auth_xuid", ""),
        ("clientid", "0"),
        ("user_properties", "{}"),
    ];

    let mut result = value.to_string();
    for (placeholder, replacement) in replacements {
        result = result.replace(&format!("${{{}}}", placeholder), replacement);
    }

    let auth_session = format!("token:{}:{}", ctx.access_token, ctx.uuid);
    result.replace("${auth_session}", &auth_session)
}

fn os_name_matches(rule_name: &str, actual_os: &str) -> bool {
    rule_name == actual_os || (rule_name == "osx" && actual_os == "macos")
}

fn rule_matches(rule: &serde_json::Value, ctx: &ArgumentContext) -> bool {
    if let Some(os) = rule.get("os") {
        if let Some(name) = os.get("name").and_then(|value| value.as_str()) {
            if !os_name_matches(name, ctx.os) {
                return false;
            }
        }

        if let Some(arch) = os.get("arch").and_then(|value| value.as_str()) {
            if arch != crate::models::normalize_arch(ctx.arch) {
                return false;
            }
        }

        if let Some(version) = os.get("version").and_then(|value| value.as_str()) {
            if !version.is_empty() {
                return false;
            }
        }
    }

    if let Some(features) = rule.get("features").and_then(|value| value.as_object()) {
        for enabled in features.values() {
            if enabled.as_bool().unwrap_or(false) {
                return false;
            }
        }
    }

    true
}

fn rules_allow(rules: &[serde_json::Value], ctx: &ArgumentContext) -> bool {
    let mut allowed = false;
    for rule in rules {
        let action = rule
            .get("action")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let matches = rule_matches(rule, ctx);

        if action == "allow" && matches {
            allowed = true;
        } else if action == "disallow" && matches {
            return false;
        }
    }
    allowed
}

fn evaluate_arguments(
    arguments: &[serde_json::Value],
    ctx: &ArgumentContext,
) -> Vec<String> {
    let mut evaluated = Vec::new();

    for argument in arguments {
        match argument {
            serde_json::Value::String(value) => {
                evaluated.push(substitute_placeholders(value, ctx));
            }
            serde_json::Value::Object(object) => {
                let allowed = object
                    .get("rules")
                    .and_then(|rules| rules.as_array())
                    .map(|rules| rules_allow(rules, ctx))
                    .unwrap_or(true);
                if !allowed {
                    continue;
                }

                match object.get("value") {
                    Some(serde_json::Value::String(value)) => {
                        evaluated.push(substitute_placeholders(value, ctx));
                    }
                    Some(serde_json::Value::Array(values)) => {
                        for value in values {
                            if let Some(value) = value.as_str() {
                                evaluated.push(substitute_placeholders(value, ctx));
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    evaluated
}

fn default_jvm_arguments(natives_dir: &str) -> Vec<String> {
    vec![
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
        format!("-Djava.library.path={}", natives_dir),
    ]
}

fn build_jvm_arguments(
    version_details: &VersionDetails,
    ctx: &ArgumentContext,
    custom_args: Option<&str>,
    authlib_injector_jar: Option<&std::path::Path>,
    prefetched_metadata: Option<&str>,
    api_url: Option<&str>,
) -> Vec<String> {
    let mut args = Vec::new();

    let manifest_jvm_arguments = version_details
        .arguments
        .as_ref()
        .map(|arguments| arguments.jvm.as_slice())
        .filter(|arguments| !arguments.is_empty());

    if let Some(arguments) = manifest_jvm_arguments {
        args.push("-Xmx2G".to_string());
        args.push("-Xms1G".to_string());

        // ${version_name} inside JVM arguments refers to the resolved game
        // jar (e.g. 1.21.1), not the loader profile id. NeoForge relies on
        // this for -DignoreList=client-extra,${version_name}.jar.
        let jar_name = version_details
            .jar
            .as_deref()
            .unwrap_or(&version_details.id);
        let jvm_ctx = ArgumentContext {
            version_name: jar_name,
            ..*ctx
        };
        args.extend(evaluate_arguments(arguments, &jvm_ctx));
    } else {
        args.extend(default_jvm_arguments(ctx.natives_dir));
    }

    if let (Some(jar_path), Some(prefetched)) = (authlib_injector_jar, prefetched_metadata) {
        let api = api_url.unwrap_or("");
        args.push(format!("-javaagent:{}={}", jar_path.display(), api));
        args.push(format!("-Dauthlibinjector.yggdrasil.prefetched={}", prefetched));
        println!(
            "Using authlib-injector: {} with API: {}",
            jar_path.display(),
            api
        );
    }

    if !args.iter().any(|arg| arg.starts_with("-Djava.library.path=")) {
        args.push(format!("-Djava.library.path={}", ctx.natives_dir));
    }

    if !args.iter().any(|arg| arg == "-cp" || arg == "-classpath") {
        args.push("-cp".to_string());
        args.push(ctx.classpath.to_string());
    }

    if let Some(custom) = custom_args {
        args.extend(custom.split_whitespace().map(String::from));
    }

    args
}

fn redact_sensitive_arguments(args: &[String]) -> Vec<String> {
    let mut redacted = args.to_vec();
    let mut redact_next = false;

    for arg in &mut redacted {
        if redact_next {
            *arg = "<redacted>".to_string();
            redact_next = false;
            continue;
        }

        if matches!(arg.as_str(), "--accessToken" | "--session" | "--access-token") {
            redact_next = true;
        } else if let Some((flag, _)) = arg.split_once('=') {
            if flag == "--accessToken" || flag == "--session" {
                let replacement = format!("{}=<redacted>", flag);
                *arg = replacement;
            }
        }
    }

    redacted
}

fn build_game_arguments(
    version_details: &VersionDetails,
    ctx: &ArgumentContext,
) -> Vec<String> {
    if let Some(arguments) = &version_details.arguments {
        if !arguments.game.is_empty() {
            return evaluate_arguments(&arguments.game, ctx);
        }
    }

    if let Some(legacy_arguments) = &version_details.minecraft_arguments {
        return legacy_arguments
            .split_whitespace()
            .map(|argument| substitute_placeholders(argument, ctx))
            .collect();
    }

    vec![
        "--username".to_string(),
        ctx.username.to_string(),
        "--version".to_string(),
        ctx.version_name.to_string(),
        "--gameDir".to_string(),
        ctx.game_dir.to_string(),
        "--assetsDir".to_string(),
        ctx.assets_dir.to_string(),
        "--assetIndex".to_string(),
        ctx.asset_index_name.to_string(),
        "--accessToken".to_string(),
        ctx.access_token.to_string(),
        "--clientId".to_string(),
        "0".to_string(),
        "--uuid".to_string(),
        ctx.uuid.to_string(),
        "--userType".to_string(),
        ctx.user_type.to_string(),
        "--userProperties".to_string(),
        "{}".to_string(),
        "--versionType".to_string(),
        ctx.version_type.to_string(),
    ]
}

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

        let version_details =
            crate::version::load_version_details(&self.versions_dir, version_id)?;

        let required_java = version_details
            .java_version
            .as_ref()
            .map(|java| java.major_version);
        let java_path = select_java(required_java, java_path_override)?;

        println!("Using Java: {:?}", java_path);

        // Verify and extract native libraries if needed
        let version_natives_dir = version_dir.join("natives");
        self.verify_and_extract_natives(
            &version_details, &version_natives_dir)?;

        let classpath = self.build_classpath(&version_details)?;

        let game_dir = self.minecraft_dir.to_string_lossy().to_string();
        let assets_dir = self.assets_dir.to_string_lossy().to_string();
        let natives_dir = version_natives_dir.to_string_lossy().to_string();
        let library_dir = self.libraries_dir.to_string_lossy().to_string();
        let classpath_separator = if cfg!(windows) { ";" } else { ":" };
        let asset_index_name = version_details
            .asset_index
            .as_ref()
            .map(|index| index.id.clone())
            .unwrap_or_else(|| version_id.to_string());
        let ctx = ArgumentContext {
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            game_dir: &game_dir,
            assets_dir: &assets_dir,
            natives_dir: &natives_dir,
            library_dir: &library_dir,
            classpath: &classpath,
            classpath_separator,
            version_name: version_id,
            version_type: &version_details.version_type,
            asset_index_name: &asset_index_name,
            username: &username,
            access_token: &access_token,
            uuid: &uuid,
            user_type: &user_type,
            launcher_name: "mclc",
            launcher_version: env!("CARGO_PKG_VERSION"),
        };

        let mut command_args = build_jvm_arguments(
            &version_details,
            &ctx,
            jvm_args.as_deref(),
            authlib_injector_jar.as_deref(),
            prefetched_metadata.as_deref(),
            api_url.as_deref(),
        );
        command_args.push(version_details.main_class.clone());
        command_args.extend(build_game_arguments(&version_details, &ctx));

        println!(
            "Launching with command: {} {:?}",
            java_path.display(),
            redact_sensitive_arguments(&command_args)
        );

        // Spawn the game process and let launcher exit
        let mut cmd = Command::new(&java_path);
        cmd.args(&command_args);
        // Match the official launcher: relative paths (logs, crash reports,
        // options) must resolve inside the game directory.
        cmd.current_dir(&self.minecraft_dir);

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

        let os = std::env::consts::OS;
        let arch = std::env::consts::ARCH;
        let natives = crate::models::selected_native_artifacts(
            &version_details.libraries,
            os,
            arch,
        );
        let mut needs_extraction = false;

        for artifact in &natives {
            let native_path = self.libraries_dir.join(&artifact.path);

            if native_path.exists() {
                if !self.check_natives_exist(natives_dir) {
                    needs_extraction = true;
                    break;
                }
            } else {
                println!(
                    "Warning: Native library not found: {:?}. Please run install first.",
                    native_path
                );
            }
        }

        if needs_extraction {
            println!("Extracting native libraries...");
            for artifact in &natives {
                let native_path = self.libraries_dir.join(&artifact.path);
                if native_path.exists() {
                    self.extract_lwjgl3_native_library(&native_path, natives_dir)?;
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
            let outpath = extract_dir.join(
                file.mangled_name()
                    .with_context(|| format!("Failed to get entry name {} from archive: {:?}", i, jar_path))?,
            );
            let file_name = file
                .name()
                .with_context(|| format!("Failed to get entry name {} from archive: {:?}", i, jar_path))?
                .to_lowercase();

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
        version_details: &VersionDetails,
    ) -> anyhow::Result<String> {
        let jar_id = version_details
            .jar
            .as_deref()
            .unwrap_or(&version_details.id);
        let mut classpath = vec![
            self.versions_dir
                .join(jar_id)
                .join(format!("{}.jar", jar_id)),
        ];

        for library in &version_details.libraries {
            // Check if library should be included based on rules
            if !library.is_allowed_for_current_platform() {
                continue;
            }

            if let Some(artifact) = library.artifact() {
                let library_path = self.libraries_dir.join(&artifact.path);
                if library_path.exists() {
                    classpath.push(library_path);
                }
            }
        }

        Ok(classpath
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join(if cfg!(windows) { ";" } else { ":" }))
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_context(os: &str) -> ArgumentContext<'_> {
        ArgumentContext {
            os,
            arch: "x86_64",
            game_dir: "/game",
            assets_dir: "/assets",
            natives_dir: "/natives",
            library_dir: "/libraries",
            classpath: "/libraries/a.jar:/libraries/b.jar",
            classpath_separator: ":",
            version_name: "1.21.1",
            version_type: "release",
            asset_index_name: "17",
            username: "Alice",
            access_token: "secret-token",
            uuid: "00000000-0000-0000-0000-000000000000",
            user_type: "msa",
            launcher_name: "mclc",
            launcher_version: "0.1.1",
        }
    }

    fn details(json: &str) -> VersionDetails {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn jvm_args_include_os_specific_flags() {
        let details = details(
            r#"{
                "id": "1.21.1",
                "type": "release",
                "mainClass": "net.minecraft.client.main.Main",
                "libraries": [],
                "arguments": {
                    "jvm": [
                        {
                            "rules": [{ "action": "allow", "os": { "name": "osx" } }],
                            "value": ["-XstartOnFirstThread"]
                        },
                        "-Djava.library.path=${natives_directory}"
                    ],
                    "game": []
                }
            }"#,
        );

        let mac_args = build_jvm_arguments(&details, &test_context("macos"), None, None, None, None);
        assert!(mac_args.contains(&"-XstartOnFirstThread".to_string()));
        assert!(mac_args.contains(&"-Djava.library.path=/natives".to_string()));

        let windows_args =
            build_jvm_arguments(&details, &test_context("windows"), None, None, None, None);
        assert!(!windows_args.contains(&"-XstartOnFirstThread".to_string()));
    }

    #[test]
    fn game_args_are_expanded_from_manifest() {
        let details = details(
            r#"{
                "id": "1.21.1",
                "type": "release",
                "mainClass": "net.minecraft.client.main.Main",
                "libraries": [],
                "arguments": {
                    "jvm": [],
                    "game": [
                        "--username", "${auth_player_name}",
                        "--uuid", "${auth_uuid}",
                        "--accessToken", "${auth_access_token}",
                        "--versionType", "${version_type}"
                    ]
                }
            }"#,
        );

        let args = build_game_arguments(&details, &test_context("windows"));

        assert_eq!(
            args,
            vec![
                "--username",
                "Alice",
                "--uuid",
                "00000000-0000-0000-0000-000000000000",
                "--accessToken",
                "secret-token",
                "--versionType",
                "release",
            ]
        );
    }

    #[test]
    fn legacy_minecraft_arguments_are_expanded() {
        let details = details(
            r#"{
                "id": "1.6.4",
                "type": "release",
                "mainClass": "net.minecraft.client.main.Main",
                "libraries": [],
                "minecraftArguments": "--username ${auth_player_name} --session ${auth_session} --gameDir ${game_directory}"
            }"#,
        );

        let args = build_game_arguments(&details, &test_context("windows"));

        assert_eq!(
            args,
            vec![
                "--username",
                "Alice",
                "--session",
                "token:secret-token:00000000-0000-0000-0000-000000000000",
                "--gameDir",
                "/game",
            ]
        );
    }

    #[test]
    fn jvm_args_ensure_classpath_and_append_custom_args() {
        let details = details(
            r#"{
                "id": "1.21.1",
                "type": "release",
                "mainClass": "net.minecraft.client.main.Main",
                "libraries": [],
                "arguments": { "jvm": ["-Dfoo=bar"], "game": [] }
            }"#,
        );

        let args = build_jvm_arguments(
            &details,
            &test_context("windows"),
            Some("-Xmx4G -Dcustom=1"),
            None,
            None,
            None,
        );

        let classpath_flag = args
            .iter()
            .position(|arg| arg == "-cp")
            .expect("classpath must be present");
        assert_eq!(args[classpath_flag + 1], "/libraries/a.jar:/libraries/b.jar");
        assert!(args.contains(&"-Xmx4G".to_string()));
        assert!(args.contains(&"-Dcustom=1".to_string()));
    }

    #[test]
    fn jvm_args_use_the_resolved_jar_name_for_version_name() {
        let details = details(
            r#"{
                "id": "neoforge-1",
                "type": "release",
                "jar": "1.21.1",
                "mainClass": "Main",
                "libraries": [],
                "arguments": {
                    "jvm": ["-DignoreList=client-extra,${version_name}.jar"],
                    "game": ["--version", "${version_name}"]
                }
            }"#,
        );
        let mut ctx = test_context("windows");
        ctx.version_name = "neoforge-1";

        let jvm = build_jvm_arguments(&details, &ctx, None, None, None, None);
        assert!(jvm.contains(&"-DignoreList=client-extra,1.21.1.jar".to_string()));

        let game = build_game_arguments(&details, &ctx);
        assert_eq!(game, vec!["--version", "neoforge-1"]);
    }

    #[test]
    fn sensitive_arguments_are_redacted() {
        let args: Vec<String> = [
            "--username",
            "Alice",
            "--accessToken",
            "secret-token",
            "--session",
            "token:secret-token:uuid",
            "--version",
            "1.21.1",
        ]
        .iter()
        .map(|value| value.to_string())
        .collect();

        let redacted = redact_sensitive_arguments(&args);

        assert_eq!(redacted[3], "<redacted>");
        assert_eq!(redacted[5], "<redacted>");
        assert!(!redacted.iter().any(|arg| arg == "secret-token"));
        assert!(!redacted
            .iter()
            .any(|arg| arg.starts_with("token:secret-token")));
    }

    #[test]
    fn jvm_args_do_not_duplicate_manifest_classpath() {
        let details = details(
            r#"{
                "id": "1.21.1",
                "type": "release",
                "mainClass": "net.minecraft.client.main.Main",
                "libraries": [],
                "arguments": {
                    "jvm": ["-cp", "${classpath}"],
                    "game": []
                }
            }"#,
        );

        let args = build_jvm_arguments(&details, &test_context("windows"), None, None, None, None);

        assert_eq!(args.iter().filter(|arg| *arg == "-cp").count(), 1);
        assert!(args.contains(&"/libraries/a.jar:/libraries/b.jar".to_string()));
    }
}
