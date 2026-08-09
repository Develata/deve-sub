//! Probe application commands: probe source CRUD and probe run lifecycle.
//!
//! This module orchestrates domain services and port interfaces. It does not
//! execute SQL directly. See `docs/plan/03-architecture.md` §"Lightweight
//! CQRS".

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use deve_sub_domain::{
    ProbeRun, ProbeRunRepository, ProbeRunStatus, ProbeSource, ProbeSourceKind,
    ProbeSourceRepository, ProbeType,
};
use deve_sub_kernel::{ProbeRunId, ProbeSourceId, Timestamp};

use super::error::ProbeAppError;

/// Parameters for creating a probe source.
#[derive(Debug, Clone)]
pub struct CreateProbeSourceParams {
    pub kind: ProbeSourceKind,
    pub name: String,
    pub endpoint_url: String,
    pub auth_config: String,
    pub subscription_id: Option<deve_sub_kernel::SubscriptionId>,
}

/// Create a new probe source.
///
/// # Errors
/// Returns [`ProbeAppError::InvalidInput`] if the name is empty or the URL is
/// malformed, [`ProbeAppError::NameExists`] if the name is taken.
pub async fn create_probe_source(
    repo: &dyn ProbeSourceRepository,
    params: CreateProbeSourceParams,
) -> Result<ProbeSource, ProbeAppError> {
    if params.name.trim().is_empty() {
        return Err(ProbeAppError::InvalidInput(
            "name must not be empty".to_owned(),
        ));
    }
    if params.endpoint_url.trim().is_empty() {
        return Err(ProbeAppError::InvalidInput(
            "endpoint_url must not be empty".to_owned(),
        ));
    }
    // Validate URL format.
    if url::Url::parse(&params.endpoint_url).is_err() {
        return Err(ProbeAppError::InvalidInput(
            "endpoint_url is not a valid URL".to_owned(),
        ));
    }

    let now = Timestamp::now();
    let source = ProbeSource {
        id: ProbeSourceId::new(),
        kind: params.kind,
        name: params.name,
        endpoint_url: params.endpoint_url,
        auth_config: params.auth_config,
        subscription_id: params.subscription_id,
        enabled: true,
        last_sync_at: None,
        last_sync_status: None,
        last_counter_snapshot: None,
        created_at: now,
        updated_at: now,
    };
    repo.create(&source).await?;
    Ok(source)
}

/// Get a probe source by ID.
///
/// # Errors
/// Returns [`ProbeAppError::SourceNotFound`] if the source does not exist.
pub async fn get_probe_source(
    repo: &dyn ProbeSourceRepository,
    id: ProbeSourceId,
) -> Result<ProbeSource, ProbeAppError> {
    repo.find_by_id(id)
        .await?
        .ok_or(ProbeAppError::SourceNotFound)
}

/// List probe sources with cursor pagination.
pub async fn list_probe_sources(
    repo: &dyn ProbeSourceRepository,
    cursor: Option<ProbeSourceId>,
    limit: u32,
    kind: Option<ProbeSourceKind>,
) -> Result<Vec<ProbeSource>, ProbeAppError> {
    Ok(repo.list(cursor, limit, kind).await?)
}

/// Parameters for updating a probe source.
#[derive(Debug, Clone)]
pub struct UpdateProbeSourceParams {
    pub id: ProbeSourceId,
    pub name: Option<String>,
    pub endpoint_url: Option<String>,
    pub auth_config: Option<String>,
    pub subscription_id: Option<Option<deve_sub_kernel::SubscriptionId>>,
    pub enabled: Option<bool>,
}

/// Update a probe source. Only provided fields are mutated.
///
/// # Errors
/// Returns [`ProbeAppError::SourceNotFound`] if the source does not exist.
pub async fn update_probe_source(
    repo: &dyn ProbeSourceRepository,
    params: UpdateProbeSourceParams,
) -> Result<ProbeSource, ProbeAppError> {
    let mut source = repo
        .find_by_id(params.id)
        .await?
        .ok_or(ProbeAppError::SourceNotFound)?;

    if let Some(name) = params.name {
        if name.trim().is_empty() {
            return Err(ProbeAppError::InvalidInput(
                "name must not be empty".to_owned(),
            ));
        }
        source.name = name;
    }
    if let Some(url) = params.endpoint_url {
        if url::Url::parse(&url).is_err() {
            return Err(ProbeAppError::InvalidInput(
                "endpoint_url is not a valid URL".to_owned(),
            ));
        }
        source.endpoint_url = url;
    }
    if let Some(auth) = params.auth_config {
        source.auth_config = auth;
    }
    if let Some(sub_id) = params.subscription_id {
        source.subscription_id = sub_id;
    }
    if let Some(enabled) = params.enabled {
        source.enabled = enabled;
    }
    source.updated_at = Timestamp::now();

    repo.update(&source).await?;
    Ok(source)
}

/// Delete a probe source.
///
/// # Errors
/// Returns [`ProbeAppError::SourceNotFound`] if the source does not exist.
pub async fn delete_probe_source(
    repo: &dyn ProbeSourceRepository,
    id: ProbeSourceId,
) -> Result<(), ProbeAppError> {
    repo.delete(id).await?;
    Ok(())
}

/// Parameters for starting a probe run.
#[derive(Debug, Clone)]
pub struct StartProbeRunParams {
    pub probe_type: ProbeType,
    pub node_ids: Vec<deve_sub_kernel::NodeId>,
}

/// Create a probe run in `Pending` status. The runner picks it up and
/// executes it asynchronously.
///
/// # Errors
/// Returns [`ProbeAppError::InvalidInput`] if the node list is empty.
pub async fn start_probe_run(
    run_repo: &dyn ProbeRunRepository,
    params: StartProbeRunParams,
) -> Result<ProbeRun, ProbeAppError> {
    if params.node_ids.is_empty() {
        return Err(ProbeAppError::InvalidInput(
            "node_ids must not be empty".to_owned(),
        ));
    }
    let run = ProbeRun {
        id: ProbeRunId::new(),
        probe_type: params.probe_type,
        node_ids: params.node_ids,
        status: ProbeRunStatus::Pending,
        results: Vec::new(),
        created_at: Timestamp::now(),
        completed_at: None,
    };
    run_repo.create(&run).await?;
    Ok(run)
}

/// Get a probe run by ID.
///
/// # Errors
/// Returns [`ProbeAppError::RunNotFound`] if the run does not exist.
pub async fn get_probe_run(
    run_repo: &dyn ProbeRunRepository,
    id: ProbeRunId,
) -> Result<ProbeRun, ProbeAppError> {
    run_repo
        .find_by_id(id)
        .await?
        .ok_or(ProbeAppError::RunNotFound)
}

/// Cancel a probe run by setting the cancellation flag. The runner observes
/// the flag, aborts pending probes, and marks the run `Cancelled`
/// (NODE-016).
///
/// # Errors
/// Returns [`ProbeAppError::RunNotFound`] if the run does not exist, or
/// [`ProbeAppError::RunAlreadyTerminal`] if the run is already terminal.
pub async fn cancel_probe_run(
    run_repo: &dyn ProbeRunRepository,
    cancelled_flags: &std::collections::HashMap<ProbeRunId, Arc<AtomicBool>>,
    id: ProbeRunId,
) -> Result<(), ProbeAppError> {
    let run = run_repo
        .find_by_id(id)
        .await?
        .ok_or(ProbeAppError::RunNotFound)?;

    if run.status.is_terminal() {
        return Err(ProbeAppError::RunAlreadyTerminal);
    }

    // If the runner has a cancellation flag for this run, fire it.
    if let Some(flag) = cancelled_flags.get(&id) {
        flag.store(true, Ordering::Relaxed);
    } else {
        // The run hasn't been picked up by the runner yet, or the runner
        // manages it differently. Mark it as cancelled directly so the
        // runner sees the terminal status when it tries to update.
        run_repo
            .update_status(
                id,
                ProbeRunStatus::Cancelled,
                &run.results,
                Some(Timestamp::now()),
            )
            .await?;
    }

    Ok(())
}

/// Mark any probe runs left in `Running` or `Pending` status as `Failed`
/// (crash recovery on startup, constraint #20).
pub async fn recover_crashed_runs(run_repo: &dyn ProbeRunRepository) -> Result<u64, ProbeAppError> {
    Ok(run_repo.recover_crashed_runs().await?)
}
