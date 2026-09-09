//! Traffic history aggregation and query (M10).
//!
//! Reads transactionally maintained daily snapshots for chart rendering.
//!
//! See `docs/plan/milestones/M10-observability-and-audit.md`.

use std::collections::BTreeMap;

use deve_sub_domain::{
    SubscriptionError, TrafficDailySnapshot, TrafficDailySnapshotRepository, TrafficSourceKind,
};
use deve_sub_kernel::SubscriptionId;

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

type DayAccumulator = (u64, u64, BTreeMap<TrafficSourceKind, (u64, u64)>);

fn fill_gaps(
    snapshots: Vec<TrafficDailySnapshot>,
    start_date: &str,
    end_date: &str,
) -> Vec<TrafficHistoryPoint> {
    // Defensive guard: an inverted range (start > end) would cause the loop
    // to iterate until year 9999. The public API guarantees start <= end via
    // compute_date_range, but this prevents a hang if the contract is
    // violated.
    if start_date > end_date {
        return Vec::new();
    }

    let mut by_date: BTreeMap<String, DayAccumulator> = BTreeMap::new();
    for snap in snapshots {
        let entry = by_date
            .entry(snap.date)
            .or_insert_with(|| (0, 0, BTreeMap::new()));
        entry.0 = entry.0.saturating_add(snap.total_upload);
        entry.1 = entry.1.saturating_add(snap.total_download);
        for (kind, up, down) in &snap.source_breakdown {
            let ke = entry.2.entry(*kind).or_insert((0, 0));
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
            .map(|(kind, (u, d))| (kind, u, d))
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
