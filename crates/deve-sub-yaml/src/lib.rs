//! Bounded YAML compatibility at the existing template and source boundaries.
//!
//! Input uses granit's safe event parser. Output uses serde-saphyr's serializer
//! only: its deserializer, include/environment extensions and snippet errors
//! are not part of the dependency feature graph.

mod error;
mod generated;
mod number;
mod parser;
mod projection;
mod scalar;
mod value;
mod value_de;

pub use error::{Error, ErrorKind};
pub use generated::{SequenceState, inspect_generated_sequence};
pub use number::Number;
pub use value::{Mapping, Value, from_json};

/// Resource limits applied before expanding any alias into an owned value tree.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Maximum UTF-8 input bytes.
    pub input_bytes: usize,
    /// Maximum decoded string bytes plus 64 bytes per logical value.
    pub expanded_bytes: usize,
    /// Maximum value depth, with the root at zero.
    pub depth: usize,
}

impl Limits {
    /// Existing template limits; application schema validation remains separate.
    pub const TEMPLATE: Self = Self {
        input_bytes: 1024 * 1024,
        expanded_bytes: 8 * 1024 * 1024,
        depth: 10,
    };

    /// Source downloads are already capped at 10 MiB before parsing. The larger
    /// value budget accommodates 10,000 nodes while bounding alias amplification.
    pub const SOURCE: Self = Self {
        input_bytes: 10 * 1024 * 1024,
        expanded_bytes: 128 * 1024 * 1024,
        depth: 128,
    };
}

/// Parse template YAML, retaining mapping order, rejecting duplicate keys, and
/// resolving merge keys after charging their complete pre-merge logical cost.
pub fn parse_template(input: &str) -> Result<Value, Error> {
    parse_value_with_limits(input, Limits::TEMPLATE)
}

/// Parse an ordered YAML value with explicit limits and template merge rules.
pub fn parse_value_with_limits(input: &str, limits: Limits) -> Result<Value, Error> {
    projection::ordered(parser::parse(input, limits, false, true)?.as_ref())
}

/// Deserialize ordered YAML for trusted tooling and output verification.
/// Incoming templates must use [`parse_template`] and their application checks.
pub fn from_str<T: serde::de::DeserializeOwned>(input: &str) -> Result<T, Error> {
    from_value(parse_value_with_limits(input, Limits::SOURCE)?)
}

/// Preserve the source parser's JSON projection: scalar map keys keep their raw
/// spelling, duplicate keys use the last value, and `<<` stays an ordinary key.
pub fn parse_source_json(input: &str) -> Result<serde_json::Value, Error> {
    projection::json(parser::parse(input, Limits::SOURCE, true, true)?.as_ref())
}

/// Read a configuration already serialized by the server while assembling
/// generated output. Expanded group membership may exceed input limits; this
/// trusted path retains depth/duplicate checks and rejects every alias.
/// Never use this entry point for incoming template or source text.
pub fn parse_generated_value(input: &str) -> Result<Value, Error> {
    let limits = Limits {
        input_bytes: usize::MAX,
        expanded_bytes: usize::MAX,
        depth: 128,
    };
    projection::ordered(parser::parse(input, limits, false, false)?.as_ref())
}

/// Convert the ordered value into a typed document without a JSON round trip.
pub fn from_value<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, Error> {
    T::deserialize(value)
}

/// Serialize YAML without exposing values through dependency error messages.
pub fn to_string<T: serde::Serialize + ?Sized>(value: &T) -> Result<String, Error> {
    serde_saphyr::to_string(&value).map_err(|_| Error::new(ErrorKind::Serialize))
}
