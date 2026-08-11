//! `deve-sub health` — HTTP loopback health probes for Docker HEALTHCHECK.
//!
//! Probes the running server's `/health/live` and `/health/ready` endpoints
//! via HTTP. Exits 0 on 2xx, 1 on non-2xx or connection failure. See
//! ADR-0006 for the boundary justification (CLI infrastructure probe, not
//! business logic).

use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Args, Subcommand};

/// Default liveness endpoint. Uses 127.0.0.1 (not localhost) to avoid IPv6
/// `::1` resolution ambiguity in minimal containers where the server binds
/// 0.0.0.0 (IPv4 only).
const DEFAULT_LIVE_URL: &str = "http://127.0.0.1:8080/health/live";

/// Default readiness endpoint. Same IPv4 rationale as `DEFAULT_LIVE_URL`.
const DEFAULT_READY_URL: &str = "http://127.0.0.1:8080/health/ready";

/// Health probe command container.
#[derive(Args)]
pub struct HealthArgs {
    #[command(subcommand)]
    pub command: HealthSubCommand,
}

/// Health subcommands.
#[derive(Subcommand)]
pub enum HealthSubCommand {
    /// Liveness probe — exit 0 if the server's /health/live returns 2xx.
    Live(HealthProbeArgs),
    /// Readiness probe — exit 0 if the server's /health/ready returns 2xx.
    Ready(HealthProbeArgs),
}

/// Arguments shared by `health live` and `health ready`.
#[derive(Args)]
pub struct HealthProbeArgs {
    /// Health endpoint URL to probe. Defaults to the corresponding endpoint
    /// on 127.0.0.1:8080.
    #[arg(long)]
    pub url: Option<String>,

    /// Request timeout in seconds. Must be less than the Docker HEALTHCHECK
    /// timeout (3s) so the CLI timeout fires before Docker's.
    #[arg(long, default_value_t = 2)]
    pub timeout: u64,
}

/// Run `health live`.
pub async fn health_live(args: HealthProbeArgs) -> Result<()> {
    let url = args.url.as_deref().unwrap_or(DEFAULT_LIVE_URL);
    probe(url, args.timeout).await
}

/// Run `health ready`.
pub async fn health_ready(args: HealthProbeArgs) -> Result<()> {
    let url = args.url.as_deref().unwrap_or(DEFAULT_READY_URL);
    probe(url, args.timeout).await
}

/// Probe a health endpoint via HTTP GET. Returns `Ok(())` (exit 0) on 2xx,
/// `Err` (exit 1) on non-2xx or connection failure/timeout.
async fn probe(url: &str, timeout_secs: u64) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .context("failed to build HTTP client")?;

    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("health probe failed: {url}"))?;

    let status = response.status();
    if status.is_success() {
        Ok(())
    } else {
        anyhow::bail!("health probe returned {status}")
    }
}
