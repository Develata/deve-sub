//! `deve-sub update` — self-update with checksum verification and rollback.
//!
//! UPDATE-001: download a new binary from a release manifest, verify its
//! SHA-256 checksum, swap it in, and health-check. On failure, roll back to
//! the previous binary (UPDATE-002).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Args;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::commands::load_config;

/// GitHub Releases API response (subset).
#[derive(Debug, Deserialize)]
struct ReleaseManifest {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

/// Arguments for `deve-sub update`.
#[derive(Args)]
pub struct UpdateArgs {
    /// Release manifest URL (GitHub API JSON). Defaults to the latest release.
    #[arg(long, env = "DEVE_SUB_MANIFEST_URL")]
    manifest_url: Option<String>,

    /// Health endpoint to probe after swapping the binary.
    #[arg(long, default_value = "http://127.0.0.1:8080/health/live")]
    health_url: String,

    /// Binary path to update. Defaults to the current executable.
    #[arg(long)]
    binary_path: Option<PathBuf>,

    /// Skip version comparison; always download and swap.
    #[arg(long)]
    force: bool,

    /// Health-check timeout in seconds.
    #[arg(long, default_value = "30")]
    timeout: u64,

    /// Do not attempt systemd restart.
    #[arg(long)]
    no_restart: bool,

    /// Config file path (for reading the bind address).
    #[arg(long, env = "DEVE_SUB_CONFIG")]
    config: Option<PathBuf>,
}

const GITHUB_LATEST: &str = "https://api.github.com/repos/Develata/deve-sub/releases/latest";

pub async fn update(args: UpdateArgs) -> Result<()> {
    let current_version = env!("CARGO_PKG_VERSION");
    let binary_path = match &args.binary_path {
        Some(p) => p.clone(),
        None => std::env::current_exe().context("failed to determine current executable path")?,
    };

    let manifest_url = args.manifest_url.as_deref().unwrap_or(GITHUB_LATEST);

    println!("Deve Sub self-update");
    println!("  current version: {current_version}");
    println!("  binary:          {}", binary_path.display());

    println!("fetching release manifest...");
    let manifest = fetch_manifest(manifest_url).await?;
    let latest_version = manifest.tag_name.trim_start_matches('v');
    println!("  latest version:  {latest_version}");

    if !args.force && !is_newer(latest_version, current_version) {
        println!("already up to date.");
        return Ok(());
    }

    let asset_name = platform_asset_name()?;
    let asset = manifest
        .assets
        .iter()
        .find(|a| a.name == asset_name)
        .with_context(|| format!("no asset named {asset_name} in release"))?;
    let checksum_asset = manifest
        .assets
        .iter()
        .find(|a| a.name == "checksums.txt")
        .context("no checksums.txt in release")?;

    println!("downloading {asset_name}...");
    let binary_bytes = download(&asset.browser_download_url).await?;

    println!("downloading checksums.txt...");
    let checksum_bytes = download(&checksum_asset.browser_download_url).await?;
    let checksum_text = String::from_utf8_lossy(&checksum_bytes);

    let expected_hash = parse_checksum(&checksum_text, &asset_name)
        .with_context(|| format!("no checksum entry for {asset_name}"))?;

    println!("verifying checksum...");
    let actual_hash = sha256_hex(&binary_bytes);
    if actual_hash != expected_hash {
        bail!("checksum mismatch: expected {expected_hash}, got {actual_hash}");
    }
    println!("  checksum OK");

    let backup_path = backup_path(&binary_path);
    println!("backing up current binary to {}...", backup_path.display());
    std::fs::copy(&binary_path, &backup_path)
        .with_context(|| format!("failed to back up {}", binary_path.display()))?;

    println!("installing new binary...");
    atomic_write(&binary_path, &binary_bytes)
        .with_context(|| format!("failed to write {}", binary_path.display()))?;
    set_executable(&binary_path)?;

    let restarted = if !args.no_restart {
        try_systemd_restart().await
    } else {
        false
    };

    if !restarted {
        println!("service not managed by systemd — restart manually if needed.");
    }

    println!("health-checking new binary (timeout {}s)...", args.timeout);
    let healthy = wait_healthy(&args.health_url, args.timeout).await;

    if healthy {
        let _ = std::fs::remove_file(&backup_path);
        println!("update successful: {current_version} → {latest_version}");
        Ok(())
    } else {
        println!("health check failed — rolling back...");
        rollback(&binary_path, &backup_path)?;
        if !args.no_restart {
            let _ = try_systemd_restart().await;
        }
        bail!(
            "update failed: new binary did not become healthy. Rolled back to {current_version}."
        );
    }
}

async fn fetch_manifest(url: &str) -> Result<ReleaseManifest> {
    let client = reqwest::Client::builder()
        .user_agent(format!("deve-sub/{}", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let resp = client
        .get(url)
        .send()
        .await
        .context("manifest fetch failed")?;
    if !resp.status().is_success() {
        bail!("manifest fetch returned {}", resp.status());
    }
    let body = resp.bytes().await.context("failed to read manifest body")?;
    let manifest: ReleaseManifest =
        serde_json::from_slice(&body).context("failed to parse manifest JSON")?;
    Ok(manifest)
}

async fn download(url: &str) -> Result<Vec<u8>> {
    let client = reqwest::Client::builder()
        .user_agent(format!("deve-sub/{}", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(120))
        .build()?;
    let resp = client.get(url).send().await.context("download failed")?;
    if !resp.status().is_success() {
        bail!("download from {url} returned {}", resp.status());
    }
    let bytes = resp.bytes().await.context("failed to read response body")?;
    Ok(bytes.to_vec())
}

fn platform_asset_name() -> Result<String> {
    let arch = std::env::consts::ARCH;
    let os = std::env::consts::OS;
    if os != "linux" {
        bail!("self-update only supports Linux (got {os})");
    }
    match arch {
        "x86_64" => Ok("deve-sub-linux-amd64".to_owned()),
        "aarch64" => Ok("deve-sub-linux-arm64".to_owned()),
        _ => bail!("unsupported architecture: {arch}"),
    }
}

fn is_newer(latest: &str, current: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> { v.split('.').filter_map(|s| s.parse().ok()).collect() };
    let l = parse(latest);
    let c = parse(current);
    l > c
}

fn parse_checksum(text: &str, asset: &str) -> Option<String> {
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() == 2 && parts[1] == asset {
            return Some(parts[0].to_owned());
        }
    }
    None
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let result = hasher.finalize();
    result.iter().map(|b| format!("{b:02x}")).collect()
}

fn backup_path(binary: &Path) -> PathBuf {
    let mut p = binary.to_path_buf();
    if let Some(name) = p.file_name() {
        p.set_file_name(format!("{}.bak", name.to_string_lossy()));
    }
    p
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp.new");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn set_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms)?;
    }
    Ok(())
}

fn rollback(binary: &Path, backup: &Path) -> Result<()> {
    if !backup.exists() {
        bail!("backup file not found: {}", backup.display());
    }
    let failed_path = binary.with_extension("failed");
    let _ = std::fs::rename(binary, &failed_path);
    std::fs::rename(backup, binary)
        .with_context(|| format!("failed to restore backup to {}", binary.display()))?;
    set_executable(binary)?;
    println!("rolled back to previous binary.");
    Ok(())
}

async fn try_systemd_restart() -> bool {
    let service = "/etc/systemd/system/deve-sub.service";
    if !Path::new(service).exists() {
        return false;
    }
    let result = tokio::process::Command::new("systemctl")
        .args(["restart", "deve-sub"])
        .output()
        .await;
    match result {
        Ok(out) if out.status.success() => {
            println!("systemd service restarted.");
            true
        }
        _ => false,
    }
}

async fn wait_healthy(url: &str, timeout_secs: u64) -> bool {
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    while std::time::Instant::now() < deadline {
        if let Ok(resp) = client.get(url).send().await
            && resp.status().is_success()
        {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    false
}

#[allow(dead_code)]
fn load_bind_from_config(config_path: &Option<PathBuf>) -> String {
    match load_config(config_path) {
        Ok(config) => config.server.bind,
        Err(_) => "127.0.0.1:8080".to_owned(),
    }
}
