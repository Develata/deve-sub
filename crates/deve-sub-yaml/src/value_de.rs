//! Consume a Value directly through Serde, keeping errors free of input data.

use serde::{
    Deserializer,
    de::{
        IntoDeserializer, Visitor,
        value::{MapDeserializer, SeqDeserializer, StringDeserializer},
    },
};

use crate::{Error, ErrorKind, Mapping, Value};

impl<'de> IntoDeserializer<'de, Error> for Value {
    type Deserializer = Self;

    fn into_deserializer(self) -> Self {
        self
    }
}

macro_rules! deserialize_number {
    ($($method:ident)*) => {
        $(fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
            match self {
                Self::Number(value) => value.visit(visitor),
                _ => Err(Error::new(ErrorKind::Type)),
            }
        })*
    };
}

impl<'de> Deserializer<'de> for Value {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self {
            Self::Null => visitor.visit_unit(),
            Self::Bool(value) => visitor.visit_bool(value),
            Self::Number(value) => value.visit(visitor),
            Self::String(value) => visitor.visit_string(value),
            Self::Sequence(value) => {
                SeqDeserializer::new(value.into_iter()).deserialize_any(visitor)
            }
            Self::Mapping(value) => {
                MapDeserializer::new(value.into_iter()).deserialize_any(visitor)
            }
        }
    }

    deserialize_number! {
        deserialize_i8 deserialize_i16 deserialize_i32 deserialize_i64 deserialize_i128
        deserialize_u8 deserialize_u16 deserialize_u32 deserialize_u64 deserialize_u128
        deserialize_f32 deserialize_f64
    }

    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self {
            Self::Bool(value) => visitor.visit_bool(value),
            _ => Err(Error::new(ErrorKind::Type)),
        }
    }

    fn deserialize_char<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        self.deserialize_string(visitor)
    }

    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        self.deserialize_string(visitor)
    }

    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self {
            Self::String(value) => visitor.visit_string(value),
            _ => Err(Error::new(ErrorKind::Type)),
        }
    }

    fn deserialize_bytes<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        self.deserialize_byte_buf(visitor)
    }

    fn deserialize_byte_buf<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self {
            Self::String(_) | Self::Sequence(_) => self.deserialize_any(visitor),
            _ => Err(Error::new(ErrorKind::Type)),
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self {
            Self::Null => visitor.visit_none(),
            _ => visitor.visit_some(self),
        }
    }

    fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self {
            Self::Null => visitor.visit_unit(),
            _ => Err(Error::new(ErrorKind::Type)),
        }
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.deserialize_unit(visitor)
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self {
            Self::Sequence(_) => self.deserialize_any(visitor),
            // WHY: existing authored V3 documents use null as an empty
            // collection. Keep that typed conversion independent of parsing.
            Self::Null => Self::Sequence(Vec::new()).deserialize_any(visitor),
            _ => Err(Error::new(ErrorKind::Type)),
        }
    }

    fn deserialize_tuple<V: Visitor<'de>>(
        self,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_map<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self {
            Self::Mapping(_) => self.deserialize_any(visitor),
            Self::Null => Self::Mapping(Mapping::new()).deserialize_any(visitor),
            _ => Err(Error::new(ErrorKind::Type)),
        }
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.deserialize_map(visitor)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        match self {
            Self::String(value) => {
                StringDeserializer::<Error>::new(value).deserialize_enum(name, variants, visitor)
            }
            // WHY: legacy Value conversion accepted only string unit enums
            // or YAML tags. Custom tags are rejected by the input boundary;
            // accepting an external-tag mapping here would widen that input.
            _ => Err(Error::new(ErrorKind::Type)),
        }
    }

    fn deserialize_identifier<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        self.deserialize_string(visitor)
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_unit()
    }
}
