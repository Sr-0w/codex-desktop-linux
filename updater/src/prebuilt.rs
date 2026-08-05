//! Download and verification for release-built postmarketOS APK updates.

use crate::{install, upstream};
use anyhow::{Context, Result};
use futures_util::StreamExt;
use reqwest::Client;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::{fs::File, io::AsyncWriteExt};

pub const APK_ASSET_NAME: &str = "codex-desktop-linux-postmarketos-aarch64.apk";
pub const APK_RELEASE_URL: &str = "https://github.com/Sr-0w/codex-desktop-linux/releases/latest/download/codex-desktop-linux-postmarketos-aarch64.apk";
pub const APK_CHECKSUM_URL: &str = "https://github.com/Sr-0w/codex-desktop-linux/releases/latest/download/codex-desktop-linux-postmarketos-aarch64.apk.sha256";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadedApk {
    pub path: PathBuf,
    pub sha256: String,
    pub candidate_version: String,
}

pub async fn fetch_remote_metadata(client: &Client) -> Result<upstream::RemoteMetadata> {
    upstream::fetch_remote_metadata(client, APK_CHECKSUM_URL).await
}

pub async fn download_apk(client: &Client, destination_dir: &Path) -> Result<DownloadedApk> {
    download_apk_from(client, APK_RELEASE_URL, APK_CHECKSUM_URL, destination_dir).await
}

async fn download_apk_from(
    client: &Client,
    apk_url: &str,
    checksum_url: &str,
    destination_dir: &Path,
) -> Result<DownloadedApk> {
    tokio::fs::create_dir_all(destination_dir)
        .await
        .with_context(|| format!("Failed to create {}", destination_dir.display()))?;

    let checksum_response = client
        .get(checksum_url)
        .send()
        .await
        .with_context(|| format!("Failed GET request for {checksum_url}"))?
        .error_for_status()
        .with_context(|| format!("GET request for {checksum_url} returned an error status"))?;
    let checksum_body = checksum_response
        .text()
        .await
        .context("Failed to read APK checksum response")?;
    let expected_sha256 = parse_checksum(&checksum_body, APK_ASSET_NAME)?;

    let destination = destination_dir.join(APK_ASSET_NAME);
    let temporary = destination_dir.join(format!(".{APK_ASSET_NAME}.part"));
    let mut file = File::create(&temporary)
        .await
        .with_context(|| format!("Failed to create {}", temporary.display()))?;
    let response = client
        .get(apk_url)
        .send()
        .await
        .with_context(|| format!("Failed GET request for {apk_url}"))?
        .error_for_status()
        .with_context(|| format!("GET request for {apk_url} returned an error status"))?;
    let mut hasher = Sha256::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.with_context(|| format!("Failed downloading {apk_url}"))?;
        file.write_all(&chunk)
            .await
            .with_context(|| format!("Failed writing {}", temporary.display()))?;
        hasher.update(&chunk);
    }
    file.flush()
        .await
        .with_context(|| format!("Failed flushing {}", temporary.display()))?;
    drop(file);

    let actual_sha256 = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    anyhow::ensure!(
        actual_sha256 == expected_sha256,
        "Downloaded APK SHA-256 mismatch: expected {expected_sha256}, got {actual_sha256}"
    );
    tokio::fs::rename(&temporary, &destination)
        .await
        .with_context(|| format!("Failed to finalize {}", destination.display()))?;
    let candidate_version = install::apk_package_version(&destination)?;

    Ok(DownloadedApk {
        path: destination,
        sha256: actual_sha256,
        candidate_version,
    })
}

fn parse_checksum(content: &str, expected_file_name: &str) -> Result<String> {
    let line = content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .context("APK checksum response is empty")?;
    let mut fields = line.split_whitespace();
    let checksum = fields.next().context("APK checksum is missing")?;
    let file_name = fields
        .next()
        .map(|value| value.trim_start_matches('*'))
        .context("APK checksum filename is missing")?;
    anyhow::ensure!(
        fields.next().is_none(),
        "APK checksum response contains unexpected fields"
    );
    anyhow::ensure!(
        file_name == expected_file_name,
        "APK checksum names {file_name}, expected {expected_file_name}"
    );
    anyhow::ensure!(
        checksum.len() == 64 && checksum.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "APK checksum is not a SHA-256 value"
    );
    Ok(checksum.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_release_checksum() -> Result<()> {
        let checksum = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        assert_eq!(
            parse_checksum(&format!("{checksum}  {APK_ASSET_NAME}\n"), APK_ASSET_NAME)?,
            checksum
        );
        Ok(())
    }

    #[test]
    fn rejects_checksum_for_another_asset() {
        let checksum = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let error = parse_checksum(&format!("{checksum}  foreign.apk\n"), APK_ASSET_NAME)
            .expect_err("foreign checksum filenames must fail");
        assert!(error.to_string().contains("expected"));
    }
}
