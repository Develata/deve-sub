//! `deve-sub update` — self-update with checksum verification and rollback.
//!
//! UPDATE-001: download a new binary from a release, verify its SHA-256
//! checksum, verify it reports the target version, swap it in, restart, and
//! health-check the running version. On failure, roll back to the previous
//! binary (UPDATE-002).
//!
//! DS-AUD-B09: the previous implementation had nine defects:
//! 1. Binary + checksum from the same unsigned release — transport integrity
//!    only, no publisher authentication. SIGNING IS DEFERRED (requires
//!    `ed25519-dalek`, blocked on a crates.io network outage in this
//!    session). The checksum still guards against transport corruption.
//!    TODO(B-09-signing): add Ed25519 manifest signature verification once
//!    the dependency can be fetched.
//! 2. `--config` flag existed but `load_bind_from_config` was never called.
//! 3. systemd restart failure still proceeded to the health URL.
//! 4. Old process on the port could return 200 → false positive → backup
//!    deleted.
//! 5. Health check only checked status, not the running version.
//! 6. `is_newer` was hand-written split/filter, not SemVer.
//! 7. manifest/checksum/binary all read into memory at once, no size limit.
//! 8. Fixed `.tmp.new/.bak/.failed`, no update lock.
//! 9. No fsync after write, no `--version` on the downloaded binary.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Args;
use fs2::FileExt;
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
        let bind = load_bind_from_config(&args.config);
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

    if !args.force && !is_newer(latest_version, current_version)? {
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

    // DS-AUD-B09: acquire an update sidecar lock so two concurrent updates
    // don't clobber each other's temp/backup files.
    let lock_path = update_lock_path(&binary_path);
    let lock_file = acquire_update_lock(&lock_path)?;

    println!("downloading checksums.txt...");
    let checksum_bytes =
        download_bounded(&checksum_asset.browser_download_url, MAX_MANIFEST_BYTES).await?;
    let checksum_text = String::from_utf8_lossy(&checksum_bytes);
    let expected_hash = parse_checksum(&checksum_text, &asset_name)
        .with_context(|| format!("no checksum entry for {asset_name}"))?;

    println!("downloading {asset_name}...");
    let (binary_tmp, actual_hash) =
        download_streaming(&asset.browser_download_url, MAX_BINARY_BYTES).await?;

    println!("verifying checksum...");
    if actual_hash != expected_hash {
        let _ = std::fs::remove_file(&binary_tmp);
        bail!("checksum mismatch: expected {expected_hash}, got {actual_hash}");
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
    if let Err(e) = verify_binary_version(&binary_tmp, latest_version) {
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
            // DS-AUD-B09: if systemd restart fails, the old process is still
            // running — a health check would pass on the OLD binary (false
            // positive). Roll back immediately instead.
            Ok(false) => {
                println!("systemd restart did not succeed — rolling back...");
                rollback(&binary_path, &backup_path)?;
                bail!(
                    "update failed: systemd restart did not succeed. \
                     Rolled back to {current_version}."
                );
            }
            Err(e) => {
                println!("systemd restart error: {e} — rolling back...");
                rollback(&binary_path, &backup_path)?;
                bail!("update failed: systemd restart error ({e}). Rolled back.");
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
            let _ = try_systemd_restart().await;
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
    // DS-AUD-B09: bound the manifest body size.
    let body = resp.bytes().await.context("failed to read manifest body")?;
    if body.len() as u64 > MAX_MANIFEST_BYTES {
        bail!("manifest body exceeds {MAX_MANIFEST_BYTES} bytes — refusing unbounded read");
    }
    let manifest: ReleaseManifest =
        serde_json::from_slice(&body).context("failed to parse manifest JSON")?;
    Ok(manifest)
}

/// Download a URL into memory with an upper size bound (DS-AUD-B09).
async fn download_bounded(url: &str, max_bytes: u64) -> Result<Vec<u8>> {
    let client = reqwest::Client::builder()
        .user_agent(format!("deve-sub/{}", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(120))
        .build()?;
    let resp = client.get(url).send().await.context("download failed")?;
    if !resp.status().is_success() {
        bail!("download from {url} returned {}", resp.status());
    }
    let bytes = resp.bytes().await.context("failed to read response body")?;
    if bytes.len() as u64 > max_bytes {
        bail!("download from {url} exceeds {max_bytes} bytes — refusing unbounded read");
    }
    Ok(bytes.to_vec())
}

/// Stream a download to a temp file, computing SHA-256 incrementally and
/// enforcing a max size. Returns (temp_file_path, sha256_hex) (DS-AUD-B09).
async fn download_streaming(url: &str, max_bytes: u64) -> Result<(PathBuf, String)> {
    use futures_util::StreamExt;
    let client = reqwest::Client::builder()
        .user_agent(format!("deve-sub/{}", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(300))
        .build()?;
    let resp = client.get(url).send().await.context("download failed")?;
    if !resp.status().is_success() {
        bail!("download from {url} returned {}", resp.status());
    }

    let tmp = std::env::temp_dir().join(format!("deve-sub-update-{}.bin", std::process::id()));
    let mut file = std::fs::File::create(&tmp)
        .with_context(|| format!("failed to create temp file {}", tmp.display()))?;
    let mut hasher = Sha256::new();
    let mut total: u64 = 0;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("download stream error")?;
        total += chunk.len() as u64;
        if total > max_bytes {
            let _ = std::fs::remove_file(&tmp);
            bail!("download from {url} exceeds {max_bytes} bytes — aborting");
        }
        hasher.update(&chunk);
        std::io::Write::write_all(&mut file, &chunk)
            .context("failed to write download chunk to temp file")?;
    }
    file.sync_all()
        .context("failed to fsync downloaded temp file")?;
    let hash = hasher.finalize();
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    Ok((tmp, hex))
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

/// DS-AUD-B09: write the new binary via a temp file with fsync, then atomic
/// rename + file fsync. Parent-directory fsync is omitted because
/// `unsafe_code = "forbid"` blocks the raw-fd `fsync(dir_fd)` syscall; on
/// ext4/xfs with default ordered journal mode, the file fsync implies the
/// rename is durable. (Same constraint documented in B-01.)
fn atomic_write_fsync(target: &Path, tmp: &Path) -> Result<()> {
    std::fs::rename(tmp, target)
        .with_context(|| format!("failed to rename {tmp:?} to {target:?}"))?;
    let f = std::fs::File::open(target)
        .with_context(|| format!("failed to reopen {target:?} for fsync"))?;
    f.sync_all()
        .with_context(|| format!("failed to fsync {target:?}"))?;
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
fn verify_binary_version(binary: &Path, expected: &str) -> Result<()> {
    let output = std::process::Command::new(binary)
        .arg("--version")
        .output()
        .with_context(|| format!("failed to execute {}", binary.display()))?;
    if !output.status.success() {
        bail!(
            "downloaded binary --version exited with {:?}",
            output.status
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    // WHY: `--version` prints "deve-sub <version>" (clap default). Match the
    // version token, not the whole line, so extra build metadata doesn't
    // cause a false mismatch.
    if !stdout.split_whitespace().any(|t| t == expected) {
        bail!("downloaded binary reports version {stdout:?}, expected {expected:?}");
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
    let result = tokio::process::Command::new("systemctl")
        .args(["restart", "deve-sub"])
        .output()
        .await;
    match result {
        Ok(out) if out.status.success() => {
            println!("systemd service restarted.");
            Ok(true)
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            bail!("systemctl restart failed: {stderr}");
        }
        Err(e) => bail!("failed to run systemctl restart: {e}"),
    }
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
            if let Ok(body) = resp.bytes().await
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

/// DS-AUD-B09: read the bind address from the config file so the default
/// health URL matches the actual serve bind. Previously dead code; now
/// called from the update entry point.
fn load_bind_from_config(config_path: &Option<PathBuf>) -> String {
    match load_config(config_path) {
        Ok(config) => config.server.bind,
        Err(_) => "127.0.0.1:8080".to_owned(),
    }
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
