//! Shared transport-kind mapping for URI emitters.

use deve_sub_domain::TransportKind;

/// Map a [`TransportKind`] to the `type`/`net` query parameter string.
pub(crate) fn transport_kind_str(kind: TransportKind) -> &'static str {
    match kind {
        TransportKind::Tcp => "tcp",
        TransportKind::Kcp => "kcp",
        TransportKind::Ws => "ws",
        TransportKind::H2 => "h2",
        TransportKind::Quic => "quic",
        TransportKind::Grpc => "grpc",
        TransportKind::HttpUpgrade => "httpupgrade",
        TransportKind::Xtls => "xtls",
    }
}
