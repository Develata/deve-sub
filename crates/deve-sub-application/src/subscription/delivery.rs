//! Subscription delivery query: resolve a delivery token, short code, or temp
//! link to generated subscription content.
//!
//! The delivery pipeline (M6 Slice 2 + Slice 3):
//! 1. Resolve the subscription via one of three entry points:
//!    - permanent token (`/sub/{token}`): HMAC-SHA256 digest → token row
//!    - short code (`/s/{code}`): code string → short code row → subscription
//!    - temp link (`/sub/{temp_token}`): HMAC-SHA256 digest → temp link row
//!      (checks revoked + expires_at)
//! 2. Resolve the Subscription; check `enabled`.
//! 3. Resolve the owning User; check `enabled` (disabled → 404, no leak).
//! 4. Resolve the profile: explicit path segment or User-Agent auto-detect.
//! 5. Cache lookup by `cache_key`; on miss, run `generate_for_delivery`
//!    (stores inactive, does NOT activate — preserves admin's active
//!    generation).
//! 6. Compute ETag (SHA-256 of content), set `subscription-userinfo`,
//!    `Content-Type`, `Content-Disposition`, `Cache-Control`.
//!
//! Traffic/expiry quota enforcement is M6 Slice 5; this query checks
//! `enabled` only. See `docs/plan/milestones/M6-subscription-distribution.md`
//! §"Delivery pipeline" and §"Slicing".

use deve_sub_compatibility::ProfileKind;
use deve_sub_domain::{
    GenerationCacheRepository, GenerationMode, GenerationRequest, NodePoolRepository,
    PoolMetaRepository, ShortCodeRepository, Subscription, SubscriptionRepository,
    SubscriptionTokenRepository, TempLinkRepository, TemplateRepository, TemplateVersionRepository,
    TrafficRepository, TrafficSummary, UserRepository,
};
use deve_sub_security::{MasterKey, hmac_digest};
use sha2::{Digest, Sha256};

use super::commands::PURPOSE_SUBSCRIPTION_TOKEN;
use super::error::SubscriptionAppError;

/// The result of a successful delivery: generated content plus the HTTP
/// response headers the delivery handler should set.
#[derive(Debug, Clone)]
pub struct DeliveryResult {
    /// The generated subscription content (YAML, JSON, or URI list).
    pub content: String,
    /// The resolved profile (kebab-case).
    pub profile: String,
    /// Strong ETag: quoted SHA-256 hex of the content.
    pub etag: String,
    /// HTTP `Content-Type` for the resolved profile.
    pub content_type: &'static str,
    /// HTTP `Content-Disposition` (attachment with profile-specific filename).
    pub content_disposition: String,
    /// HTTP `subscription-userinfo` header value.
    pub subscription_userinfo: String,
}

/// Bundled storage and crypto dependencies for delivery. Passing these as a
/// single struct keeps the delivery functions under the argument-count lint
/// and mirrors the `AppState` grouping the Delivery layer already holds.
pub struct DeliveryDeps<'a> {
    pub token_repo: &'a dyn SubscriptionTokenRepository,
    pub short_code_repo: &'a dyn ShortCodeRepository,
    pub temp_link_repo: &'a dyn TempLinkRepository,
    pub sub_repo: &'a dyn SubscriptionRepository,
    pub user_repo: &'a dyn UserRepository,
    pub traffic_repo: &'a dyn TrafficRepository,
    pub template_repo: &'a dyn TemplateRepository,
    pub version_repo: &'a dyn TemplateVersionRepository,
    pub pool_repo: &'a dyn NodePoolRepository,
    pub cache_repo: &'a dyn GenerationCacheRepository,
    pub pool_meta_repo: &'a dyn PoolMetaRepository,
    pub master_key: &'a MasterKey,
}

/// Resolve a permanent delivery token to subscription content.
///
/// `token_plaintext` is the raw token from the URL path. `profile` is the
/// explicit path segment (`Some("mihomo")`) or `None` for User-Agent
/// auto-detect. `user_agent` is the request's User-Agent header (used only
/// when `profile` is `None`).
///
/// # Errors
/// - [`SubscriptionAppError::TokenNotFound`] — token digest does not match
///   any row (OUT-009: 404, no existence leak).
/// - [`SubscriptionAppError::SubscriptionNotFound`] — token resolved but the
///   subscription was deleted (404).
/// - [`SubscriptionAppError::SubscriptionDisabled`] — subscription is
///   disabled (404, no leak).
/// - [`SubscriptionAppError::UserInactive`] — owning user is disabled (404).
/// - [`SubscriptionAppError::UnknownProfile`] — explicit profile not
///   recognized, or User-Agent could not be auto-detected (404 for no-leak
///   on bad explicit profile; 404 for undetectable User-Agent).
/// - [`SubscriptionAppError::GenerationFailed`] — on-demand generation failed
///   and no cached content is available (503, constraint #19).
/// - [`SubscriptionAppError::Storage`] / [`SubscriptionAppError::Security`]
///   — infra errors.
pub async fn deliver_subscription(
    deps: &DeliveryDeps<'_>,
    token_plaintext: &str,
    profile: Option<&str>,
    user_agent: Option<&str>,
) -> Result<DeliveryResult, SubscriptionAppError> {
    let token_digest = hmac_digest(
        PURPOSE_SUBSCRIPTION_TOKEN,
        token_plaintext,
        deps.master_key.as_bytes(),
    )?;

    // WHY two-stage lookup: the active token is the common path (every
    // non-rotated subscription). Only on an active-digest miss do we check
    // the previous_token_digest column to honor the rotation grace window.
    // `None` grace means the old token stays valid permanently (blueprint
    // §301-304); an expired grace (`until <= now`) returns 404.
    let token = if let Some(t) = deps.token_repo.find_by_token_hash(&token_digest).await? {
        t
    } else {
        let prev = deps
            .token_repo
            .find_by_previous_token_hash(&token_digest)
            .await?
            .ok_or(SubscriptionAppError::TokenNotFound)?;

        if !prev.is_previous_token_valid_at(deve_sub_kernel::Timestamp::now()) {
            return Err(SubscriptionAppError::TokenNotFound);
        }
        prev
    };

    let subscription = deps
        .sub_repo
        .find_by_id(token.subscription_id)
        .await?
        .ok_or(SubscriptionAppError::SubscriptionNotFound)?;

    deliver_for_subscription(deps, &subscription, profile, user_agent).await
}

/// Resolve a short code to subscription content (`GET /s/{code}`).
///
/// `code` is the base62 short code from the URL path. Unlike the permanent
/// token, the short code is stored in the clear and looked up directly. The
/// resolved subscription is then served through the standard delivery
/// pipeline.
///
/// # Errors
/// - [`SubscriptionAppError::ShortCodeNotFound`] — code does not match any
///   row (404, no existence leak).
/// - [`SubscriptionAppError::SubscriptionNotFound`] — code resolved but the
///   subscription was deleted (404).
/// - See [`deliver_subscription`] for the remaining error variants.
pub async fn deliver_by_short_code(
    deps: &DeliveryDeps<'_>,
    code: &str,
    profile: Option<&str>,
    user_agent: Option<&str>,
) -> Result<DeliveryResult, SubscriptionAppError> {
    let short_code = deps
        .short_code_repo
        .find_by_code(code)
        .await?
        .ok_or(SubscriptionAppError::ShortCodeNotFound)?;

    let subscription = deps
        .sub_repo
        .find_by_id(short_code.subscription_id)
        .await?
        .ok_or(SubscriptionAppError::SubscriptionNotFound)?;

    deliver_for_subscription(deps, &subscription, profile, user_agent).await
}

/// Resolve a temp link token to subscription content (`GET /sub/{temp_token}`).
///
/// `token_plaintext` is the raw temp token from the URL path. The digest is
/// looked up in the temp link table; if found, `revoked` and `expires_at` are
/// checked before delegating to the standard delivery pipeline.
///
/// # Errors
/// - [`SubscriptionAppError::TokenNotFound`] — temp token digest does not
///   match any row (404, no existence leak).
/// - [`SubscriptionAppError::TempLinkInvalid`] — temp link is revoked or
///   expired (404, no leak).
/// - [`SubscriptionAppError::SubscriptionNotFound`] — temp link resolved but
///   the subscription was deleted (404).
/// - See [`deliver_subscription`] for the remaining error variants.
pub async fn deliver_by_temp_link(
    deps: &DeliveryDeps<'_>,
    token_plaintext: &str,
    profile: Option<&str>,
    user_agent: Option<&str>,
) -> Result<DeliveryResult, SubscriptionAppError> {
    let token_digest = hmac_digest(
        PURPOSE_SUBSCRIPTION_TOKEN,
        token_plaintext,
        deps.master_key.as_bytes(),
    )?;

    let temp_link = deps
        .temp_link_repo
        .find_by_token_hash(&token_digest)
        .await?
        .ok_or(SubscriptionAppError::TokenNotFound)?;

    if !temp_link.is_valid_at(deve_sub_kernel::Timestamp::now()) {
        return Err(SubscriptionAppError::TempLinkInvalid);
    }

    let subscription = deps
        .sub_repo
        .find_by_id(temp_link.subscription_id)
        .await?
        .ok_or(SubscriptionAppError::SubscriptionNotFound)?;

    deliver_for_subscription(deps, &subscription, profile, user_agent).await
}

/// Shared delivery pipeline: given a resolved subscription, check access
/// control, resolve the profile, run generation, and build the response.
///
/// Enforcement order (blueprint §278-283):
/// 1. `!subscription.enabled` → 404 (no leak)
/// 2. `!user.enabled` → 404 (no leak)
/// 3. `user.is_expired()` → 403 (OUT-010, clear error)
/// 4. `subscription.is_expired(now)` → 403 (OUT-010)
/// 5. `user.is_traffic_exceeded(user_consumed)` → 429 (OUT-011)
/// 6. `subscription.is_traffic_exceeded(sub_consumed)` → 429 (OUT-011)
async fn deliver_for_subscription(
    deps: &DeliveryDeps<'_>,
    subscription: &Subscription,
    profile: Option<&str>,
    user_agent: Option<&str>,
) -> Result<DeliveryResult, SubscriptionAppError> {
    if !subscription.enabled {
        return Err(SubscriptionAppError::SubscriptionDisabled);
    }

    let user = deps
        .user_repo
        .find_by_id(subscription.owner_id)
        .await?
        .ok_or(SubscriptionAppError::UserInactive)?;

    if !user.enabled {
        return Err(SubscriptionAppError::UserInactive);
    }

    let now = deve_sub_kernel::Timestamp::now();

    if user.is_expired() {
        return Err(SubscriptionAppError::UserExpired);
    }

    if subscription.is_expired(now) {
        return Err(SubscriptionAppError::SubscriptionExpired);
    }

    let sub_traffic = deps.traffic_repo.get_summary(subscription.id).await?;

    let user_traffic = deps
        .traffic_repo
        .get_summary_for_user(subscription.owner_id)
        .await?;

    if user.is_traffic_exceeded(user_traffic.total()) {
        return Err(SubscriptionAppError::TrafficExceeded);
    }

    if subscription.is_traffic_exceeded(sub_traffic.total()) {
        return Err(SubscriptionAppError::TrafficExceeded);
    }

    let resolved_profile = resolve_profile(profile, user_agent, subscription)?;

    let request = GenerationRequest {
        template_id: subscription.template_id,
        profile: resolved_profile.as_kebab().to_owned(),
        mode: GenerationMode::Lenient,
        node_selection: Some(subscription.node_selection.clone()),
        template_version_pin: subscription.template_version_pin,
    };

    let result = crate::template::generate_for_delivery(
        deps.template_repo,
        deps.version_repo,
        deps.pool_repo,
        deps.cache_repo,
        deps.pool_meta_repo,
        request,
    )
    .await
    .map_err(|e| SubscriptionAppError::GenerationFailed(e.to_string()))?;

    let etag = compute_etag(&result.content);
    let content_type = content_type_for(resolved_profile);
    let content_disposition = content_disposition_for(resolved_profile, &subscription.slug);
    let subscription_userinfo =
        build_subscription_userinfo(subscription, &sub_traffic, user.traffic_quota);

    Ok(DeliveryResult {
        content: result.content,
        profile: resolved_profile.as_kebab().to_owned(),
        etag,
        content_type,
        content_disposition,
        subscription_userinfo,
    })
}

/// Resolve the target profile from the explicit path segment or User-Agent.
///
/// If `profile` is `Some`, validate it against the Subscription's profile
/// (must match) and parse to `ProfileKind`. If `None`, auto-detect from the
/// User-Agent header and validate the detected profile against the
/// Subscription's bound profile.
///
/// WHY the resolved profile must match the Subscription's configured profile:
/// a Subscription is bound to one profile at creation; serving a different
/// profile for the same token would produce content the client did not
/// subscribe to. The `/sub/{token}/{profile}` path segment is a convenience
/// for clients that include it, not a profile-switching mechanism.
///
/// DS-AUD-B16: the auto-detect path previously returned the UA-detected
/// profile without comparing it to `subscription.profile`, so a sing-box-
/// bound subscription requested by a Mihomo client silently served Mihomo
/// content. The explicit-path branch already enforced binding; the
/// auto-detect branch now enforces it too, rejecting a mismatch with
/// `UnknownProfile` naming both the detected and bound profiles. Erroring
/// (not falling back to the bound profile) keeps the two paths consistent:
/// the same subscription 404s whether the client names the wrong profile
/// or omits it while sending a mismatched UA.
fn resolve_profile(
    profile: Option<&str>,
    user_agent: Option<&str>,
    subscription: &Subscription,
) -> Result<ProfileKind, SubscriptionAppError> {
    match profile {
        Some(p) => {
            if p != subscription.profile {
                return Err(SubscriptionAppError::UnknownProfile(p.to_owned()));
            }
            ProfileKind::from_kebab(p)
                .ok_or_else(|| SubscriptionAppError::UnknownProfile(p.to_owned()))
        }
        None => {
            let detected = detect_profile_from_user_agent(user_agent).ok_or_else(|| {
                SubscriptionAppError::UnknownProfile(
                    "could not auto-detect profile from User-Agent".to_owned(),
                )
            })?;
            if detected.as_kebab() != subscription.profile {
                return Err(SubscriptionAppError::UnknownProfile(format!(
                    "auto-detected profile '{}' does not match subscription's bound profile '{}'",
                    detected.as_kebab(),
                    subscription.profile
                )));
            }
            Ok(detected)
        }
    }
}

/// Infer the target profile from the User-Agent header.
///
/// Common proxy client User-Agents:
/// - Clash / Mihomo: contains "clash" or "mihomo"
/// - sing-box: contains "singbox" or "sing-box"
/// - V2RayN / V2Ray: contains "v2ray"
/// - Xray: contains "xray"
/// - Shadowrocket: contains "shadowrocket"
/// - Fallback: `None` (caller returns 404).
#[must_use]
pub fn detect_profile_from_user_agent(user_agent: Option<&str>) -> Option<ProfileKind> {
    let ua = user_agent?.to_ascii_lowercase();
    if ua.contains("shadowrocket") {
        Some(ProfileKind::Shadowrocket)
    } else if ua.contains("clash") || ua.contains("mihomo") {
        Some(ProfileKind::Mihomo)
    } else if ua.contains("sing-box") || ua.contains("singbox") {
        Some(ProfileKind::SingBox)
    } else if ua.contains("xray") {
        Some(ProfileKind::Xray)
    } else if ua.contains("v2ray") {
        Some(ProfileKind::V2Ray)
    } else {
        None
    }
}

/// Compute a strong ETag: quoted SHA-256 hex of the content.
fn compute_etag(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for b in digest.iter() {
        hex.push_str(&format!("{b:02x}"));
    }
    format!("\"{hex}\"")
}

/// Return the HTTP `Content-Type` for a profile.
#[must_use]
fn content_type_for(profile: ProfileKind) -> &'static str {
    match profile {
        ProfileKind::Mihomo => "text/yaml; charset=utf-8",
        ProfileKind::SingBox | ProfileKind::Xray | ProfileKind::V2Ray | ProfileKind::Json => {
            "application/json; charset=utf-8"
        }
        ProfileKind::Shadowrocket | ProfileKind::UriList => "text/plain; charset=utf-8",
    }
}

/// Return the HTTP `Content-Disposition` header (attachment with a
/// profile-specific filename).
fn content_disposition_for(profile: ProfileKind, slug: &str) -> String {
    let ext = match profile {
        ProfileKind::Mihomo => "yaml",
        ProfileKind::SingBox | ProfileKind::Xray | ProfileKind::V2Ray | ProfileKind::Json => "json",
        ProfileKind::Shadowrocket | ProfileKind::UriList => "txt",
    };
    format!("attachment; filename=\"{slug}.{ext}\"")
}

/// Build the `subscription-userinfo` response header.
///
/// Format: `upload=BYTES; download=BYTES; total=BYTES; expire=UNIX_SECONDS`
///
/// `upload`/`download` are the aggregated consumed bytes from traffic records.
/// `total` is the effective limit: the Subscription's `traffic_limit`, or the
/// owning User's `traffic_quota` if the subscription is unlimited (whichever
/// is smaller and non-zero), or 0 if both are unlimited. `expire` is the
/// Subscription's `expires_at` as a Unix timestamp in seconds (or 0 if never).
fn build_subscription_userinfo(
    subscription: &Subscription,
    traffic: &TrafficSummary,
    user_quota: u64,
) -> String {
    let upload = traffic.upload;
    let download = traffic.download;
    let sub_limit = subscription.traffic_limit.unwrap_or(0);
    let total = if sub_limit > 0 {
        if user_quota > 0 {
            sub_limit.min(user_quota)
        } else {
            sub_limit
        }
    } else {
        user_quota
    };
    let expire = subscription
        .expires_at
        .map(|ts| (ts.unix_ms() / 1000).max(0) as u64)
        .unwrap_or(0);
    format!("upload={upload}; download={download}; total={total}; expire={expire}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use deve_sub_kernel::{TemplateId, UserId};

    #[test]
    fn etag_is_quoted_sha256_hex() {
        let etag = compute_etag("hello");
        assert!(etag.starts_with('"') && etag.ends_with('"'));
        let inner = &etag[1..etag.len() - 1];
        assert_eq!(inner.len(), 64);
        assert!(inner.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn etag_is_deterministic() {
        assert_eq!(compute_etag("abc"), compute_etag("abc"));
        assert_ne!(compute_etag("abc"), compute_etag("abd"));
    }

    #[test]
    fn detect_clash() {
        assert_eq!(
            detect_profile_from_user_agent(Some("Clash/0.20")),
            Some(ProfileKind::Mihomo)
        );
    }

    #[test]
    fn detect_mihomo() {
        assert_eq!(
            detect_profile_from_user_agent(Some("mihomo/1.18")),
            Some(ProfileKind::Mihomo)
        );
    }

    #[test]
    fn detect_singbox() {
        assert_eq!(
            detect_profile_from_user_agent(Some("sing-box/1.8")),
            Some(ProfileKind::SingBox)
        );
    }

    #[test]
    fn detect_xray() {
        assert_eq!(
            detect_profile_from_user_agent(Some("Xray/1.8")),
            Some(ProfileKind::Xray)
        );
    }

    #[test]
    fn detect_v2ray() {
        assert_eq!(
            detect_profile_from_user_agent(Some("v2rayN/6.0")),
            Some(ProfileKind::V2Ray)
        );
    }

    #[test]
    fn detect_shadowrocket() {
        assert_eq!(
            detect_profile_from_user_agent(Some("Shadowrocket/2.2")),
            Some(ProfileKind::Shadowrocket)
        );
    }

    #[test]
    fn detect_unknown_returns_none() {
        assert_eq!(detect_profile_from_user_agent(Some("Mozilla/5.0")), None);
    }

    #[test]
    fn detect_none_user_agent_returns_none() {
        assert_eq!(detect_profile_from_user_agent(None), None);
    }

    #[test]
    fn detect_is_case_insensitive() {
        assert_eq!(
            detect_profile_from_user_agent(Some("CLASH/Verge")),
            Some(ProfileKind::Mihomo)
        );
    }

    #[test]
    fn content_type_yaml_for_mihomo() {
        assert_eq!(
            content_type_for(ProfileKind::Mihomo),
            "text/yaml; charset=utf-8"
        );
    }

    #[test]
    fn content_type_json_for_singbox() {
        assert_eq!(
            content_type_for(ProfileKind::SingBox),
            "application/json; charset=utf-8"
        );
    }

    #[test]
    fn content_disposition_includes_slug_and_ext() {
        let cd = content_disposition_for(ProfileKind::Mihomo, "my-sub");
        assert_eq!(cd, "attachment; filename=\"my-sub.yaml\"");
        let cd = content_disposition_for(ProfileKind::SingBox, "test");
        assert_eq!(cd, "attachment; filename=\"test.json\"");
    }

    #[test]
    fn subscription_userinfo_zero_traffic_no_expiry() {
        let sub = Subscription::new(
            "n",
            "s",
            UserId::new(),
            TemplateId::new(),
            "mihomo",
            deve_sub_domain::NodeSelector::default(),
            deve_sub_kernel::SubscriptionTokenId::new(),
        );
        let traffic = TrafficSummary {
            upload: 0,
            download: 0,
            by_source: vec![],
        };
        let info = build_subscription_userinfo(&sub, &traffic, 0);
        assert_eq!(info, "upload=0; download=0; total=0; expire=0");
    }

    #[test]
    fn subscription_userinfo_with_traffic_limit() {
        let mut sub = Subscription::new(
            "n",
            "s",
            UserId::new(),
            TemplateId::new(),
            "mihomo",
            deve_sub_domain::NodeSelector::default(),
            deve_sub_kernel::SubscriptionTokenId::new(),
        );
        sub.traffic_limit = Some(1_073_741_824);
        let traffic = TrafficSummary {
            upload: 0,
            download: 0,
            by_source: vec![],
        };
        let info = build_subscription_userinfo(&sub, &traffic, 0);
        assert!(info.contains("total=1073741824"));
    }

    #[test]
    fn subscription_userinfo_reflects_consumed_traffic() {
        let sub = Subscription::new(
            "n",
            "s",
            UserId::new(),
            TemplateId::new(),
            "mihomo",
            deve_sub_domain::NodeSelector::default(),
            deve_sub_kernel::SubscriptionTokenId::new(),
        );
        let traffic = TrafficSummary {
            upload: 1_000,
            download: 2_000,
            by_source: vec![],
        };
        let info = build_subscription_userinfo(&sub, &traffic, 0);
        assert!(
            info.contains("upload=1000") && info.contains("download=2000"),
            "userinfo should reflect consumed traffic: {info}"
        );
    }

    fn sub_bound_to(profile: &str) -> Subscription {
        Subscription::new(
            "n",
            "s",
            UserId::new(),
            TemplateId::new(),
            profile,
            deve_sub_domain::NodeSelector::default(),
            deve_sub_kernel::SubscriptionTokenId::new(),
        )
    }

    #[test]
    fn resolve_profile_explicit_match_returns_profile() {
        let sub = sub_bound_to("mihomo");
        let p = resolve_profile(Some("mihomo"), None, &sub).expect("explicit match");
        assert_eq!(p, ProfileKind::Mihomo);
    }

    #[test]
    fn resolve_profile_explicit_mismatch_rejects() {
        let sub = sub_bound_to("sing-box");
        let err = resolve_profile(Some("mihomo"), None, &sub).expect_err("explicit mismatch");
        assert!(matches!(err, SubscriptionAppError::UnknownProfile(_)));
        assert!(format!("{err}").contains("mihomo"));
    }

    #[test]
    fn resolve_profile_auto_detect_match_returns_profile() {
        let sub = sub_bound_to("mihomo");
        let p = resolve_profile(None, Some("Clash/0.20"), &sub).expect("auto match");
        assert_eq!(p, ProfileKind::Mihomo);
    }

    #[test]
    fn resolve_profile_auto_detect_mismatch_rejects() {
        // DS-AUD-B16 regression guard: a sing-box UA on a mihomo-bound
        // subscription must NOT silently serve mihomo content.
        let sub = sub_bound_to("mihomo");
        let err = resolve_profile(None, Some("sing-box/1.8"), &sub).expect_err("auto mismatch");
        match err {
            SubscriptionAppError::UnknownProfile(msg) => {
                assert!(msg.contains("sing-box"), "msg must name detected: {msg}");
                assert!(msg.contains("mihomo"), "msg must name bound: {msg}");
            }
            other => panic!("expected UnknownProfile, got {other:?}"),
        }
    }

    #[test]
    fn resolve_profile_auto_detect_unknown_ua_rejects() {
        let sub = sub_bound_to("mihomo");
        let err = resolve_profile(None, Some("Mozilla/5.0"), &sub).expect_err("unknown UA");
        assert!(matches!(err, SubscriptionAppError::UnknownProfile(_)));
    }

    #[test]
    fn resolve_profile_auto_detect_none_ua_rejects() {
        let sub = sub_bound_to("mihomo");
        let err = resolve_profile(None, None, &sub).expect_err("no UA");
        assert!(matches!(err, SubscriptionAppError::UnknownProfile(_)));
    }
}
