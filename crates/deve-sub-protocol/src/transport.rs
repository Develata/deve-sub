//! Shared transport-kind mapping for URI parsers.

use deve_sub_domain::TransportKind;

use crate::error::ParseError;

/// Map the `type`/`net` query parameter to a [`TransportKind`].
pub(crate) fn map_transport_kind(value: &str) -> Result<TransportKind, ParseError> {
    match value {
        "tcp" => Ok(TransportKind::Tcp),
        "ws" => Ok(TransportKind::Ws),
        "grpc" => Ok(TransportKind::Grpc),
        "h2" => Ok(TransportKind::H2),
        "kcp" => Ok(TransportKind::Kcp),
        "quic" => Ok(TransportKind::Quic),
        "httpupgrade" => Ok(TransportKind::HttpUpgrade),
        "xtls" => Ok(TransportKind::Xtls),
        _ => Err(ParseError::InvalidField {
            field: "type (transport)",
            value: value.to_owned(),
        }),
    }
}
