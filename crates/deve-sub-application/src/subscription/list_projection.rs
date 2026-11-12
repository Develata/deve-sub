//! Owner-scoped subscription list projection with current delivery short codes.

use std::collections::HashMap;

use deve_sub_domain::{ShortCodeRepository, SubscriptionRepository};
use deve_sub_kernel::{SubscriptionId, UserId};

use super::error::map_subscription_error;
use super::{SubscriptionAppError, SubscriptionWithShortCode, list_subscriptions};

/// List the owner's page with its current short codes using one bulk lookup.
///
/// The authorized subscription page bounds the credential query. Codes for
/// other owners or pages cannot enter this projection.
///
/// # Errors
/// Returns a storage error if either query fails.
pub async fn list_subscriptions_with_short_codes(
    sub_repo: &dyn SubscriptionRepository,
    short_code_repo: &dyn ShortCodeRepository,
    owner_id: UserId,
    cursor: Option<SubscriptionId>,
    limit: Option<u32>,
) -> Result<Vec<SubscriptionWithShortCode>, SubscriptionAppError> {
    let subscriptions = list_subscriptions(sub_repo, owner_id, cursor, limit).await?;
    let ids = subscriptions.iter().map(|s| s.id).collect::<Vec<_>>();
    let mut codes: HashMap<_, _> = short_code_repo
        .find_by_subscriptions(&ids)
        .await
        .map_err(map_subscription_error)?
        .into_iter()
        .map(|code| (code.subscription_id, code.code))
        .collect();
    Ok(subscriptions
        .into_iter()
        .map(|subscription| SubscriptionWithShortCode {
            short_code: codes.remove(&subscription.id),
            subscription,
        })
        .collect())
}
