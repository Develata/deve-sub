//! Stable probe HTTP error mapping.
use crate::auth::err;
use axum::{http::StatusCode, response::Json};
use deve_sub_application::probe::ProbeAppError;

pub(super) fn map_probe_error(
    e: ProbeAppError,
) -> (StatusCode, Json<deve_sub_contract::ErrorResponse>) {
    match e {
        ProbeAppError::InvalidInput(msg) => err(StatusCode::BAD_REQUEST, "invalid_input", &msg),
        ProbeAppError::SourceNotFound => err(
            StatusCode::NOT_FOUND,
            "source_not_found",
            "probe source does not exist",
        ),
        ProbeAppError::RunNotFound => err(
            StatusCode::NOT_FOUND,
            "run_not_found",
            "probe run does not exist",
        ),
        ProbeAppError::NameExists => err(
            StatusCode::CONFLICT,
            "name_exists",
            "probe source name is already taken",
        ),
        ProbeAppError::RunAlreadyTerminal => err(
            StatusCode::CONFLICT,
            "run_already_terminal",
            "probe run is already completed, cancelled, or failed",
        ),
        ProbeAppError::Traffic(msg) => err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "traffic_persist_failed",
            &msg,
        ),
        ProbeAppError::Domain(deve_sub_domain::ProbeError::Conflict) => err(
            StatusCode::CONFLICT,
            "source_changed",
            "probe source changed concurrently; retry",
        ),
        ProbeAppError::Domain(deve_sub_domain::ProbeError::NameExists) => err(
            StatusCode::CONFLICT,
            "name_exists",
            "probe source name is already taken",
        ),
        ProbeAppError::Domain(deve_sub_domain::ProbeError::SourceNotFound) => err(
            StatusCode::NOT_FOUND,
            "source_not_found",
            "probe source does not exist",
        ),
        ProbeAppError::Domain(deve_sub_domain::ProbeError::RunNotFound) => err(
            StatusCode::NOT_FOUND,
            "run_not_found",
            "probe run does not exist",
        ),
        other => {
            tracing::warn!(error = %other, "probe operation failed");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "probe operation failed",
            )
        }
    }
}
