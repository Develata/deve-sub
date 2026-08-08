//! Xray JSON container emitter.
//!
//! Emits an `outbounds` array with one entry per compatible node. Xray and
//! V2Ray share a near-identical outbound schema; the difference is the set of
//! supported protocols/transports (filtered by the compatibility layer), not
//! the JSON shape. See `v2ray.rs` for the shared emitter.

use deve_sub_domain::Node;

use crate::error::EmitError;

pub fn emit(nodes: &[Node]) -> Result<String, EmitError> {
    super::v2ray::emit(nodes)
}
