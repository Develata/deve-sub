//! Domain-to-REST mappings shared by probe and dashboard views.
use crate::auth::ts_to_iso8601;
use deve_sub_contract::{
    ErrorClassDto, LatencyRecordDto, ProbeRunDto, ProbeRunResultDto, ProbeRunStatusDto,
    ProbeSourceDto, ProbeSourceKindDto, ProbeTypeDto, SyncStatusDto,
};
use deve_sub_domain::{
    ErrorClass, LatencyRecord, ProbeRun, ProbeRunStatus, ProbeSource, ProbeSourceKind, ProbeType,
    SyncStatus,
};

pub(crate) fn kind_to_dto(k: ProbeSourceKind) -> ProbeSourceKindDto {
    match k {
        ProbeSourceKind::Nezha => ProbeSourceKindDto::Nezha,
        ProbeSourceKind::DStatus => ProbeSourceKindDto::Dstatus,
        ProbeSourceKind::Komari => ProbeSourceKindDto::Komari,
    }
}

pub(super) fn kind_from_dto(d: ProbeSourceKindDto) -> ProbeSourceKind {
    match d {
        ProbeSourceKindDto::Nezha => ProbeSourceKind::Nezha,
        ProbeSourceKindDto::Dstatus => ProbeSourceKind::DStatus,
        ProbeSourceKindDto::Komari => ProbeSourceKind::Komari,
    }
}

pub(super) fn kind_from_kebab(s: &str) -> Option<ProbeSourceKind> {
    match s {
        "nezha" => Some(ProbeSourceKind::Nezha),
        "dstatus" => Some(ProbeSourceKind::DStatus),
        "komari" => Some(ProbeSourceKind::Komari),
        _ => None,
    }
}

pub(crate) fn probe_type_to_dto(t: ProbeType) -> ProbeTypeDto {
    match t {
        ProbeType::TcpConnect => ProbeTypeDto::TcpConnect,
        ProbeType::QuicHandshake => ProbeTypeDto::QuicHandshake,
        ProbeType::RealProxy => ProbeTypeDto::RealProxy,
    }
}

pub(super) fn probe_type_from_dto(d: ProbeTypeDto) -> ProbeType {
    match d {
        ProbeTypeDto::TcpConnect => ProbeType::TcpConnect,
        ProbeTypeDto::QuicHandshake => ProbeType::QuicHandshake,
        ProbeTypeDto::RealProxy => ProbeType::RealProxy,
    }
}

pub(super) fn run_status_to_dto(s: ProbeRunStatus) -> ProbeRunStatusDto {
    match s {
        ProbeRunStatus::Pending => ProbeRunStatusDto::Pending,
        ProbeRunStatus::Running => ProbeRunStatusDto::Running,
        ProbeRunStatus::Completed => ProbeRunStatusDto::Completed,
        ProbeRunStatus::Cancelled => ProbeRunStatusDto::Cancelled,
        ProbeRunStatus::Failed => ProbeRunStatusDto::Failed,
    }
}

pub(crate) fn error_class_to_dto(c: ErrorClass) -> ErrorClassDto {
    match c {
        ErrorClass::Refused => ErrorClassDto::Refused,
        ErrorClass::DnsFailed => ErrorClassDto::DnsFailed,
        ErrorClass::Timeout => ErrorClassDto::Timeout,
        ErrorClass::TlsFailed => ErrorClassDto::TlsFailed,
        ErrorClass::QuicFailed => ErrorClassDto::QuicFailed,
        ErrorClass::Ok => ErrorClassDto::Ok,
    }
}

pub(crate) fn sync_status_to_dto(s: &SyncStatus) -> SyncStatusDto {
    match s {
        SyncStatus::Ok => SyncStatusDto::Ok,
        SyncStatus::Failed(msg) => SyncStatusDto::Failed {
            message: msg.clone(),
        },
        SyncStatus::Stale => SyncStatusDto::Stale,
    }
}

pub(super) fn source_to_dto(source: &ProbeSource) -> ProbeSourceDto {
    ProbeSourceDto {
        id: source.id.to_string(),
        kind: kind_to_dto(source.kind),
        name: source.name.clone(),
        endpoint_url: source.endpoint_url.clone(),
        has_auth: !source.auth_config.is_empty(),
        subscription_id: source.subscription_id.map(|id| id.to_string()),
        enabled: source.enabled,
        last_sync_at: source.last_sync_at.map(ts_to_iso8601),
        last_sync_status: source.last_sync_status.as_ref().map(sync_status_to_dto),
        created_at: ts_to_iso8601(source.created_at),
        updated_at: ts_to_iso8601(source.updated_at),
    }
}

pub(super) fn run_to_dto(run: &ProbeRun) -> ProbeRunDto {
    ProbeRunDto {
        id: run.id.to_string(),
        probe_type: probe_type_to_dto(run.probe_type),
        node_ids: run.node_ids.iter().map(|id| id.to_string()).collect(),
        status: run_status_to_dto(run.status),
        results: run
            .results
            .iter()
            .map(|r| ProbeRunResultDto {
                node_id: r.node_id.to_string(),
                rtt_ms: r.rtt_ms,
                error_class: error_class_to_dto(r.error_class),
                skipped: r.skipped,
            })
            .collect(),
        created_at: ts_to_iso8601(run.created_at),
        completed_at: run.completed_at.map(ts_to_iso8601),
    }
}

pub(super) fn record_to_dto(record: &LatencyRecord) -> LatencyRecordDto {
    LatencyRecordDto {
        id: record.id.to_string(),
        run_id: record.run_id.to_string(),
        node_id: record.node_id.to_string(),
        probe_type: probe_type_to_dto(record.probe_type),
        rtt_ms: record.rtt_ms,
        error_class: error_class_to_dto(record.error_class),
        measured_at: ts_to_iso8601(record.measured_at),
    }
}
