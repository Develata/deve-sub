//! Auth route handlers and session extraction.
//!
//! Implements the `/api/v1/auth/*` endpoints: setup, login, logout, and
//! current-user. Session tokens are exchanged via `HttpOnly` `SameSite=Lax`
//! cookies. See `docs/plan/milestones/M2-auth-and-users.md`.

use axum::extract::{FromRequestParts, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{AppendHeaders, IntoResponse, Json};
use deve_sub_application::auth;
use deve_sub_contract::{
    CurrentUserResponse, ErrorResponse, LoginRequest, LoginResponse, RoleDto, SetupAdminRequest,
    SetupAdminResponse, UserDto,
};
use deve_sub_domain::{Role, Session, User};
use deve_sub_kernel::Timestamp;
use time::format_description::well_known::Rfc3339;

use crate::AppState;

/// Session cookie name.
const SESSION_COOKIE: &str = "deve_sub_session";

/// Authenticated session and user, extracted from the request cookie.
///
/// Implements [`FromRequestParts`] for use as an Axum extractor on protected
/// routes.
pub struct AuthSession {
    /// The authenticated user.
    pub user: User,
    /// The active session.
    pub session: Session,
}

impl FromRequestParts<AppState> for AuthSession {
    type Rejection = (StatusCode, Json<ErrorResponse>);

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = extract_session_token(&parts.headers)
            .ok_or_else(|| err(StatusCode::UNAUTHORIZED, "unauthorized", "no session"))?;

        let principal = auth::authenticate_session(
            state.session_repo.as_ref(),
            state.user_repo.as_ref(),
            &state.master_key,
            &token,
        )
        .await
        .map_err(|_| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "session verify error",
            )
        })?
        .ok_or_else(|| {
            err(
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "invalid or expired session",
            )
        })?;

        Ok(Self {
            user: principal.user,
            session: principal.session,
        })
    }
}

/// Admin-only guard. Wraps [`AuthSession`] and rejects non-admin users with
/// 403 Forbidden (AUTH-008).
///
/// Handlers that require admin access use this as an Axum extractor instead
/// of [`AuthSession`]. The extractor first authenticates the session (same
/// as `AuthSession`), then checks the user's role.
pub struct AdminUser {
    /// The authenticated admin user.
    pub user: User,
    /// The active session.
    pub session: Session,
}

impl FromRequestParts<AppState> for AdminUser {
    type Rejection = (StatusCode, Json<ErrorResponse>);

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let auth_session = AuthSession::from_request_parts(parts, state).await?;
        if auth_session.user.role != Role::Admin {
            return Err(err(
                StatusCode::FORBIDDEN,
                "forbidden",
                "admin access required",
            ));
        }
        Ok(Self {
            user: auth_session.user,
            session: auth_session.session,
        })
    }
}

fn extract_session_token(headers: &HeaderMap) -> Option<String> {
    let header = headers.get("cookie")?.to_str().ok()?;
    for cookie in header.split(';') {
        let cookie = cookie.trim();
        if let Some(value) = cookie.strip_prefix(&format!("{SESSION_COOKIE}=")) {
            return Some(value.to_owned());
        }
    }
    None
}

/// Extract the client IP address from the `X-Real-IP` header (preferred) or
/// the last hop of `X-Forwarded-For`. Returns `None` if neither is present.
///
/// WHY: we prefer `X-Real-IP` because reverse proxies (Caddy, nginx) set
/// it to the actual client IP, overwriting any client-sent value. We take
/// the LAST hop of `X-Forwarded-For` because the proxy appends the real
/// client IP at the end — the first hop is client-controlled and spoofable.
///
/// This assumes a single trusted reverse proxy layer. Without a proxy,
/// clients can still send these headers to evade IP-based rate limiting,
/// but username-based rate limiting remains effective. A
/// `trusted_proxies` config option for multi-layer proxy chains is a
/// future improvement.
fn extract_client_ip(headers: &HeaderMap) -> Option<String> {
    if let Some(ip) = headers.get("x-real-ip").and_then(|v| v.to_str().ok()) {
        return Some(ip.trim().to_owned());
    }
    if let Some(xff) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        // WHY: take the LAST hop, not the first. The reverse proxy appends
        // the real client IP at the end; earlier entries may be forged by
        // the client.
        return xff.split(',').next_back().map(|s| s.trim().to_owned());
    }
    None
}

fn set_cookie_header(token: &str, max_age_secs: u64, secure: bool) -> String {
    let secure_attr = if secure { "; Secure" } else { "" };
    format!(
        "{SESSION_COOKIE}={token}; HttpOnly; SameSite=Lax; Path=/; Max-Age={max_age_secs}{secure_attr}"
    )
}

fn clear_cookie_header(secure: bool) -> String {
    let secure_attr = if secure { "; Secure" } else { "" };
    format!("{SESSION_COOKIE}=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0{secure_attr}")
}

pub(crate) fn err(
    status: StatusCode,
    error: &str,
    message: &str,
) -> (StatusCode, Json<ErrorResponse>) {
    (
        status,
        Json(ErrorResponse {
            error: error.to_owned(),
            message: message.to_owned(),
        }),
    )
}

pub(crate) fn ts_to_iso8601(ts: Timestamp) -> String {
    ts.as_offset_date_time()
        .format(&Rfc3339)
        .unwrap_or_else(|_| ts.as_offset_date_time().to_string())
}

pub(crate) fn role_to_dto(role: Role) -> RoleDto {
    match role {
        Role::Admin => RoleDto::Admin,
        Role::User => RoleDto::User,
    }
}

pub(crate) fn user_to_dto(user: &User) -> UserDto {
    UserDto {
        id: user.id.to_string(),
        username: user.username.clone(),
        role: role_to_dto(user.role),
        enabled: user.enabled,
        expires_at: user.expires_at.map(ts_to_iso8601),
        traffic_quota: user.traffic_quota,
        created_at: ts_to_iso8601(user.created_at),
    }
}

/// `POST /api/v1/auth/setup` — create the first admin user.
///
/// Returns 400 for invalid input (empty username, password too short).
/// Returns 409 if any users already exist (AUTH-001).
#[utoipa::path(
    post,
    path = "/api/v1/auth/setup",
    request_body = SetupAdminRequest,
    responses(
        (status = 201, description = "Admin created", body = SetupAdminResponse),
        (status = 400, description = "Invalid input", body = ErrorResponse),
        (status = 409, description = "Already initialized", body = ErrorResponse),
    )
)]
async fn setup(
    State(state): State<AppState>,
    Json(req): Json<SetupAdminRequest>,
) -> Result<(StatusCode, Json<SetupAdminResponse>), (StatusCode, Json<ErrorResponse>)> {
    let user = auth::setup_admin(state.user_repo.as_ref(), &req.username, &req.password)
        .await
        .map_err(|e| match e {
            auth::AuthError::InvalidInput(msg) => {
                err(StatusCode::BAD_REQUEST, "invalid_input", msg)
            }
            auth::AuthError::AlreadyInitialized => err(
                StatusCode::CONFLICT,
                "already_initialized",
                "admin already exists",
            ),
            other => {
                tracing::warn!(error = %other, "setup_admin failed");
                err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "setup failed",
                )
            }
        })?;
    Ok((
        StatusCode::CREATED,
        Json(SetupAdminResponse {
            user: user_to_dto(&user),
        }),
    ))
}

/// `POST /api/v1/auth/login` — authenticate and create a session.
///
/// Returns 401 for unknown user, wrong password, or disabled account — the
/// same error for all three to avoid leaking user existence (AUTH-003).
/// Returns 429 if the account or IP is temporarily locked (AUTH-004).
#[utoipa::path(
    post,
    path = "/api/v1/auth/login",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "Login successful", body = LoginResponse),
        (status = 401, description = "Invalid credentials", body = ErrorResponse),
        (status = 429, description = "Rate limited", body = ErrorResponse),
    )
)]
async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<LoginRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let ip = extract_client_ip(&headers);
    let ttl = time::Duration::seconds(state.config.security.session_ttl_secs as i64);
    let (user, _session, token) = auth::login(auth::LoginParams {
        user_repo: state.user_repo.as_ref(),
        session_repo: state.session_repo.as_ref(),
        rate_limiter: state.rate_limiter.as_ref(),
        master_key: &state.master_key,
        username: &req.username,
        password: &req.password,
        ip: ip.as_deref(),
        session_ttl: ttl,
    })
    .await
    .map_err(|e| match e {
        auth::AuthError::InvalidCredentials => err(
            StatusCode::UNAUTHORIZED,
            "invalid_credentials",
            "invalid username or password",
        ),
        auth::AuthError::RateLimited => err(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "too many failed attempts, try again later",
        ),
        _ => {
            tracing::warn!(error = %e, "login failed");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "login failed",
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
        }),
    ))
}

/// `POST /api/v1/auth/logout` — revoke the current session.
#[utoipa::path(
    post,
    path = "/api/v1/auth/logout",
    responses(
        (status = 200, description = "Logged out"),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
    )
)]
async fn logout(
    State(state): State<AppState>,
    auth_session: AuthSession,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    auth::logout(state.session_repo.as_ref(), auth_session.session.id)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "logout failed");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "logout failed",
            )
        })?;
    Ok((
        StatusCode::OK,
        AppendHeaders([(
            axum::http::header::SET_COOKIE,
            clear_cookie_header(state.config.security.cookie_secure),
        )]),
        Json(serde_json::json!({})),
    ))
}

/// `GET /api/v1/auth/me` — return the current authenticated user.
#[utoipa::path(
    get,
    path = "/api/v1/auth/me",
    responses(
        (status = 200, description = "Current user", body = CurrentUserResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
    )
)]
async fn me(
    auth_session: AuthSession,
) -> Result<Json<CurrentUserResponse>, (StatusCode, Json<ErrorResponse>)> {
    Ok(Json(CurrentUserResponse {
        user: user_to_dto(&auth_session.user),
    }))
}

/// Register all auth routes on the given `OpenApiRouter`.
pub fn register(
    router: utoipa_axum::router::OpenApiRouter<AppState>,
) -> utoipa_axum::router::OpenApiRouter<AppState> {
    use utoipa_axum::routes;
    router
        .routes(routes!(setup))
        .routes(routes!(login))
        .routes(routes!(logout))
        .routes(routes!(me))
}
