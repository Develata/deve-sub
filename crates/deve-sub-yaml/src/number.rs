//! Numeric identity for YAML values and mapping keys.

use std::hash::{Hash, Hasher};

use serde::{Serialize, Serializer, de::Visitor};

use crate::Error;

/// A signed integer, unsigned integer, or IEEE 754 floating-point value.
///
/// Nonnegative integers share one representation. Floats remain distinct from
/// integers; all NaNs compare equal, as do positive and negative floating zero.
#[derive(Clone, Copy, Debug)]
pub struct Number(Repr);

#[derive(Clone, Copy, Debug)]
enum Repr {
    I64(i64),
    U64(u64),
    F64(f64),
}

impl Number {
    /// Return an integer if it fits in the signed 64-bit range.
    pub fn as_i64(&self) -> Option<i64> {
        match self.0 {
            Repr::I64(value) => Some(value),
            Repr::U64(value) => value.try_into().ok(),
            Repr::F64(_) => None,
        }
    }

    /// Return a nonnegative integer without converting floating-point values.
    pub fn as_u64(&self) -> Option<u64> {
        match self.0 {
            Repr::U64(value) => Some(value),
            Repr::I64(_) | Repr::F64(_) => None,
        }
    }

    /// Return the number as a float, possibly rounding large integers.
    pub fn as_f64(&self) -> Option<f64> {
        Some(match self.0 {
            Repr::I64(value) => value as f64,
            Repr::U64(value) => value as f64,
            Repr::F64(value) => value,
        })
    }

    pub(crate) fn visit<'de, V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.0 {
            Repr::I64(value) => visitor.visit_i64(value),
            Repr::U64(value) => visitor.visit_u64(value),
            Repr::F64(value) => visitor.visit_f64(value),
        }
    }
}

impl From<i64> for Number {
    fn from(value: i64) -> Self {
        Self(match u64::try_from(value) {
            Ok(value) => Repr::U64(value),
            Err(_) => Repr::I64(value),
        })
    }
}

impl From<u64> for Number {
    fn from(value: u64) -> Self {
        Self(Repr::U64(value))
    }
}

impl From<f64> for Number {
    fn from(value: f64) -> Self {
        Self(Repr::F64(value))
    }
}

impl PartialEq for Number {
    fn eq(&self, other: &Self) -> bool {
        match (self.0, other.0) {
            (Repr::I64(left), Repr::I64(right)) => left == right,
            (Repr::U64(left), Repr::U64(right)) => left == right,
            (Repr::F64(left), Repr::F64(right)) => {
                left == right || (left.is_nan() && right.is_nan())
            }
            _ => false,
        }
    }
}

impl Eq for Number {}

impl Hash for Number {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(&self.0).hash(state);
        match self.0 {
            Repr::I64(value) => value.hash(state),
            Repr::U64(value) => value.hash(state),
            Repr::F64(value) => {
                // WHY: mapping lookup must agree with numeric equality even
                // for different NaN payloads or the two IEEE zero encodings.
                let bits = if value.is_nan() {
                    f64::NAN.to_bits()
                } else if value == 0.0 {
                    0
                } else {
                    value.to_bits()
                };
                bits.hash(state);
            }
        }
    }
}

impl Serialize for Number {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Repr::I64(value) => serializer.serialize_i64(value),
            Repr::U64(value) => serializer.serialize_u64(value),
            Repr::F64(value) => serializer.serialize_f64(value),
        }
    }
}
