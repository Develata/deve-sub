//! `deve-sub update` — self-update with checksum verification and rollback.
//!
//! UPDATE-001: download a new binary from a release, verify its SHA-256
//! checksum, verify it reports the target version, swap it in, restart, and
//! health-check the running version. On failure, roll back to the previous
//! binary (UPDATE-002).
//!
//! Signed manifests authenticate the publisher before binary download. Unsigned
//! development sources require an explicit `--allow-unsigned`; bad or partial
//! signatures always fail. Native Web assets are updated by the installer.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Args;
use fs2::FileExt;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::commands::load_config;
use crate::update_manifest;

/// GitHub Releases API response (subset).
#[derive(Debug, Deserialize)]
struct ReleaseManifest {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Debug, Clone, Deserialize)]
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

    /// Explicitly accept a binary-only update, including possible frontend skew.
    #[arg(long)]
    binary_only: bool,

    /// Explicitly allow an older release (never bypasses authentication).
    #[arg(long)]
    allow_downgrade: bool,

    /// Reinstall the current version (downgrades require --allow-downgrade).
    #[arg(long)]
    force: bool,

    /// Allow checksum-only updates from a manually trusted development source.
    #[arg(long)]
    allow_unsigned: bool,

    /// Health-check timeout in seconds.
    #[arg(long, default_value = "30")]
    timeout: u64,

    /// Do not attempt systemd restart.
    #[arg(long)]
    no_restart: bool,

    /// Config file path (for reading the bind address when health_url is the
    /// default). DS-AUD-B09: this was previously ignored — now actually read.
    #[arg(long, env = "DEVE_SUB_CONFIG")]
    config: Option<PathBuf>,
}

const GITHUB_LATEST: &str = "https://api.github.com/repos/Develata/deve-sub/releases/latest";

/// DS-AUD-B09: maximum download size for the binary (256 MB) and the
/// manifest/checksum (1 MB). Prevents unbounded memory growth from a
/// hostile or corrupted server.
const MAX_BINARY_BYTES: u64 = 256 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;

pub async fn update(args: UpdateArgs) -> Result<()> {
    let config = load_config(&args.config)?;
    if config.server.serve_web && !args.binary_only {
        bail!(
            "Native installation includes versioned frontend assets. Use the complete installer update, or pass --binary-only explicitly to accept binary/frontend version skew. No files have changed."
        );
    }
    let current_version = env!("CARGO_PKG_VERSION");
    let binary_path = match &args.binary_path {
        Some(p) => p.clone(),
        None => std::env::current_exe().context("failed to determine current executable path")?,
    };

    let manifest_url = args.manifest_url.as_deref().unwrap_or(GITHUB_LATEST);

    // DS-AUD-B09: read the bind address from the config file so the default
    // health URL matches the actual serve bind. If the operator passed an
    // explicit --health-url, use it as-is.
    let health_url = if args.health_url == "http://127.0.0.1:8080/health/live" {
        let bind = config.server.bind;
        format!("http://{bind}/health/live")
    } else {
        args.health_url.clone()
    };

    println!("Deve Sub self-update");
    println!("  current version: {current_version}");
    println!("  binary:          {}", binary_path.display());
    println!("  health url:      {health_url}");

    println!("fetching release manifest...");
    let manifest = fetch_manifest(manifest_url).await?;
    let latest_version = manifest.tag_name.trim_start_matches('v');
    println!("  latest version:  {latest_version}");

    let older = is_newer(current_version, latest_version)?;
    if older && !args.allow_downgrade {
        bail!(
            "release {latest_version} is older than {current_version}; downgrade requires --allow-downgrade (even with --force)"
        );
    }
    if !older && !args.force && !is_newer(latest_version, current_version)? {
        println!("already up to date.");
        return Ok(());
    }

    let asset_name = platform_asset_name()?;
    let asset = manifest
        .assets
        .iter()
        .find(|a| a.name == asset_name)
        .with_context(|| format!("no asset named {asset_name} in release"))?;

    // DS-AUD-B09: acquire an update sidecar lock so two concurrent updates
    // don't clobber each other's temp/backup files.
    let lock_path = update_lock_path(&binary_path);
    let lock_file = acquire_update_lock(&lock_path)?;

    // First-party releases require publisher authentication. The development
    // opt-in below cannot turn a partial or invalid signature into a fallback.
    let (expected_hash, expected_size) = match try_fetch_signed_manifest(&manifest, latest_version)
        .await?
    {
        SignedManifestResult::Verified(signed) => {
            println!("  signed manifest verified (Ed25519 signature OK)");
            let asset_entry = signed
                .find_asset(&asset_name)
                .with_context(|| format!("asset {asset_name} not in signed manifest"))?;
            (asset_entry.sha256.clone(), Some(asset_entry.size))
        }
        SignedManifestResult::UnsignedChecksums(checksum_asset) => {
            if !args.allow_unsigned {
                bail!(
                    "release is unsigned — refusing update; --allow-unsigned is only for manually trusted development sources"
                );
            }
            println!(
                "  WARNING: no signed manifest in release — falling back to unsigned checksums.txt"
            );
            let checksum_bytes =
                download_bounded(&checksum_asset.browser_download_url, MAX_MANIFEST_BYTES).await?;
            let checksum_text = String::from_utf8_lossy(&checksum_bytes);
            let hash = parse_checksum(&checksum_text, &asset_name)
                .with_context(|| format!("no checksum entry for {asset_name}"))?;
            (hash, None)
        }
        SignedManifestResult::None => {
            bail!(
                "release has neither a signed manifest nor checksums.txt — refusing to update from unauthenticated source"
            );
        }
    };

    println!("downloading {asset_name}...");
    let target_dir = binary_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let (binary_tmp, actual_hash, actual_size) =
        download_streaming(&asset.browser_download_url, MAX_BINARY_BYTES, target_dir).await?;

    println!("verifying checksum...");
    if actual_hash != expected_hash {
        let _ = std::fs::remove_file(&binary_tmp);
        bail!("checksum mismatch: expected {expected_hash}, got {actual_hash}");
    }
    if let Some(exp_size) = expected_size
        && actual_size != exp_size
    {
        let _ = std::fs::remove_file(&binary_tmp);
        bail!("size mismatch: signed manifest says {exp_size} bytes, got {actual_size}");
    }
    println!("  checksum OK");

    // WHY: HTTP downloads arrive without the executable bit. verify_binary_version
    // executes the temp file, and atomic_write_fsync (rename) preserves the
    // temp file's mode — so chmod must happen here, before both verify and the
    // atomic swap. Otherwise the swapped-in binary would not be executable.
    set_executable(&binary_tmp)?;

    // DS-AUD-B09: verify the downloaded binary actually runs and reports the
    // target version. Catches a corrupted/truncated binary that passes the
    // checksum (e.g. a correct checksum of the wrong artifact) or a binary
    // for the wrong architecture.
    println!("verifying downloaded binary reports version {latest_version}...");
    if let Err(e) = verify_binary_version(&binary_tmp, latest_version).await {
        let _ = std::fs::remove_file(&binary_tmp);
        return Err(e);
    }

    let backup_path = backup_path(&binary_path);
    println!("backing up current binary to {}...", backup_path.display());
    std::fs::copy(&binary_path, &backup_path)
        .with_context(|| format!("failed to back up {}", binary_path.display()))?;

    println!("installing new binary...");
    atomic_write_fsync(&binary_path, &binary_tmp)
        .with_context(|| format!("failed to write {}", binary_path.display()))?;
    set_executable(&binary_path)?;

    let restarted = if !args.no_restart {
        match try_systemd_restart().await {
            Ok(true) => true,
            // WHY: Ok(false) means no systemd unit is present (not a restart
            // failure). On a non-systemd host the binary was swapped
            // successfully; rolling back would undo a valid update. Fall
            // through to the no-restart path so the operator can restart
            // manually. See R3-30.
            Ok(false) => {
                println!(
                    "no systemd unit found at /etc/systemd/system/deve-sub.service — \
                     binary updated; restart manually to activate the new version."
                );
                false
            }
            // DS-AUD-B09: if systemd restart fails (unit exists but systemctl
            // returned nonzero), the old process is still running — a health
            // check would pass on the OLD binary (false positive). Roll back.
            Err(e) => {
                println!("systemd restart error: {e} — rolling back...");
                rollback(&binary_path, &backup_path)?;
                bail!(
                    "update failed: systemd restart error ({e}). Previous binary restored; service state is unverified. A systemd job may still be running; inspect it and restart the previous binary."
                );
            }
        }
    } else {
        println!("--no-restart: operator must restart manually before the new binary is live.");
        false
    };

    if restarted {
        // DS-AUD-B09: health check now verifies the RUNNING version, not just
        // HTTP 200. The old process on the same port would return 200 but
        // report the old version — caught here.
        println!("health-checking new binary (timeout {}s)...", args.timeout);
        let healthy = wait_healthy_version(&health_url, latest_version, args.timeout).await;

        if healthy {
            let _ = std::fs::remove_file(&backup_path);
            // Keep the lock until the update is confirmed; drop releases it.
            drop(lock_file);
            println!("update successful: {current_version} → {latest_version}");
            return Ok(());
        }
        println!("health check failed (version {latest_version} not live) — rolling back...");
        rollback(&binary_path, &backup_path)?;
        if !args.no_restart {
            try_systemd_restart().await.context(
                "binary rollback succeeded but service restart failed; manual restart required",
            )?;
        }
        bail!(
            "update failed: new binary did not report version {latest_version}. \
             Rolled back to {current_version}."
        );
    }

    // --no-restart path: can't verify health without a restart. Keep the
    // backup so the operator can roll back manually if needed.
    drop(lock_file);
    println!(
        "new binary installed at {}. Restart manually; backup kept at {}.",
        binary_path.display(),
        backup_path.display()
    );
    Ok(())
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
    // DS-AUD-B09: bound the manifest body size DURING the read.
    let body = read_body_bounded(resp, MAX_MANIFEST_BYTES).await?;
    let manifest: ReleaseManifest =
        serde_json::from_slice(&body).context("failed to parse manifest JSON")?;
    Ok(manifest)
}

/// Read a response body into memory, enforcing `max_bytes` DURING the read,
/// not after it. `resp.bytes()` buffers the entire body before returning, so
/// a post-hoc length check would let a hostile server push an arbitrarily
/// large payload into memory first (DS-AUD-B09 item 7 regression guard).
async fn read_body_bounded(resp: reqwest::Response, max_bytes: u64) -> Result<Vec<u8>> {
    use futures_util::StreamExt;
    let mut body = Vec::new();
    let mut stream = resp.bytes_stream();
    let mut total: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("failed to read response body")?;
        total += chunk.len() as u64;
        if total > max_bytes {
            bail!("response body exceeds {max_bytes} bytes — refusing unbounded read");
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Result of attempting to fetch and verify a signed manifest from a release.
enum SignedManifestResult {
    /// Signature verified — the contained hash/size are publisher-authenticated.
    Verified(update_manifest::SignedManifest),
    /// No signed manifest asset, but a `checksums.txt` exists (unsigned fallback).
    UnsignedChecksums(Asset),
    /// Neither signed manifest nor checksums.txt — refuse to update.
    None,
}

/// DS-AUD-B09: look for `deve-sub-manifest.json` + `deve-sub-manifest.json.sig`
/// assets in the release. If both exist, download and verify the Ed25519
/// signature. If the signature is invalid or from the wrong key, abort (do NOT
/// fall back to unsigned checksums — a failed signature is an attack signal,
/// not a missing-manifest condition). If no signed manifest assets exist,
/// return the `checksums.txt` asset for unsigned fallback.
async fn try_fetch_signed_manifest(
    release: &ReleaseManifest,
    expected_version: &str,
) -> Result<SignedManifestResult> {
    let manifest_asset = release
        .assets
        .iter()
        .find(|a| a.name == "deve-sub-manifest.json");
    let sig_asset = release
        .assets
        .iter()
        .find(|a| a.name == "deve-sub-manifest.json.sig");

    match (manifest_asset, sig_asset) {
        (Some(m), Some(s)) => {
            println!("downloading signed manifest...");
            let manifest_bytes =
                download_bounded(&m.browser_download_url, MAX_MANIFEST_BYTES).await?;
            let sig_bytes = download_bounded(&s.browser_download_url, MAX_MANIFEST_BYTES).await?;

            let signed = update_manifest::verify_signed_manifest(&manifest_bytes, &sig_bytes)?;

            // WHY: the signed manifest's version must match the release tag.
            // A mismatch means the signature is over a different version's
            // manifest — a potential downgrade or replay attack.
            if signed.version != expected_version {
                bail!(
                    "signed manifest version {} does not match release tag {} — refusing update",
                    signed.version,
                    expected_version
                );
            }
            Ok(SignedManifestResult::Verified(signed))
        }
        (None, None) => {
            let checksum_asset = release.assets.iter().find(|a| a.name == "checksums.txt");
            match checksum_asset {
                Some(c) => Ok(SignedManifestResult::UnsignedChecksums(c.clone())),
                None => Ok(SignedManifestResult::None),
            }
        }
        _ => {
            // One exists without the other — suspicious, refuse.
            bail!(
                "release has a partial signed manifest (need both deve-sub-manifest.json and .sig) — refusing update"
            );
        }
    }
}

/// Download a URL into memory with an upper size bound enforced during the
/// read (DS-AUD-B09).
async fn download_bounded(url: &str, max_bytes: u64) -> Result<Vec<u8>> {
    let client = reqwest::Client::builder()
        .user_agent(format!("deve-sub/{}", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(120))
        .build()?;
    let resp = client.get(url).send().await.context("download failed")?;
    if !resp.status().is_success() {
        bail!("download from {url} returned {}", resp.status());
    }
    read_body_bounded(resp, max_bytes).await
}

/// Stream a download to a temp file, computing SHA-256 incrementally and
/// enforcing a max size. Returns (temp_file_path, sha256_hex, total_bytes)
/// (DS-AUD-B09).
///
/// WHY (P0-05): the temp file is created in `target_dir` (the target
/// binary's parent directory), NOT `std::env::temp_dir()`. The final
/// install step is `fs::rename(tmp, target)`, and `rename` fails with
/// `EXDEV` if the source and destination are on different filesystems.
/// `/tmp` is typically a separate tmpfs/ext4 mount from `/usr/local/bin`,
/// so a `/tmp` temp file makes the atomic swap fail on every standard
/// Linux deployment. Keeping the temp file on the same filesystem as the
/// target guarantees the rename is atomic.
async fn download_streaming(
    url: &str,
    max_bytes: u64,
    target_dir: &Path,
) -> Result<(tempfile::TempPath, String, u64)> {
    let client = reqwest::Client::builder()
        .user_agent(format!("deve-sub/{}", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(300))
        .build()?;
    let resp = client.get(url).send().await.context("download failed")?;
    if !resp.status().is_success() {
        bail!("download from {url} returned {}", resp.status());
    }

    let mut file = tempfile::Builder::new()
        .prefix(".deve-sub-update-")
        .tempfile_in(target_dir)?;
    let result = download_body(resp, file.as_file_mut(), max_bytes).await;
    let tmp = file.into_temp_path();
    let (hex, total) = result?;
    Ok((tmp, hex, total))
}

async fn download_body(
    resp: reqwest::Response,
    file: &mut std::fs::File,
    max_bytes: u64,
) -> Result<(String, u64)> {
    use futures_util::StreamExt;
    let mut hasher = Sha256::new();
    let mut total: u64 = 0;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("download stream error")?;
        total += chunk.len() as u64;
        if total > max_bytes {
            bail!("download exceeds {max_bytes} bytes — aborting");
        }
        hasher.update(&chunk);
        std::io::Write::write_all(file, &chunk)
            .context("failed to write download chunk to temp file")?;
    }
    file.sync_all()
        .context("failed to fsync downloaded temp file")?;
    let hash = hasher.finalize();
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    Ok((hex, total))
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

/// DS-AUD-B09: proper SemVer comparison via the `semver` crate. Pre-release
/// versions sort below release versions per the SemVer spec. Returns an
/// error if either version is not valid SemVer (strict parse).
fn is_newer(latest: &str, current: &str) -> Result<bool> {
    let l = semver::Version::parse(latest)
        .with_context(|| format!("latest version {latest:?} is not valid SemVer"))?;
    let c = semver::Version::parse(current)
        .with_context(|| format!("current version {current:?} is not valid SemVer"))?;
    Ok(l > c)
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

fn backup_path(binary: &Path) -> PathBuf {
    let mut p = binary.to_path_buf();
    if let Some(name) = p.file_name() {
        p.set_file_name(format!("{}.bak", name.to_string_lossy()));
    }
    p
}

fn update_lock_path(binary: &Path) -> PathBuf {
    let mut p = binary.to_path_buf();
    if let Some(name) = p.file_name() {
        p.set_file_name(format!("{}.deve-sub.update.lock", name.to_string_lossy()));
    }
    p
}

/// DS-AUD-B09: acquire an exclusive sidecar lock so two concurrent updates
/// don't clobber each other's temp/backup files. The lock is held until the
/// returned `File` is dropped (same pattern as `DbLock`).
fn acquire_update_lock(lock_path: &Path) -> Result<std::fs::File> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(lock_path)
        .with_context(|| format!("failed to open update lock {}", lock_path.display()))?;
    file.try_lock_exclusive().with_context(|| {
        format!(
            "another update is in progress (lock held at {})",
            lock_path.display()
        )
    })?;
    Ok(file)
}

/// Persist the rename as well as the downloaded contents. Opening a directory
/// and calling sync_all is safe Rust on the supported Linux platforms.
fn atomic_write_fsync(target: &Path, tmp: &Path) -> Result<()> {
    std::fs::rename(tmp, target)
        .with_context(|| format!("failed to rename {tmp:?} to {target:?}"))?;
    let f = std::fs::File::open(target)
        .with_context(|| format!("failed to reopen {target:?} for fsync"))?;
    f.sync_all()
        .with_context(|| format!("failed to fsync {target:?}"))?;
    std::fs::File::open(target.parent().unwrap_or(Path::new(".")))?.sync_all()?;
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

/// DS-AUD-B09: run `<binary> --version` and assert it reports the target
/// version. Catches a wrong-arch or corrupted binary before the swap.
async fn verify_binary_version(binary: &Path, expected: &str) -> Result<()> {
    use tokio::io::AsyncReadExt;
    let mut child = tokio::process::Command::new(binary)
        .arg("--version")
        .kill_on_drop(true)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .with_context(|| format!("failed to execute {}", binary.display()))?;
    // WHY: timeout covers pipe reads and process exit, while kill_on_drop
    // terminates the child on error/cancellation. An untrusted version output
    // cannot consume unbounded memory or block the async executor.
    let output = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let stdout = child.stdout.take().context("missing version stdout")?;
        let mut output = Vec::new();
        stdout.take(4097).read_to_end(&mut output).await?;
        if output.len() > 4096 {
            bail!("binary version output exceeds 4096 bytes");
        }
        let status = child.wait().await?;
        if !status.success() {
            bail!("downloaded binary --version exited with {status}");
        }
        Ok::<_, anyhow::Error>(output)
    })
    .await
    .context("binary --version timed out")??;
    let stdout = String::from_utf8_lossy(&output);
    if !stdout.split_whitespace().any(|t| t == expected) {
        bail!("downloaded binary version does not match {expected:?}");
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

async fn try_systemd_restart() -> Result<bool> {
    let service = "/etc/systemd/system/deve-sub.service";
    if !Path::new(service).exists() {
        return Ok(false);
    }
    let status = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::process::Command::new("systemctl")
            .args(["restart", "deve-sub"])
            .kill_on_drop(true)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status(),
    )
    .await
    .context("systemctl restart timed out")?
    .context("failed to run systemctl restart")?;
    if !status.success() {
        bail!("systemctl restart failed: {status}");
    }
    println!("systemd service restarted.");
    Ok(true)
}

/// DS-AUD-B09: poll the health endpoint until it returns 200 AND the reported
/// version equals `expected`. The old process on the same port returns 200
/// but reports the old version — caught here.
async fn wait_healthy_version(url: &str, expected: &str, timeout_secs: u64) -> bool {
    #[derive(Deserialize)]
    struct HealthLiveResponse {
        version: String,
    }

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
            // WHY: reqwest's `json()` needs the `json` feature, which the
            // workspace dep does not enable (default-features = false). Parse
            // the body with serde_json directly — already a dependency.
            if let Ok(body) = read_body_bounded(resp, 4096).await
                && let Ok(view) = serde_json::from_slice::<HealthLiveResponse>(&body)
                && view.version == expected
            {
                return true;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    false
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;

    #[test]
    fn is_newer_semver_basic() {
        assert!(is_newer("0.2.0", "0.1.0").unwrap());
        assert!(!is_newer("0.1.0", "0.2.0").unwrap());
        assert!(!is_newer("0.1.0", "0.1.0").unwrap());
    }

    #[test]
    fn is_newer_semver_prerelease() {
        // WHY: per SemVer, a prerelease sorts below the release of the same
        // version. 0.2.0-rc.1 is NOT newer than 0.2.0.
        assert!(!is_newer("0.2.0-rc.1", "0.2.0").unwrap());
        assert!(is_newer("0.2.0", "0.2.0-rc.1").unwrap());
        assert!(is_newer("0.2.0-rc.2", "0.2.0-rc.1").unwrap());
    }

    #[test]
    fn is_newer_rejects_invalid_semver() {
        assert!(is_newer("not-a-version", "0.1.0").is_err());
        assert!(is_newer("0.1.0", "v0.1.0").is_err());
    }

    #[test]
    fn parse_checksum_finds_entry() {
        let text = "abc123  deve-sub-linux-amd64\ndef456  deve-sub-linux-arm64\n";
        assert_eq!(
            parse_checksum(text, "deve-sub-linux-amd64"),
            Some("abc123".to_owned())
        );
        assert_eq!(
            parse_checksum(text, "deve-sub-linux-arm64"),
            Some("def456".to_owned())
        );
        assert_eq!(parse_checksum(text, "missing"), None);
    }

    #[test]
    fn backup_and_lock_path_are_siblings() {
        let bin = Path::new("/usr/local/bin/deve-sub");
        assert_eq!(backup_path(bin), Path::new("/usr/local/bin/deve-sub.bak"));
        assert_eq!(
            update_lock_path(bin),
            Path::new("/usr/local/bin/deve-sub.deve-sub.update.lock")
        );
    }
}

#[cfg(all(test, unix))]
#[path = "update_process_tests.rs"]
mod process_tests;
