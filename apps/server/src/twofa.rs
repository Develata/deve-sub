//! 2FA route handlers for the `/api/v1/auth/2fa/*` and
//! `/api/v1/auth/login/2fa` endpoints.
//!
//! See `docs/plan/milestones/M2-auth-and-users.md` (Slice 4).

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{AppendHeaders, IntoResponse, Json};
use deve_sub_application::auth;
use deve_sub_contract::{
    ErrorResponse, LoginResponse, LoginTwoFactorRequest, RegenerateRecoveryCodesRequest,
    RegenerateRecoveryCodesResponse, TwoFactorDisableRequest, TwoFactorSetupResponse,
    TwoFactorVerifyRequest, TwoFactorVerifyResponse,
};

use crate::AppState;
use crate::auth::{AuthSession, err, set_cookie_header, user_to_dto};

/// `POST /api/v1/auth/2fa/setup` — generate a TOTP secret.
///
/// Returns the Base32 secret and `otpauth://` URI. The secret is not yet
/// active until verified via `POST /api/v1/auth/2fa/verify`.
#[utoipa::path(
    post,
    path = "/api/v1/auth/2fa/setup",
    responses(
        (status = 200, description = "TOTP secret generated", body = TwoFactorSetupResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 409, description = "2FA already enabled", body = ErrorResponse),
    )
)]
async fn setup(
    State(state): State<AppState>,
    auth_session: AuthSession,
) -> Result<Json<TwoFactorSetupResponse>, (StatusCode, Json<ErrorResponse>)> {
    let result = auth::setup_2fa(
        state.user_repo.as_ref(),
        state.totp_secret_repo.as_ref(),
        &state.master_key,
        auth_session.user.id,
        &state.config.product_name,
    )
    .await
    .map_err(|e| match e {
        auth::AuthError::TwoFactorAlreadyEnabled => err(
            StatusCode::CONFLICT,
            "already_enabled",
            "2FA is already enabled",
        ),
        other => {
            tracing::warn!(error = %other, "2fa setup failed");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "2fa setup failed",
            )
        }
    })?;

    Ok(Json(TwoFactorSetupResponse {
        secret: result.secret,
        otpauth_uri: result.otpauth_uri,
    }))
}

/// `POST /api/v1/auth/2fa/verify` — verify a TOTP code and enable 2FA.
///
/// Returns recovery codes (shown once).
#[utoipa::path(
    post,
    path = "/api/v1/auth/2fa/verify",
    request_body = TwoFactorVerifyRequest,
    responses(
        (status = 200, description = "2FA enabled", body = TwoFactorVerifyResponse),
        (status = 400, description = "No TOTP secret found", body = ErrorResponse),
        (status = 401, description = "Invalid TOTP code", body = ErrorResponse),
        (status = 409, description = "2FA already enabled", body = ErrorResponse),
    )
)]
async fn verify(
    State(state): State<AppState>,
    auth_session: AuthSession,
    Json(req): Json<TwoFactorVerifyRequest>,
) -> Result<Json<TwoFactorVerifyResponse>, (StatusCode, Json<ErrorResponse>)> {
    let code: u32 = req.code.parse().map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_input",
            "code must be 6 digits",
        )
    })?;

    let result = auth::verify_2fa(
        state.user_repo.as_ref(),
        state.totp_secret_repo.as_ref(),
        state.recovery_code_repo.as_ref(),
        &state.master_key,
        auth_session.user.id,
        code,
    )
    .await
    .map_err(|e| match e {
        auth::AuthError::InvalidTwoFactorCode => err(
            StatusCode::UNAUTHORIZED,
            "invalid_2fa_code",
            "invalid TOTP code",
        ),
        auth::AuthError::TotpSecretNotFound => err(
            StatusCode::BAD_REQUEST,
            "totp_secret_not_found",
            "call setup first",
        ),
        auth::AuthError::TwoFactorAlreadyEnabled => err(
            StatusCode::CONFLICT,
            "already_enabled",
            "2FA is already enabled",
        ),
        other => {
            tracing::warn!(error = %other, "2fa verify failed");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "2fa verify failed",
            )
        }
    })?;

    Ok(Json(TwoFactorVerifyResponse {
        recovery_codes: result.recovery_codes,
    }))
}

/// `POST /api/v1/auth/2fa/disable` — disable 2FA.
///
/// Requires the current password for re-authentication.
#[utoipa::path(
    post,
    path = "/api/v1/auth/2fa/disable",
    request_body = TwoFactorDisableRequest,
    responses(
        (status = 200, description = "2FA disabled"),
        (status = 400, description = "2FA not enabled", body = ErrorResponse),
        (status = 401, description = "Wrong password", body = ErrorResponse),
    )
)]
async fn disable(
    State(state): State<AppState>,
    auth_session: AuthSession,
    Json(req): Json<TwoFactorDisableRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    auth::disable_2fa(
        state.user_repo.as_ref(),
        state.totp_secret_repo.as_ref(),
        state.recovery_code_repo.as_ref(),
        auth_session.user.id,
        &req.password,
    )
    .await
    .map_err(|e| match e {
        auth::AuthError::TwoFactorNotEnabled => {
            err(StatusCode::BAD_REQUEST, "not_enabled", "2FA is not enabled")
        }
        auth::AuthError::InvalidCredentials => err(
            StatusCode::UNAUTHORIZED,
            "invalid_credentials",
            "wrong password",
        ),
        other => {
            tracing::warn!(error = %other, "2fa disable failed");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "2fa disable failed",
            )
        }
    })?;

    Ok(Json(serde_json::json!({})))
}

/// `POST /api/v1/auth/2fa/recovery-codes` — regenerate recovery codes.
///
/// Requires the current password. Old recovery codes are invalidated.
#[utoipa::path(
    post,
    path = "/api/v1/auth/2fa/recovery-codes",
    request_body = RegenerateRecoveryCodesRequest,
    responses(
        (status = 200, description = "New recovery codes", body = RegenerateRecoveryCodesResponse),
        (status = 400, description = "2FA not enabled", body = ErrorResponse),
        (status = 401, description = "Wrong password", body = ErrorResponse),
    )
)]
async fn recovery_codes(
    State(state): State<AppState>,
    auth_session: AuthSession,
    Json(req): Json<RegenerateRecoveryCodesRequest>,
) -> Result<Json<RegenerateRecoveryCodesResponse>, (StatusCode, Json<ErrorResponse>)> {
    let codes = auth::regenerate_recovery_codes(
        state.user_repo.as_ref(),
        state.recovery_code_repo.as_ref(),
        &state.master_key,
        auth_session.user.id,
        &req.password,
    )
    .await
    .map_err(|e| match e {
        auth::AuthError::TwoFactorNotEnabled => {
            err(StatusCode::BAD_REQUEST, "not_enabled", "2FA is not enabled")
        }
        auth::AuthError::InvalidCredentials => err(
            StatusCode::UNAUTHORIZED,
            "invalid_credentials",
            "wrong password",
        ),
        other => {
            tracing::warn!(error = %other, "recovery code regeneration failed");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "recovery code regeneration failed",
            )
        }
    })?;

    Ok(Json(RegenerateRecoveryCodesResponse {
        recovery_codes: codes,
    }))
}

/// `POST /api/v1/auth/login/2fa` — complete a 2FA login.
///
/// Takes the challenge token from the login response and a TOTP code or
/// recovery code. On success, creates a session and sets the session cookie.
#[utoipa::path(
    post,
    path = "/api/v1/auth/login/2fa",
    request_body = LoginTwoFactorRequest,
    responses(
        (status = 200, description = "Login successful", body = LoginResponse),
        (status = 401, description = "Invalid 2FA code or challenge token", body = ErrorResponse),
        (status = 429, description = "Rate limited", body = ErrorResponse),
    )
)]
async fn login_2fa(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<LoginTwoFactorRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let ip = crate::auth::extract_client_ip(&headers);
    let ttl = time::Duration::seconds(state.config.security.session_ttl_secs as i64);
    let (user, _session, token) = auth::login_2fa(auth::LoginTwoFactorParams {
        user_repo: state.user_repo.as_ref(),
        session_repo: state.session_repo.as_ref(),
        totp_secret_repo: state.totp_secret_repo.as_ref(),
        recovery_code_repo: state.recovery_code_repo.as_ref(),
        rate_limiter: state.rate_limiter.as_ref(),
        master_key: &state.master_key,
        challenge_token: &req.challenge_token,
        code: &req.code,
        ip: ip.as_deref(),
        session_ttl: ttl,
    })
    .await
    .map_err(|e| match e {
        auth::AuthError::InvalidTwoFactorCode => err(
            StatusCode::UNAUTHORIZED,
            "invalid_2fa_code",
            "invalid 2FA code",
        ),
        auth::AuthError::ChallengeTokenInvalid => err(
            StatusCode::UNAUTHORIZED,
            "invalid_challenge",
            "invalid or expired challenge token",
        ),
        auth::AuthError::TotpSecretNotFound => err(
            StatusCode::UNAUTHORIZED,
            "invalid_2fa_code",
            "invalid 2FA code",
        ),
        auth::AuthError::RateLimited => err(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "too many failed attempts, try again later",
        ),
        other => {
            tracing::warn!(error = %other, "2fa login failed");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "2fa login failed",
            )
        }
    })?;

    let cookie = set_cookie_header(
        &token,
        state.config.security.session_ttl_secs,
        state.config.security.cookie_secure,
    );

    Ok((
        StatusCode::OK,
        AppendHeaders([(axum::http::header::SET_COOKIE, cookie)]),
        Json(LoginResponse {
            user: user_to_dto(&user),
            requires_2fa: false,
            challenge_token: None,
        }),
    ))
}

/// Register all 2FA routes on the given `OpenApiRouter`.
pub fn register(
    router: utoipa_axum::router::OpenApiRouter<AppState>,
) -> utoipa_axum::router::OpenApiRouter<AppState> {
    use utoipa_axum::routes;
    router
        .routes(routes!(setup))
        .routes(routes!(verify))
        .routes(routes!(disable))
        .routes(routes!(recovery_codes))
        .routes(routes!(login_2fa))
}
