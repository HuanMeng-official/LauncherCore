use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaRuntime {
    pub path: PathBuf,
    pub version: Option<u32>,
    pub version_string: Option<String>,
    pub arch: Option<String>,
}

#[derive(Debug, Default)]
struct ReleaseInfo {
    version: Option<String>,
    arch: Option<String>,
}

pub fn detect_java_runtimes() -> Vec<JavaRuntime> {
    let mut runtimes = Vec::new();
    let mut seen = HashSet::new();

    for java_path in candidate_java_paths() {
        if !java_path.is_file() {
            continue;
        }

        let key = fs::canonicalize(&java_path).unwrap_or_else(|_| java_path.clone());
        if !seen.insert(key) {
            continue;
        }

        if let Some(runtime) = runtime_from_java_path(&java_path) {
            runtimes.push(runtime);
        }
    }

    runtimes
}

pub fn select_java(
    required_major: Option<u32>,
    override_path: Option<String>,
) -> anyhow::Result<PathBuf> {
    if let Some(path) = override_path {
        crate::outln!("Using explicitly provided Java path: {}", path);
        return Ok(PathBuf::from(path));
    }

    let runtimes = detect_java_runtimes();
    if let Some(runtime) = select_from(&runtimes, required_major) {
        match runtime.version {
            Some(version) => crate::outln!("Using Java {}: {}", version, runtime.path.display()),
            None => crate::outln!("Using Java: {}", runtime.path.display()),
        }
        return Ok(runtime.path.clone());
    }

    if runtimes.is_empty() {
        let required = required_major
            .map(|major| format!(" Java {} is required.", major))
            .unwrap_or_default();
        anyhow::bail!(
            "No Java runtime found.{}{}",
            required,
            " Install Java or pass --runtime <path>."
        );
    }

    let available = runtimes
        .iter()
        .map(|runtime| match runtime.version {
            Some(version) => format!("Java {} ({})", version, runtime.path.display()),
            None => format!("unknown ({})", runtime.path.display()),
        })
        .collect::<Vec<_>>()
        .join(", ");

    let required = required_major
        .map(|major| format!(" (Java {} required)", major))
        .unwrap_or_default();
    anyhow::bail!(
        "No suitable Java runtime found{}. Detected: {}. Pass --runtime <path> or install the required Java version.",
        required,
        available
    );
}

fn select_from<'a>(
    runtimes: &'a [JavaRuntime],
    required_major: Option<u32>,
) -> Option<&'a JavaRuntime> {
    match required_major {
        Some(required) => runtimes
            .iter()
            .find(|runtime| runtime.version == Some(required)),
        None => runtimes.first(),
    }
}

fn candidate_java_paths() -> Vec<PathBuf> {
    let java_name = java_executable_name();
    let mut candidates = Vec::new();

    for variable in ["JAVA_HOME", "JDK_HOME", "JRE_HOME"] {
        if let Some(home) = std::env::var_os(variable) {
            candidates.push(PathBuf::from(home).join("bin").join(java_name));
        }
    }

    if let Some(paths) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&paths) {
            candidates.push(directory.join(java_name));
        }
    }

    for home in common_java_homes() {
        candidates.push(home.join("bin").join(java_name));
    }

    candidates
}

fn java_executable_name() -> &'static str {
    if cfg!(windows) { "java.exe" } else { "java" }
}

fn common_java_homes() -> Vec<PathBuf> {
    let mut homes = Vec::new();

    #[cfg(windows)]
    {
        let mut roots = Vec::new();
        for variable in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(root) = std::env::var_os(variable) {
                let root = PathBuf::from(root);
                for vendor in [
                    "Java",
                    "Eclipse Adoptium",
                    "Eclipse Foundation",
                    "Microsoft",
                    "Zulu",
                    "Amazon Corretto",
                    "BellSoft",
                    "Semeru",
                    "AdoptOpenJDK",
                    "RedHat",
                ] {
                    roots.push(root.join(vendor));
                }
            }
        }

        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            roots.push(
                PathBuf::from(local_app_data)
                    .join("Programs")
                    .join("Eclipse Adoptium"),
            );
        }
        if let Some(profile) = std::env::var_os("USERPROFILE") {
            roots.push(PathBuf::from(profile).join(".jdks"));
        }

        for root in roots {
            collect_home_candidates(&root, &mut homes);
        }
    }

    #[cfg(target_os = "linux")]
    {
        for root in ["/usr/lib/jvm", "/usr/java", "/opt/java", "/opt"] {
            collect_home_candidates(Path::new(root), &mut homes);
        }

        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            for root in [
                home.join(".sdkman").join("candidates").join("java"),
                home.join(".jdks"),
                home.join(".asdf").join("installs").join("java"),
                home.join(".local")
                    .join("share")
                    .join("mise")
                    .join("installs")
                    .join("java"),
            ] {
                collect_home_candidates(&root, &mut homes);
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        for root in [
            Path::new("/Library/Java/JavaVirtualMachines"),
            Path::new("/usr/local/opt"),
            Path::new("/opt/homebrew/opt"),
        ] {
            collect_macos_candidates(root, &mut homes);
        }

        if let Some(home) = std::env::var_os("HOME") {
            collect_macos_candidates(
                &PathBuf::from(home).join("Library/Java/JavaVirtualMachines"),
                &mut homes,
            );
        }
    }

    homes
}

#[cfg(any(windows, target_os = "linux"))]
fn collect_home_candidates(root: &Path, homes: &mut Vec<PathBuf>) {
    let java_name = java_executable_name();

    if root.join("bin").join(java_name).is_file() {
        push_unique(homes, root.to_path_buf());
    }

    let Ok(entries) = fs::read_dir(root) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && path.join("bin").join(java_name).is_file() {
            push_unique(homes, path);
        }
    }
}

#[cfg(target_os = "macos")]
fn collect_macos_candidates(root: &Path, homes: &mut Vec<PathBuf>) {
    let java_name = java_executable_name();

    let direct_jvm = root.join("Contents").join("Home");
    if direct_jvm.join("bin").join(java_name).is_file() {
        push_unique(homes, direct_jvm);
    }

    let direct_brew = root
        .join("libexec")
        .join("openjdk.jdk")
        .join("Contents")
        .join("Home");
    if direct_brew.join("bin").join(java_name).is_file() {
        push_unique(homes, direct_brew);
    }

    let Ok(entries) = fs::read_dir(root) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let jvm_home = path.join("Contents").join("Home");
        if jvm_home.join("bin").join(java_name).is_file() {
            push_unique(homes, jvm_home);
        }

        let brew_home = path
            .join("libexec")
            .join("openjdk.jdk")
            .join("Contents")
            .join("Home");
        if brew_home.join("bin").join(java_name).is_file() {
            push_unique(homes, brew_home);
        }
    }
}

fn push_unique(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.contains(&path) {
        paths.push(path);
    }
}

fn runtime_from_java_path(java_path: &Path) -> Option<JavaRuntime> {
    let bin_dir = java_path.parent()?;
    let home = bin_dir.parent().unwrap_or(bin_dir);
    let ReleaseInfo { version, arch } = read_release(home);

    let version_string = version.or_else(|| java_version_from_command(java_path));
    let major = version_string.as_deref().and_then(parse_major);

    Some(JavaRuntime {
        path: java_path.to_path_buf(),
        version: major,
        version_string,
        arch,
    })
}

fn read_release(home: &Path) -> ReleaseInfo {
    fs::read_to_string(home.join("release"))
        .map(|content| parse_release(&content))
        .unwrap_or_default()
}

fn parse_release(content: &str) -> ReleaseInfo {
    let mut info = ReleaseInfo::default();

    for line in content.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"');
        if value.is_empty() {
            continue;
        }

        match key.trim() {
            "JAVA_VERSION" => info.version = Some(value.to_string()),
            "OS_ARCH" => info.arch = Some(value.to_string()),
            _ => {}
        }
    }

    info
}

fn java_version_from_command(java_path: &Path) -> Option<String> {
    let mut command = Command::new(java_path);
    command.arg("-version");

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }

    let output = command.output().ok()?;
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    extract_quoted_version(&text)
}

fn extract_quoted_version(output: &str) -> Option<String> {
    let start = output.find('"')? + 1;
    let end = output[start..].find('"')? + start;
    Some(output[start..end].to_string())
}

fn parse_major(version: &str) -> Option<u32> {
    let version = version.trim();
    let version = version.strip_prefix("1.").unwrap_or(version);
    let digits: String = version
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn runtime(path: &str, version: Option<u32>) -> JavaRuntime {
        JavaRuntime {
            path: std::path::PathBuf::from(path),
            version,
            version_string: version.map(|version| version.to_string()),
            arch: None,
        }
    }

    #[test]
    fn parses_java_major_versions() {
        assert_eq!(parse_major("1.8.0_392"), Some(8));
        assert_eq!(parse_major("1.7.0_80"), Some(7));
        assert_eq!(parse_major("17.0.9"), Some(17));
        assert_eq!(parse_major("21"), Some(21));
        assert_eq!(parse_major("21-ea"), Some(21));
        assert_eq!(parse_major("not-a-version"), None);
    }

    #[test]
    fn extracts_version_from_java_version_output() {
        assert_eq!(
            extract_quoted_version(
                "openjdk version \"21.0.2\" 2024-01-16\nOpenJDK Runtime Environment"
            ),
            Some("21.0.2".to_string())
        );
        assert_eq!(
            extract_quoted_version("java version \"1.8.0_392\"\nJava(TM) SE Runtime"),
            Some("1.8.0_392".to_string())
        );
    }

    #[test]
    fn parses_release_file() {
        let info = parse_release("JAVA_VERSION=\"21.0.2\"\nOS_ARCH=\"x86_64\"\n");

        assert_eq!(info.version.as_deref(), Some("21.0.2"));
        assert_eq!(info.arch.as_deref(), Some("x86_64"));
    }

    #[test]
    fn reads_runtime_metadata_from_release_file() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("jdk-17");
        let java_name = if cfg!(windows) { "java.exe" } else { "java" };
        fs::create_dir_all(home.join("bin")).unwrap();
        fs::write(home.join("bin").join(java_name), b"").unwrap();
        fs::write(
            home.join("release"),
            "JAVA_VERSION=\"17.0.9\"\nOS_ARCH=\"x86_64\"\n",
        )
        .unwrap();

        let runtime = runtime_from_java_path(&home.join("bin").join(java_name)).unwrap();

        assert_eq!(runtime.version, Some(17));
        assert_eq!(runtime.version_string.as_deref(), Some("17.0.9"));
        assert_eq!(runtime.arch.as_deref(), Some("x86_64"));
    }

    #[test]
    fn selects_exact_required_major() {
        let runtimes = vec![
            runtime("java8", Some(8)),
            runtime("java17", Some(17)),
            runtime("java21", Some(21)),
        ];

        assert_eq!(
            select_from(&runtimes, Some(17)).unwrap().path,
            std::path::PathBuf::from("java17")
        );
        assert!(select_from(&runtimes, Some(11)).is_none());
        assert_eq!(
            select_from(&runtimes, None).unwrap().path,
            std::path::PathBuf::from("java8")
        );
    }

    #[test]
    fn ignores_unknown_versions_when_a_major_is_required() {
        let runtimes = vec![runtime("unknown", None), runtime("java21", Some(21))];

        assert_eq!(
            select_from(&runtimes, Some(21)).unwrap().path,
            std::path::PathBuf::from("java21")
        );
    }
}
