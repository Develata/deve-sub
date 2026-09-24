//! Input validation shared by subscription lifecycle commands.

use deve_sub_compatibility::ProfileKind;
use deve_sub_domain::NodeSelector;
use deve_sub_kernel::Timestamp;
use time::format_description::well_known::Rfc3339;

use super::error::SubscriptionAppError;

/// Maximum subscription name length.
const MAX_NAME_LEN: usize = 128;

/// Maximum slug length.
const MAX_SLUG_LEN: usize = 128;

/// Parse an ISO 8601 (RFC 3339) timestamp string into a [`Timestamp`].
pub(super) fn parse_iso8601(s: &str) -> Result<Timestamp, SubscriptionAppError> {
    time::OffsetDateTime::parse(s, &Rfc3339)
        .map(Timestamp::from_offset_date_time)
        .map_err(|e| SubscriptionAppError::InvalidInput(format!("invalid expires_at: {e}")))
}

/// Validate a subscription name at the application boundary.
pub(super) fn validate_name(name: &str) -> Result<(), SubscriptionAppError> {
    if name.is_empty() {
        return Err(SubscriptionAppError::InvalidInput(
            "name must not be empty".to_owned(),
        ));
    }
    if name.len() > MAX_NAME_LEN {
        return Err(SubscriptionAppError::InvalidInput(format!(
            "name must not exceed {MAX_NAME_LEN} characters"
        )));
    }
    Ok(())
}

/// Validate a subscription slug at the application boundary.
pub(super) fn validate_slug(slug: &str) -> Result<(), SubscriptionAppError> {
    if slug.is_empty() {
        return Err(SubscriptionAppError::InvalidInput(
            "slug must not be empty".to_owned(),
        ));
    }
    if slug.len() > MAX_SLUG_LEN {
        return Err(SubscriptionAppError::InvalidInput(format!(
            "slug must not exceed {MAX_SLUG_LEN} characters"
        )));
    }
    Ok(())
}

/// Validate a profile string and return the parsed [`ProfileKind`].
pub(super) fn validate_profile(profile: &str) -> Result<ProfileKind, SubscriptionAppError> {
    ProfileKind::from_kebab(profile)
        .ok_or_else(|| SubscriptionAppError::UnknownProfile(profile.to_owned()))
}

/// Parse a [`NodeSelector`] from a raw JSON value.
pub(super) fn parse_node_selection(
    value: serde_json::Value,
) -> Result<NodeSelector, SubscriptionAppError> {
    serde_json::from_value(value)
        .map_err(|e| SubscriptionAppError::InvalidInput(format!("invalid node_selection: {e}")))
}

/// Validate a positive traffic limit representable by the storage contract.
pub(super) fn validate_traffic_limit(limit: Option<u64>) -> Result<(), SubscriptionAppError> {
    // WHY: `is_traffic_exceeded` treats `Some(0)` as unlimited, so accepting it
    // would persist state that contradicts delivered behavior (F-003).
    if limit.is_some_and(|value| value == 0 || value > i64::MAX as u64) {
        return Err(SubscriptionAppError::InvalidInput(
            "traffic_limit must be between 1 and 9223372036854775807; use null for unlimited"
                .to_owned(),
        ));
    }
    Ok(())
}

/// Validate a pinned template version against the storage integer contract.
pub(super) fn validate_template_version_pin(pin: Option<u64>) -> Result<(), SubscriptionAppError> {
    if pin.is_some_and(|value| value > i64::MAX as u64) {
        return Err(SubscriptionAppError::InvalidInput(
            "template_version_pin must not exceed 9223372036854775807".to_owned(),
        ));
    }
    Ok(())
}
