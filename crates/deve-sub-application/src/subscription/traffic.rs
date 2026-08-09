//! Subscription traffic accounting commands.
//!
//! These functions orchestrate the [`TrafficRepository`] port to record and
//! query traffic observations used for quota enforcement (OUT-010/OUT-011) and
//! the `subscription-userinfo` response header. They do not execute SQL
//! directly. See `docs/plan/milestones/M6-subscription-distribution.md`
//! §"Traffic and expiry policy framework".
//!
//! M6 does not infer real proxy traffic from download counts (terminology
//! §116-121). Only explicit records (airport header parse, manual correction)
//! feed quota enforcement.

use deve_sub_domain::{TrafficRecord, TrafficRepository, TrafficSourceKind, TrafficSummary};
use deve_sub_kernel::SubscriptionId;

use super::error::{SubscriptionAppError, map_subscription_error};

/// Maximum length of the free-form `source_ref` field.
const MAX_SOURCE_REF_LEN: usize = 512;

/// Parameters for [`record_traffic`].
pub struct RecordTrafficParams {
    /// The subscription this observation belongs to.
    pub subscription_id: SubscriptionId,
    /// The origin of this observation.
    pub source_kind: TrafficSourceKind,
    /// Upload bytes in this sample.
    pub upload: u64,
    /// Download bytes in this sample.
    pub download: u64,
    /// Free-form source reference (e.g. source URL or admin note).
    pub source_ref: String,
}

/// Record a single traffic observation for a subscription.
///
/// Used by the airport-header parser (when an upstream source emits
/// `subscription-userinfo`) and by manual admin input. The record is appended;
/// aggregation is computed at read time by [`get_traffic_summary`].
///
/// # Errors
/// - [`SubscriptionAppError::InvalidInput`] — `source_ref` too long.
/// - [`SubscriptionAppError::Subscription`] — storage error.
pub async fn record_traffic(
    traffic_repo: &dyn TrafficRepository,
    params: RecordTrafficParams,
) -> Result<TrafficRecord, SubscriptionAppError> {
    if params.source_ref.len() > MAX_SOURCE_REF_LEN {
        return Err(SubscriptionAppError::InvalidInput(format!(
            "source_ref must not exceed {MAX_SOURCE_REF_LEN} characters"
        )));
    }

    let record = TrafficRecord::new(
        params.subscription_id,
        params.source_kind,
        params.upload,
        params.download,
        params.source_ref,
    );

    traffic_repo
        .create(&record)
        .await
        .map_err(map_subscription_error)?;

    Ok(record)
}

/// Get the aggregated traffic summary for a subscription.
///
/// Returns the total upload/download and a per-source-kind breakdown. Used by
/// the admin traffic dashboard and to compute the `subscription-userinfo`
/// header at delivery time.
///
/// # Errors
/// - [`SubscriptionAppError::Subscription`] — storage error.
pub async fn get_traffic_summary(
    traffic_repo: &dyn TrafficRepository,
    subscription_id: SubscriptionId,
) -> Result<TrafficSummary, SubscriptionAppError> {
    traffic_repo
        .get_summary(subscription_id)
        .await
        .map_err(map_subscription_error)
}

/// Parameters for [`apply_manual_correction`].
pub struct ManualCorrectionParams {
    /// The subscription this correction applies to.
    pub subscription_id: SubscriptionId,
    /// Upload bytes to record.
    pub upload: u64,
    /// Download bytes to record.
    pub download: u64,
    /// Admin note explaining the correction.
    pub note: String,
}

/// Apply a manual traffic correction to a subscription.
///
/// Records a [`TrafficSourceKind::ManualCorrection`] entry. This is the admin
/// escape hatch for correcting drifted traffic totals (e.g. after a probe
/// mismatch or an airport header that stopped reporting). The correction is
/// appended like any other record; aggregation is sum-based.
///
/// # Errors
/// - [`SubscriptionAppError::InvalidInput`] — `note` too long.
/// - [`SubscriptionAppError::Subscription`] — storage error.
pub async fn apply_manual_correction(
    traffic_repo: &dyn TrafficRepository,
    params: ManualCorrectionParams,
) -> Result<TrafficRecord, SubscriptionAppError> {
    if params.note.len() > MAX_SOURCE_REF_LEN {
        return Err(SubscriptionAppError::InvalidInput(format!(
            "note must not exceed {MAX_SOURCE_REF_LEN} characters"
        )));
    }

    let record = TrafficRecord::new(
        params.subscription_id,
        TrafficSourceKind::ManualCorrection,
        params.upload,
        params.download,
        params.note,
    );

    traffic_repo
        .create(&record)
        .await
        .map_err(map_subscription_error)?;

    Ok(record)
}
