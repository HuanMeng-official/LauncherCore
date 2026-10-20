use anyhow::Context;
use futures_util::StreamExt;
use serde::de::DeserializeOwned;
use sha1::{Digest, Sha1};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

pub const MAX_ATTEMPTS: usize = 3;
pub const RETRY_DELAY: Duration = Duration::from_millis(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum DownloadSource {
    Auto,
    Official,
    Bmclapi,
}

impl DownloadSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "auto (official, BMCLAPI fallback)",
            Self::Official => "official (Mojang)",
            Self::Bmclapi => "BMCLAPI mirror (official fallback)",
        }
    }
}

pub fn url_candidates(url: &str, source: DownloadSource) -> Vec<String> {
    let official = url.to_string();
    let mirror = mirror_url(url);

    match source {
        DownloadSource::Official => vec![official],
        DownloadSource::Bmclapi => {
            let mut candidates = Vec::new();
            if let Some(mirror) = mirror {
                if mirror != official {
                    candidates.push(mirror);
                }
            }
            candidates.push(official);
            candidates
        }
        DownloadSource::Auto => {
            let mut candidates = vec![official.clone()];
            if let Some(mirror) = mirror {
                if mirror != official {
                    candidates.push(mirror);
                }
            }
            candidates
        }
    }
}

fn mirror_url(url: &str) -> Option<String> {
    const MAPPINGS: &[(&str, &str)] = &[
        (
            "https://launchermeta.mojang.com",
            "https://bmclapi2.bangbang93.com",
        ),
        (
            "http://launchermeta.mojang.com",
            "https://bmclapi2.bangbang93.com",
        ),
        (
            "https://piston-meta.mojang.com",
            "https://bmclapi2.bangbang93.com",
        ),
        (
            "https://piston-data.mojang.com",
            "https://bmclapi2.bangbang93.com",
        ),
        (
            "https://launcher.mojang.com",
            "https://bmclapi2.bangbang93.com",
        ),
        (
            "https://resources.download.minecraft.net",
            "https://bmclapi2.bangbang93.com/assets",
        ),
        (
            "http://resources.download.minecraft.net",
            "https://bmclapi2.bangbang93.com/assets",
        ),
        (
            "https://libraries.minecraft.net",
            "https://bmclapi2.bangbang93.com/maven",
        ),
        (
            "https://repo1.maven.org/maven2",
            "https://bmclapi2.bangbang93.com/maven",
        ),
        (
            "https://files.minecraftforge.net/maven",
            "https://bmclapi2.bangbang93.com/maven",
        ),
        (
            "https://maven.fabricmc.net",
            "https://bmclapi2.bangbang93.com/maven",
        ),
        (
            "https://meta.fabricmc.net",
            "https://bmclapi2.bangbang93.com/fabric-meta",
        ),
        (
            "https://maven.minecraftforge.net",
            "https://bmclapi2.bangbang93.com/maven",
        ),
        (
            "https://maven.neoforged.net/releases",
            "https://bmclapi2.bangbang93.com/maven",
        ),
        (
            "https://maven.quiltmc.org/repository/release",
            "https://bmclapi2.bangbang93.com/maven",
        ),
    ];

    MAPPINGS.iter().find_map(|(prefix, replacement)| {
        url.strip_prefix(prefix)
            .map(|rest| format!("{}{}", replacement, rest))
    })
}

pub async fn fetch_json<T: DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
    source: DownloadSource,
) -> anyhow::Result<T> {
    let candidates = url_candidates(url, source);
    fetch_json_from_candidates(client, &candidates, MAX_ATTEMPTS, RETRY_DELAY)
        .await
        .with_context(|| format!("Failed to download {}", url))
}

async fn fetch_json_from_candidates<T: DeserializeOwned>(
    client: &reqwest::Client,
    candidates: &[String],
    attempts: usize,
    delay: Duration,
) -> anyhow::Result<T> {
    let mut errors = Vec::new();

    for attempt in 0..attempts {
        for candidate in candidates {
            match fetch_json_once(client, candidate).await {
                Ok(value) => return Ok(value),
                Err(error) => {
                    errors.push(format!(
                        "{} (attempt {}/{}): {}",
                        candidate,
                        attempt + 1,
                        attempts,
                        error
                    ));
                }
            }
        }
        sleep_before_retry(attempt, attempts, delay).await;
    }

    anyhow::bail!("All download attempts failed:\n{}", errors.join("\n"))
}

async fn fetch_json_once<T: DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
) -> anyhow::Result<T> {
    let response = client.get(url).send().await?.error_for_status()?;
    Ok(response.json::<T>().await?)
}

pub async fn fetch_text(
    client: &reqwest::Client,
    url: &str,
    source: DownloadSource,
) -> anyhow::Result<String> {
    let candidates = url_candidates(url, source);
    fetch_text_from_candidates(client, &candidates, MAX_ATTEMPTS, RETRY_DELAY)
        .await
        .with_context(|| format!("Failed to download {}", url))
}

async fn fetch_text_from_candidates(
    client: &reqwest::Client,
    candidates: &[String],
    attempts: usize,
    delay: Duration,
) -> anyhow::Result<String> {
    let mut errors = Vec::new();

    for attempt in 0..attempts {
        for candidate in candidates {
            match fetch_text_once(client, candidate).await {
                Ok(value) => return Ok(value),
                Err(error) => {
                    errors.push(format!(
                        "{} (attempt {}/{}): {}",
                        candidate,
                        attempt + 1,
                        attempts,
                        error
                    ));
                }
            }
        }
        sleep_before_retry(attempt, attempts, delay).await;
    }

    anyhow::bail!("All download attempts failed:\n{}", errors.join("\n"))
}

async fn fetch_text_once(
    client: &reqwest::Client,
    url: &str,
) -> anyhow::Result<String> {
    let response = client.get(url).send().await?.error_for_status()?;
    Ok(response.text().await?)
}

pub async fn download_file(
    client: &reqwest::Client,
    candidates: &[String],
    path: &Path,
    expected_sha1: Option<&str>,
    expected_size: Option<u64>,
) -> anyhow::Result<()> {
    download_file_with_policy(
        client,
        candidates,
        path,
        expected_sha1,
        expected_size,
        MAX_ATTEMPTS,
        RETRY_DELAY,
    )
    .await
}

async fn download_file_with_policy(
    client: &reqwest::Client,
    candidates: &[String],
    path: &Path,
    expected_sha1: Option<&str>,
    expected_size: Option<u64>,
    attempts: usize,
    delay: Duration,
) -> anyhow::Result<()> {
    if path.exists() {
        let size_matches = expected_size
            .map(|expected| {
                fs::metadata(path)
                    .map(|metadata| metadata.len() == expected)
                    .unwrap_or(false)
            })
            .unwrap_or(true);
        if size_matches {
            return Ok(());
        }
    }

    let mut errors = Vec::new();

    for attempt in 0..attempts {
        for candidate in candidates {
            match download_once(client, candidate, path, expected_sha1, expected_size).await {
                Ok(()) => return Ok(()),
                Err(error) => {
                    errors.push(format!(
                        "{} (attempt {}/{}): {}",
                        candidate,
                        attempt + 1,
                        attempts,
                        error
                    ));
                }
            }
        }
        sleep_before_retry(attempt, attempts, delay).await;
    }

    anyhow::bail!(
        "All download attempts failed for {:?}:\n{}",
        path,
        errors.join("\n")
    )
}

async fn download_once(
    client: &reqwest::Client,
    url: &str,
    path: &Path,
    expected_sha1: Option<&str>,
    expected_size: Option<u64>,
) -> anyhow::Result<()> {
    let response = client
        .get(url)
        .send()
        .await?
        .error_for_status()
        .with_context(|| format!("Failed to download {}", url))?;

    if let (Some(expected), Some(actual)) = (expected_size, response.content_length()) {
        if expected != actual {
            anyhow::bail!(
                "Unexpected content length for {}: expected {} bytes, got {}",
                url,
                expected,
                actual
            );
        }
    }

    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Invalid download path: {:?}", path))?;
    fs::create_dir_all(parent)?;

    let mut temp = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("Failed to create temporary file in {:?}", parent))?;

    let mut hasher = Sha1::new();
    let mut downloaded = 0u64;
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("Failed to read chunk")?;
        temp.write_all(&chunk).context("Failed to write chunk")?;
        hasher.update(&chunk);
        downloaded += chunk.len() as u64;
    }
    temp.flush().context("Failed to flush downloaded data")?;

    if let Some(expected) = expected_size {
        if downloaded != expected {
            anyhow::bail!(
                "Size mismatch for {}: expected {} bytes, got {}",
                url,
                expected,
                downloaded
            );
        }
    }

    if let Some(expected) = expected_sha1 {
        let actual: String = hasher
            .finalize()
            .iter()
            .map(|byte| format!("{:02x}", byte))
            .collect();
        if !actual.eq_ignore_ascii_case(expected) {
            anyhow::bail!(
                "SHA-1 mismatch for {}: expected {}, got {}",
                url,
                expected,
                actual
            );
        }
    }

    temp.persist(path)
        .map_err(|error| anyhow::anyhow!("Failed to save downloaded file {:?}: {}", path, error))?;

    Ok(())
}

async fn sleep_before_retry(attempt: usize, attempts: usize, delay: Duration) {
    if attempt + 1 < attempts {
        let factor = 1u32 << attempt.min(10);
        tokio::time::sleep(delay.saturating_mul(factor)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_http;
    use std::path::PathBuf;

    fn sha1_hex(data: &[u8]) -> String {
        Sha1::digest(data)
            .iter()
            .map(|byte| format!("{:02x}", byte))
            .collect()
    }

    #[test]
    fn maps_official_urls_to_bmclapi() {
        assert_eq!(
            mirror_url("https://launchermeta.mojang.com/mc/game/version_manifest.json")
                .as_deref(),
            Some("https://bmclapi2.bangbang93.com/mc/game/version_manifest.json")
        );
        assert_eq!(
            mirror_url("https://piston-meta.mojang.com/v1/packages/hash/1.21.1.json").as_deref(),
            Some("https://bmclapi2.bangbang93.com/v1/packages/hash/1.21.1.json")
        );
        assert_eq!(
            mirror_url(
                "https://resources.download.minecraft.net/ab/abcdef"
            )
            .as_deref(),
            Some("https://bmclapi2.bangbang93.com/assets/ab/abcdef")
        );
        assert_eq!(
            mirror_url("https://libraries.minecraft.net/com/example/lib.jar").as_deref(),
            Some("https://bmclapi2.bangbang93.com/maven/com/example/lib.jar")
        );
        assert_eq!(
            mirror_url("https://repo1.maven.org/maven2/com/example/lib.jar").as_deref(),
            Some("https://bmclapi2.bangbang93.com/maven/com/example/lib.jar")
        );
        assert_eq!(
            mirror_url("https://maven.minecraftforge.net/net/example/lib.jar").as_deref(),
            Some("https://bmclapi2.bangbang93.com/maven/net/example/lib.jar")
        );
        assert_eq!(
            mirror_url("https://maven.neoforged.net/releases/net/example/lib.jar").as_deref(),
            Some("https://bmclapi2.bangbang93.com/maven/net/example/lib.jar")
        );
        assert_eq!(mirror_url("https://example.invalid/file"), None);
    }

    #[test]
    fn source_candidates_follow_the_selected_source() {
        let official = "https://piston-data.mojang.com/v1/objects/hash/client.jar";

        assert_eq!(
            url_candidates(official, DownloadSource::Official),
            vec![official.to_string()]
        );
        assert_eq!(
            url_candidates(official, DownloadSource::Bmclapi),
            vec![
                "https://bmclapi2.bangbang93.com/v1/objects/hash/client.jar".to_string(),
                official.to_string()
            ]
        );
        assert_eq!(
            url_candidates(official, DownloadSource::Auto),
            vec![
                official.to_string(),
                "https://bmclapi2.bangbang93.com/v1/objects/hash/client.jar".to_string()
            ]
        );
    }

    #[tokio::test]
    async fn retries_a_download_until_it_succeeds() {
        let body = b"hello";
        let expected_sha1 = sha1_hex(body);
        let server = test_http::serve_sequence(vec![
            test_http::response("500 Internal Server Error", &[], b"fail"),
            test_http::response("500 Internal Server Error", &[], b"fail"),
            test_http::response("200 OK", &[], body),
        ])
        .await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.bin");

        download_file_with_policy(
            &reqwest::Client::new(),
            &[format!("{}/file.bin", server.base_url)],
            &path,
            Some(&expected_sha1),
            Some(body.len() as u64),
            3,
            Duration::ZERO,
        )
        .await
        .unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), body);
        assert_eq!(server.requests.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn falls_back_to_the_next_source() {
        let body = b"mirror-content";
        let expected_sha1 = sha1_hex(body);
        let failing =
            test_http::serve_once(test_http::response("500 Internal Server Error", &[], b"fail"))
                .await;
        let working = test_http::serve_once(test_http::response("200 OK", &[], body)).await;
        let dir = tempfile::tempdir().unwrap();
        let path = PathBuf::from(dir.path()).join("file.bin");

        download_file_with_policy(
            &reqwest::Client::new(),
            &[
                format!("{}/file.bin", failing.base_url),
                format!("{}/file.bin", working.base_url),
            ],
            &path,
            Some(&expected_sha1),
            Some(body.len() as u64),
            1,
            Duration::ZERO,
        )
        .await
        .unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), body);
    }

    #[tokio::test]
    async fn reports_an_error_after_every_attempt_fails() {
        let server = test_http::serve_sequence(vec![
            test_http::response("500 Internal Server Error", &[], b"fail"),
            test_http::response("500 Internal Server Error", &[], b"fail"),
        ])
        .await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.bin");

        let result = download_file_with_policy(
            &reqwest::Client::new(),
            &[format!("{}/file.bin", server.base_url)],
            &path,
            None,
            None,
            2,
            Duration::ZERO,
        )
        .await;

        assert!(result.is_err());
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn retries_text_requests_until_they_succeed() {
        let server = test_http::serve_sequence(vec![
            test_http::response("500 Internal Server Error", &[], b"fail"),
            test_http::response("200 OK", &[], b"checksum"),
        ])
        .await;

        let text = fetch_text_from_candidates(
            &reqwest::Client::new(),
            &[format!("{}/file.sha1", server.base_url)],
            3,
            Duration::ZERO,
        )
        .await
        .unwrap();

        assert_eq!(text, "checksum");
        assert_eq!(server.requests.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn retries_json_requests_until_they_succeed() {
        #[derive(serde::Deserialize)]
        struct Payload {
            value: u32,
        }

        let server = test_http::serve_sequence(vec![
            test_http::response("500 Internal Server Error", &[], b"fail"),
            test_http::response("200 OK", &[], br#"{"value":42}"#),
        ])
        .await;

        let payload: Payload = fetch_json_from_candidates(
            &reqwest::Client::new(),
            &[format!("{}/data.json", server.base_url)],
            3,
            Duration::ZERO,
        )
        .await
        .unwrap();

        assert_eq!(payload.value, 42);
        assert_eq!(server.requests.lock().unwrap().len(), 2);
    }
}
