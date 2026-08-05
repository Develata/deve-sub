//! User management route handlers (admin-only).
//!
//! Implements the `/api/v1/users/*` endpoints: create user, list users,
//! disable user, and force logout. All routes require an authenticated
//! admin via the [`AdminUser`] extractor (AUTH-008). See
//! `docs/plan/milestones/M2-auth-and-users.md` Slice 2.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use deve_sub_application::auth;
use deve_sub_contract::{
    CreateUserRequest, CreateUserResponse, ErrorResponse, ListUsersResponse, RoleDto, UserDto,
};
use deve_sub_domain::Role;
use deve_sub_kernel::UserId;

use crate::AppState;
use crate::auth::{AdminUser, err, user_to_dto};

/// Query parameters for `GET /api/v1/users` (cursor pagination).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ListUsersQuery {
    /// Maximum number of users to return (1-100, default 20).
    #[serde(default = "default_page_size")]
    pub limit: u32,
    /// Pagination cursor — the ULID of the last user from the previous page.
    pub cursor: Option<String>,
}

fn default_page_size() -> u32 {
    20
}

/// `POST /api/v1/users` — create a new user (admin only).
///
/// Returns 400 for invalid input (empty username, password too short).
/// Returns 409 if the username is already taken.
#[utoipa::path(
    post,
    path = "/api/v1/users",
    security(("cookie_auth" = [])),
    request_body = CreateUserRequest,
    responses(
        (status = 201, description = "User created", body = CreateUserResponse),
        (status = 400, description = "Invalid input", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 409, description = "Username already exists", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn create_user(
    State(state): State<AppState>,
    _admin: AdminUser,
    Json(req): Json<CreateUserRequest>,
) -> Result<(StatusCode, Json<CreateUserResponse>), (StatusCode, Json<ErrorResponse>)> {
    let role = match req.role {
        RoleDto::Admin => Role::Admin,
        RoleDto::User => Role::User,
    };

    let user = auth::create_user(state.user_repo.as_ref(), &req.username, &req.password, role)
        .await
        .map_err(|e| match e {
            auth::AuthError::InvalidInput(msg) => {
                err(StatusCode::BAD_REQUEST, "invalid_input", msg)
            }
            auth::AuthError::Identity(deve_sub_domain::IdentityError::UsernameExists) => err(
                StatusCode::CONFLICT,
                "username_exists",
                "username is already taken",
            ),
            other => {
                tracing::warn!(error = %other, "create_user failed");
                err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "failed to create user",
                )
            }
        })?;

    Ok((
        StatusCode::CREATED,
        Json(CreateUserResponse {
            user: user_to_dto(&user),
        }),
    ))
}

/// `GET /api/v1/users` — list users with cursor pagination (admin only).
#[utoipa::path(
    get,
    path = "/api/v1/users",
    security(("cookie_auth" = [])),
    params(
        ("limit" = Option<u32>, Query, description = "Max users per page (1-100, default 20)"),
        ("cursor" = Option<String>, Query, description = "Pagination cursor (last user ULID)"),
    ),
    responses(
        (status = 200, description = "User list", body = ListUsersResponse),
        (status = 400, description = "Invalid cursor", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn list_users(
    State(state): State<AppState>,
    _admin: AdminUser,
    Query(q): Query<ListUsersQuery>,
) -> Result<Json<ListUsersResponse>, (StatusCode, Json<ErrorResponse>)> {
    let limit = q.limit.clamp(1, 100);

    let cursor = q
        .cursor
        .as_deref()
        .map(UserId::parse)
        .transpose()
        .map_err(|_| {
            err(
                StatusCode::BAD_REQUEST,
                "invalid_cursor",
                "cursor is not a valid ULID",
            )
        })?;

    let users = auth::list_users(state.user_repo.as_ref(), cursor, limit)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "list_users failed");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "failed to list users",
            )
        })?;

    let next_cursor = if users.len() as u32 >= limit {
        users.last().map(|u| u.id.to_string())
    } else {
        None
    };

    let user_dtos: Vec<UserDto> = users.iter().map(user_to_dto).collect();
    Ok(Json(ListUsersResponse {
        users: user_dtos,
        next_cursor,
    }))
}

/// `POST /api/v1/users/{id}/disable` — disable a user and revoke all
/// their sessions (admin only). AUTH-007.
///
/// Returns 409 if the target is the requesting admin's own account
/// (self-disable is rejected to prevent lockout).
#[utoipa::path(
    post,
    path = "/api/v1/users/{id}/disable",
    security(("cookie_auth" = [])),
    params(("id" = String, Path, description = "Target user ULID")),
    responses(
        (status = 200, description = "User disabled"),
        (status = 400, description = "Invalid user id", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 404, description = "User not found", body = ErrorResponse),
        (status = 409, description = "Cannot disable yourself", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn disable_user(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let user_id = UserId::parse(&id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_id",
            "user id is not a valid ULID",
        )
    })?;

    auth::disable_user(
        state.user_repo.as_ref(),
        state.session_repo.as_ref(),
        admin.user.id,
        user_id,
    )
    .await
    .map_err(|e| match e {
        auth::AuthError::SelfDisableForbidden => err(
            StatusCode::CONFLICT,
            "self_disable",
            "cannot disable your own account",
        ),
        auth::AuthError::Identity(deve_sub_domain::IdentityError::UserNotFound) => err(
            StatusCode::NOT_FOUND,
            "user_not_found",
            "user does not exist",
        ),
        other => {
            tracing::warn!(error = %other, "disable_user failed");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "failed to disable user",
            )
        }
    })?;

    Ok(StatusCode::OK)
}

/// `POST /api/v1/users/{id}/force-logout` — revoke all sessions for a
/// user without disabling the account (admin only). AUTH-010.
#[utoipa::path(
    post,
    path = "/api/v1/users/{id}/force-logout",
    security(("cookie_auth" = [])),
    params(("id" = String, Path, description = "Target user ULID")),
    responses(
        (status = 200, description = "Sessions revoked"),
        (status = 400, description = "Invalid user id", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 404, description = "User not found", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn force_logout(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let user_id = UserId::parse(&id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_id",
            "user id is not a valid ULID",
        )
    })?;

    auth::force_logout(
        state.user_repo.as_ref(),
        state.session_repo.as_ref(),
        user_id,
    )
    .await
    .map_err(|e| match e {
        auth::AuthError::Identity(deve_sub_domain::IdentityError::UserNotFound) => err(
            StatusCode::NOT_FOUND,
            "user_not_found",
            "user does not exist",
        ),
        other => {
            tracing::warn!(error = %other, "force_logout failed");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "force logout failed",
            )
        }
    })?;

    Ok(StatusCode::OK)
}

/// Register all user management routes on the given `OpenApiRouter`.
pub fn register(
    router: utoipa_axum::router::OpenApiRouter<AppState>,
) -> utoipa_axum::router::OpenApiRouter<AppState> {
    use utoipa_axum::routes;
    router
        .routes(routes!(create_user))
        .routes(routes!(list_users))
        .routes(routes!(disable_user))
        .routes(routes!(force_logout))
}
