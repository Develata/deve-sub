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
pub mod subscription;
pub mod template;

pub use auth::{
    CreateUserRequest, CreateUserResponse, CurrentUserResponse, ErrorResponse, ListUsersResponse,
    LoginRequest, LoginResponse, LoginTwoFactorRequest, RegenerateRecoveryCodesRequest,
    RegenerateRecoveryCodesResponse, RoleDto, SetupAdminRequest, SetupAdminResponse,
    TwoFactorDisableRequest, TwoFactorSetupResponse, TwoFactorVerifyRequest,
    TwoFactorVerifyResponse, UserDto,
};
pub use health::{HealthLiveResponse, HealthReadyResponse, HealthStatusDto};
pub use node::{
    BatchEnabledRequest, BatchResultDto, BatchTagsRequest, CreateTagRequest, ImportNodesRequest,
    ImportNodesResponse, ImportOutcomeDto, ListNodesResponse, ListTagsResponse, NodeDto,
    NodeOverrideDto, NodeOverrideResponse, NodeResponse, NodeTagAssignmentDto, RegionMethodDto,
    RegionResponse, SetNodeTagsRequest, SetRegionRequest, TagDto, TagResponse,
    UpdateOverrideRequest,
};
pub use source::{
    CreateSourceRequest, ListSourcesResponse, ReconcileCountsDto, RefreshSourceResponse, SourceDto,
    SourceFilterRulesDto, SourceResponse, SourceTypeDto, UpdateSourceRequest,
};
pub use subscription::{
    CreateSubscriptionRequest, CreateTempLinkRequest, CreateTempLinkResponse,
    GetSubscriptionResponse, ListSubscriptionsQuery, ListSubscriptionsResponse, RotateTokenRequest,
    ShortCodeResponse, SubscriptionDto, SubscriptionResponse, TokenRotationResponse,
    UpdateSubscriptionRequest,
};
pub use template::{
    ActiveGenerationQuery, ActiveGenerationResponse, ChainEdgeDto, CompatibilityQuery,
    CompatibilityReportDto, CreateTemplateRequest, ExcludedNodeDto, GenerateQuery,
    GenerationResultDto, GetTemplateResponse, GroupResolutionDto, ListTemplatesQuery,
    ListTemplatesResponse, ListVersionsResponse, MissingNodeRefDto, ResolveTemplateResponse,
    RollbackRequest, RollbackTemplateResponse, TemplateDto, TemplateResponse, TemplateVersionDto,
    UpdateTemplateRequest,
};
