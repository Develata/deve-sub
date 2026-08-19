//! Subscription management route handlers (admin-only).
//!
//! Implements the `/api/v1/subscriptions/*` endpoints: create, list, get,
//! update, delete, and token rotation. All routes require an authenticated
//! admin via the [`AdminUser`] extractor. The plaintext delivery token is
//! returned only at create/rotate time and is never persisted. See
//! `docs/plan/milestones/M6-subscription-distribution.md` Slice 1.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use deve_sub_application::{
    audit,
    subscription::{
        self, CreateSubscriptionParams, CreateTempLinkParams, UpdateSubscriptionParams,
    },
};
use deve_sub_contract::{
    CreateSubscriptionRequest, CreateTempLinkRequest, CreateTempLinkResponse, ErrorResponse,
    GetSubscriptionResponse, ListSubscriptionsQuery, ListSubscriptionsResponse, RotateTokenRequest,
    ShortCodeResponse, SubscriptionDto, SubscriptionResponse, TokenRotationResponse,
    UpdateSubscriptionRequest,
};
use deve_sub_domain::Subscription;
use deve_sub_kernel::{SubscriptionId, TempLinkId, TemplateId, Timestamp};
use time::format_description::well_known::Rfc3339;

use crate::AppState;
use crate::auth::{AdminUser, err, ts_to_iso8601};

fn subscription_to_dto(s: &Subscription, short_code: Option<String>) -> SubscriptionDto {
    SubscriptionDto {
        id: s.id.to_string(),
        name: s.name.clone(),
        slug: s.slug.clone(),
        owner_id: s.owner_id.to_string(),
        template_id: s.template_id.to_string(),
        template_version_pin: s.template_version_pin,
        profile: s.profile.clone(),
        node_selection: serde_json::to_value(&s.node_selection).unwrap_or(serde_json::Value::Null),
        traffic_limit: s.traffic_limit,
        expires_at: s.expires_at.map(ts_to_iso8601),
        enabled: s.enabled,
        created_at: ts_to_iso8601(s.created_at),
        updated_at: ts_to_iso8601(s.updated_at),
        short_code,
    }
}

fn parse_iso8601(s: &str) -> Result<Timestamp, (StatusCode, Json<ErrorResponse>)> {
    time::OffsetDateTime::parse(s, &Rfc3339)
        .map(Timestamp::from_offset_date_time)
        .map_err(|e| {
            err(
                StatusCode::BAD_REQUEST,
                "invalid_expires_at",
                &format!("expires_at is not valid RFC 3339: {e}"),
            )
        })
}

/// `POST /api/v1/subscriptions` — create a new subscription (admin).
#[utoipa::path(
    post,
    path = "/api/v1/subscriptions",
    security(("cookie_auth" = [])),
    request_body = CreateSubscriptionRequest,
    responses(
        (status = 201, description = "Subscription created", body = SubscriptionResponse),
        (status = 400, description = "Invalid input", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 409, description = "Slug already exists", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn create_subscription(
    State(state): State<AppState>,
    admin: AdminUser,
    Json(req): Json<CreateSubscriptionRequest>,
) -> Result<(StatusCode, Json<SubscriptionResponse>), (StatusCode, Json<ErrorResponse>)> {
    let template_id = TemplateId::parse(&req.template_id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_template_id",
            "template_id is not a valid ULID",
        )
    })?;

    let result = subscription::create_subscription(
        state.subscription_repo.as_ref(),
        &state.master_key,
        CreateSubscriptionParams {
            name: req.name,
            slug: req.slug,
            owner_id: admin.user.id,
            template_id,
            profile: req.profile,
            node_selection: req.node_selection,
            traffic_limit: req.traffic_limit,
            expires_at: req.expires_at,
        },
    )
    .await
    .map_err(|e| map_subscription_app_error(e, "create_subscription"))?;

    let entry = audit::audit_subscription_create(
        admin.user.id,
        &result.subscription.id.to_string(),
        &result.subscription.slug,
    );
    if let Err(e) = audit::record_audit_log(state.audit_log_repo.as_ref(), &entry).await {
        tracing::warn!(error = %e, "audit log write failed for subscription.create");
    }

    Ok((
        StatusCode::CREATED,
        Json(SubscriptionResponse {
            subscription: subscription_to_dto(&result.subscription, None),
            token_plaintext: result.token_plaintext,
        }),
    ))
}

/// `GET /api/v1/subscriptions` — list subscriptions with cursor pagination
/// (admin). Lists subscriptions owned by the authenticated admin.
#[utoipa::path(
    get,
    path = "/api/v1/subscriptions",
    security(("cookie_auth" = [])),
    params(
        ("cursor" = Option<String>, Query, description = "Pagination cursor (last subscription ULID)"),
        ("limit" = Option<u32>, Query, description = "Max subscriptions per page (default 50, max 100)"),
    ),
    responses(
        (status = 200, description = "Subscription list", body = ListSubscriptionsResponse),
        (status = 400, description = "Invalid cursor", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn list_subscriptions(
    State(state): State<AppState>,
    admin: AdminUser,
    Query(q): Query<ListSubscriptionsQuery>,
) -> Result<Json<ListSubscriptionsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let limit = q.limit.unwrap_or(50).clamp(1, 100);

    let cursor = q
        .cursor
        .as_deref()
        .map(SubscriptionId::parse)
        .transpose()
        .map_err(|_| {
            err(
                StatusCode::BAD_REQUEST,
                "invalid_cursor",
                "cursor is not a valid ULID",
            )
        })?;

    let subs = subscription::list_subscriptions(
        state.subscription_repo.as_ref(),
        admin.user.id,
        cursor,
        Some(limit),
    )
    .await
    .map_err(|e| map_subscription_app_error(e, "list_subscriptions"))?;

    let next_cursor = if subs.len() as u32 >= limit {
        subs.last().map(|s| s.id.to_string())
    } else {
        None
    };

    let dtos: Vec<SubscriptionDto> = subs.iter().map(|s| subscription_to_dto(s, None)).collect();
    Ok(Json(ListSubscriptionsResponse {
        subscriptions: dtos,
        next_cursor,
    }))
}

/// `GET /api/v1/subscriptions/{id}` — get a subscription by ID (admin).
#[utoipa::path(
    get,
    path = "/api/v1/subscriptions/{id}",
    security(("cookie_auth" = [])),
    params(("id" = String, Path, description = "Subscription ULID")),
    responses(
        (status = 200, description = "Subscription found", body = GetSubscriptionResponse),
        (status = 400, description = "Invalid subscription id", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 404, description = "Subscription not found", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn get_subscription(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<String>,
) -> Result<Json<GetSubscriptionResponse>, (StatusCode, Json<ErrorResponse>)> {
    let subscription_id = SubscriptionId::parse(&id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_id",
            "subscription id is not a valid ULID",
        )
    })?;

    let sub = subscription::get_subscription(state.subscription_repo.as_ref(), subscription_id)
        .await
        .map_err(|e| map_subscription_app_error(e, "get_subscription"))?
        .ok_or_else(|| {
            err(
                StatusCode::NOT_FOUND,
                "subscription_not_found",
                "subscription does not exist",
            )
        })?;

    let short_code = state
        .short_code_repo
        .find_by_subscription(sub.id)
        .await
        .map_err(|e| map_subscription_app_error(e.into(), "get_subscription"))?
        .map(|sc| sc.code);

    Ok(Json(GetSubscriptionResponse {
        subscription: subscription_to_dto(&sub, short_code),
    }))
}

/// `PUT /api/v1/subscriptions/{id}` — update an existing subscription (admin).
#[utoipa::path(
    put,
    path = "/api/v1/subscriptions/{id}",
    security(("cookie_auth" = [])),
    params(("id" = String, Path, description = "Subscription ULID")),
    request_body = UpdateSubscriptionRequest,
    responses(
        (status = 200, description = "Subscription updated", body = GetSubscriptionResponse),
        (status = 400, description = "Invalid input", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 404, description = "Subscription not found", body = ErrorResponse),
        (status = 409, description = "Slug already exists", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn update_subscription(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<String>,
    Json(req): Json<UpdateSubscriptionRequest>,
) -> Result<Json<GetSubscriptionResponse>, (StatusCode, Json<ErrorResponse>)> {
    let subscription_id = SubscriptionId::parse(&id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_id",
            "subscription id is not a valid ULID",
        )
    })?;

    let sub = subscription::update_subscription(
        state.subscription_repo.as_ref(),
        UpdateSubscriptionParams {
            id: subscription_id,
            name: req.name,
            slug: req.slug,
            template_version_pin: req.template_version_pin,
            profile: req.profile,
            node_selection: req.node_selection,
            traffic_limit: req.traffic_limit,
            expires_at: req.expires_at,
            enabled: req.enabled,
        },
    )
    .await
    .map_err(|e| map_subscription_app_error(e, "update_subscription"))?;

    let short_code = state
        .short_code_repo
        .find_by_subscription(sub.id)
        .await
        .map_err(|e| map_subscription_app_error(e.into(), "update_subscription"))?
        .map(|sc| sc.code);

    let entry = audit::audit_subscription_update(admin.user.id, &sub.id.to_string());
    if let Err(e) = audit::record_audit_log(state.audit_log_repo.as_ref(), &entry).await {
        tracing::warn!(error = %e, "audit log write failed for subscription.update");
    }

    Ok(Json(GetSubscriptionResponse {
        subscription: subscription_to_dto(&sub, short_code),
    }))
}

/// `DELETE /api/v1/subscriptions/{id}` — delete a subscription (admin).
#[utoipa::path(
    delete,
    path = "/api/v1/subscriptions/{id}",
    security(("cookie_auth" = [])),
    params(("id" = String, Path, description = "Subscription ULID")),
    responses(
        (status = 200, description = "Subscription deleted"),
        (status = 400, description = "Invalid subscription id", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 404, description = "Subscription not found", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn delete_subscription(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let subscription_id = SubscriptionId::parse(&id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_id",
            "subscription id is not a valid ULID",
        )
    })?;

    subscription::delete_subscription(state.subscription_repo.as_ref(), subscription_id)
        .await
        .map_err(|e| map_subscription_app_error(e, "delete_subscription"))?;

    let entry = audit::audit_subscription_delete(admin.user.id, &subscription_id.to_string());
    if let Err(e) = audit::record_audit_log(state.audit_log_repo.as_ref(), &entry).await {
        tracing::warn!(error = %e, "audit log write failed for subscription.delete");
    }

    Ok(StatusCode::OK)
}

/// `POST /api/v1/subscriptions/{id}/rotate-token` — rotate the delivery token
/// (admin). The new plaintext token is returned once.
#[utoipa::path(
    post,
    path = "/api/v1/subscriptions/{id}/rotate-token",
    security(("cookie_auth" = [])),
    params(("id" = String, Path, description = "Subscription ULID")),
    request_body = RotateTokenRequest,
    responses(
        (status = 200, description = "Token rotated", body = TokenRotationResponse),
        (status = 400, description = "Invalid subscription id", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 404, description = "Subscription not found", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn rotate_token(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<String>,
    Json(req): Json<RotateTokenRequest>,
) -> Result<Json<TokenRotationResponse>, (StatusCode, Json<ErrorResponse>)> {
    let subscription_id = SubscriptionId::parse(&id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_id",
            "subscription id is not a valid ULID",
        )
    })?;

    // WHY: null or -1 grace_seconds maps to None (permanent grace), matching
    // the M6 blueprint config model. 0 means no grace (old token immediately
    // invalid).
    let grace = req
        .grace_seconds
        .filter(|s| *s >= 0)
        .map(time::Duration::seconds);

    let result = subscription::rotate_token(
        state.subscription_repo.as_ref(),
        state.subscription_token_repo.as_ref(),
        &state.master_key,
        subscription_id,
        grace,
    )
    .await
    .map_err(|e| map_subscription_app_error(e, "rotate_token"))?;

    let entry = audit::audit_subscription_token_rotate(admin.user.id, &subscription_id.to_string());
    if let Err(e) = audit::record_audit_log(state.audit_log_repo.as_ref(), &entry).await {
        tracing::warn!(error = %e, "audit log write failed for subscription.token.rotate");
    }

    Ok(Json(TokenRotationResponse {
        token_id: result.token_id.to_string(),
        token_plaintext: result.token_plaintext,
    }))
}

/// `POST /api/v1/subscriptions/{id}/regenerate-short-code` — (re)generate the
/// short code for a subscription (admin). If a short code already exists, it is
/// replaced. The short code is a public lookup key, not a secret.
#[utoipa::path(
    post,
    path = "/api/v1/subscriptions/{id}/regenerate-short-code",
    security(("cookie_auth" = [])),
    params(("id" = String, Path, description = "Subscription ULID")),
    responses(
        (status = 200, description = "Short code generated", body = ShortCodeResponse),
        (status = 400, description = "Invalid subscription id", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 404, description = "Subscription not found", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn regenerate_short_code(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<String>,
) -> Result<Json<ShortCodeResponse>, (StatusCode, Json<ErrorResponse>)> {
    let subscription_id = SubscriptionId::parse(&id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_id",
            "subscription id is not a valid ULID",
        )
    })?;

    let result = subscription::regenerate_short_code(
        state.subscription_repo.as_ref(),
        state.short_code_repo.as_ref(),
        subscription_id,
    )
    .await
    .map_err(|e| map_subscription_app_error(e, "regenerate_short_code"))?;

    Ok(Json(ShortCodeResponse {
        short_code_id: result.short_code_id.to_string(),
        code: result.code,
    }))
}

/// `POST /api/v1/subscriptions/{id}/temp-links` — create a temporary delivery
/// link (admin). The plaintext temp token is returned once and never persisted.
#[utoipa::path(
    post,
    path = "/api/v1/subscriptions/{id}/temp-links",
    security(("cookie_auth" = [])),
    params(("id" = String, Path, description = "Subscription ULID")),
    request_body = CreateTempLinkRequest,
    responses(
        (status = 201, description = "Temp link created", body = CreateTempLinkResponse),
        (status = 400, description = "Invalid input", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 404, description = "Subscription not found", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn create_temp_link(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<String>,
    Json(req): Json<CreateTempLinkRequest>,
) -> Result<(StatusCode, Json<CreateTempLinkResponse>), (StatusCode, Json<ErrorResponse>)> {
    let subscription_id = SubscriptionId::parse(&id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_id",
            "subscription id is not a valid ULID",
        )
    })?;

    let expires_at = parse_iso8601(&req.expires_at)?;

    let result = subscription::create_temp_link(
        state.subscription_repo.as_ref(),
        state.temp_link_repo.as_ref(),
        &state.master_key,
        CreateTempLinkParams {
            subscription_id,
            expires_at,
        },
    )
    .await
    .map_err(|e| map_subscription_app_error(e, "create_temp_link"))?;

    Ok((
        StatusCode::CREATED,
        Json(CreateTempLinkResponse {
            temp_link_id: result.temp_link_id.to_string(),
            token_plaintext: result.token_plaintext,
            expires_at: ts_to_iso8601(result.expires_at),
        }),
    ))
}

/// `DELETE /api/v1/subscriptions/{id}/temp-links/{temp_link_id}` — revoke a
/// temporary delivery link (admin). Subsequent delivery via the temp token
/// returns 404.
#[utoipa::path(
    delete,
    path = "/api/v1/subscriptions/{id}/temp-links/{temp_link_id}",
    security(("cookie_auth" = [])),
    params(
        ("id" = String, Path, description = "Subscription ULID"),
        ("temp_link_id" = String, Path, description = "Temp link ULID"),
    ),
    responses(
        (status = 204, description = "Temp link revoked"),
        (status = 400, description = "Invalid id", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 404, description = "Temp link not found", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn revoke_temp_link(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path((id, temp_link_id)): Path<(String, String)>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    let _subscription_id = SubscriptionId::parse(&id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_id",
            "subscription id is not a valid ULID",
        )
    })?;

    let temp_link_id = TempLinkId::parse(&temp_link_id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_temp_link_id",
            "temp_link_id is not a valid ULID",
        )
    })?;

    subscription::revoke_temp_link(state.temp_link_repo.as_ref(), temp_link_id)
        .await
        .map_err(|e| map_subscription_app_error(e, "revoke_temp_link"))?;

    Ok(StatusCode::NO_CONTENT)
}

/// Map a [`SubscriptionAppError`] to an HTTP error response with context.
fn map_subscription_app_error(
    e: deve_sub_application::SubscriptionAppError,
    ctx: &str,
) -> (StatusCode, Json<ErrorResponse>) {
    use deve_sub_application::SubscriptionAppError;
    match e {
        SubscriptionAppError::InvalidInput(msg) => {
            err(StatusCode::BAD_REQUEST, "invalid_input", &msg)
        }
        SubscriptionAppError::UnknownProfile(p) => err(
            StatusCode::BAD_REQUEST,
            "unknown_profile",
            &format!("profile '{p}' is not recognized"),
        ),
        SubscriptionAppError::SubscriptionNotFound => err(
            StatusCode::NOT_FOUND,
            "subscription_not_found",
            "subscription does not exist",
        ),
        SubscriptionAppError::TokenNotFound => err(
            StatusCode::NOT_FOUND,
            "token_not_found",
            "subscription token does not exist",
        ),
        SubscriptionAppError::ShortCodeNotFound => err(
            StatusCode::NOT_FOUND,
            "short_code_not_found",
            "short code does not exist",
        ),
        SubscriptionAppError::TempLinkNotFound => err(
            StatusCode::NOT_FOUND,
            "temp_link_not_found",
            "temp link does not exist",
        ),
        SubscriptionAppError::TempLinkInvalid => err(
            StatusCode::NOT_FOUND,
            "temp_link_invalid",
            "temp link is revoked or expired",
        ),
        SubscriptionAppError::TemplateNotFound => err(
            StatusCode::NOT_FOUND,
            "template_not_found",
            "template does not exist",
        ),
        SubscriptionAppError::SlugExists => err(
            StatusCode::CONFLICT,
            "slug_exists",
            "subscription slug is already taken for this owner",
        ),
        other => {
            tracing::warn!(error = %other, "{ctx} failed");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &format!("failed to {ctx}"),
            )
        }
    }
}

/// Register all subscription management routes on the given `OpenApiRouter`.
pub fn register(
    router: utoipa_axum::router::OpenApiRouter<AppState>,
) -> utoipa_axum::router::OpenApiRouter<AppState> {
    use utoipa_axum::routes;
    router
        .routes(routes!(create_subscription))
        .routes(routes!(list_subscriptions))
        .routes(routes!(get_subscription))
        .routes(routes!(update_subscription))
        .routes(routes!(delete_subscription))
        .routes(routes!(rotate_token))
        .routes(routes!(regenerate_short_code))
        .routes(routes!(create_temp_link))
        .routes(routes!(revoke_temp_link))
}
