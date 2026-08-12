//! Traffic accounting entities for subscription delivery quota enforcement.
//!
//! A [`TrafficRecord`] captures one observed upload/download sample for a
//! subscription. Records are aggregated per subscription (summed by
//! `source_kind`) to compute consumed traffic, which is then compared against
//! the Subscription's `traffic_limit` and the owning User's `traffic_quota`.
//!
//! M6 does not infer real proxy traffic from download counts (terminology
//! §116-121). Download counts may be recorded as observability data but never
//! feed quota enforcement. The `subscription-userinfo` response header
//! reflects the aggregated traffic state. See
//! `docs/plan/milestones/M6-subscription-distribution.md` §"Traffic and
//! expiry policy framework".

use deve_sub_kernel::{SubscriptionId, Timestamp};

/// The origin of a traffic observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrafficSourceKind {
    /// Parsed from an upstream source's `subscription-userinfo` response
    /// header.
    AirportHeader,
    /// Manually entered by an admin (correction or initial import).
    ManualCorrection,
    /// Reserved for M7 probe-based measurement; not populated in M6.
    Probe,
}

impl TrafficSourceKind {
    /// Convert to the single-character discriminator stored in the database.
    #[must_use]
    pub const fn as_db_char(&self) -> &'static str {
        match self {
            Self::AirportHeader => "A",
            Self::ManualCorrection => "M",
            Self::Probe => "P",
        }
    }

    /// Parse from the single-character database discriminator.
    ///
    /// # Errors
    /// Returns `None` if the character does not match a known source kind.
    #[must_use]
    pub fn from_db_char(c: &str) -> Option<Self> {
        match c {
            "A" => Some(Self::AirportHeader),
            "M" => Some(Self::ManualCorrection),
            "P" => Some(Self::Probe),
            _ => None,
        }
    }

    /// Convert to the kebab-case string used in API responses.
    #[must_use]
    pub const fn as_kebab(&self) -> &'static str {
        match self {
            Self::AirportHeader => "airport-header",
            Self::ManualCorrection => "manual-correction",
            Self::Probe => "probe",
        }
    }
}

/// A single traffic observation for a subscription.
///
/// Each record captures the upload and download byte counts from one source
/// at one point in time. Aggregation (sum per subscription, optionally
/// grouped by `source_kind`) produces the consumed-traffic totals used for
/// quota enforcement and the `subscription-userinfo` header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrafficRecord {
    /// Unique identifier (ULID).
    pub id: deve_sub_kernel::TrafficRecordId,
    /// The subscription this record belongs to.
    pub subscription_id: SubscriptionId,
    /// The origin of this observation.
    pub source_kind: TrafficSourceKind,
    /// Upload bytes in this sample.
    pub upload: u64,
    /// Download bytes in this sample.
    pub download: u64,
    /// When this record was observed/recorded.
    pub recorded_at: Timestamp,
    /// Free-form source reference (e.g. the source URL or admin note).
    pub source_ref: String,
}

impl TrafficRecord {
    /// Create a new traffic record.
    #[must_use]
    pub fn new(
        subscription_id: SubscriptionId,
        source_kind: TrafficSourceKind,
        upload: u64,
        download: u64,
        source_ref: String,
    ) -> Self {
        Self {
            id: deve_sub_kernel::TrafficRecordId::new(),
            subscription_id,
            source_kind,
            upload,
            download,
            recorded_at: Timestamp::now(),
            source_ref,
        }
    }
}

/// Aggregated traffic totals for a subscription, optionally broken down by
/// source kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrafficSummary {
    /// Total upload bytes across all source kinds.
    pub upload: u64,
    /// Total download bytes across all source kinds.
    pub download: u64,
    /// Per-source-kind breakdown (kebab-case kind → (upload, download)).
    pub by_source: Vec<(TrafficSourceKind, u64, u64)>,
}

impl TrafficSummary {
    /// Total consumed traffic (upload + download).
    #[must_use]
    pub fn total(&self) -> u64 {
        self.upload.saturating_add(self.download)
    }
}

/// A daily traffic snapshot for one subscription.
///
/// Computed by the M10 aggregation job: sums all [`TrafficRecord`]s for a
/// subscription on a given UTC day. The `(subscription_id, date)` pair is
/// unique — re-running the aggregation upserts the row. See
/// `docs/plan/milestones/M10-observability-and-audit.md` §"Traffic daily
/// snapshot model".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrafficDailySnapshot {
    pub subscription_id: SubscriptionId,
    pub date: String,
    pub total_upload: u64,
    pub total_download: u64,
    pub source_breakdown: Vec<(TrafficSourceKind, u64, u64)>,
    pub computed_at: Timestamp,
}

impl TrafficDailySnapshot {
    /// Create a new daily snapshot.
    #[must_use]
    pub fn new(
        subscription_id: SubscriptionId,
        date: String,
        total_upload: u64,
        total_download: u64,
        source_breakdown: Vec<(TrafficSourceKind, u64, u64)>,
    ) -> Self {
        Self {
            subscription_id,
            date,
            total_upload,
            total_download,
            source_breakdown,
            computed_at: Timestamp::now(),
        }
    }

    /// Total traffic for this day (upload + download).
    #[must_use]
    pub fn total(&self) -> u64 {
        self.total_upload.saturating_add(self.total_download)
    }
}
