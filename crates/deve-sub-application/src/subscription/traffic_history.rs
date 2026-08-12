//! Traffic history aggregation and query (M10).
//!
//! `aggregate_daily_traffic` sums all [`TrafficRecord`]s for a given UTC day
//! and upserts a [`TrafficDailySnapshot`] per subscription. `list_traffic_history`
//! reads snapshots for chart rendering, filling gaps with zero-value entries.
//!
//! See `docs/plan/milestones/M10-observability-and-audit.md`.

use std::collections::BTreeMap;

use deve_sub_domain::{
    SubscriptionError, TrafficDailySnapshot, TrafficDailySnapshotRepository, TrafficRepository,
    TrafficSourceKind,
};
use deve_sub_kernel::SubscriptionId;

/// Aggregate traffic records for a single UTC day into daily snapshots.
///
/// For each subscription with traffic records in `[day_start, day_end)`, sums
/// upload/download grouped by `source_kind` and upserts a snapshot row. The
/// job is idempotent: re-running for the same day replaces existing snapshots.
///
/// # Parameters
/// - `traffic_repo`: raw traffic records.
/// - `snapshot_repo`: daily snapshot upsert target.
/// - `day`: the UTC date string (`YYYY-MM-DD`).
/// - `day_start_iso`: ISO 8601 timestamp for the start of the day (inclusive).
/// - `day_end_iso`: ISO 8601 timestamp for the start of the next day (exclusive).
///
/// # Returns
/// The number of subscription snapshots upserted.
pub async fn aggregate_daily_traffic(
    traffic_repo: &dyn TrafficRepository,
    snapshot_repo: &dyn TrafficDailySnapshotRepository,
    day: &str,
    day_start_iso: &str,
    day_end_iso: &str,
) -> Result<usize, SubscriptionError> {
    let sub_ids = traffic_repo
        .subscriptions_with_traffic_in_range(day_start_iso, day_end_iso)
        .await?;

    let mut count = 0;
    for sub_id in sub_ids {
        let summary = get_daily_summary(traffic_repo, sub_id, day_start_iso, day_end_iso).await?;
        let snapshot = TrafficDailySnapshot::new(
            sub_id,
            day.to_owned(),
            summary.total_upload,
            summary.total_download,
            summary.by_source,
        );
        snapshot_repo.upsert(&snapshot).await?;
        count += 1;
    }
    Ok(count)
}

struct DailySummary {
    total_upload: u64,
    total_download: u64,
    by_source: Vec<(TrafficSourceKind, u64, u64)>,
}

async fn get_daily_summary(
    traffic_repo: &dyn TrafficRepository,
    subscription_id: SubscriptionId,
    day_start_iso: &str,
    day_end_iso: &str,
) -> Result<DailySummary, SubscriptionError> {
    let summary = traffic_repo
        .get_summary_in_range(subscription_id, day_start_iso, day_end_iso)
        .await?;
    Ok(DailySummary {
        total_upload: summary.upload,
        total_download: summary.download,
        by_source: summary.by_source,
    })
}

/// A single day's traffic data point for the history chart.
#[derive(Debug, Clone)]
pub struct TrafficHistoryPoint {
    pub date: String,
    pub total_upload: u64,
    pub total_download: u64,
    pub source_breakdown: Vec<(TrafficSourceKind, u64, u64)>,
}

/// List daily traffic history for a subscription, filling gaps with zero-value
/// entries so the chart is continuous.
///
/// # Parameters
/// - `snapshot_repo`: daily snapshot storage.
/// - `subscription_id`: the subscription to query.
/// - `start_date`: inclusive start (`YYYY-MM-DD`).
/// - `end_date`: inclusive end (`YYYY-MM-DD`).
pub async fn list_traffic_history_for_subscription(
    snapshot_repo: &dyn TrafficDailySnapshotRepository,
    subscription_id: SubscriptionId,
    start_date: &str,
    end_date: &str,
) -> Result<Vec<TrafficHistoryPoint>, SubscriptionError> {
    let snapshots = snapshot_repo
        .list_for_subscription(subscription_id, start_date, end_date)
        .await?;
    Ok(fill_gaps(snapshots, start_date, end_date))
}

/// List global daily traffic history (all subscriptions aggregated per day),
/// filling gaps with zero-value entries.
pub async fn list_traffic_history_global(
    snapshot_repo: &dyn TrafficDailySnapshotRepository,
    start_date: &str,
    end_date: &str,
) -> Result<Vec<TrafficHistoryPoint>, SubscriptionError> {
    let snapshots = snapshot_repo.list_global(start_date, end_date).await?;
    Ok(fill_gaps(snapshots, start_date, end_date))
}

type DayAccumulator = (u64, u64, BTreeMap<&'static str, (u64, u64)>);

fn fill_gaps(
    snapshots: Vec<TrafficDailySnapshot>,
    start_date: &str,
    end_date: &str,
) -> Vec<TrafficHistoryPoint> {
    let mut by_date: BTreeMap<String, DayAccumulator> = BTreeMap::new();
    for snap in snapshots {
        let entry = by_date
            .entry(snap.date)
            .or_insert_with(|| (0, 0, BTreeMap::new()));
        entry.0 = entry.0.saturating_add(snap.total_upload);
        entry.1 = entry.1.saturating_add(snap.total_download);
        for (kind, up, down) in &snap.source_breakdown {
            let ke = entry.2.entry(kind.as_db_char()).or_insert((0, 0));
            ke.0 = ke.0.saturating_add(*up);
            ke.1 = ke.1.saturating_add(*down);
        }
    }

    let mut result = Vec::new();
    let mut current = start_date.to_owned();
    loop {
        let (up, down, breakdown_map) = by_date.remove(&current).unwrap_or((0, 0, BTreeMap::new()));
        let source_breakdown: Vec<(TrafficSourceKind, u64, u64)> = breakdown_map
            .into_iter()
            .filter_map(|(key, (u, d))| TrafficSourceKind::from_db_char(key).map(|k| (k, u, d)))
            .collect();
        result.push(TrafficHistoryPoint {
            date: current.clone(),
            total_upload: up,
            total_download: down,
            source_breakdown,
        });
        if current == end_date {
            break;
        }
        match increment_date(&current) {
            Some(next) => current = next,
            None => break,
        }
    }
    result
}

fn increment_date(date: &str) -> Option<String> {
    let parts: Vec<&str> = date.split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    let year: u32 = parts[0].parse().ok()?;
    let month: u32 = parts[1].parse().ok()?;
    let day: u32 = parts[2].parse().ok()?;

    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400) {
                29
            } else {
                28
            }
        }
        _ => return None,
    };

    if day < days_in_month {
        Some(format!("{year:04}-{month:02}-{:02}", day + 1))
    } else if month < 12 {
        Some(format!("{year:04}-{:02}-01", month + 1))
    } else {
        Some(format!("{:04}-01-01", year + 1))
    }
}
