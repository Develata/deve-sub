//! Template management route handlers (admin-only).
//!
//! Implements the `/api/v1/templates/*` endpoints (excluding generation, which
//! lives in [`crate::template_generation`]): create, list, get, update, delete,
//! list versions, rollback, resolve, and compatibility. All routes require an
//! authenticated admin via the [`crate::auth::AdminUser`] extractor. See
//! `docs/plan/milestones/M5-generator-and-v3-template.md`.
//!
//! Split into submodules to keep each file under the ~500-line hard fuse
//! (follow-up F8.1): [`crud`] for CRUD/versions/rollback, [`resolve`] for
//! resolution/compatibility, [`mappers`] for DTO conversion, [`error`] for
//! application-error mapping.

mod crud;
mod error;
mod mappers;
mod resolve;

/// Register all template management routes on the given `OpenApiRouter`.
pub fn register(
    router: utoipa_axum::router::OpenApiRouter<crate::AppState>,
) -> utoipa_axum::router::OpenApiRouter<crate::AppState> {
    use utoipa_axum::routes;
    router
        .routes(routes!(crud::create_template))
        .routes(routes!(crud::list_templates))
        .routes(routes!(crud::get_template))
        .routes(routes!(crud::update_template))
        .routes(routes!(crud::delete_template))
        .routes(routes!(crud::list_versions))
        .routes(routes!(crud::rollback_template))
        .routes(routes!(resolve::resolve_template_route))
        .routes(routes!(resolve::check_compatibility_route))
}
