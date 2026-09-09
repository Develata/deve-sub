//! SQLite discriminators are adapter-owned, independent of API names.
use deve_sub_domain::{
    ErrorClass, ProbeRunStatus, ProbeSourceKind, ProbeType, RefreshPhase, SourceRefreshJobStatus,
    TrafficSourceKind,
};

/// Stable physical representation of the existing SQLite schema.
pub(crate) trait SqliteDiscriminant: Sized {
    fn encode(self) -> &'static str;
    fn decode(value: &str) -> Option<Self>;
}

impl SqliteDiscriminant for TrafficSourceKind {
    fn encode(self) -> &'static str {
        match self {
            Self::AirportHeader => "A",
            Self::ManualCorrection => "M",
            Self::Probe => "P",
        }
    }

    fn decode(c: &str) -> Option<Self> {
        match c {
            "A" => Some(Self::AirportHeader),
            "M" => Some(Self::ManualCorrection),
            "P" => Some(Self::Probe),
            _ => None,
        }
    }
}

impl SqliteDiscriminant for SourceRefreshJobStatus {
    fn encode(self) -> &'static str {
        match self {
            Self::Pending => "P",
            Self::Running => "R",
            Self::Completed => "C",
            Self::Failed => "F",
            Self::Cancelled => "X",
        }
    }

    fn decode(c: &str) -> Option<Self> {
        match c {
            "P" => Some(Self::Pending),
            "R" => Some(Self::Running),
            "C" => Some(Self::Completed),
            "F" => Some(Self::Failed),
            "X" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

impl SqliteDiscriminant for RefreshPhase {
    fn encode(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Fetching => "fetching",
            Self::Parsing => "parsing",
            Self::Enriching => "enriching",
            Self::Reconciling => "reconciling",
            Self::Publishing => "publishing",
        }
    }

    fn decode(s: &str) -> Option<Self> {
        match s {
            "idle" => Some(Self::Idle),
            "fetching" => Some(Self::Fetching),
            "parsing" => Some(Self::Parsing),
            "enriching" => Some(Self::Enriching),
            "reconciling" => Some(Self::Reconciling),
            "publishing" => Some(Self::Publishing),
            _ => None,
        }
    }
}

impl SqliteDiscriminant for ProbeSourceKind {
    fn encode(self) -> &'static str {
        match self {
            Self::Nezha => "N",
            Self::DStatus => "D",
            Self::Komari => "K",
        }
    }

    fn decode(c: &str) -> Option<Self> {
        match c {
            "N" => Some(Self::Nezha),
            "D" => Some(Self::DStatus),
            "K" => Some(Self::Komari),
            _ => None,
        }
    }
}

impl SqliteDiscriminant for ProbeType {
    fn encode(self) -> &'static str {
        match self {
            Self::TcpConnect => "T",
            Self::QuicHandshake => "Q",
            Self::RealProxy => "R",
        }
    }

    fn decode(c: &str) -> Option<Self> {
        match c {
            "T" => Some(Self::TcpConnect),
            "Q" => Some(Self::QuicHandshake),
            "R" => Some(Self::RealProxy),
            _ => None,
        }
    }
}

impl SqliteDiscriminant for ErrorClass {
    fn encode(self) -> &'static str {
        match self {
            Self::Refused => "R",
            Self::DnsFailed => "D",
            Self::Timeout => "T",
            Self::TlsFailed => "L",
            Self::QuicFailed => "Q",
            Self::Ok => "O",
        }
    }

    fn decode(c: &str) -> Option<Self> {
        match c {
            "R" => Some(Self::Refused),
            "D" => Some(Self::DnsFailed),
            "T" => Some(Self::Timeout),
            "L" => Some(Self::TlsFailed),
            "Q" => Some(Self::QuicFailed),
            "O" => Some(Self::Ok),
            _ => None,
        }
    }
}

impl SqliteDiscriminant for ProbeRunStatus {
    fn encode(self) -> &'static str {
        match self {
            Self::Pending => "P",
            Self::Running => "R",
            Self::Completed => "C",
            Self::Cancelled => "X",
            Self::Failed => "F",
        }
    }

    fn decode(c: &str) -> Option<Self> {
        match c {
            "P" => Some(Self::Pending),
            "R" => Some(Self::Running),
            "C" => Some(Self::Completed),
            "X" => Some(Self::Cancelled),
            "F" => Some(Self::Failed),
            _ => None,
        }
    }
}
