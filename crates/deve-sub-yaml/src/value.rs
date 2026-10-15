//! Ordered, format-neutral YAML values; input limits belong to the parser.

use std::{
    collections::hash_map::DefaultHasher,
    fmt,
    hash::{Hash, Hasher},
    ops::Index,
};

use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, MapAccess, SeqAccess, Visitor},
    ser::{SerializeMap, SerializeSeq},
};

use crate::Number;

/// A YAML mapping that iterates in authored insertion order.
pub type Mapping = indexmap::IndexMap<Value, Value>;

/// A YAML value with ordered mappings and support for complex mapping keys.
///
/// Mapping equality ignores insertion order without changing iteration order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Value {
    /// An absent value.
    #[default]
    Null,
    /// A boolean scalar.
    Bool(bool),
    /// An integer or floating-point scalar.
    Number(Number),
    /// A text scalar.
    String(String),
    /// An ordered sequence.
    Sequence(Vec<Value>),
    /// An ordered mapping.
    Mapping(Mapping),
}

impl Value {
    /// Look up a string key in a mapping.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.as_mapping()?.get(&StringKey(key))
    }

    /// Borrow the text if this value is a string.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    /// Return the boolean if this value is a boolean.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    /// Return an integer if it fits in the unsigned 64-bit range.
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Self::Number(value) => value.as_u64(),
            _ => None,
        }
    }

    /// Return an integer if it fits in the signed 64-bit range.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Number(value) => value.as_i64(),
            _ => None,
        }
    }

    /// Return a number as a float, possibly rounding large integers.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Number(value) => value.as_f64(),
            _ => None,
        }
    }

    /// Borrow an ordered mapping.
    pub fn as_mapping(&self) -> Option<&Mapping> {
        match self {
            Self::Mapping(value) => Some(value),
            _ => None,
        }
    }

    /// Mutably borrow an ordered mapping.
    pub fn as_mapping_mut(&mut self) -> Option<&mut Mapping> {
        match self {
            Self::Mapping(value) => Some(value),
            _ => None,
        }
    }

    /// Borrow a sequence.
    pub fn as_sequence(&self) -> Option<&Vec<Value>> {
        match self {
            Self::Sequence(value) => Some(value),
            _ => None,
        }
    }

    /// Whether this value is a sequence.
    pub fn is_sequence(&self) -> bool {
        matches!(self, Self::Sequence(_))
    }

    /// Whether this value is null.
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }
}

impl Hash for Value {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Self::Null => {}
            Self::Bool(value) => value.hash(state),
            Self::Number(value) => value.hash(state),
            Self::String(value) => value.hash(state),
            Self::Sequence(value) => value.hash(state),
            Self::Mapping(value) => {
                // WHY: complex mapping keys compare without order. Hash each
                // entry before combining, so keys and values stay associated.
                value.len().hash(state);
                let sum = value.iter().fold(0_u64, |sum, entry| {
                    let mut entry_hash = DefaultHasher::new();
                    entry.hash(&mut entry_hash);
                    sum.wrapping_add(entry_hash.finish())
                });
                sum.hash(state);
            }
        }
    }
}

struct StringKey<'a>(&'a str);

impl Hash for StringKey<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // WHY: a borrowed lookup must hash identically to Value::String;
        // String::new does not allocate.
        std::mem::discriminant(&Value::String(String::new())).hash(state);
        self.0.hash(state);
    }
}

impl indexmap::Equivalent<Value> for StringKey<'_> {
    fn equivalent(&self, key: &Value) -> bool {
        key.as_str() == Some(self.0)
    }
}

static NULL: Value = Value::Null;

impl Index<&str> for Value {
    type Output = Value;

    fn index(&self, key: &str) -> &Self::Output {
        self.get(key).unwrap_or(&NULL)
    }
}

impl Index<usize> for Value {
    type Output = Value;

    fn index(&self, index: usize) -> &Self::Output {
        self.as_sequence()
            .and_then(|sequence| sequence.get(index))
            .unwrap_or(&NULL)
    }
}

impl PartialEq<str> for Value {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == Some(other)
    }
}

impl PartialEq<&str> for Value {
    fn eq(&self, other: &&str) -> bool {
        self == *other
    }
}

impl Serialize for Value {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Null => serializer.serialize_unit(),
            Self::Bool(value) => serializer.serialize_bool(*value),
            Self::Number(value) => value.serialize(serializer),
            Self::String(value) => serializer.serialize_str(value),
            Self::Sequence(value) => {
                let mut sequence = serializer.serialize_seq(Some(value.len()))?;
                for item in value {
                    sequence.serialize_element(item)?;
                }
                sequence.end()
            }
            Self::Mapping(value) => {
                let mut mapping = serializer.serialize_map(Some(value.len()))?;
                for (key, item) in value {
                    mapping.serialize_entry(key, item)?;
                }
                mapping.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(ValueVisitor)
    }
}

struct ValueVisitor;

impl<'de> Visitor<'de> for ValueVisitor {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a YAML value")
    }

    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_none<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        Value::deserialize(deserializer)
    }

    fn visit_newtype_struct<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Value, D::Error> {
        Value::deserialize(deserializer)
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Value, E> {
        Ok(Value::Number(value.into()))
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Value, E> {
        Ok(Value::Number(value.into()))
    }

    fn visit_i128<E: de::Error>(self, value: i128) -> Result<Value, E> {
        if let Ok(value) = u64::try_from(value) {
            self.visit_u64(value)
        } else {
            i64::try_from(value)
                .map_err(|_| E::custom("integer out of range"))
                .and_then(|value| self.visit_i64(value))
        }
    }

    fn visit_u128<E: de::Error>(self, value: u128) -> Result<Value, E> {
        u64::try_from(value)
            .map_err(|_| E::custom("integer out of range"))
            .and_then(|value| self.visit_u64(value))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Value, E> {
        Ok(Value::Number(value.into()))
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Value, E> {
        self.visit_string(value.to_owned())
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<Value, E> {
        Ok(Value::String(value))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Value, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element()? {
            values.push(value);
        }
        Ok(Value::Sequence(values))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut mapping: A) -> Result<Value, A::Error> {
        let mut values = Mapping::new();
        while let Some((key, value)) = mapping.next_entry()? {
            values.insert(key, value);
        }
        Ok(Value::Mapping(values))
    }
}

/// Convert an existing JSON tree without serializing it to text.
///
/// This is not a parser and does not apply the YAML input budgets.
pub fn from_json(value: serde_json::Value) -> Value {
    match value {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(value) => Value::Bool(value),
        serde_json::Value::Number(value) => {
            let number = if let Some(value) = value.as_u64() {
                value.into()
            } else if let Some(value) = value.as_i64() {
                value.into()
            } else {
                // WHY: serde_json without arbitrary_precision stores only
                // i64, u64 and finite f64, so the remaining case is a float.
                #[allow(clippy::expect_used)]
                let value = value
                    .as_f64()
                    .expect("JSON number has a numeric representation");
                value.into()
            };
            Value::Number(number)
        }
        serde_json::Value::String(value) => Value::String(value),
        serde_json::Value::Array(value) => {
            Value::Sequence(value.into_iter().map(from_json).collect())
        }
        serde_json::Value::Object(value) => Value::Mapping(
            value
                .into_iter()
                .map(|(key, value)| (Value::String(key), from_json(value)))
                .collect(),
        ),
    }
}
