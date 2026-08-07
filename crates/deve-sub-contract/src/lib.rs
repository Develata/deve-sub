//! API DTOs, event DTOs, and client capability contracts shared across the
//! delivery and application layers.
//!
//! `ToSchema` implementations for OpenAPI generation live here. See
//! `docs/plan/03-architecture.md` and ADR-0004 for the API boundary policy.

#![cfg_attr(test, allow(clippy::expect_used))]

pub mod auth;
pub mod health;
pub mod node;
pub mod source;

pub use auth::{
    CreateUserRequest, CreateUserResponse, CurrentUserResponse, ErrorResponse, ListUsersResponse,
    LoginRequest, LoginResponse, LoginTwoFactorRequest, RegenerateRecoveryCodesRequest,
    RegenerateRecoveryCodesResponse, RoleDto, SetupAdminRequest, SetupAdminResponse,
    TwoFactorDisableRequest, TwoFactorSetupResponse, TwoFactorVerifyRequest,
    TwoFactorVerifyResponse, UserDto,
};
pub use health::{HealthLiveResponse, HealthReadyResponse, HealthStatusDto};
pub use node::{
    ImportNodesRequest, ImportNodesResponse, ImportOutcomeDto, ListNodesResponse, NodeDto,
    NodeResponse,
};
pub use source::{
    CreateSourceRequest, ListSourcesResponse, ReconcileCountsDto, RefreshSourceResponse, SourceDto,
    SourceResponse, SourceTypeDto, UpdateSourceRequest,
};
